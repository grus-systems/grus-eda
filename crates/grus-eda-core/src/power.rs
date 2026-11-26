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
//! Activity is resolved per net by [`resolve_activity`] in the measured seed
//! order (constant → clock → global → `user` `-pins`/`-input_ports` →
//! annotation → input →
//! default, then sequential Q=D copy and combinational propagation in one
//! fixed point, leftovers `unknown`; docs/results.md 2026-11-21 N7): a dump
//! annotates nets through their driver pins, every
//! other net takes its class default, combinational cell outputs derive
//! the M1 local-marginal fill, and a sequential output copies its data
//! pin's net. Slews come from
//! [`crate::sta::sta_pin_slews`]. Edges involving x/z count
//! half a transition each, and per-direction lookups consume half the effective
//! count — both measured OpenSTA behaviors (docs/results.md N7). `when` forms
//! outside the literal-conjunction grammar, naming no sibling pin, or whose
//! literal nets carry no measured state are skipped and counted in the
//! diagnostics.

use crate::activity::{ActivityOrigin, ActivityOverrides, ResolvedActivity, resolve_activity};
use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinDir, PinId};
use crate::lib_db::{LibCell, LibDb, LibPin, parse_when_expr};
use crate::readers::vcd::VcdActivity;
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
    /// Driven nets with no measured, seeded, or derived activity source —
    /// resolved origin `Unknown` (post-resolution `Default` is impossible).
    /// Such a net's `prob_one` is `Some(0.0)`, so `when` literals on it
    /// evaluate at weight 0 rather than counting inexpressible.
    pub driven_nets_without_activity: usize,
    /// Nets per resolved activity origin — the `report_activity_annotation`
    /// split shape. OpenSTA's split is per pin; ours is per net (recorded
    /// difference).
    pub activity_origin_counts: std::collections::BTreeMap<String, usize>,
    /// Nets carrying resolved activity but no cell-output driver (input ports,
    /// undriven); their switching power is not attributed anywhere, as in
    /// OpenSTA.
    pub active_nets_without_cell_driver: usize,
    /// `internal_power` triggers whose pin slew STA could not reach.
    pub internal_power_without_slew: usize,
    /// `internal_power` `related_pin` names that match no sibling pin of the cell.
    pub unresolved_related_pin: usize,
    /// Dump `pin_signals` entries that named no net driver — a mirror of
    /// `ActivityResolution::dead_annotation_entries` so the CLI/JSON
    /// report surfaces it (the counters stay single-sourced on the
    /// resolution).
    pub dead_annotation_entries: usize,
    /// `-pins` overrides whose pin is not a net driver — mirrors
    /// `ActivityResolution::dead_user_entries`. Always 0 through the CLI
    /// (no `--pins` flag); surfaced so an API-driven report keeps the
    /// count visible.
    pub dead_user_entries: usize,
    /// `internal_power`/`leakage_power` `when` blocks skipped because the
    /// expression is outside the literal-conjunction grammar, names no sibling
    /// pin, or a literal net carries no measured state. Seed probabilities
    /// make `when` literals on unannotated nets expressible, so only a
    /// never-defined `Annotated` dump can still land here.
    pub conditional_power_blocks: usize,
    /// SAIF `INSTANCE` scopes matching no design cell (0 on the VCD path;
    /// merged in from `SaifDiagnostics` by the caller).
    pub saif_unmatched_scopes: usize,
    /// SAIF signal names matching no design net, port or pin (0 on the VCD path).
    pub saif_unmatched_signals: usize,
}

/// Total power attribution for one design cell.
#[derive(Clone, Debug, Default, Serialize)]
pub struct InstancePower {
    /// DEF component name, resolved at estimate time.
    pub instance: String,
    pub internal_w: f64,
    pub switching_w: f64,
    pub leakage_w: f64,
    pub total_w: f64,
}

/// Estimated power over the report window, grouped like `report_power`.
#[derive(Clone, Debug, Serialize)]
pub struct PowerReport {
    /// Report window in seconds: the annotation window (`vcd.duration_s`)
    /// when a dump is loaded and positive, else `1.0` — absolute-density
    /// mode, where seeded counts equal rates numerically (`duration_s`
    /// cancels in `switching_w` either way).
    pub duration_s: f64,
    /// `nom_voltage` the switching `V^2` term used.
    pub voltage_v: f32,
    pub internal_w: f64,
    pub switching_w: f64,
    pub leakage_w: f64,
    pub total_w: f64,
    pub groups: Vec<PowerGroup>,
    /// Parallel to `DesignDb` cell order: `instances[cell.0]` is the
    /// power used for `CellId`'s sinks. Includes zero-valued entries for
    /// cells without a Liberty cell so indexing never shifts.
    pub instances: Vec<InstancePower>,
    pub diagnostics: PowerDiagnostics,
}

/// Group slots, in `report_power` order.
const SEQ: usize = 0;
const COMB: usize = 1;
const CLOCK: usize = 2;
const GROUP_NAMES: [&str; 3] = ["Sequential", "Combinational", "Clock"];

