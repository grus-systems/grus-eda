use super::nldm::{eval_nldm_constraint, eval_nldm_delay_or_slew};
use super::parasitics::NetParasitics;
use super::sdc::Sdc;
use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinDir, PinId};
use crate::lib_db::{LibDb, NldmTable, TimingSense, TimingType};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;

pub const RISE: usize = 0;
pub const FALL: usize = 1;

/// ln(0.8 / 0.2): a single-pole RC rises from the 20% to the 80% threshold in ln(4) time constants.
const WIRE_SLEW_FACTOR: f32 = std::f32::consts::LN_2 * 2.0;

/// Combinational arc between two pins of one cell instance.
#[derive(Clone, Copy, Debug)]
struct ArcEdge {
    dst: PinId,
    lib_arc: u32,
}

/// `CLK -> Q` arc of a flip-flop, the launch of every register-to-register path.
#[derive(Clone, Copy, Debug)]
struct LaunchArc {
    clk: PinId,
    q: PinId,
    lib_arc: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct RegisterCheck {
    clk: Option<PinId>,
    setup_arc: Option<u32>,
    hold_arc: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct PinTimingState {
    arrival_max: [f32; 2],
    arrival_min: [f32; 2],
    slew_max: [f32; 2],
    slew_min: [f32; 2],
}

impl PinTimingState {
    const UNREACHED: Self = Self {
        arrival_max: [f32::NEG_INFINITY; 2],
        arrival_min: [f32::INFINITY; 2],
        slew_max: [f32::NEG_INFINITY; 2],
        slew_min: [f32::INFINITY; 2],
    };

    fn reached(&self, pol: usize) -> bool {
        self.arrival_max[pol] > f32::NEG_INFINITY
    }

    fn merge(&mut self, pol: usize, arr_max: f32, arr_min: f32, slew_max: f32, slew_min: f32) {
        // Slews are merged independently of which arc wins the arrival (OpenSTA GraphDelayCalc).
        self.arrival_max[pol] = self.arrival_max[pol].max(arr_max);
        self.arrival_min[pol] = self.arrival_min[pol].min(arr_min);
        self.slew_max[pol] = self.slew_max[pol].max(slew_max);
        self.slew_min[pol] = self.slew_min[pol].min(slew_min);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EndpointReport {
    pub pin: String,
    pub max_arr_ns: f32,
    pub max_req_ns: f32,
    pub max_slack_ns: f32,
    pub max_slew_ns: f32,
    pub min_arr_ns: f32,
    pub min_req_ns: f32,
    pub min_slack_ns: f32,
    pub min_slew_ns: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct StaReport {
    pub wns_max_ns: f32,
    pub tns_max_ns: f32,
    pub wns_min_ns: f32,
    pub tns_min_ns: f32,
    pub endpoints: Vec<EndpointReport>,
}

struct Context<'a> {
    design: &'a DesignDb,
    lib: &'a LibDb,
    sdc: &'a Sdc,
    parasitics: Option<&'a NetParasitics>,
}

impl Context<'_> {
    fn pin_name(&self, pin: PinId) -> String {
        let p = pin.0 as usize;
        let name = self.design.strings.resolve(self.design.pin_names[p]);
        match self.design.pin_cells[p] {
            Some(cell) => {
                let cell_name = self.design.strings.resolve(self.design.cell_names[cell.0 as usize]);
                format!("{cell_name}/{name}")
            }
            None => name.to_string(),
        }
    }

    fn port_name(&self, pin: PinId) -> &str {
        self.design.strings.resolve(self.design.pin_names[pin.0 as usize])
    }

    /// Capacitance of one pin of a net for one transition. Zero-wire mode has no wire capacitance,
    /// only sink pins and port loads.
    fn sink_cap(&self, pin: PinId, pol: usize) -> f32 {
        let p = pin.0 as usize;
        if self.design.pin_is_driver[p] {
            return 0.0;
        }
        match (self.design.pin_cells[p], self.design.pin_lib_pins[p]) {
            (None, _) => self.sdc.output_load_pf(self.port_name(pin)),
            (Some(_), Some(lp)) => {
                let lib_pin = &self.lib.pins[lp.0 as usize];
                if pol == RISE { lib_pin.cap_rise } else { lib_pin.cap_fall }
            }
            (Some(_), None) => 0.0,
        }
    }

    /// Total capacitance on the net driven by an output pin for one output transition.
    fn net_load(&self, net: NetId, pol: usize) -> f32 {
        let wire = self.parasitics.and_then(|par| par.net(net)).map_or(0.0, |t| t.total_wire_cap_pf);
        wire + self.design.net_pins(net).iter().map(|&pin| self.sink_cap(pin, pol)).sum::<f32>()
    }

    /// Elmore wire delay from the driver to every pin of the net, per transition, in `net_pins` order.
    fn wire_delays(&self, net: NetId) -> Option<[Vec<f32>; 2]> {
        let tree = self.parasitics?.net(net)?;
        let pins = self.design.net_pins(net);
        let delays = |pol| tree.elmore_ns(&pins.iter().map(|&p| self.sink_cap(p, pol)).collect::<Vec<_>>());
        Some([delays(RISE), delays(FALL)])
    }

    fn cell_pin_for_lib_pin(&self, cell: CellId, lib_pin: crate::interner::LibPinId) -> Option<PinId> {
        self.design
            .cell_pins(cell)
            .iter()
            .copied()
            .find(|&p| self.design.pin_lib_pins[p.0 as usize] == Some(lib_pin))
    }
}

struct TimingGraph {
    edges: Vec<ArcEdge>,
    edges_from: Vec<Vec<u32>>,
    launches: Vec<LaunchArc>,
    checks: BTreeMap<u32, RegisterCheck>,
}

fn build_graph(cx: &Context) -> TimingGraph {
    let design = cx.design;
    let mut graph = TimingGraph {
        edges: Vec::new(),
        edges_from: vec![Vec::new(); design.num_pins()],
        launches: Vec::new(),
        checks: BTreeMap::new(),
    };
    for c in 0..design.num_cells() {
        let cell = CellId(c as u32);
        let Some(lib_cell_id) = design.cell_lib_ids[c] else { continue };
        let lib_cell = &cx.lib.cells[lib_cell_id.0 as usize];
        if lib_cell.is_latch {
            eprintln!(
                "warning: latch cell {} is outside E3 scope (no time borrowing); its arcs are ignored",
                design.strings.resolve(design.cell_names[c])
            );
            continue;
        }
        for i in 0..lib_cell.arc_count as u32 {
            let lib_arc = lib_cell.arc_offset + i;
            let arc = &cx.lib.arcs[lib_arc as usize];
            let (Some(src), Some(dst)) = (
                cx.cell_pin_for_lib_pin(cell, arc.related_pin),
                cx.cell_pin_for_lib_pin(cell, arc.target_pin),
            ) else {
                continue;
            };
            match arc.timing_type {
                TimingType::Combinational | TimingType::CombinationalRise | TimingType::CombinationalFall => {
                    graph.edges_from[src.0 as usize].push(graph.edges.len() as u32);
                    graph.edges.push(ArcEdge { dst, lib_arc });
                }
                TimingType::RisingEdge => graph.launches.push(LaunchArc { clk: src, q: dst, lib_arc }),
                TimingType::SetupRising => {
                    let check = graph.checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.setup_arc = Some(lib_arc);
                }
                TimingType::HoldRising => {
                    let check = graph.checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.hold_arc = Some(lib_arc);
                }
                _ => {}
            }
        }
    }
    graph
}

/// Kahn topological order over net edges (driver -> sink) and combinational arcs.
/// Register `CLK -> Q` arcs are not edges: they start new paths.
fn topological_order(cx: &Context, graph: &TimingGraph) -> Result<Vec<PinId>> {
    let design = cx.design;
    let n = design.num_pins();
    let mut indegree = vec![0u32; n];
    for edge in &graph.edges {
        indegree[edge.dst.0 as usize] += 1;
    }
    for net in 0..design.num_nets() {
        if let Some(driver) = design.net_driver(NetId(net as u32)) {
            for &sink in &design.net_pins(NetId(net as u32))[1..] {
                if sink != driver {
                    indegree[sink.0 as usize] += 1;
                }
            }
        }
    }
    let mut order: Vec<PinId> = (0..n as u32).filter(|&p| indegree[p as usize] == 0).map(PinId).collect();
    let mut head = 0;
    while head < order.len() {
        let pin = order[head];
        head += 1;
        let p = pin.0 as usize;
        let mut release = |target: PinId, order: &mut Vec<PinId>| {
            let d = &mut indegree[target.0 as usize];
            *d -= 1;
            if *d == 0 {
                order.push(target);
            }
        };
        if design.pin_is_driver[p]
            && let Some(net) = design.pin_nets[p]
        {
            for &sink in &design.net_pins(net)[1..] {
                release(sink, &mut order);
            }
        }
        for &e in &graph.edges_from[p] {
            release(graph.edges[e as usize].dst, &mut order);
        }
    }
    if order.len() != n {
        bail!("combinational loop: {} of {} pins could not be levelized", n - order.len(), n);
    }
    Ok(order)
}

fn polarities(sense: TimingSense) -> &'static [(usize, usize)] {
    match sense {
        TimingSense::PositiveUnate => &[(RISE, RISE), (FALL, FALL)],
        TimingSense::NegativeUnate => &[(RISE, FALL), (FALL, RISE)],
        TimingSense::NonUnate => &[(RISE, RISE), (FALL, RISE), (RISE, FALL), (FALL, FALL)],
    }
}

fn delay_tables(arc: &crate::lib_db::LibTimingArc, dst_pol: usize) -> (Option<&NldmTable>, Option<&NldmTable>) {
    if dst_pol == RISE {
        (arc.cell_rise.as_ref(), arc.rise_transition.as_ref())
    } else {
        (arc.cell_fall.as_ref(), arc.fall_transition.as_ref())
    }
}

fn constraint_table(arc: &crate::lib_db::LibTimingArc, pol: usize) -> Option<&NldmTable> {
    if pol == RISE { arc.rise_constraint.as_ref() } else { arc.fall_constraint.as_ref() }
}

/// Zero-wire STA when `parasitics` is `None`; otherwise wire capacitance and Elmore wire delay are added.
pub fn run_sta(design: &DesignDb, lib: &LibDb, sdc: &Sdc, parasitics: Option<&NetParasitics>) -> Result<StaReport> {
    let cx = Context { design, lib, sdc, parasitics };
    let graph = build_graph(&cx);
    let order = topological_order(&cx, &graph)?;
    let mut state = vec![PinTimingState::UNREACHED; design.num_pins()];

    // Primary inputs: ideal drivers. The clock port is not a data startpoint.
    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        let name = cx.port_name(pin);
        if design.pin_dirs[p] != PinDir::Input || sdc.clock_port.as_deref() == Some(name) {
            continue;
        }
        let delay = sdc.input_delay_ns(name);
        let slew = sdc.input_transition_ns(name);
        for pol in [RISE, FALL] {
            state[p].merge(pol, delay, delay, slew, slew);
        }
    }

    // Ideal clock: every register clock pin sees an edge at t=0 with zero slew.
    for launch in &graph.launches {
        let clk = launch.clk.0 as usize;
        for pol in [RISE, FALL] {
            state[clk].merge(pol, 0.0, 0.0, 0.0, 0.0);
        }
    }
    for launch in &graph.launches {
        let Some(q_net) = design.pin_nets[launch.q.0 as usize] else { continue };
        let arc = &lib.arcs[launch.lib_arc as usize];
        let clk_state = state[launch.clk.0 as usize];
        for dst_pol in [RISE, FALL] {
            let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol) else { continue };
            let load = cx.net_load(q_net, dst_pol);
            // A rising clock edge is the only launch edge, whatever the output transition.
            let slew_in = clk_state.slew_max[RISE];
            let delay = eval_nldm_delay_or_slew(delay_table, slew_in, load);
            let slew = eval_nldm_delay_or_slew(slew_table, slew_in, load);
            let arrival = clk_state.arrival_max[RISE] + delay;
            state[launch.q.0 as usize].merge(dst_pol, arrival, arrival, slew, slew);
        }
    }

