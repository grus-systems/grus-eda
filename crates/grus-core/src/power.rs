//! Power estimation driven by switching-activity annotation (VCD or SAIF).
//!
//! Reports the same three components as OpenSTA's `report_power`:
//! - **Leakage**: `cell_leakage_power` summed over instances.
//! - **Switching**: `0.5 * C_load * V^2 * transition_rate` per driven net, with
//!   `C_load` = wire capacitance + sink pin capacitance (per transition direction,
//!   matching the STA load model).
//! - **Internal**: `internal_power` Liberty tables evaluated at the trigger pin's
//!   slew and the driven net's total load, times the trigger net's transition count.
//!
//! Activity comes from a VCD dump, so numbers are exact for the simulated window
//! rather than OpenSTA's statistical propagation. Slews come from [`sta_pin_slews`].
//! `when`-qualified `internal_power` blocks (ebu*/einv* cells) are skipped and
//! counted in the diagnostics — evaluating them needs value-correlated activity
//! the VCD net counts do not provide.

use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinDir, PinId};
use crate::lib_db::{LibCell, LibDb, LibPin};
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
    /// Driven nets absent from the activity input (treated as zero activity).
    pub driven_nets_without_activity: usize,
    /// Nets with VCD activity but no cell-output driver (input ports, undriven);
    /// their switching power is not attributed anywhere, as in OpenSTA.
    pub active_nets_without_cell_driver: usize,
    /// `internal_power` triggers whose pin slew STA could not reach.
    pub internal_power_without_slew: usize,
    /// `internal_power` `related_pin` names that match no sibling pin of the cell.
    pub unresolved_related_pin: usize,
    /// `internal_power` blocks skipped because of a `when` state expression.
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
    /// (rise, fall) transition counts per net from the VCD.
    net_activity: Vec<(u64, u64)>,
    /// Total load capacitance per net per output transition, pF: wire cap plus
    /// sink pin caps (and SDC output loads), the same quantity STA's `net_load`
    /// computes for delay lookups.
    net_load_pf: Vec<[f32; 2]>,
    pin_slews: &'a [PinSlew],
    duration_s: f64,
    diagnostics: PowerDiagnostics,
}

impl PowerCx<'_> {
    fn net_activity(&self, net: NetId) -> (u64, u64) {
        self.net_activity[net.0 as usize]
    }

    /// `0.5 * C * V^2 * rate` for one net, split by transition direction so rise
    /// and fall see their own pin capacitances.
    fn switching_w(&self, net: NetId, voltage_v: f32) -> f64 {
        let (n_rise, n_fall) = self.net_activity(net);
        if n_rise + n_fall == 0 || self.duration_s <= 0.0 {
            return 0.0;
        }
        let load = self.net_load_pf[net.0 as usize];
        let energy_pf = 0.5
            * voltage_v as f64
            * voltage_v as f64
            * (load[RISE] as f64 * n_rise as f64 + load[FALL] as f64 * n_fall as f64);
        energy_pf * 1e-12 / self.duration_s
    }

    /// Worst of the two transition load caps; `total_output_net_capacitance` is a
    /// single Liberty axis, and OpenSTA evaluates `loadCap` once per pin.
    fn total_output_cap_pf(&self, net: NetId) -> f32 {
        let load = self.net_load_pf[net.0 as usize];
        load[RISE].max(load[FALL])
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

/// Internal power of one cell pin's `internal_power` blocks, attributed to `group`.
fn pin_internal_power(
    cx: &mut PowerCx,
    cell: CellId,
    pin: PinId,
    lib_pin: &LibPin,
    group: usize,
    groups: &mut [PowerGroup; 3],
) {
    for ip in &lib_pin.internal_power {
        if ip.when.is_some() {
            cx.diagnostics.conditional_power_blocks += 1;
            continue;
        }
        let trigger = match ip.related_pin {
            // No related pin: the block's own pin transition is the trigger
            // (the input-pin self-power form).
            None => pin,
            Some(sym) => match sibling_pin_by_lib_name(cx.design, cx.lib, cell, sym) {
                Some(p) => p,
                None => {
                    cx.diagnostics.unresolved_related_pin += 1;
                    continue;
                }
            },
        };
        // OpenSTA counts internal power by the power-owning pin's own net
        // transitions, not the related pin's: a DFF's Q `power_outputs` block is
        // indexed by CLK slew but multiplied by Q's activity (verified against
        // `report_power` on a clocked micro-benchmark, docs/results.md N7).
        let Some(count_net) = cx.design.pin_nets[pin.0 as usize] else {
            continue;
        };
        // The 2-D `power_outputs_*` tables index the load on the pin's own driven
        // net; input-pin `power_inputs_*` tables are 1-D and ignore it.
        let is_output = matches!(lib_pin.dir, PinDir::Output | PinDir::Inout);
        let load_pf = if is_output && let Some(net) = cx.design.pin_nets[pin.0 as usize] {
            cx.total_output_cap_pf(net)
        } else {
            0.0
        };
        for (table, pol, count) in [
            (ip.rise.as_ref(), RISE, cx.net_activity(count_net).0),
            (ip.fall.as_ref(), FALL, cx.net_activity(count_net).1),
        ] {
            let (Some(table), count) = (table, count) else {
                continue;
            };
            if count == 0 || cx.duration_s <= 0.0 {
                continue;
            }
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
            groups[group].internal_w += energy_j * count as f64 / cx.duration_s;
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
        let (rise, fall) = vcd
            .signals
            .get(name)
            .map(|s| (s.rise, s.fall))
            .unwrap_or((0, 0));
        if design.net_driver(net).is_some() && !vcd.signals.contains_key(name) {
            cx.diagnostics.driven_nets_without_activity += 1;
        }
        cx.net_activity.push((rise, fall));
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
        groups[g].leakage_w += lib_cell
            .leakage_power_w
            .unwrap_or(lib.default_cell_leakage_power_w) as f64;

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
        let (n_rise, n_fall) = cx.net_activity(net);
        if n_rise + n_fall == 0 {
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
        groups[g].switching_w += cx.switching_w(net, lib.nominal_voltage_v);
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

        // Every mb.saif net reconstructs its VCD edge count (clk TC=200 ->
        // (100,100), the others TC=4 -> (2,2)), so the SAIF-driven report must
        // equal the VCD-driven report exactly.
        let vcd = crate::readers::load_vcd(&vcd_path).expect("vcd");
        let vcd_report = estimate_power(&design, &lib, &sdc, None, &vcd, &pin_slews);
        assert_eq!(report.total_w, vcd_report.total_w);
        for (a, b) in report.groups.iter().zip(&vcd_report.groups) {
            assert_eq!(a.internal_w, b.internal_w);
            assert_eq!(a.switching_w, b.switching_w);
            assert_eq!(a.leakage_w, b.leakage_w);
        }
    }
}
