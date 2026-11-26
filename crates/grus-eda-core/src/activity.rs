//! Per-net switching-activity resolution: turns annotation (VCD/SAIF), SDC
//! clocks, and the measured OpenSTA seed defaults into one resolved
//! `{density, prob}` record per net.
//!
//! The fill order is the measured precedence (docs/results.md 2026-11-21 N7):
//! constant cone → clock → `-global` override → `-pins` → `-input_ports` →
//! annotation → input/undriven → leftover `Default` — then the unified
//! propagation pass
//! (seq Q=D copy plus combinational evaluation in one fixed point) overwrites
//! `Default` entries it can derive (`Propagated`), and every leftover
//! `Default` becomes `unknown` `{0, 0}` — the measured semantics for
//! sequential-cone leftovers.
//!
//! Annotation matches in the pin namespace (the dump's `$scope`/`INSTANCE`
//! paths): a dump entry resolves a design pin, and a pin annotates its net
//! iff it is the net's driver. Flat internal net names in a dump are dead
//! entries — OpenSTA annotates pins, and a net's annotated value is its
//! driver pin's (docs/results.md N7 P4).
//!
//! Counts, not rates: `ResolvedActivity::eff_toggles` is the number of
//! effective transitions over the report window (`window_s`), so the power
//! arithmetic downstream (`switching_w`, `pin_internal_power`) is identical
//! for annotated and seeded nets. The defined conversion is
//! `count = rate × window_s`; `window_s` is `vcd.duration_s` when a dump is
//! loaded and positive, else `1.0` (absolute-density mode — counts then equal
//! rates numerically and the window cancels in `switching_w` regardless).

use crate::bool_expr;
use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinId};
use crate::lib_db::LibDb;
use crate::readers::vcd::{SignalActivity, VcdActivity};
use crate::sta::sdc::Sdc;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where a net's resolved activity came from. Labels mirror OpenSTA's
/// `get_property <pin> activity` source column; `Sequential` is declared
/// for a possible measured source and never emitted (OpenSTA prints
/// `propagated` for the Q=D copy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ActivityOrigin {
    /// VCD/SAIF dump entry resolving to the net's driver pin.
    Annotated,
    /// Net driven by an SDC `create_clock` source port.
    Clock,
    /// `-global` override.
    Global,
    /// `set_power_activity -pins` / `-input_ports` — an explicit user
    /// set, measured source label `user` (P0b). Sits below
    /// `global`/`clock`/`constant` and above dump annotation in the
    /// seed order.
    User,
    /// Input-port-driven or undriven net.
    Input,
    /// Combinational propagation: gate-function evaluation over resolved
    /// inputs, level-ordered over the net graph.
    Propagated,
    /// Sequential rules (AP3) — declared, never emitted.
    Sequential,
    /// `function : "0"/"1"` cone.
    Constant,
    /// Unresolvable cone — the post-pass fill for every net the seeds
    /// and propagation left `Default` (`{0, 0}`, measured).
    Unknown,
    /// Interim fill during resolution: converted to `Unknown` after the
    /// propagation pass, so `Default` never appears in a finished
    /// `ActivityResolution`.
    Default,
}

impl ActivityOrigin {
    /// The source column label, as OpenSTA's activity accessor prints it
    /// (`annotated` stands in for its per-mechanism `vcd`/`saif`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Annotated => "annotated",
            Self::Clock => "clock",
            Self::Global => "global",
            Self::User => "user",
            Self::Input => "input",
            Self::Propagated => "propagated",
            Self::Sequential => "sequential",
            Self::Constant => "constant",
            Self::Unknown => "unknown",
            Self::Default => "default",
        }
    }
}

/// A resolved {density, prob} override, density in transitions/second.
/// `-activity a` is clock-normalized (`a × f_clk`), `-density d` is per
/// library time unit — the conversion happens at the call site (no CLI
/// flag in AP1); the API always carries absolute rates.
#[derive(Clone, Copy, Debug)]
pub struct ActivitySetting {
    pub density_per_s: f64,
    pub prob_one: f64,
}