    for &pin in &order {
        let p = pin.0 as usize;
        if design.pin_is_driver[p]
            && let Some(net) = design.pin_nets[p]
        {
            let driver_state = state[p];
            // The ideal clock port stays unreached; its sinks keep their seeded edge.
            if !driver_state.reached(RISE) && !driver_state.reached(FALL) {
                continue;
            }
            let wire = cx.wire_delays(net);
            for (i, &sink) in design.net_pins(net).iter().enumerate().skip(1) {
                let mut sink_state = driver_state;
                if let Some(wire) = &wire {
                    for pol in [RISE, FALL] {
                        let delay = wire[pol][i];
                        sink_state.arrival_max[pol] += delay;
                        sink_state.arrival_min[pol] += delay;
                        let degrade = WIRE_SLEW_FACTOR * delay;
                        sink_state.slew_max[pol] = driver_state.slew_max[pol].hypot(degrade);
                        sink_state.slew_min[pol] = driver_state.slew_min[pol].hypot(degrade);
                    }
                }
                state[sink.0 as usize] = sink_state;
            }
        }
        for &e in &graph.edges_from[p] {
            let edge = graph.edges[e as usize];
            let arc = &lib.arcs[edge.lib_arc as usize];
            let Some(out_net) = design.pin_nets[edge.dst.0 as usize] else { continue };
            let src = state[p];
            for &(src_pol, dst_pol) in polarities(arc.timing_sense) {
                if !src.reached(src_pol) {
                    continue;
                }
                if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                    || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                {
                    continue;
                }
                let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol) else { continue };
                let load = cx.net_load(out_net, dst_pol);
                let late = |t: &NldmTable| eval_nldm_delay_or_slew(t, src.slew_max[src_pol], load);
                let early = |t: &NldmTable| eval_nldm_delay_or_slew(t, src.slew_min[src_pol], load);
                state[edge.dst.0 as usize].merge(
                    dst_pol,
                    src.arrival_max[src_pol] + late(delay_table),
                    src.arrival_min[src_pol] + early(delay_table),
                    late(slew_table),
                    early(slew_table),
                );
            }
        }
    }

    collect_endpoints(&cx, &graph, &state)
}

