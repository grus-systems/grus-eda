//! Power estimation driven by switching-activity annotation (VCD or SAIF).
//!
//! Reports the same three components as OpenSTA's `report_power`:
//! - **Leakage**: `leakage_power` `when` states weighted by the marginal
//!   probability each state holds (`cell_leakage_power` when the cell declares
//!   no states or covers no measurable ones).
//! - **Switching**: `0.5 * C_load * V^2 * transition_rate` per driven net, with
//!   `C_load` = wire capacitance + sink pin capacitance + the driving pin's own
//!   capacitance (per transition direction, matching OpenSTA's `loadCap`).
//! - **Internal**: `internal_power` Liberty tables evaluated at the trigger pin's
//!   slew and the driven net's total load; the power-owning pin's net transitions
//!   are apportioned over the blocks' related pins by activity rate, and a
//!   `when`-qualified block weighs its energy by the marginal probability its
//!   state expression holds.
//!
//! Activity comes from a VCD dump (or resolved SAIF marginals), so numbers are
//! exact for the simulated window rather than OpenSTA's statistical
//! propagation. Slews come from [`sta_pin_slews`]. Edges involving x/z count
//! half a transition each, and per-direction lookups consume half the effective
//! count — both measured OpenSTA behaviors (docs/results.md N7). `when` forms
//! outside the literal-conjunction grammar, naming no sibling pin, or whose
//! literal nets carry no measured state are skipped and counted in the
//! diagnostics.

use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinDir, PinId};
use crate::lib_db::{LibCell, LibDb, LibPin, parse_when_expr};
use crate::readers::vcd::{SignalActivity, VcdActivity};
use crate::sta::analysis::{FALL, PinSlew, RISE};
use crate::sta::nldm::eval_nldm_delay_or_slew;
use crate::sta::parasitics::NetParasitics;
use crate::sta::sdc::Sdc;
use serde::Serialize;

/// Power of one reporting group, in watts.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PowerGroup {
    pub name: String,
    pub internal_w: f64,
    pub switching_w: f64,
    pub leakage_w: f64,
}

/// Inputs the estimator could not model; surfaced instead of silently approximated.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PowerDiagnostics {
    /// Instances whose LEF cell has no Liberty counterpart (no leakage/pin data).
    pub cells_without_lib_cell: usize,
    /// Driven nets absent from the activity input (treated as zero activity).
    /// Such a net's `net_activity`/`net_prob_one` is `None`, so a `when`
    /// literal on one of its pins is inexpressible — those `when` blocks
    /// are counted in `conditional_power_blocks` too.
    pub driven_nets_without_activity: usize,
    /// Nets with VCD activity but no cell-output driver (input ports, undriven);
    /// their switching power is not attributed anywhere, as in OpenSTA.
    pub active_nets_without_cell_driver: usize,
    /// `internal_power` triggers whose pin slew STA could not reach.
    pub internal_power_without_slew: usize,
    /// `internal_power` `related_pin` names that match no sibling pin of the cell.
    pub unresolved_related_pin: usize,
    /// `internal_power`/`leakage_power` `when` blocks skipped because the
    /// expression is outside the literal-conjunction grammar, names no sibling
    /// pin, or a literal net carries no measured state.
    pub conditional_power_blocks: usize,
    /// SAIF `INSTANCE` scopes matching no design cell (0 on the VCD path;
    /// merged in from `SaifDiagnostics` by the caller).
    pub saif_unmatched_scopes: usize,
    /// SAIF signal names matching no design net, port or pin (0 on the VCD path).
    pub saif_unmatched_signals: usize,
}

/// Estimated power over the VCD window, grouped like `report_power`.
#[derive(Clone, Debug, Serialize)]
pub struct PowerReport {
    /// VCD window in seconds; all activity densities are per this duration.
    pub duration_s: f64,
    /// `nom_voltage` the switching `V^2` term used.
    pub voltage_v: f32,
    pub internal_w: f64,
    pub switching_w: f64,
    pub leakage_w: f64,
    pub total_w: f64,
    pub groups: Vec<PowerGroup>,
    pub diagnostics: PowerDiagnostics,
}