#[derive(Clone, Debug, Default)]
pub struct ActivityOverrides {
    /// `-global`: hits every non-clock non-constant net — beats annotation,
    /// input, and the leftover fill (measured).
    pub global: Option<ActivitySetting>,
    /// `-input`: replaces the input default; also the leftover fill value.
    pub input: Option<ActivitySetting>,
    /// `-input_ports`: user annotation on input-port-driven nets —
    /// `{density, P}` absolute, origin `user`. Port drivers only;
    /// undriven nets keep `input` (measured: `s_input_ports`'s `n_float`
    /// reads `input` `5e6/0.5`). Beats a dump entry on the same net
    /// (unmeasured — recorded); loses to `-global` and `clock`.
    pub input_ports: Option<ActivitySetting>,
    /// `-pins <list>`: per-pin user overrides, origin `user`. The call
    /// site resolves pin names to `PinId`s (name resolution is a caller
    /// concern, same as the `-activity` rate normalization). An entry
    /// annotates its net iff the pin is the net's driver — identical to
    /// the dump-annotation rule (`annotation_net_of`): input ports and
    /// instance output pins drive; sink pins and output ports are dead
    /// `user` entries, counted in `dead_user_entries`. Last entry wins
    /// on a duplicate pin. Checked before `input_ports` — the specific
    /// selector beats the `[all_inputs]` blanket (unmeasured, recorded).
    pub pins: Vec<(PinId, ActivitySetting)>,
}

/// One net's resolved activity.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedActivity {
    /// `P(net = 1)`; `None` only for an `Annotated` net whose dump never
    /// defined the state (matches the old `prob_one() == None` semantics:
    /// a `when` literal on it is inexpressible).
    pub prob_one: Option<f64>,
    /// Effective transitions over `window_s`: exact `eff_toggles()` for
    /// `Annotated`, `density_per_s × window_s` for every seeded origin —
    /// counts, not rates (the recorded representation choice).
    pub eff_toggles: f64,
    pub origin: ActivityOrigin,
}

pub struct ActivityResolution {
    /// One entry per net, indexed by `NetId`.
    pub nets: Vec<ResolvedActivity>,
    /// The report window: `vcd.duration_s` when `> 0`, else `1.0` so
    /// absolute densities pass through (counts == rates numerically).
    pub window_s: f64,
    /// Nets per origin — the `report_activity_annotation` split shape,
    /// per net (OpenSTA's split is per pin; recorded difference).
    pub origin_counts: BTreeMap<String, usize>,
    /// Nets ending `Unknown` because the driver pin's `function` was
    /// unparseable, over a parse/eval bound, or named variables matching
    /// no input pin of the cell (AP2 diagnostic — counted at
    /// `comb_funcs` build for every origin, plus eval-side failures).
    pub unresolvable_functions: usize,
    /// Dump entries matching no driver pin — flat internal net names,
    /// sink pins, unknown names. Diagnostic only.
    pub dead_annotation_entries: usize,
    /// `pins` entries whose pin is not a net driver (a sink pin, an
    /// output port, or a pin with no net) — dead `user` entries, the
    /// `dead_annotation_entries` analogue for the `-pins` arm.
    /// Diagnostic only; a driver entry is never dead even when a
    /// higher-precedence arm claims its net first — the pin *was*
    /// resolved (matching `annotation_net_of`'s contract).
    pub dead_user_entries: usize,
}

/// A combinational driver pin's parsed `function` with its
/// function-referenced input nets, in variable order.
struct CombFunc {
    expr: bool_expr::BoolExpr,
    input_nets: Vec<NetId>,
}