/// Sequential cells report as Sequential; a non-sequential cell with a pin on a
/// `Clock`-origin net is a clock-network cell; everything else with a Liberty
/// cell is Combinational — conb's leakage sits in Combinational on `mba`, not
/// Clock (docs/results.md 2026-11-21 N7). Cells on clock-tree nets deeper than
/// the source net are `Default`-origin in AP1, so mid-tree cells land in
/// Combinational until clock-network resolution lands (named residual).
fn group_of(touches_clock_net: bool, lib_cell: &LibCell) -> usize {
    if lib_cell.is_sequential || lib_cell.is_latch {
        SEQ
    } else if touches_clock_net {
        CLOCK
    } else {
        COMB
    }
}

struct PowerCx<'a> {
    design: &'a DesignDb,
    lib: &'a LibDb,
    /// Resolved activity per net (`resolve_activity` output); every net has
    /// an origin — `Unknown` marks the unresolvable cones.
    net_activity: Vec<ResolvedActivity>,
    /// Total load capacitance per net per output transition, pF: wire cap plus
    /// sink pin caps (and SDC output loads), the same quantity STA's `net_load`
    /// computes for delay lookups.
    net_load_pf: Vec<[f32; 2]>,
    pin_slews: &'a [PinSlew],
    duration_s: f64,
    diagnostics: PowerDiagnostics,
}