/// Group slots, in `report_power` order.
const SEQ: usize = 0;
const COMB: usize = 1;
const CLOCK: usize = 2;
const GROUP_NAMES: [&str; 3] = ["Sequential", "Combinational", "Clock"];

/// Sequential cells report as Sequential; cells with timing arcs as Combinational;
/// arc-less cells (tap/decap/filler) land in Clock, matching OpenSTA's grouping of
/// non-signal cells observed on `gcd` (docs/results.md N7).
fn group_of(lib_cell: &LibCell) -> usize {
    if lib_cell.is_sequential || lib_cell.is_latch {
        SEQ
    } else if lib_cell.arc_count > 0 {
        COMB
    } else {
        CLOCK
    }
}

struct PowerCx<'a> {
    design: &'a DesignDb,
    lib: &'a LibDb,
    /// Activity per net; `None` = the net carries no annotation entry at all
    /// (distinct from an annotated constant, which reports a real duty).
    net_activity: Vec<Option<SignalActivity>>,
    /// Total load capacitance per net per output transition, pF: wire cap plus
    /// sink pin caps (and SDC output loads), the same quantity STA's `net_load`
    /// computes for delay lookups.
    net_load_pf: Vec<[f32; 2]>,
    pin_slews: &'a [PinSlew],
    duration_s: f64,
    diagnostics: PowerDiagnostics,
}

impl PowerCx<'_> {
    fn net_activity(&self, net: NetId) -> SignalActivity {
        self.net_activity[net.0 as usize].unwrap_or_default()
    }

    /// Marginal `P(net = 1)` over the annotated window; `None` when the net has
    /// no annotation or was never in a defined state.
    fn net_prob_one(&self, net: NetId) -> Option<f64> {
        self.net_activity[net.0 as usize].and_then(|a| a.prob_one())
    }

    /// The driving pin's own capacitance, pF per direction. OpenSTA's
    /// switching/internal `loadCap` adds it on top of wire + sink + `set_load`
    /// (measured on the mbw fixture: `set_load 0.05` + Z pin cap 0.00226
    /// reproduces the golden to ~0.01%, docs/results.md N7).
    fn driver_cap(&self, driver: PinId, pol: usize) -> f32 {
        match self.design.pin_lib_pins[driver.0 as usize] {
            Some(lp) => {
                let lib_pin = &self.lib.pins[lp.0 as usize];
                if pol == RISE {
                    lib_pin.cap_rise
                } else {
                    lib_pin.cap_fall
                }
            }
            None => 0.0,
        }
    }

    /// `0.5 * C * V^2 * rate` for one net. OpenSTA keeps a single transition
    /// density per net, so each direction consumes half the effective toggles
    /// and its own directional load.
    fn switching_w(&self, net: NetId, driver: PinId, voltage_v: f32) -> f64 {
        let eff = self.net_activity(net).eff_toggles();
        if eff <= 0.0 || self.duration_s <= 0.0 {
            return 0.0;
        }
        let load = self.net_load_pf[net.0 as usize];
        let c = (load[RISE] + self.driver_cap(driver, RISE)) as f64
            + (load[FALL] + self.driver_cap(driver, FALL)) as f64;
        let energy_pf = 0.5 * voltage_v as f64 * voltage_v as f64 * (eff * 0.5) * c;
        energy_pf * 1e-12 / self.duration_s
    }

    /// `total_output_net_capacitance` axis value for a pin's own driven net:
    /// worst direction of wire + sinks + SDC load + the driver pin's own cap.
    fn total_output_cap_pf(&self, net: NetId, driver: PinId) -> f32 {
        let load = self.net_load_pf[net.0 as usize];
        (load[RISE] + self.driver_cap(driver, RISE)).max(load[FALL] + self.driver_cap(driver, FALL))
    }
}