/// One parse per `function`, built once before the seed pass and shared
/// by `constant_cone` (3-valued marking) and `propagate_combinational`.
/// Indexed by `NetId`: `Some` for every net whose driver pin sits on a
/// `!is_sequential && !is_latch` cell and whose `LibPin.function` parsed
/// and resolved all vars to nets. `function: None` is not a failure and
/// produces `None` uncounted; a parse `None` or a var matching no pin /
/// a pin with no net counts `unresolvable` — at build, for any origin
/// (PG forms like `!VPWR+VGND` parse fine but their vars match no
/// design pin and land here).
fn comb_function_table(
    design: &DesignDb,
    lib: &LibDb,
    unresolvable: &mut usize,
) -> Vec<Option<CombFunc>> {
    let mut table: Vec<Option<CombFunc>> =
        (0..design.num_nets()).map(|_| None).collect();
    for (n, slot) in table.iter_mut().enumerate() {
        let Some(drv) = design.net_driver(NetId(n as u32)) else {
            continue;
        };
        let Some(cell) = design.pin_cells[drv.0 as usize] else {
            continue;
        };
        let Some(lib_cell) = design.cell_lib_ids[cell.0 as usize]
            .map(|lc| &lib.cells[lc.0 as usize])
        else {
            continue;
        };
        if lib_cell.is_sequential || lib_cell.is_latch {
            continue;
        }
        let Some(func) = design.pin_lib_pins[drv.0 as usize]
            .and_then(|lp| lib.pins[lp.0 as usize].function)
        else {
            continue; // no function to evaluate — not counted
        };
        let Some((expr, vars)) = bool_expr::parse(lib.strings.resolve(func)) else {
            *unresolvable += 1;
            continue;
        };
        let mut input_nets = Vec::with_capacity(vars.len());
        let mut ok = true;
        for v in &vars {
            // A var matching no pin, or a pin with no net, makes the
            // function unresolvable.
            let pin = lib
                .strings
                .get(v)
                .and_then(|sym| design.pin_by_lib_name(lib, cell, sym));
            match pin.and_then(|p| design.pin_nets[p.0 as usize]) {
                Some(dep) => input_nets.push(dep),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            *slot = Some(CombFunc { expr, input_nets });
        } else {
            *unresolvable += 1;
        }
    }
    table
}

/// A sequential output pin's copy rule: the net takes the resolved
/// activity of the cell's data-pin net — verbatim, or `1−P` on the
/// inverted state output.
struct SeqCopy {
    data_net: NetId,
    /// `true` when the driver pin's `function` names `state_var_n`
    /// (`"IQ_N"` — `Q_N` outputs on `*bp`/`*bn` cells).
    inverted: bool,
}

/// The sequential producer table, parallel to `comb_funcs` and indexed
/// by `NetId`: `Some` for every net whose driver pin sits on an
/// `ff`/`latch` cell (the `seq` block), whose `LibCell::seq::data_pin`
/// is a bare name resolvable to a connected design pin, and whose driver
/// pin `function` names a state variable (`state_var` → verbatim,
/// `state_var_n` → `1−P`). Expression-form `next_state`/`data_in`
/// (`data_pin: None`) is never a candidate — its output nets fall to
/// `Default` → `Unknown`, the named seq-expression residual
/// (docs/results.md census); it is *not* counted in
/// `unresolvable_functions` — that counter is combinational-`function`
/// failures only. A driver function naming neither var is
/// unclassifiable (sky130 has none — every seq output pin's function is
/// `IQ` or `IQ_N`).
fn seq_function_table(design: &DesignDb, lib: &LibDb) -> Vec<Option<SeqCopy>> {
    let mut table: Vec<Option<SeqCopy>> = (0..design.num_nets()).map(|_| None).collect();
    for (n, slot) in table.iter_mut().enumerate() {
        let Some(drv) = design.net_driver(NetId(n as u32)) else {
            continue;
        };
        let Some(cell) = design.pin_cells[drv.0 as usize] else {
            continue;
        };
        let Some(lib_cell) = design.cell_lib_ids[cell.0 as usize]
            .map(|lc| &lib.cells[lc.0 as usize])
        else {
            continue;
        };
        let Some(seq) = &lib_cell.seq else { continue };
        let Some(data_sym) = seq.data_pin else { continue };
        let func = design.pin_lib_pins[drv.0 as usize]
            .and_then(|lp| lib.pins[lp.0 as usize].function);
        let inverted = if func == Some(seq.state_var) {
            false
        } else if seq.state_var_n.is_some() && func == seq.state_var_n {
            true
        } else {
            continue;
        };
        let Some(data_net) = design
            .pin_by_lib_name(lib, cell, data_sym)
            .and_then(|p| design.pin_nets[p.0 as usize])
        else {
            continue;
        };
        *slot = Some(SeqCopy { data_net, inverted });
    }
    table
}

/// Marks a net `Constant` when its driver function has a definite value
/// under 3-valued (Kleene) evaluation over the `comb_funcs` table — the
/// measured marking rule (docs/results.md 2026-11-21 N7 cp.def probe):
/// `or(1,X)=1` and `and(0,X)=0` mark even with a toggling other input
/// (absorption), while `and(1,X)`/`or(0,X)`/`xor(c,X)` do not. The
/// literal `conb` case folds in as `Const` exprs. Only
/// function-referenced inputs gate marking (an unreferenced floating
/// pin cannot block a mark — the same rule propagation taint uses);
/// sequential/latch drivers never enter the table and never mark.
/// Monotone in the information ordering, so the sweep's fixed point is
/// order-independent. Returns the marked set; the stored marginal is
/// `{0.0, 0.0}` on both polarities (the measured internal value — the
/// true constant value is only used by this marking logic).
fn constant_cone(design: &DesignDb, comb_funcs: &[Option<CombFunc>]) -> Vec<bool> {
    let mut vals: Vec<Option<bool>> = vec![None; design.num_nets()];
    // Seed: literal `function : "0"/"1"` drivers parse to `Const` exprs.
    for (n, f) in comb_funcs.iter().enumerate() {
        if let Some(CombFunc {
            expr: bool_expr::BoolExpr::Const(c),
            ..
        }) = f
        {
            vals[n] = Some(*c);
        }
    }
    // Two-phase sweep to a fixed point: newly-definite outputs are
    // collected on an immutable pass, then applied.
    loop {
        let mut newly = Vec::new();
        for (n, f) in comb_funcs.iter().enumerate() {
            if vals[n].is_some() {
                continue;
            }
            let Some(f) = f else { continue };
            let input_vals: Vec<Option<bool>> = f
                .input_nets
                .iter()
                .map(|&d| vals[d.0 as usize])
                .collect();
            if let Some(v) = bool_expr::const_eval(&f.expr, &input_vals) {
                newly.push((n, v));
            }
        }
        if newly.is_empty() {
            return vals.iter().map(|v| v.is_some()).collect();
        }
        for (n, v) in newly {
            vals[n] = Some(v);
        }
    }
}

/// Dump path → candidate driver pin: the last two `/`-components are
/// `inst`/`leaf` → cell `inst`'s pin `leaf`; otherwise the last component
/// → the io pin `leaf`. A pin annotates its net iff `pin_is_driver` —
/// input ports and instance output pins drive; output ports and instance
/// input pins do not (their dump entries are dead for net resolution).
/// Once a `cell`/`pin` pair resolves, the entry is decided here — a
/// non-driver cell pin never falls through to an io-pin name match.
fn annotation_net_of(
    design: &DesignDb,
    cell_of: &HashMap<&str, CellId>,
    io_pin_of: &HashMap<&str, PinId>,
    path: &str,
) -> Option<(PinId, NetId)> {
    let driver_net = |pin: PinId| -> Option<(PinId, NetId)> {
        design.pin_is_driver[pin.0 as usize]
            .then(|| design.pin_nets[pin.0 as usize].map(|n| (pin, n)))
            .flatten()
    };
    let mut parts = path.rsplit('/');
    let leaf = parts.next()?;
    if let Some(inst) = parts.next()
        && let Some(&cell) = cell_of.get(inst)
        && let Some(pin) = design.cell_pins(cell).iter().copied().find(|&p| {
            design.strings.resolve(design.pin_names[p.0 as usize]) == leaf
        })
    {
        return driver_net(pin);
    }
    driver_net(*io_pin_of.get(leaf)?)
}

/// Unified propagation: overwrites `Default` seed entries that have a
/// producer (`comb_funcs` or `seq_funcs` slot `Some`), in Kahn order over
/// the candidate-net dependency graph — seq-copy nodes join as
/// single-dependency nodes, so a resolved Q unblocks downstream
/// combinational candidates in the same pass (Q→gate→D→Q chains resolve;
/// sequential feedback cycles are mutual candidates that never reach
/// `indeg 0` and stay `Default`, exactly like true combinational
/// cycles). Consumes only resolvable inputs — any
/// `Default`/`Unknown`/`prob_one: None` input taints the output to
/// `Default` (the measured `unknown`-cone semantics; a `prob_one: None`
/// data net taints the Q copy like a combinational input).
fn propagate(
    comb_funcs: &[Option<CombFunc>],
    seq_funcs: &[Option<SeqCopy>],
    nets: &mut [ResolvedActivity],
    window_s: f64,
    unresolvable_functions: &mut usize,
) {
    let num_nets = comb_funcs.len();
    // Candidates: `Default` nets with a resolved producer.
    let is_candidate: Vec<bool> = (0..num_nets)
        .map(|n| {
            nets[n].origin == ActivityOrigin::Default
                && (comb_funcs[n].is_some() || seq_funcs[n].is_some())
        })
        .collect();

    // `indeg(m)` counts `deps(m)` that are themselves candidates — the
    // only edges that can block. Queue pops in `NetId` order for
    // deterministic output.
    let mut indeg = vec![0usize; num_nets];
    let mut dependents: Vec<Vec<NetId>> = (0..num_nets).map(|_| Vec::new()).collect();
    for n in 0..num_nets {
        if !is_candidate[n] {
            continue;
        }
        let deps = if let Some(cand) = &comb_funcs[n] {
            let mut deps = cand.input_nets.clone();
            deps.sort_unstable();
            deps.dedup();
            deps
        } else {
            vec![seq_funcs[n].as_ref().unwrap().data_net]
        };
        for d in deps {
            if is_candidate[d.0 as usize] {
                indeg[n] += 1;
                dependents[d.0 as usize].push(NetId(n as u32));
            }
        }
    }
    let mut queue: BTreeSet<NetId> = (0..num_nets)
        .filter(|&n| is_candidate[n] && indeg[n] == 0)
        .map(|n| NetId(n as u32))
        .collect();
    // The shared table stays immutable — `pending` carries the
    // enqueue-once invariant the `Option::take` slot used to.
    let mut pending = is_candidate;
    while let Some(m) = queue.iter().next().copied() {
        queue.remove(&m);
        // A candidate enqueues once — its `indeg` reaches 0 exactly once.
        debug_assert!(pending[m.0 as usize], "candidate dequeued twice");
        pending[m.0 as usize] = false;
        if let Some(cand) = &comb_funcs[m.0 as usize] {
            let mut resolvable = true;
            let mut p_in = Vec::with_capacity(cand.input_nets.len());
            for &dep in &cand.input_nets {
                let r = nets[dep.0 as usize];
                match r.prob_one {
                    Some(p)
                        if !matches!(
                            r.origin,
                            ActivityOrigin::Default | ActivityOrigin::Unknown
                        ) =>
                    {
                        p_in.push((p, r.eff_toggles / window_s))
                    }
                    _ => {
                        resolvable = false;
                        break;
                    }
                }
            }
            if resolvable {
                let p: Vec<f64> = p_in.iter().map(|&(p, _)| p).collect();
                match bool_expr::prob(&cand.expr, &p) {
                    Some(p_out) => {
                        let mut d_out = 0.0;
                        let mut eval_ok = true;
                        for (i, &(_, d_i)) in p_in.iter().enumerate() {
                            match bool_expr::sensitivity(&cand.expr, i, &p) {
                                Some(s) => d_out += d_i * s,
                                None => {
                                    eval_ok = false;
                                    break;
                                }
                            }
                        }
                        if eval_ok {
                            nets[m.0 as usize] = ResolvedActivity {
                                prob_one: Some(p_out),
                                eff_toggles: d_out * window_s,
                                origin: ActivityOrigin::Propagated,
                            };
                        } else {
                            *unresolvable_functions += 1;
                        }
                    }
                    None => *unresolvable_functions += 1,
                }
            }
        } else if let Some(copy) = &seq_funcs[m.0 as usize] {
            // Q = D: the resolved data-net record copies verbatim
            // (`eff_toggles` shares `window_s`); `1−P` on the inverted
            // state output. Origin `Propagated` — the golden's label
            // (OpenSTA prints no `sequential` source).
            let data = nets[copy.data_net.0 as usize];
            if let Some(p) = data.prob_one
                && !matches!(
                    data.origin,
                    ActivityOrigin::Default | ActivityOrigin::Unknown
                )
            {
                nets[m.0 as usize] = ResolvedActivity {
                    prob_one: Some(if copy.inverted { 1.0 - p } else { p }),
                    eff_toggles: data.eff_toggles,
                    origin: ActivityOrigin::Propagated,
                };
            }
        } else {
            debug_assert!(false, "candidate without a resolved producer");
        }
        // Visit order is scheduling only: a dependent whose input stayed
        // `Default` taints on its own evaluation.
        for &dep_m in &dependents[m.0 as usize] {
            indeg[dep_m.0 as usize] -= 1;
            if indeg[dep_m.0 as usize] == 0 {
                queue.insert(dep_m);
            }
        }
    }
}

/// Resolves every net's activity in the measured seed order.
pub fn resolve_activity(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    vcd: &VcdActivity,
    overrides: &ActivityOverrides,
) -> ActivityResolution {
    let window_s = if vcd.duration_s > 0.0 {
        vcd.duration_s
    } else {
        1.0
    };
    // First port-bearing clock row, per `Sdc::period_ns` — single-domain,
    // matching the probe.
    let period_ns = sdc.period_ns();
    let f_clk = if period_ns > 0.0 {
        1e9 / period_ns as f64
    } else {
        0.0
    };
    // `f_clk / 10.0`, not `0.1 * f_clk`, keeps `5e7 → 5e6` bit-exact.
    let input_seed = overrides.input.unwrap_or(ActivitySetting {
        density_per_s: f_clk / 10.0,
        prob_one: 0.5,
    });

    // One parse per driver `function`, shared by the constant marking
    // and the propagation pass; counted at build for any origin, so an
    // annotated net's broken function counts too. `seq_funcs` carries
    // the sequential producer table (Q=D copy) alongside.
    let mut unresolvable_functions = 0usize;
    let comb_funcs = comb_function_table(design, lib, &mut unresolvable_functions);
    let seq_funcs = seq_function_table(design, lib);
    let constant = constant_cone(design, &comb_funcs);

    // The pin-namespaced annotation map: one `SignalActivity` per net,
    // resolved from the dump paths that name the net's driver pin. Sorted
    // paths keep emission deterministic; entries naming nothing a driver
    // pin can claim are dead.
    let cell_of: HashMap<&str, CellId> = design
        .cell_names
        .iter()
        .enumerate()
        .map(|(i, &s)| (design.strings.resolve(s), CellId(i as u32)))
        .collect();
    let io_pin_of: HashMap<&str, PinId> = design
        .io_pins
        .iter()
        .map(|&p| (design.strings.resolve(design.pin_names[p.0 as usize]), p))
        .collect();
    let mut annotation: HashMap<NetId, SignalActivity> = HashMap::new();
    let mut dead_annotation_entries = 0usize;
    let mut dump_paths: Vec<(&String, &SignalActivity)> = vcd.pin_signals.iter().collect();
    dump_paths.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (path, sa) in dump_paths {
        match annotation_net_of(design, &cell_of, &io_pin_of, path) {
            Some((_pin, net)) => {
                // First path wins: distinct dump paths can collapse onto
                // the same driver pin (hierarchy deeper than the flat
                // cell namespace). Sorted iteration makes the choice
                // deterministic; on consistent dumps the colliding
                // streams are VCD aliases of one identifier code and
                // hold identical activity, so the rule only bites on
                // pathological dumps.
                annotation.entry(net).or_insert(*sa);
            }
            None => dead_annotation_entries += 1,
        }
    }

    // `-pins` per-pin user overrides keyed by `PinId` — `collect` keeps
    // the last entry on a duplicate pin (repeated `set_power_activity`
    // semantics; unmeasured, recorded). Entries whose pin is not a net
    // driver are dead `user` entries, counted once up front — the same
    // rule `annotation_net_of` applies to dump paths.
    let pin_overrides: HashMap<PinId, ActivitySetting> =
        overrides.pins.iter().copied().collect();
    let dead_user_entries = overrides
        .pins
        .iter()
        .filter(|&&(p, _)| {
            !design.pin_is_driver[p.0 as usize]
                || design.pin_nets[p.0 as usize].is_none()
        })
        .count();

    let mut nets = Vec::with_capacity(design.num_nets());
    for (n, &is_constant) in constant.iter().enumerate() {
        let net = NetId(n as u32);
        let resolved = if is_constant {
            // The stored marginal is {d=0, P=0} on both polarities —
            // the measured internal value, not a display artifact:
            // `xor(HI, a)` propagates `a`'s 0.55 (computed with
            // `P(HI)=0`), not `¬a`'s 0.45 (cp.def probe, 2026-11-21).
            // Only `constant_cone`'s marking uses the true polarity.
            ResolvedActivity {
                prob_one: Some(0.0),
                eff_toggles: 0.0,
                origin: ActivityOrigin::Constant,
            }
        } else {
            let driver: Option<PinId> = design.net_driver(net);
            let clock = driver
                .filter(|&d| design.pin_cells[d.0 as usize].is_none())
                .and_then(|d| {
                    let port = design.strings.resolve(design.pin_names[d.0 as usize]);
                    sdc.clocks.iter().find(|c| c.port.as_deref() == Some(port))
                });
            if let Some(clk) = clock {
                // `2/period` on the own clock row — multi-clock ready.
                // Cell-driven downstream clock nets are not `Clock`; they
                // fall to the propagation pass (named residual, unmeasured).
                ResolvedActivity {
                    prob_one: Some(0.5),
                    eff_toggles: 2.0e9 / clk.period_ns as f64 * window_s,
                    origin: ActivityOrigin::Clock,
                }
            } else if let Some(g) = overrides.global {
                ResolvedActivity {
                    prob_one: Some(g.prob_one),
                    eff_toggles: g.density_per_s * window_s,
                    origin: ActivityOrigin::Global,
                }
            } else if let Some(&u) = driver.and_then(|d| pin_overrides.get(&d)) {
                // `-pins <list>`: the net's driver pin was named —
                // per-pin user override, checked before the
                // `-input_ports` blanket (unmeasured order, recorded).
                ResolvedActivity {
                    prob_one: Some(u.prob_one),
                    eff_toggles: u.density_per_s * window_s,
                    origin: ActivityOrigin::User,
                }
            } else if let Some(ip) = overrides.input_ports.filter(|_| {
                driver.is_some_and(|d| design.pin_cells[d.0 as usize].is_none())
            }) {
                // `-input_ports` annotates only nets driven by an io pin —
                // undriven nets (`driver.is_none()`) keep the `input`
                // default below (measured: `s_input_ports`'s `n_float`
                // reads `input 5e6/0.5`). Origin `user` — the measured
                // source label.
                ResolvedActivity {
                    prob_one: Some(ip.prob_one),
                    eff_toggles: ip.density_per_s * window_s,
                    origin: ActivityOrigin::User,
                }
            } else if let Some(sa) = annotation.get(&net) {
                // Pin-namespace match: the net's driver pin was dumped.
                ResolvedActivity {
                    prob_one: sa.prob_one(),
                    eff_toggles: sa.eff_toggles(),
                    origin: ActivityOrigin::Annotated,
                }
            } else if driver.is_none()
                || design.pin_cells[driver.unwrap().0 as usize].is_none()
            {
                ResolvedActivity {
                    prob_one: Some(input_seed.prob_one),
                    eff_toggles: input_seed.density_per_s * window_s,
                    origin: ActivityOrigin::Input,
                }
            } else {
                // Interim fill for nets nothing resolved: the effective
                // input seed (api.md: unannotated nets get the P1-measured
                // default {P, d} instead of zero). The propagation pass
                // overwrites entries it can derive; the conversion below
                // turns the leftovers `unknown`.
                ResolvedActivity {
                    prob_one: Some(input_seed.prob_one),
                    eff_toggles: input_seed.density_per_s * window_s,
                    origin: ActivityOrigin::Default,
                }
            }
        };
        nets.push(resolved);
    }

    propagate(
        &comb_funcs,
        &seq_funcs,
        &mut nets,
        window_s,
        &mut unresolvable_functions,
    );

    // Every leftover `Default` — sequential feedback cycles, comb nets
    // tainted by them, seq outputs whose data pin is an expression form
    // or unclassifiable, comb eval failures — lands `unknown` {0,0},
    // the measured P3 semantics. `Some(0.0)` (not `None`) matches the
    // golden's printed prob and keeps `when` literals on unknown nets
    // expressible at weight 0 (chosen; unmeasured). Post-resolution
    // `Default` is an impossible origin.
    for r in &mut nets {
        if r.origin == ActivityOrigin::Default {
            *r = ResolvedActivity {
                prob_one: Some(0.0),
                eff_toggles: 0.0,
                origin: ActivityOrigin::Unknown,
            };
        }
    }

    let mut origin_counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &nets {
        *origin_counts.entry(r.origin.label().to_string()).or_insert(0) += 1;
    }
    ActivityResolution {
        nets,
        window_s,
        origin_counts,
        unresolvable_functions,
        dead_annotation_entries,
        dead_user_entries,
    }
}