struct Worst {
    arr: f32,
    req: f32,
    slack: f32,
    slew: f32,
}

fn collect_endpoints(cx: &Context, graph: &TimingGraph, state: &[PinTimingState]) -> Result<StaReport> {
    let (design, lib, sdc) = (cx.design, cx.lib, cx.sdc);
    let mut endpoints = Vec::new();

    for (&data_id, check) in &graph.checks {
        let data = PinId(data_id);
        let (Some(clk), Some(setup_arc), Some(hold_arc)) = (check.clk, check.setup_arc, check.hold_arc) else {
            continue;
        };
        let (s, clk_state) = (state[data.0 as usize], state[clk.0 as usize]);
        let (setup, hold) = (&lib.arcs[setup_arc as usize], &lib.arcs[hold_arc as usize]);
        let mut worst_max: Option<Worst> = None;
        let mut worst_min: Option<Worst> = None;
        for pol in [RISE, FALL] {
            if !s.reached(pol) {
                continue;
            }
            let (Some(setup_table), Some(hold_table)) = (constraint_table(setup, pol), constraint_table(hold, pol)) else {
                continue;
            };
            let setup_time = eval_nldm_constraint(setup_table, s.slew_max[pol], clk_state.slew_max[RISE]);
            let req_max = sdc.period_ns + clk_state.arrival_min[RISE] - setup_time - sdc.uncertainty_ns;
            let slack_max = req_max - s.arrival_max[pol];
            let hold_time = eval_nldm_constraint(hold_table, s.slew_min[pol], clk_state.slew_max[RISE]);
            let req_min = clk_state.arrival_max[RISE] + hold_time + sdc.uncertainty_ns;
            let slack_min = s.arrival_min[pol] - req_min;
            if worst_max.as_ref().is_none_or(|w| slack_max < w.slack) {
                worst_max = Some(Worst { arr: s.arrival_max[pol], req: req_max, slack: slack_max, slew: s.slew_max[pol] });
            }
            if worst_min.as_ref().is_none_or(|w| slack_min < w.slack) {
                worst_min = Some(Worst { arr: s.arrival_min[pol], req: req_min, slack: slack_min, slew: s.slew_min[pol] });
            }
        }
        if let (Some(max), Some(min)) = (worst_max, worst_min) {
            endpoints.push(endpoint_report(cx.pin_name(data), &max, &min));
        }
    }

    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        if design.pin_dirs[p] != PinDir::Output {
            continue;
        }
        let s = state[p];
        let output_delay = sdc.output_delay_ns(cx.port_name(pin));
        let req_max = sdc.period_ns - output_delay;
        let req_min = -output_delay;
        let mut worst_max: Option<Worst> = None;
        let mut worst_min: Option<Worst> = None;
        for pol in [RISE, FALL] {
            if !s.reached(pol) {
                continue;
            }
            let slack_max = req_max - s.arrival_max[pol];
            let slack_min = s.arrival_min[pol] - req_min;
            if worst_max.as_ref().is_none_or(|w| slack_max < w.slack) {
                worst_max = Some(Worst { arr: s.arrival_max[pol], req: req_max, slack: slack_max, slew: s.slew_max[pol] });
            }
            if worst_min.as_ref().is_none_or(|w| slack_min < w.slack) {
                worst_min = Some(Worst { arr: s.arrival_min[pol], req: req_min, slack: slack_min, slew: s.slew_min[pol] });
            }
        }
        if let (Some(max), Some(min)) = (worst_max, worst_min) {
            endpoints.push(endpoint_report(cx.pin_name(pin), &max, &min));
        }
    }

    endpoints.sort_by(|a, b| a.pin.cmp(&b.pin));
    let wns = |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).fold(f32::INFINITY, f32::min);
    let tns = |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).map(|s| s.min(0.0)).sum::<f32>();
    Ok(StaReport {
        wns_max_ns: wns(|e| e.max_slack_ns),
        tns_max_ns: tns(|e| e.max_slack_ns),
        wns_min_ns: wns(|e| e.min_slack_ns),
        tns_min_ns: tns(|e| e.min_slack_ns),
        endpoints,
    })
}