/// Capacitance one pin contributes to its net's load for transition `pol`, pF.
/// Mirrors STA's `sink_cap`: drivers contribute nothing, unmapped pins nothing,
/// primary outputs take the SDC `set_load`.
fn sink_cap(design: &DesignDb, lib: &LibDb, sdc: &Sdc, pin: PinId, pol: usize) -> f32 {
    let p = pin.0 as usize;
    if design.pin_is_driver[p] {
        return 0.0;
    }
    match (design.pin_cells[p], design.pin_lib_pins[p]) {
        (None, _) => {
            if matches!(design.pin_dirs[p], PinDir::Output) {
                sdc.output_load_pf(design.strings.resolve(design.pin_names[p]))
            } else {
                0.0
            }
        }
        (Some(_), Some(lp)) => {
            let lib_pin = &lib.pins[lp.0 as usize];
            if pol == RISE {
                lib_pin.cap_rise
            } else {
                lib_pin.cap_fall
            }
        }
        (Some(_), None) => 0.0,
    }
}

fn net_load_pf(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
    net: NetId,
    pol: usize,
) -> f32 {
    let wire = parasitics
        .and_then(|p| p.net(net))
        .map_or(0.0, |t| t.total_wire_cap_pf);
    wire + design
        .net_pins(net)
        .iter()
        .map(|&pin| sink_cap(design, lib, sdc, pin, pol))
        .sum::<f32>()
}

/// Finds the design pin of `cell` whose Liberty pin name is `sym`, for
/// `internal_power` `related_pin` resolution.
fn sibling_pin_by_lib_name(
    design: &DesignDb,
    lib: &LibDb,
    cell: CellId,
    sym: crate::interner::SymbolId,
) -> Option<PinId> {
    design.cell_pins(cell).iter().copied().find(|&p| {
        design.pin_lib_pins[p.0 as usize]
            .map(|lp| lib.pins[lp.0 as usize].name == sym)
            .unwrap_or(false)
    })
}

/// Marginal probability a `when` expression holds: the product over literals
/// of each literal pin's net state probability — OpenSTA's independence model
/// (no joint distributions exist in its activity model; docs/results.md N7).
/// `None` = inexpressible: grammar outside the literal conjunction, a literal
/// naming no sibling pin, a floating pin, or a net with no measured state.
fn eval_when(cx: &PowerCx, cell: CellId, when: crate::interner::SymbolId) -> Option<f64> {
    let lits = parse_when_expr(cx.lib.strings.resolve(when), &cx.lib.strings)?;
    let mut p = 1.0f64;
    for lit in lits {
        let pin = sibling_pin_by_lib_name(cx.design, cx.lib, cell, lit.pin)?;
        let net = cx.design.pin_nets[pin.0 as usize]?;
        let p1 = cx.net_prob_one(net)?;
        p *= if lit.inverted { 1.0 - p1 } else { p1 };
    }
    Some(p)
}

/// Effective toggles on the net `pin` connects to (0 when floating or unannotated).
fn net_eff_toggles(cx: &PowerCx, pin: PinId) -> f64 {
    cx.design.pin_nets[pin.0 as usize]
        .map(|n| cx.net_activity(n).eff_toggles())
        .unwrap_or(0.0)
}