impl PowerCx<'_> {
    fn net_activity(&self, net: NetId) -> ResolvedActivity {
        self.net_activity[net.0 as usize]
    }

    /// Marginal `P(net = 1)`; `None` only for an `Annotated` net whose dump
    /// never defined the state — every seeded origin carries a probability.
    fn net_prob_one(&self, net: NetId) -> Option<f64> {
        self.net_activity[net.0 as usize].prob_one
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
        let eff = self.net_activity(net).eff_toggles;
        if eff <= 0.0 || self.duration_s <= 0.0 {
            return 0.0;
        }
        let load = self.net_load_pf[net.0 as usize];
        let c = (load[RISE] + self.driver_cap(driver, RISE)) as f64
            + (load[FALL] + self.driver_cap(driver, FALL)) as f64;
        let energy_pf = 0.5 * voltage_v as f64 * voltage_v as f64 * (eff * 0.5) * c;
        energy_pf * 1e-12 / self.duration_s
    }

    /// A cell is on the clock network when one of its pins sits on a
    /// `Clock`-origin (SDC clock source) net.
    fn cell_touches_clock_net(&self, cell: CellId) -> bool {
        self.design.cell_pins(cell).iter().any(|&p| {
            self.design.pin_nets[p.0 as usize]
                .map(|n| self.net_activity(n).origin == ActivityOrigin::Clock)
                .unwrap_or(false)
        })
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

/// Marginal probability a `when` expression holds: the product over literals
/// of each literal pin's net state probability — OpenSTA's independence model
/// (no joint distributions exist in its activity model; docs/results.md N7).
/// `None` = inexpressible: grammar outside the literal conjunction, a literal
/// naming no sibling pin, a floating pin, or a net with no measured state.
fn eval_when(cx: &PowerCx, cell: CellId, when: crate::interner::SymbolId) -> Option<f64> {
    let lits = parse_when_expr(cx.lib.strings.resolve(when), &cx.lib.strings)?;
    let mut p = 1.0f64;
    for lit in lits {
        let pin = cx.design.pin_by_lib_name(cx.lib, cell, lit.pin)?;
        let net = cx.design.pin_nets[pin.0 as usize]?;
        let p1 = cx.net_prob_one(net)?;
        p *= if lit.inverted { 1.0 - p1 } else { p1 };
    }
    Some(p)
}

/// Effective toggles on the net `pin` connects to (0 when floating).
fn net_eff_toggles(cx: &PowerCx, pin: PinId) -> f64 {
    cx.design.pin_nets[pin.0 as usize]
        .map(|n| cx.net_activity(n).eff_toggles)
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
    let own_eff = cx.net_activity(own_net).eff_toggles;
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
            Some(sym) => match cx.design.pin_by_lib_name(cx.lib, cell, sym) {
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

/// Estimates power for `design` over the report window.
///
/// `pin_slews` comes from [`crate::sta::sta_pin_slews`] on the same design/lib/SDC
/// and parasitic mode; internal-power lookups need the operating-point slew.
/// `overrides` carries the `-global`/`-input`-style seeds `resolve_activity`
/// applies (API level; no CLI flags in AP1).
pub fn estimate_power(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
    vcd: &VcdActivity,
    pin_slews: &[PinSlew],
    overrides: &ActivityOverrides,
) -> PowerReport {
    let t_resolve = crate::timing::phase("power.activity.resolve");
    let resolution = resolve_activity(design, lib, sdc, vcd, overrides);
    drop(t_resolve);
    let window_s = resolution.window_s;
    let mut cx = PowerCx {
        design,
        lib,
        net_activity: resolution.nets,
        net_load_pf: vec![[0.0; 2]; design.num_nets()],
        pin_slews,
        duration_s: window_s,
        diagnostics: PowerDiagnostics {
            activity_origin_counts: resolution.origin_counts,
            dead_annotation_entries: resolution.dead_annotation_entries,
            dead_user_entries: resolution.dead_user_entries,
            ..Default::default()
        },
    };
    // Per-net load caps; driven nets whose origin is `Unknown` have no
    // measured, seeded, or derived source.
    let t_loads = crate::timing::phase("power.net_loads");
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        if design.net_driver(net).is_some()
            && cx.net_activity(net).origin == ActivityOrigin::Unknown
        {
            cx.diagnostics.driven_nets_without_activity += 1;
        }
        for pol in [RISE, FALL] {
            cx.net_load_pf[n][pol] = net_load_pf(design, lib, sdc, parasitics, net, pol);
        }
    }
    drop(t_loads);

    let mut groups: [PowerGroup; 3] = std::array::from_fn(|i| PowerGroup {
        name: GROUP_NAMES[i].to_string(),
        ..Default::default()
    });
    let mut instances: Vec<InstancePower> = (0..design.num_cells())
        .map(|c| InstancePower {
            instance: design.strings.resolve(design.cell_names[c]).to_string(),
            ..Default::default()
        })
        .collect();

    // Leakage + internal power, per cell.
    let t_cells = crate::timing::phase("power.cells");
    for cell_idx in 0..design.num_cells() {
        let cell = CellId(cell_idx as u32);
        let Some(lib_cell_id) = design.cell_lib_ids[cell_idx] else {
            cx.diagnostics.cells_without_lib_cell += 1;
            continue;
        };
        let lib_cell = &lib.cells[lib_cell_id.0 as usize];
        let g = group_of(cx.cell_touches_clock_net(cell), lib_cell);
        // Leakage: `leakage_power` `when` states are weighted by the marginal
        // probability each state holds — the measured OpenSTA rule
        // (docs/results.md N7). Probability mass no state covers falls back to
        // `cell_leakage_power` (or the group declaring no `when`).
        let flat = lib_cell
            .leakage_power_w
            .unwrap_or(lib.default_cell_leakage_power_w) as f64;
        let leak = if lib_cell.leakage_states.is_empty() {
            flat
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
            weighted + (1.0 - covered).max(0.0) * base
        };
        groups[g].leakage_w += leak;
        instances[cell_idx].leakage_w += leak;

        for &pin in design.cell_pins(cell) {
            let Some(lp_id) = design.pin_lib_pins[pin.0 as usize] else {
                continue;
            };
            let lib_pin = &lib.pins[lp_id.0 as usize];
            if lib_pin.internal_power.is_empty() {
                continue;
            }
            let before = groups[g].internal_w;
            pin_internal_power(&mut cx, cell, pin, lib_pin, g, &mut groups);
            instances[cell_idx].internal_w += groups[g].internal_w - before;
        }
    }
    drop(t_cells);

    // Switching power, per cell-driven net, attributed to the driver's group.
    let t_sw = crate::timing::phase("power.switching");
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let Some(driver) = design.net_driver(net) else {
            continue;
        };
        if cx.net_activity(net).eff_toggles <= 0.0 {
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
        let g = group_of(
            cx.cell_touches_clock_net(cell),
            &lib.cells[lib_cell_id.0 as usize],
        );
        let sw = cx.switching_w(net, driver, lib.nominal_voltage_v);
        groups[g].switching_w += sw;
        instances[cell.0 as usize].switching_w += sw;
    }
    drop(t_sw);

    for ip in &mut instances {
        ip.total_w = ip.internal_w + ip.switching_w + ip.leakage_w;
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
        duration_s: window_s,
        voltage_v: lib.nominal_voltage_v,
        internal_w,
        switching_w,
        leakage_w,
        total_w: internal_w + switching_w + leakage_w,
        groups: groups.to_vec(),
        instances,
        diagnostics: cx.diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::ActivitySetting;
    use crate::test_support::{design_dir, require, sky130_hd};

    #[test]
    fn group_of_classifies_by_cell_kind() {
        let mut cell = LibCell {
            name: crate::interner::SymbolId(0),
            area: 0.0,
            is_sequential: false,
            is_latch: false,
            seq: None,
            pin_offset: 0,
            pin_count: 0,
            arc_offset: 0,
            arc_count: 0,
            leakage_power_w: None,
            leakage_states: Vec::new(),
        };
        // Arc-less and not on a clock net: Combinational, per the mba
        // measurement (conb's leakage sits in OpenSTA's Combinational group).
        assert_eq!(group_of(false, &cell), COMB);
        assert_eq!(group_of(true, &cell), CLOCK);
        cell.arc_count = 1;
        assert_eq!(group_of(false, &cell), COMB);
        cell.is_sequential = true;
        assert_eq!(group_of(false, &cell), SEQ);
        cell.is_sequential = false;
        cell.is_latch = true;
        assert_eq!(group_of(false, &cell), SEQ);
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
        let mut report = estimate_power(
            &design,
            &lib,
            &sdc,
            None,
            &activity,
            &pin_slews,
            &ActivityOverrides::default(),
        );
        report.diagnostics.saif_unmatched_scopes = diag.unmatched_scopes;
        report.diagnostics.saif_unmatched_signals = diag.unmatched_signals;

        // The fixture carries one ghost scope so the unmatched counters are
        // exercised end-to-end; OpenSTA drops it silently (docs/results.md).
        assert_eq!(diag.unmatched_scopes, 1);
        assert_eq!(diag.unmatched_signals, 0);

        // Per-instance decomposition: one entry per DEF cell in order,
        // component fields sum to `total_w`, and the instance total is the
        // aggregate `total_w` (the N7-to-PDN current feed, E8).
        assert_eq!(report.instances.len(), design.num_cells());
        for (c, ip) in report.instances.iter().enumerate() {
            assert_eq!(
                ip.instance,
                design.strings.resolve(design.cell_names[c]),
                "instance name at cell {c}"
            );
            assert_eq!(
                ip.total_w,
                ip.internal_w + ip.switching_w + ip.leakage_w,
                "instance {c} total_w vs components"
            );
        }
        let inst_sum: f64 = report.instances.iter().map(|ip| ip.total_w).sum();
        assert!(
            (inst_sum - report.total_w).abs() <= 1e-15,
            "sum(instances.total_w) {inst_sum:e} vs report.total_w {:e}",
            report.total_w
        );

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
        let vcd_report = estimate_power(
            &design,
            &lib,
            &sdc,
            None,
            &vcd,
            &pin_slews,
            &ActivityOverrides::default(),
        );
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
        let report = estimate_power(
            &design,
            &lib,
            &sdc,
            None,
            &vcd,
            &pin_slews,
            &ActivityOverrides::default(),
        );
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
        let mut report = estimate_power(
            &design,
            &lib,
            &sdc,
            None,
            &activity,
            &pin_slews,
            &ActivityOverrides::default(),
        );
        report.diagnostics.saif_unmatched_scopes = diag.unmatched_scopes;
        report.diagnostics.saif_unmatched_signals = diag.unmatched_signals;
        assert_eq!(report.diagnostics.conditional_power_blocks, 0);
        assert_eq!((diag.unmatched_scopes, diag.unmatched_signals), (0, 0));
        check_groups(&ref_json["power_saif"], &report, "saif");
    }

    /// AP3 activity-propagation golden: `resolve_activity` on `mba` must
    /// reproduce OpenSTA's per-pin `{density, prob, source}` in all 12
    /// committed sessions of `mba.activity.reference.json` (emitted by the
    /// committed `generate_activity_ref.sh` inside LibreLane 3.0.16).
    /// Every session asserts all 43 pins strict under the source mapping
    /// rules below — `EXPECTED_DIVERGENT` is empty: net `q`'s sequential
    /// Q=D copy (`u_df/Q`, `qd`) lands `propagated` verbatim (tracking
    /// `a`, including `s_ports_a2x`'s 6e6/0.55 density doubling and
    /// `s_xz`'s x-degraded 0.40), the 11 `unknown`-cone pins on nets
    /// `n_q0`/`n_q0n`/`n_q1`/`n_c1d` compare strict at `{0, 0}` `unknown`,
    /// and `s_input_ports`'s `a`/`b` compare strict at the native `user`
    /// origin (AP4 — no `annotated` collapse).
    /// `s_full`'s output port `y` stays the one per-pin residual
    /// (`PER_PIN_RESIDUAL` — per-pin vs per-net annotation, AP4 scope).
    #[test]
    fn test_activity_defaults_mba_matches_opensta() {
        let hd = sky130_hd();
        let lib_path = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef_path = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let dir = design_dir("microbench");
        let def_path = require(dir.join("mba.def"));
        let sdc_path = require(dir.join("mba.sdc"));
        let ref_path = require(dir.join("mba.activity.reference.json"));

        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut interner = crate::interner::StringInterner::new();
        let (tech_db, _report) =
            crate::readers::load_tech_db(&tlef_path, &lef_path, &mut interner).expect("tech db");
        let design = crate::readers::parse_def(&def_path, &tech_db, Some(&lib)).expect("def");
        let sdc = crate::sta::load_sdc(&sdc_path).expect("sdc");
        let pin_slews = crate::sta::sta_pin_slews(&design, &lib, &sdc, None).expect("pin slews");
        // `s_p40` runs the period-40 SDC the generator writes inline to
        // `/tmp/mba40.sdc` (committed here verbatim) — f_clk 2.5e7, input
        // seed 2.5e6, clock 5e7.
        let sdc_p40 = crate::sta::load_sdc(&require(dir.join("mba_p40.sdc"))).expect("sdc p40");
        let pin_slews_p40 =
            crate::sta::sta_pin_slews(&design, &lib, &sdc_p40, None).expect("pin slews p40");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("reference json"))
                .expect("parse reference json");

        // Golden keys are pins; our model resolves per net. Port pins map by
        // port name, cell pins by `cell/pin`. `pin_obj` keeps the PinId for
        // the driver check the source mapping rules need.
        let mut pin_net: std::collections::HashMap<String, NetId> =
            std::collections::HashMap::new();
        let mut pin_obj: std::collections::HashMap<String, PinId> =
            std::collections::HashMap::new();
        for &p in &design.io_pins {
            if let Some(n) = design.pin_nets[p.0 as usize] {
                let name = design.strings.resolve(design.pin_names[p.0 as usize]).to_string();
                pin_net.insert(name.clone(), n);
                pin_obj.insert(name, p);
            }
        }
        for c in 0..design.num_cells() {
            let cell = CellId(c as u32);
            let cname = design.strings.resolve(design.cell_names[c]).to_string();
            for &p in design.cell_pins(cell) {
                if let Some(n) = design.pin_nets[p.0 as usize] {
                    let pname = design.strings.resolve(design.pin_names[p.0 as usize]);
                    let key = format!("{cname}/{pname}");
                    pin_net.insert(key.clone(), n);
                    pin_obj.insert(key, p);
                }
            }
        }

        // Golden sources our model reproduces verbatim; `vcd`/`saif`
        // collapse onto the `annotated` label — but only when the pin
        // resolves its net (the pin is the net's driver). A golden
        // annotated pin that is not its net's driver is a per-pin
        // annotation residual our per-net model cannot represent.
        // `user` is a native origin since AP4 — no collapse.
        fn expected_label(src: &str) -> &str {
            match src {
                "vcd" | "saif" => "annotated",
                other => other,
            }
        }
        // AP3 closed the 13-pin residual: net `q`'s sequential Q=D copy
        // reads `propagated` and the 11 `unknown`-cone pins compare
        // strict at `{0, 0}` — a new divergent pin fails loudly.
        static EXPECTED_DIVERGENT: &[&str] = &[];
        // Golden annotated pins that are not their net's resolving pin —
        // `s_full`'s output port `y` holds `vcd 3e6/0.25` while net `y`'s
        // driver `u_xor/X` reads `propagated 6e6/0.47`; the per-pin vs
        // per-net residual is AP4 scope.
        static PER_PIN_RESIDUAL: &[(&str, &str)] = &[("s_full", "y")];

        let setting = |density_per_s: f64, prob_one: f64| ActivitySetting {
            density_per_s,
            prob_one,
        };
        // (session, overrides, vcd file, sdc file — `None` ⇒ `mba.sdc`).
        let sessions: [(&str, ActivityOverrides, Option<&str>, Option<&str>); 12] = [
            ("s_none", ActivityOverrides::default(), None, None),
            (
                "s_input",
                ActivityOverrides {
                    input: Some(setting(1e7, 0.4)),
                    ..Default::default()
                },
                None,
                None,
            ),
            (
                "s_input_ports",
                ActivityOverrides {
                    input_ports: Some(setting(1e7, 0.4)),
                    ..Default::default()
                },
                None,
                None,
            ),
            (
                "s_global",
                ActivityOverrides {
                    global: Some(setting(1e7, 0.4)),
                    ..Default::default()
                },
                None,
                None,
            ),
            (
                "s_global_density",
                ActivityOverrides {
                    global: Some(setting(3e7, 0.35)),
                    ..Default::default()
                },
                None,
                None,
            ),
            ("s_p40", ActivityOverrides::default(), None, Some("mba_p40.sdc")),
            (
                "s_ports",
                ActivityOverrides::default(),
                Some("mba_ports.vcd"),
                None,
            ),
            (
                "s_ports_a2x",
                ActivityOverrides::default(),
                Some("mba_ports_a2x.vcd"),
                None,
            ),
            (
                "s_partial",
                ActivityOverrides::default(),
                Some("mba_partial.vcd"),
                None,
            ),
            (
                "s_full",
                ActivityOverrides::default(),
                Some("mba_full.vcd"),
                None,
            ),
            ("s_xz", ActivityOverrides::default(), Some("mba_xz.vcd"), None),
            (
                "s_partial2",
                ActivityOverrides::default(),
                Some("mba_partial2.vcd"),
                None,
            ),
        ];

        let mut residual: std::collections::BTreeSet<(&str, &str)> =
            std::collections::BTreeSet::new();
        // `s_partial`'s only difference from `s_ports` is the dead flat
        // `n_and` entry — the resolved per-net vectors must be identical.
        let mut partial_nets: Vec<crate::activity::ResolvedActivity> = Vec::new();
        // `s_input_ports`'s resolved nets feed the post-loop `-pins`
        // equivalence check.
        let mut input_ports_nets: Vec<crate::activity::ResolvedActivity> =
            Vec::new();
        for (sess, overrides, vcd_file, sdc_file) in &sessions {
            let vcd = match vcd_file {
                Some(f) => {
                    crate::readers::load_vcd(&require(dir.join(f))).expect("vcd")
                }
                None => VcdActivity::default(),
            };
            let (sess_sdc, sess_slews) = match *sdc_file {
                Some("mba_p40.sdc") => (&sdc_p40, &pin_slews_p40),
                Some(other) => panic!("{sess}: unmapped sdc {other}"),
                None => (&sdc, &pin_slews),
            };
            let golden = &ref_json[sess]["activity"];
            let resolution = crate::activity::resolve_activity(
                &design, &lib, sess_sdc, &vcd, overrides,
            );
            // Diagnostics: `s_partial`'s flat `n_and` dump entry names
            // no driver pin and lands dead (the other pins in that dump
            // resolve, keeping the session identical to `s_ports`);
            // `s_full` dumps all 18 pins — the 15 non-driver pins (cell
            // inputs plus output port `y`) are dead. Every sky130
            // `function` the bench hits parses and resolves, so
            // `unresolvable_functions` stays 0.
            let expected_dead = match *sess {
                "s_partial" => 1,
                "s_full" => 15,
                _ => 0,
            };
            assert_eq!(
                (
                    resolution.dead_annotation_entries,
                    resolution.unresolvable_functions,
                    resolution.dead_user_entries,
                ),
                (expected_dead, 0, 0),
                "{sess} diagnostics"
            );
            // `s_none` pins the seed-default split; `s_input_ports` pins
            // the `user` split — `a`/`b` move `input` → `user` (−2/+2)
            // while undriven `n_float` keeps `input` (the measured
            // `-input_ports` port-only semantics).
            if let Some(want) = match *sess {
                "s_none" => Some(std::collections::BTreeMap::from([
                    ("clock", 1),
                    ("constant", 4),
                    ("input", 3),
                    ("propagated", 6),
                    ("unknown", 4),
                ])),
                "s_input_ports" => Some(std::collections::BTreeMap::from([
                    ("clock", 1),
                    ("constant", 4),
                    ("input", 1),
                    ("propagated", 6),
                    ("unknown", 4),
                    ("user", 2),
                ])),
                _ => None,
            } {
                let counts: std::collections::BTreeMap<&str, usize> = resolution
                    .origin_counts
                    .iter()
                    .map(|(k, v)| (k.as_str(), *v))
                    .collect();
                assert_eq!(counts, want, "{sess} origin_counts");
            }
            let mut divergent: std::collections::BTreeSet<&str> =
                std::collections::BTreeSet::new();
            for (pin, g) in golden.as_object().unwrap() {
                let net = *pin_net.get(pin.as_str()).unwrap_or_else(|| {
                    panic!("{sess}: golden pin {pin} has no design pin")
                });
                let r = resolution.nets[net.0 as usize];
                let src = g["source"].as_str().unwrap();
                let strict = |label: &str| {
                    let d_golden = g["density"].as_f64().unwrap();
                    let d_ours = r.eff_toggles / resolution.window_s;
                    assert!(
                        (d_ours - d_golden).abs() <= (d_golden.abs() * 1e-9).max(1.0),
                        "{sess} {pin}: density ours {d_ours:e} vs golden {d_golden:e} (origin {})",
                        r.origin.label()
                    );
                    let p_golden = g["prob"].as_f64().unwrap();
                    let p_ours = r
                        .prob_one
                        .unwrap_or_else(|| panic!("{sess} {pin}: prob_one is None"));
                    assert!(
                        (p_ours - p_golden).abs() <= 1e-9,
                        "{sess} {pin}: prob ours {p_ours} vs golden {p_golden} (origin {})",
                        r.origin.label()
                    );
                    assert_eq!(r.origin.label(), label, "{sess} {pin}: source");
                };
                match src {
                    // Strict {0,0} + `unknown` label — the sequential
                    // feedback-cone leftovers converted post-pass.
                    "unknown" => strict("unknown"),
                    // Strict {d,P}; the origin label is `propagated` on
                    // comb-derived nets, `annotated`/`input` where the pin
                    // sits on a seeded net (OpenSTA re-derives per pin).
                    "propagated" => {
                        if matches!(
                            r.origin,
                            ActivityOrigin::Default | ActivityOrigin::Unknown
                        ) {
                            divergent.insert(pin.as_str());
                        } else {
                            let label = r.origin.label();
                            strict(label);
                            assert!(
                                ["propagated", "annotated", "user", "input"]
                                    .contains(&label),
                                "{sess} {pin}: source {label} outside the propagated mapping"
                            );
                        }
                    }
                    "vcd" | "saif" => {
                        let p = pin_obj[pin.as_str()];
                        let resolves_net = design.pin_is_driver[p.0 as usize]
                            && design.pin_nets[p.0 as usize] == Some(net);
                        if resolves_net {
                            strict("annotated");
                        } else {
                            residual.insert((sess, pin.as_str()));
                        }
                    }
                    // `set_power_activity` user overrides — a native
                    // origin since AP4, strict on driver pins (the only
                    // golden `user` pins, `s_input_ports` `a`/`b`, are).
                    "user" => {
                        let p = pin_obj[pin.as_str()];
                        let resolves_net = design.pin_is_driver[p.0 as usize]
                            && design.pin_nets[p.0 as usize] == Some(net);
                        if resolves_net {
                            strict("user");
                        } else {
                            residual.insert((sess, pin.as_str()));
                        }
                    }
                    other => strict(expected_label(other)),
                }
            }
            // AP3: the divergent set is empty on every session — a new
            // one fails loudly.
            let expected: std::collections::BTreeSet<&str> =
                EXPECTED_DIVERGENT.iter().copied().collect();
            assert_eq!(
                divergent, expected,
                "{sess}: the divergent-source pin set moved"
            );

            if *sess == "s_input_ports" {
                input_ports_nets = resolution.nets.clone();
            }
            if *sess == "s_ports" {
                partial_nets = resolution.nets;
            } else if *sess == "s_partial" {
                // Raw `eff_toggles` counts are only comparable because
                // both fixtures record the same window — `mba_partial.vcd`
                // and `mba_ports.vcd` both run to #2000, so
                // `duration_s`/`window_s` is identical. If either
                // fixture's duration ever changes, compare
                // `eff_toggles / window_s` instead.
                assert_eq!(resolution.nets.len(), partial_nets.len());
                for (n, (a, b)) in
                    resolution.nets.iter().zip(&partial_nets).enumerate()
                {
                    assert!(
                        a.prob_one == b.prob_one
                            && a.eff_toggles == b.eff_toggles
                            && a.origin == b.origin,
                        "s_partial net {n} differs from s_ports"
                    );
                }
            }

            // Group columns share the same fill pass through `estimate_power`.
            let report = estimate_power(
                &design,
                &lib,
                sess_sdc,
                None,
                &vcd,
                sess_slews,
                overrides,
            );
            let groups = &ref_json[sess]["groups"];
            let delta = |ours: f64, golden: f64| (ours - golden).abs();
            // Every session asserts at the measured bound (docs/results.md
            // N7): internal/switching <= 0.5% — OpenSTA's switched
            // capacitance appears to carry a small term we do not
            // reproduce; sequential internal and all leakage columns are
            // exact modulo the golden's print precision.
            let check =
                |label: &str, ours: f64, reference: f64, rel: f64, abs_floor: f64| {
                    let bound = (reference.abs() * rel).max(abs_floor);
                    assert!(
                        delta(ours, reference) <= bound,
                        "{sess} {label}: ours {ours:e} vs opensta {reference:e} (|diff| {:.3e} > bound {bound:.3e})",
                        delta(ours, reference)
                    );
                };
            for name in ["Sequential", "Combinational", "Clock"] {
                let g = report
                    .groups
                    .iter()
                    .find(|g| g.name == name)
                    .unwrap_or_else(|| panic!("{sess} group {name} missing"));
                let r = &groups[name];
                check(
                    &format!("{name} internal"),
                    g.internal_w,
                    r["internal_w"].as_f64().unwrap(),
                    0.005,
                    0.0,
                );
                check(
                    &format!("{name} switching"),
                    g.switching_w,
                    r["switching_w"].as_f64().unwrap(),
                    0.005,
                    0.0,
                );
                check(
                    &format!("{name} leakage"),
                    g.leakage_w,
                    r["leakage_w"].as_f64().unwrap(),
                    0.01,
                    1e-14,
                );
            }
            let t = &groups["total"];
            check("total internal", report.internal_w, t["internal_w"].as_f64().unwrap(), 0.005, 0.0);
            check(
                "total switching",
                report.switching_w,
                t["switching_w"].as_f64().unwrap(),
                0.005,
                0.0,
            );
            check("total leakage", report.leakage_w, t["leakage_w"].as_f64().unwrap(), 0.01, 1e-14);
            check("total", report.total_w, t["total_w"].as_f64().unwrap(), 0.003, 0.0);
        }
        assert_eq!(
            residual.iter().copied().collect::<Vec<_>>(),
            PER_PIN_RESIDUAL.to_vec(),
            "the per-pin annotation residual moved"
        );

        // `-pins <list>` vs `-input_ports`: per-pin overrides naming the
        // same input ports must resolve identically — `user` on `a`/`b`,
        // `input` elsewhere (no committed session ran `-pins`, so the
        // equivalence pins the arm against the measured session
        // transitively). Same-window compare applies: both runs share
        // `window_s == 1.0`, verbatim per-net like `s_partial`≡`s_ports`.
        // `pin_obj` demonstrates the call-site name → `PinId` resolution
        // the API delegates to callers.
        let via_pins = resolve_activity(
            &design,
            &lib,
            &sdc,
            &VcdActivity::default(),
            &ActivityOverrides {
                pins: vec![
                    (pin_obj["a"], setting(1e7, 0.4)),
                    (pin_obj["b"], setting(1e7, 0.4)),
                    // `u_or/A` is an instance sink pin on `n_and` — not a
                    // net driver, so a dead `user` entry. It proves the
                    // counter is live and that a dead override cannot
                    // perturb resolution (the verbatim compare below).
                    (pin_obj["u_or/A"], setting(1e7, 0.4)),
                ],
                ..Default::default()
            },
        );
        assert_eq!(via_pins.dead_user_entries, 1, "dead pins entry counted");
        assert_eq!(via_pins.nets.len(), input_ports_nets.len());
        for (n, (a, b)) in via_pins.nets.iter().zip(&input_ports_nets).enumerate()
        {
            assert!(
                a.prob_one == b.prob_one
                    && a.eff_toggles == b.eff_toggles
                    && a.origin == b.origin,
                "pins-vs-input_ports net {n} differs"
            );
        }
    }

    /// Constant-polarity golden (`cp.def`): a `conb_1` feeding one input
    /// each of `and2`/`or2`/`xor2`/`buf` gates whose other input is the
    /// annotated port `a`. Every expected value below is the verbatim
    /// OpenSTA `get_property activity` read measured by the committed
    /// `cp_probe.tcl` (docs/results.md 2026-11-21 — N7 — AP2 r3). The
    /// discriminators: `n_xorh` = `xor(HI, a)` propagates `a`'s 0.55 —
    /// the stored constant marginal is really `P=0` on both polarities —
    /// while `n_orh`/`n_andl`/`n_andhh`/`n_bufh` mark `constant` through
    /// the 3-valued absorbing-element rule, not all-inputs-constant.
    #[test]
    fn test_constant_cone_cp_matches_opensta() {
        let hd = sky130_hd();
        let lib_path = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef_path = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("microbench").join("cp.def"));

        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut interner = crate::interner::StringInterner::new();
        let (tech_db, _report) =
            crate::readers::load_tech_db(&tlef_path, &lef_path, &mut interner).expect("tech db");
        let design = crate::readers::parse_def(&def_path, &tech_db, Some(&lib)).expect("def");
        let sdc = crate::sta::sdc::Sdc::default();

        let resolution = crate::activity::resolve_activity(
            &design,
            &lib,
            &sdc,
            &VcdActivity::default(),
            &ActivityOverrides {
                input: Some(ActivitySetting {
                    density_per_s: 3e6,
                    prob_one: 0.55,
                }),
                ..Default::default()
            },
        );
        assert_eq!(
            (resolution.unresolvable_functions, resolution.dead_annotation_entries),
            (0, 0),
            "cp diagnostics — all nine functions resolve (conb literals, and2, or2, xor2, buf)"
        );
        // `window_s` is 1.0 with no dump, so `eff_toggles` equals the rate.
        let net = |name: &str| {
            let n = (0..design.num_nets())
                .map(|n| crate::interner::NetId(n as u32))
                .find(|&n| design.strings.resolve(design.net_names[n.0 as usize]) == name)
                .unwrap_or_else(|| panic!("net {name} missing"));
            resolution.nets[n.0 as usize]
        };
        use crate::activity::ActivityOrigin as O;
        let check = |name: &str, origin: O, prob: f64, toggles: f64| {
            let r = net(name);
            assert_eq!(r.origin, origin, "{name} origin");
            assert_eq!(r.prob_one, Some(prob), "{name} prob_one");
            assert_eq!(r.eff_toggles, toggles, "{name} eff_toggles");
        };
        check("a", O::Input, 0.55, 3e6);
        check("n_hi", O::Constant, 0.0, 0.0);
        check("n_lo", O::Constant, 0.0, 0.0);
        // and(1,a) — non-absorbing; P(HI)=0 in the stored record forces
        // the output read to 0/0.
        check("n_andh", O::Propagated, 0.0, 0.0);
        // or(1,a) and and(0,a) — absorptive constants; and(hi,hi) and
        // buf(hi) the same. The all-inputs-constant rule missed these.
        check("n_orh", O::Constant, 0.0, 0.0);
        check("n_andl", O::Constant, 0.0, 0.0);
        check("n_andhh", O::Constant, 0.0, 0.0);
        check("n_bufh", O::Constant, 0.0, 0.0);
        // xor(1,a)/or(0,a)/xor(0,a) pass a through — `n_xorh` at 0.55 is
        // the polarity discriminator the revision hangs on.
        check("n_xorh", O::Propagated, 0.55, 3e6);
        check("n_orl", O::Propagated, 0.55, 3e6);
        check("n_xorl", O::Propagated, 0.55, 3e6);
        let counts: std::collections::BTreeMap<&str, usize> = resolution
            .origin_counts
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        assert_eq!(
            counts,
            std::collections::BTreeMap::from([
                ("constant", 6),
                ("input", 1),
                ("propagated", 4),
            ]),
            "cp origin_counts"
        );
    }
}
