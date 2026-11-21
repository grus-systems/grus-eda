//! Per-net switching-activity resolution: turns annotation (VCD/SAIF), SDC
//! clocks, and the measured OpenSTA seed defaults into one resolved
//! `{density, prob}` record per net.
//!
//! The fill order is the measured precedence (docs/results.md 2026-11-21 N7):
//! constant cone → clock → `-global` override → annotation → input/undriven →
//! leftover `Default` — then AP2's combinational-propagation pass overwrites
//! `Default` entries it can derive (`Propagated`). Sequential rules (AP3) are
//! not landed yet — `Default` remains the interim fill for unresolvable cones
//! and takes the effective input seed.
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
/// `get_property <pin> activity` source column; `Sequential`/`Unknown`
/// are declared for AP3 and are never emitted yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ActivityOrigin {
    /// VCD/SAIF dump entry resolving to the net's driver pin.
    Annotated,
    /// Net driven by an SDC `create_clock` source port.
    Clock,
    /// `-global` override.
    Global,
    /// Input-port-driven or undriven net.
    Input,
    /// Combinational propagation: gate-function evaluation over resolved
    /// inputs, level-ordered over the net graph.
    Propagated,
    /// Sequential rules (AP3).
    Sequential,
    /// `function : "0"/"1"` cone.
    Constant,
    /// Unresolvable sequential cone (AP3).
    Unknown,
    /// Interim fill for nets the seeds and propagation cannot resolve.
    Default,
}

impl ActivityOrigin {
    /// The source column label, as OpenSTA's activity accessor prints it
    /// (`annotated` stands in for its per-mechanism `vcd`/`saif`/`user`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Annotated => "annotated",
            Self::Clock => "clock",
            Self::Global => "global",
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

#[derive(Clone, Copy, Debug, Default)]
pub struct ActivityOverrides {
    /// `-global`: hits every non-clock non-constant net — beats annotation,
    /// input, and the leftover fill (measured).
    pub global: Option<ActivitySetting>,
    /// `-input`: replaces the input default; also the leftover fill value.
    pub input: Option<ActivitySetting>,
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
    /// Nets left `Default` because the driver pin's `function` was
    /// unparseable, over a parse/eval bound, or named variables matching
    /// no input pin of the cell (AP2 diagnostic — counted at
    /// `comb_funcs` build for every origin, plus eval-side failures).
    pub unresolvable_functions: usize,
    /// Dump entries matching no driver pin — flat internal net names,
    /// sink pins, unknown names. Diagnostic only.
    pub dead_annotation_entries: usize,
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

/// AP2 combinational propagation: overwrites `Default` seed entries whose
/// `comb_funcs` slot is `Some`, in Kahn level order over the
/// candidate-net dependency graph. Consumes only resolvable inputs — any
/// `Default`/`Unknown`/`prob_one: None` input taints the output to
/// `Default` (the sequential-feedback cones stay unresolved until AP3).
/// True combinational cycles never dequeue and stay `Default` (not
/// function failures — `driven_nets_without_activity` sees them).
fn propagate_combinational(
    comb_funcs: &[Option<CombFunc>],
    nets: &mut [ResolvedActivity],
    window_s: f64,
    unresolvable_functions: &mut usize,
) {
    let num_nets = comb_funcs.len();
    // Candidates: `Default` nets whose function resolved at table build.
    let is_candidate: Vec<bool> = (0..num_nets)
        .map(|n| nets[n].origin == ActivityOrigin::Default && comb_funcs[n].is_some())
        .collect();

    // `indeg(m)` counts `deps(m)` that are themselves candidates — the
    // only edges that can block. Queue pops in `NetId` order for
    // deterministic output.
    let mut indeg = vec![0usize; num_nets];
    let mut dependents: Vec<Vec<NetId>> = (0..num_nets).map(|_| Vec::new()).collect();
    for (n, cand) in comb_funcs.iter().enumerate() {
        if !is_candidate[n] {
            continue;
        }
        let Some(cand) = cand else { continue };
        let mut deps = cand.input_nets.clone();
        deps.sort_unstable();
        deps.dedup();
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
        let Some(cand) = &comb_funcs[m.0 as usize] else {
            debug_assert!(false, "candidate without a resolved function");
            continue;
        };
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
    // annotated net's broken function counts too.
    let mut unresolvable_functions = 0usize;
    let comb_funcs = comb_function_table(design, lib, &mut unresolvable_functions);
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
                // overwrites entries it can derive; AP3 shrinks the rest.
                ResolvedActivity {
                    prob_one: Some(input_seed.prob_one),
                    eff_toggles: input_seed.density_per_s * window_s,
                    origin: ActivityOrigin::Default,
                }
            }
        };
        nets.push(resolved);
    }

    propagate_combinational(
        &comb_funcs,
        &mut nets,
        window_s,
        &mut unresolvable_functions,
    );

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
    }
}