/// Internal power of one cell pin's `internal_power` blocks, attributed to `group`.
///
/// Measured OpenSTA model (docs/results.md N7): the power-owning pin's net
/// transitions are apportioned over the blocks' distinct related pins by their
/// own net activity rates — a block whose related pin is inactive contributes
/// nothing. Each direction consumes half the net's effective toggles, and a
/// `when` block weighs its table energy by `P(when)` under the marginal model.
fn pin_internal_power(
    cx: &mut PowerCx,
    cell: CellId,
    pin: PinId,
    lib_pin: &LibPin,
    group: usize,
    groups: &mut [PowerGroup; 3],
) {
    let Some(own_net) = cx.design.pin_nets[pin.0 as usize] else {
        return;
    };
    let own_eff = cx.net_activity(own_net).eff_toggles();
    if own_eff <= 0.0 || cx.duration_s <= 0.0 {
        return;
    }

    // Resolve every block's trigger first: the distinct related pins' activity
    // rates form the apportioning denominator.
    let mut resolved: Vec<Option<PinId>> = Vec::with_capacity(lib_pin.internal_power.len());
    let mut related: Vec<PinId> = Vec::new();
    for ip in &lib_pin.internal_power {
        let trigger = match ip.related_pin {
            // No related pin: the block's own pin transition is the trigger
            // (the input-pin self-power form).
            None => Some(pin),
            Some(sym) => match sibling_pin_by_lib_name(cx.design, cx.lib, cell, sym) {
                Some(p) => Some(p),
                None => {
                    cx.diagnostics.unresolved_related_pin += 1;
                    None
                }
            },
        };
        if let Some(t) = trigger
            && !related.contains(&t)
        {
            related.push(t);
        }
        resolved.push(trigger);
    }
    let denom: f64 = related.iter().map(|&t| net_eff_toggles(cx, t)).sum();
    // No related pin carries activity → no block fires (a `when` block whose
    // trigger is idle contributes nothing, even when the owning pin toggles).
    if denom <= 0.0 {
        return;
    }

    // The 2-D `power_outputs_*` tables index the load on the pin's own driven
    // net; input-pin `power_inputs_*` tables are 1-D and ignore it.
    let is_output = matches!(lib_pin.dir, PinDir::Output | PinDir::Inout);
    let load_pf = if is_output {
        cx.total_output_cap_pf(own_net, pin)
    } else {
        0.0
    };

    for (ip, trigger) in lib_pin.internal_power.iter().zip(&resolved) {
        let Some(trigger) = *trigger else {
            continue;
        };
        let weight = match ip.when {
            None => 1.0,
            Some(sym) => match eval_when(cx, cell, sym) {
                Some(p) => p,
                None => {
                    cx.diagnostics.conditional_power_blocks += 1;
                    continue;
                }
            },
        };
        let share = net_eff_toggles(cx, trigger) / denom;
        if weight <= 0.0 || share <= 0.0 {
            continue;
        }
        for (table, pol) in [(ip.rise.as_ref(), RISE), (ip.fall.as_ref(), FALL)] {
            let Some(table) = table else {
                continue;
            };
            // OpenSTA has no "unreached" slew: a pin with no propagated transition
            // keeps slew 0 and the table is still evaluated there (the axis edge is
            // extrapolated). `set_clock_transition`-less SDCs rely on this for every
            // clock pin, so a missing slew falls back to 0 rather than skipping.
            let slew = cx.pin_slews[trigger.0 as usize];
            let slew_ns = if pol == RISE {
                slew.rise_ns
            } else {
                slew.fall_ns
            };
            let slew_ns = if slew_ns.is_finite() {
                slew_ns
            } else {
                cx.diagnostics.internal_power_without_slew += 1;
                0.0
            };
            // Table values are joules per trigger transition (pJ at parse time).
            let energy_j = eval_nldm_delay_or_slew(table, slew_ns, load_pf) as f64;
            groups[group].internal_w += energy_j * (own_eff * 0.5) * share * weight / cx.duration_s;
        }
    }
}