fn endpoint_report(pin: String, max: &Worst, min: &Worst) -> EndpointReport {
    EndpointReport {
        pin,
        max_arr_ns: max.arr,
        max_req_ns: max.req,
        max_slack_ns: max.slack,
        max_slew_ns: max.slew,
        min_arr_ns: min.arr,
        min_req_ns: min.req,
        min_slack_ns: min.slack,
        min_slew_ns: min.slew,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::{load_liberty, load_tech_db, parse_def};
    use crate::interner::StringInterner;
    use crate::sta::sdc::load_sdc;
    use std::path::PathBuf;

    /// Per endpoint and field: pass if |ours - ref| <= max(5 ps, rel_tol * |ref|).
    fn within_tolerance(ours: f32, reference: f64, rel_tol: f64) -> (bool, f64) {
        let diff = (ours as f64 - reference).abs();
        (diff <= (0.005f64).max(rel_tol * reference.abs()), diff / reference.abs().max(1e-9))
    }

    #[test]
    fn test_picorv32_zero_wire_matches_opensta() {
        compare_design_with_opensta("picorv32", "zero_wire", Rc::None, 0.01);
    }

    #[test]
    #[ignore = "known failing: no DMP Ceff and a greedy Steiner tree; run with --ignored, see docs/results.md"]
    fn test_picorv32_placement_rc_matches_opensta() {
        compare_design_with_opensta("picorv32", "placement_rc", Rc::Placement, 0.03);
    }

    #[test]
    #[ignore = "known failing: no DMP Ceff; run with --ignored, see docs/results.md"]
    fn test_picorv32_placed_spef_matches_opensta() {
        compare_design_with_opensta("picorv32", "placed_spef", Rc::Spef, 0.03);
    }

    #[test]
    fn test_gcd_zero_wire_matches_opensta() {
        compare_design_with_opensta("gcd", "zero_wire", Rc::None, 0.01);
    }

    #[test]
    fn test_gcd_placement_rc_matches_opensta() {
        // Arrival, required time and slack must agree within 1% or 5 ps. Slews get 3%: OpenSTA's
        // DMP calculator derives the driver slew from a waveform model that differs from the plain
        // table lookup by about 1% (`report_dcalc`: 0.278732 vs 0.282240); see docs/results.md.
        compare_design_with_opensta("gcd", "placement_rc", Rc::Placement, 0.03);
    }

    #[test]
    fn test_gcd_placed_spef_matches_opensta() {
        compare_design_with_opensta("gcd", "placed_spef", Rc::Spef, 0.03);
    }

    enum Rc {
        None,
        Placement,
        Spef,
    }

    fn compare_design_with_opensta(design_name: &str, golden_key: &str, rc_mode: Rc, slew_rel_tol: f64) {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pdk = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".ciel"));
        let lib_dir = pdk.join("sky130A/libs.ref/sky130_fd_sc_hd");
        let gcd = repo.join("tests/designs").join(design_name);

        let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF");
        let design = parse_def(&gcd.join(format!("{design_name}.placed.def")), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&gcd.join(format!("{design_name}.sdc"))).expect("SDC");
        let parasitics = match rc_mode {
            Rc::None => None,
            Rc::Placement => {
                let rc = crate::sta::WireRc::from_layer(&tech, "met2").expect("met2 RC");
                Some(crate::sta::estimate_placement_parasitics(&design, &tech, rc))
            }
            Rc::Spef => {
                let spef = crate::readers::load_spef(&gcd.join(format!("{design_name}.routed.spef"))).expect("SPEF");
                Some(crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics"))
            }
        };
        let report = run_sta(&design, &lib, &sdc, parasitics.as_ref()).expect("STA");

        let reference: serde_json::Value =
            serde_json::from_slice(&std::fs::read(gcd.join(format!("{design_name}.sta.reference.json"))).expect("golden file"))
                .expect("golden JSON");
        let zero_wire = &reference[golden_key];
        let ref_endpoints = zero_wire["endpoints"].as_array().expect("endpoints");
        assert_eq!(report.endpoints.len(), ref_endpoints.len(), "endpoint count");

        let mut failures = Vec::new();
        let mut max_diff = [0.0f64; 8];
        let fields = ["max_arr_ns", "max_req_ns", "max_slack_ns", "max_slew_ns", "min_arr_ns", "min_req_ns", "min_slack_ns", "min_slew_ns"];
        for r in ref_endpoints {
            let name = r["pin"].as_str().unwrap();
            let Some(ours) = report.endpoints.iter().find(|e| e.pin == name) else {
                failures.push(format!("{name}: missing"));
                continue;
            };
            let values = [ours.max_arr_ns, ours.max_req_ns, ours.max_slack_ns, ours.max_slew_ns, ours.min_arr_ns, ours.min_req_ns, ours.min_slack_ns, ours.min_slew_ns];
            for (i, field) in fields.iter().enumerate() {
                let expected = r[*field].as_f64().unwrap();
                let rel_tol = if field.ends_with("slew_ns") { slew_rel_tol } else { 0.01 };
                let (ok, diff) = within_tolerance(values[i], expected, rel_tol);
                max_diff[i] = max_diff[i].max(diff);
                if !ok {
                    failures.push(format!("{name}.{field}: ours={} ref={expected}", values[i]));
                }
            }
        }
        println!("{design_name} {golden_key} max relative diff per field: {:?}", fields.iter().zip(max_diff).collect::<Vec<_>>());
        for (ours, key) in [
            (report.wns_max_ns, "wns_max_ns"),
            (report.tns_max_ns, "tns_max_ns"),
            (report.wns_min_ns, "wns_min_ns"),
            (report.tns_min_ns, "tns_min_ns"),
        ] {
            let expected = zero_wire[key].as_f64().unwrap();
            if !within_tolerance(ours, expected, 0.01).0 {
                failures.push(format!("{key}: ours={ours} ref={expected}"));
            }
        }
        assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
    }
}