/// Estimates power for `design` over the VCD window.
///
/// `pin_slews` comes from [`crate::sta::sta_pin_slews`] on the same design/lib/SDC
/// and parasitic mode; internal-power lookups need the operating-point slew.
pub fn estimate_power(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
    vcd: &VcdActivity,
    pin_slews: &[PinSlew],
) -> PowerReport {
    let mut cx = PowerCx {
        design,
        lib,
        net_activity: Vec::with_capacity(design.num_nets()),
        net_load_pf: vec![[0.0; 2]; design.num_nets()],
        pin_slews,
        duration_s: vcd.duration_s,
        diagnostics: PowerDiagnostics::default(),
    };
    // Per-net VCD activity and load caps.
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let name = design.strings.resolve(design.net_names[n]);
        let act = vcd.signals.get(name).copied();
        if design.net_driver(net).is_some() && act.is_none() {
            cx.diagnostics.driven_nets_without_activity += 1;
        }
        cx.net_activity.push(act);
        for pol in [RISE, FALL] {
            cx.net_load_pf[n][pol] = net_load_pf(design, lib, sdc, parasitics, net, pol);
        }
    }

    let mut groups: [PowerGroup; 3] = std::array::from_fn(|i| PowerGroup {
        name: GROUP_NAMES[i].to_string(),
        ..Default::default()
    });

    // Leakage + internal power, per cell.
    for cell_idx in 0..design.num_cells() {
        let cell = CellId(cell_idx as u32);
        let Some(lib_cell_id) = design.cell_lib_ids[cell_idx] else {
            cx.diagnostics.cells_without_lib_cell += 1;
            continue;
        };
        let lib_cell = &lib.cells[lib_cell_id.0 as usize];
        let g = group_of(lib_cell);
        // Leakage: `leakage_power` `when` states are weighted by the marginal
        // probability each state holds — the measured OpenSTA rule
        // (docs/results.md N7). Probability mass no state covers falls back to
        // `cell_leakage_power` (or the group declaring no `when`).
        let flat = lib_cell
            .leakage_power_w
            .unwrap_or(lib.default_cell_leakage_power_w) as f64;
        if lib_cell.leakage_states.is_empty() {
            groups[g].leakage_w += flat;
        } else {
            let mut base = flat;
            let mut covered = 0.0f64;
            let mut weighted = 0.0f64;
            for st in &lib_cell.leakage_states {
                match st.when {
                    None => base = st.power_w as f64,
                    Some(sym) => match eval_when(&cx, cell, sym) {
                        Some(p) => {
                            weighted += p * st.power_w as f64;
                            covered += p;
                        }
                        None => cx.diagnostics.conditional_power_blocks += 1,
                    },
                }
            }
            groups[g].leakage_w += weighted + (1.0 - covered).max(0.0) * base;
        }

        for &pin in design.cell_pins(cell) {
            let Some(lp_id) = design.pin_lib_pins[pin.0 as usize] else {
                continue;
            };
            let lib_pin = &lib.pins[lp_id.0 as usize];
            if lib_pin.internal_power.is_empty() {
                continue;
            }
            pin_internal_power(&mut cx, cell, pin, lib_pin, g, &mut groups);
        }
    }

    // Switching power, per cell-driven net, attributed to the driver's group.
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let Some(driver) = design.net_driver(net) else {
            continue;
        };
        if cx.net_activity(net).eff_toggles() <= 0.0 {
            continue;
        }
        let Some(cell) = design.pin_cells[driver.0 as usize] else {
            cx.diagnostics.active_nets_without_cell_driver += 1;
            continue;
        };
        if !matches!(design.pin_dirs[driver.0 as usize], PinDir::Output) {
            continue;
        }
        let Some(lib_cell_id) = design.cell_lib_ids[cell.0 as usize] else {
            continue;
        };
        let g = group_of(&lib.cells[lib_cell_id.0 as usize]);
        groups[g].switching_w += cx.switching_w(net, driver, lib.nominal_voltage_v);
    }

    let mut internal_w = 0.0;
    let mut switching_w = 0.0;
    let mut leakage_w = 0.0;
    for g in &groups {
        internal_w += g.internal_w;
        switching_w += g.switching_w;
        leakage_w += g.leakage_w;
    }
    PowerReport {
        duration_s: vcd.duration_s,
        voltage_v: lib.nominal_voltage_v,
        internal_w,
        switching_w,
        leakage_w,
        total_w: internal_w + switching_w + leakage_w,
        groups: groups.to_vec(),
        diagnostics: cx.diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, require, sky130_hd};

    #[test]
    fn group_of_classifies_by_cell_kind() {
        let mut cell = LibCell {
            name: crate::interner::SymbolId(0),
            area: 0.0,
            is_sequential: false,
            is_latch: false,
            pin_offset: 0,
            pin_count: 0,
            arc_offset: 0,
            arc_count: 0,
            leakage_power_w: None,
            leakage_states: Vec::new(),
        };
        assert_eq!(group_of(&cell), CLOCK); // arc-less filler
        cell.arc_count = 1;
        assert_eq!(group_of(&cell), COMB);
        cell.is_sequential = true;
        assert_eq!(group_of(&cell), SEQ);
        cell.is_sequential = false;
        cell.is_latch = true;
        assert_eq!(group_of(&cell), SEQ);
    }

    /// `grus-eda power --saif mb.saif` vs the OpenROAD `read_saif` +
    /// `report_power` golden (`mb.saif.reference.json`, emitted by the
    /// committed `generate_saif_ref.sh` inside LibreLane 3.0.16).
    /// POWER_SAIF per-field bounds: rel/internal 2%, switching 5% (the
    /// Sequential-group ~3.7% residual is the same driver-pin-cap delta the
    /// VCD path carries — root cause named in docs/results.md), total 1%;
    /// leakage uses a 1.5e-12 W abs floor because the Combinational term is a
    /// state-dependent-vs-flat difference (~23% of a ~5e-12 W quantity).
    #[test]
    fn test_saif_power_mb_matches_opensta() {
        let hd = sky130_hd();
        let lib_path = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef_path = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let dir = design_dir("microbench");
        let def_path = require(dir.join("mb.def"));
        let sdc_path = require(dir.join("mb.sdc"));
        let saif_path = require(dir.join("mb.saif"));
        let vcd_path = require(dir.join("mb.vcd"));
        let ref_path = require(dir.join("mb.saif.reference.json"));

        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut interner = crate::interner::StringInterner::new();
        let (tech_db, _report) =
            crate::readers::load_tech_db(&tlef_path, &lef_path, &mut interner)
                .expect("tech db");
        let design = crate::readers::parse_def(&def_path, &tech_db, Some(&lib)).expect("def");
        let sdc = crate::sta::load_sdc(&sdc_path).expect("sdc");
        let pin_slews =
            crate::sta::sta_pin_slews(&design, &lib, &sdc, None).expect("pin slews");

        let saif = crate::readers::load_saif(&saif_path).expect("saif");
        let (activity, diag) = saif.resolve(&design, None).expect("resolve");
        let mut report = estimate_power(&design, &lib, &sdc, None, &activity, &pin_slews);
        report.diagnostics.saif_unmatched_scopes = diag.unmatched_scopes;
        report.diagnostics.saif_unmatched_signals = diag.unmatched_signals;

        // The fixture carries one ghost scope so the unmatched counters are
        // exercised end-to-end; OpenSTA drops it silently (docs/results.md).
        assert_eq!(diag.unmatched_scopes, 1);
        assert_eq!(diag.unmatched_signals, 0);

        let ref_json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&ref_path).expect("reference json"),
        )
        .expect("parse reference json");
        let golden = &ref_json["power_saif"];
        let check = |label: &str, ours: f64, reference: f64, rel: f64, abs_floor: f64| {
            let bound = (reference.abs() * rel).max(abs_floor);
            assert!(
                (ours - reference).abs() <= bound,
                "{label}: ours {ours:e} vs opensta {reference:e} (|diff| {:.3e} > bound {bound:.3e})",
                (ours - reference).abs()
            );
        };
        let group_of_name = |name: &str| {
            report
                .groups
                .iter()
                .find(|g| g.name == name)
                .unwrap_or_else(|| panic!("group {name} missing"))
        };
        for name in ["Sequential", "Combinational", "Clock"] {
            let g = group_of_name(name);
            let r = &golden[name];
            check(
                &format!("{name} internal"),
                g.internal_w,
                r["internal_w"].as_f64().unwrap(),
                0.02,
                0.0,
            );
            check(
                &format!("{name} switching"),
                g.switching_w,
                r["switching_w"].as_f64().unwrap(),
                0.05,
                0.0,
            );
            check(
                &format!("{name} leakage"),
                g.leakage_w,
                r["leakage_w"].as_f64().unwrap(),
                0.25,
                1.5e-12,
            );
        }
        let t = &golden["total"];
        check("total internal", report.internal_w, t["internal_w"].as_f64().unwrap(), 0.02, 0.0);
        check(
            "total switching",
            report.switching_w,
            t["switching_w"].as_f64().unwrap(),
            0.05,
            0.0,
        );
        check(
            "total leakage",
            report.leakage_w,
            t["leakage_w"].as_f64().unwrap(),
            0.25,
            1.5e-12,
        );
        check("total", report.total_w, t["total_w"].as_f64().unwrap(), 0.01, 0.0);

        // Per-net comparison against the VCD annotation (AGENTS §7 rule 5):
        // every net reconstructs its VCD edge counts except clk, whose TC=199
        // covers the truncated [10,1990] SAIF window — the #2000 fall is
        // outside it, so resolved clk differs by exactly one fall.
        let vcd = crate::readers::load_vcd(&vcd_path).expect("vcd");
        for (name, sa) in &activity.signals {
            let va = vcd
                .signals
                .get(name.as_str())
                .unwrap_or_else(|| panic!("net {name} missing from VCD activity"));
            let expected = if name == "clk" {
                (va.rise, va.fall - 1)
            } else {
                (va.rise, va.fall)
            };
            assert_eq!((sa.rise, sa.fall), expected, "net {name}");
        }
        assert_eq!(
            (activity.signals["clk"].rise, activity.signals["clk"].fall),
            (100, 99)
        );

        // The reports differ only by clk's one dropped fall over a window
        // 10ns shorter; the measured delta is recorded in docs/results.md.
        let vcd_report = estimate_power(&design, &lib, &sdc, None, &vcd, &pin_slews);
        let rel = (report.total_w - vcd_report.total_w).abs() / vcd_report.total_w;
        assert!(rel < 0.01, "SAIF-vs-VCD total delta {rel:e} exceeds 1%");
    }

    /// `mbw` — a single `sky130_fd_sc_hd__ebufn_1` whose `Z` pin carries an
    /// unconditional `internal_power` block plus `when : "A"` / `when : "!A"`
    /// blocks related to `TE_B`. Golden emitted by the committed
    /// `generate_when_ref.sh` inside LibreLane 3.0.16 (`mbw.reference.json`).
    /// The measured OpenSTA model this exercises: marginal `P(when)` products,
    /// related-pin activity apportioning, half-weight x/z edges, and
    /// state-weighted `leakage_power` — all in docs/results.md N7.
    #[test]
    fn test_when_power_mbw_matches_opensta() {
        let hd = sky130_hd();
        let lib_path = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef_path = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let dir = design_dir("microbench");
        let def_path = require(dir.join("mbw.def"));
        let sdc_path = require(dir.join("mbw.sdc"));
        let vcd_path = require(dir.join("mbw.vcd"));
        let saif_path = require(dir.join("mbw.saif"));
        let ref_path = require(dir.join("mbw.reference.json"));

        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut interner = crate::interner::StringInterner::new();
        let (tech_db, _report) =
            crate::readers::load_tech_db(&tlef_path, &lef_path, &mut interner).expect("tech db");
        let design = crate::readers::parse_def(&def_path, &tech_db, Some(&lib)).expect("def");
        let sdc = crate::sta::load_sdc(&sdc_path).expect("sdc");
        let pin_slews = crate::sta::sta_pin_slews(&design, &lib, &sdc, None).expect("pin slews");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("reference json"))
                .expect("parse reference json");
        let check = |label: &str, ours: f64, reference: f64, rel: f64, abs_floor: f64| {
            let bound = (reference.abs() * rel).max(abs_floor);
            assert!(
                (ours - reference).abs() <= bound,
                "{label}: ours {ours:e} vs opensta {reference:e} (|diff| {:.3e} > bound {bound:.3e})",
                (ours - reference).abs()
            );
        };
        let check_groups = |golden: &serde_json::Value, report: &PowerReport, tag: &str| {
            for name in ["Sequential", "Combinational", "Clock"] {
                let g = report
                    .groups
                    .iter()
                    .find(|g| g.name == name)
                    .unwrap_or_else(|| panic!("{tag} group {name} missing"));
                let r = &golden[name];
                check(
                    &format!("{tag} {name} internal"),
                    g.internal_w,
                    r["internal_w"].as_f64().unwrap(),
                    0.001,
                    0.0,
                );
                check(
                    &format!("{tag} {name} switching"),
                    g.switching_w,
                    r["switching_w"].as_f64().unwrap(),
                    0.001,
                    0.0,
                );
                check(
                    &format!("{tag} {name} leakage"),
                    g.leakage_w,
                    r["leakage_w"].as_f64().unwrap(),
                    0.01,
                    1.5e-12,
                );
            }
            let t = &golden["total"];
            check(
                &format!("{tag} total internal"),
                report.internal_w,
                t["internal_w"].as_f64().unwrap(),
                0.001,
                0.0,
            );
            check(
                &format!("{tag} total switching"),
                report.switching_w,
                t["switching_w"].as_f64().unwrap(),
                0.001,
                0.0,
            );
            check(
                &format!("{tag} total leakage"),
                report.leakage_w,
                t["leakage_w"].as_f64().unwrap(),
                0.01,
                1.5e-12,
            );
            check(
                &format!("{tag} total"),
                report.total_w,
                t["total_w"].as_f64().unwrap(),
                0.001,
                0.0,
            );
        };

        // VCD path: every `when` form in the fixture is a plain literal
        // (`"A"`/`"!A"`), so nothing may remain counted.
        let vcd = crate::readers::load_vcd(&vcd_path).expect("vcd");
        let report = estimate_power(&design, &lib, &sdc, None, &vcd, &pin_slews);
        assert_eq!(report.diagnostics.conditional_power_blocks, 0);
        check_groups(&ref_json["power_vcd"], &report, "vcd");

        // The marginal probabilities the `when` weights consume: `a` is 1 for
        // 1100 of 2000 ns; `z`'s 350 ns in z lands in `other_s`, dropping its
        // measured duty to 0.475 — the denominator includes x/z time.
        let a = vcd.signals.get("a").expect("a");
        assert!((a.prob_one().unwrap() - 0.55).abs() < 1e-9);
        let teb = vcd.signals.get("teb").expect("teb");
        assert!((teb.prob_one().unwrap() - 0.175).abs() < 1e-9);
        let z = vcd.signals.get("z").expect("z");
        assert_eq!((z.rise, z.fall, z.xtrans), (3, 3, 4));
        assert!((z.prob_one().unwrap() - 0.475).abs() < 1e-9);

        // SAIF path: same marginal `when` model over T0/T1; the internal and
        // switching columns differ from VCD because the SAIF fixture encodes
        // clean toggles only (TC) — not because OpenSTA uses joint waveform
        // correlation (docs/results.md N7).
        let saif = crate::readers::load_saif(&saif_path).expect("saif");
        let (activity, diag) = saif.resolve(&design, Some("tb/dut")).expect("resolve");
        let mut report = estimate_power(&design, &lib, &sdc, None, &activity, &pin_slews);
        report.diagnostics.saif_unmatched_scopes = diag.unmatched_scopes;
        report.diagnostics.saif_unmatched_signals = diag.unmatched_signals;
        assert_eq!(report.diagnostics.conditional_power_blocks, 0);
        assert_eq!((diag.unmatched_scopes, diag.unmatched_signals), (0, 0));
        check_groups(&ref_json["power_saif"], &report, "saif");
    }
}
