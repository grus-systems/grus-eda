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
use crate::interner::{CellId, NetId, PinDir, PinId};
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
    /// unparseable, over the eval bound, or named variables matching no
    /// input pin of the cell (AP2 diagnostic).
    pub unresolvable_functions: usize,
    /// Dump entries matching no driver pin — flat internal net names,
    /// sink pins, unknown names. Diagnostic only.
    pub dead_annotation_entries: usize,
}

/// Marks a net `Constant` when its driver pin's Liberty `function` is the
/// literal `"0"`/`"1"` (the `conb` seed), then grows the cone: a net driven
/// by a cell output stays constant when the cell is not sequential/latch
/// and every input pin's net is already constant — constant-through-buffer
/// is this seed, not propagation (no gate-function evaluation). Worklist to
/// a fixed point; monotone, so cycles converge.
fn constant_cone(design: &DesignDb, lib: &LibDb) -> Vec<bool> {
    let n_nets = design.num_nets();
    let mut constant = vec![false; n_nets];
    for (n, c) in constant.iter_mut().enumerate() {
        let net = NetId(n as u32);
        let Some(drv) = design.net_driver(net) else {
            continue;
        };
        let Some(func) = design.pin_lib_pins[drv.0 as usize]
            .and_then(|lp| lib.pins[lp.0 as usize].function)
        else {
            continue;
        };
        let f = lib.strings.resolve(func).trim();
        if f == "0" || f == "1" {
            *c = true;
        }
    }
    loop {
        // Two-phase sweep: the newly-constant set is collected on an
        // immutable pass, then applied — monotone, so the fixed point is
        // the same as an in-place sweep's.
        let mut newly = Vec::new();
        for (n, &c) in constant.iter().enumerate() {
            if c {
                continue;
            }
            let net = NetId(n as u32);
            let Some(drv) = design.net_driver(net) else {
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
            // Mixed-input gates (one constant, one toggling input) are not
            // resolved — unmeasured; they fall through to the per-net pass.
            let all_constant = design.cell_pins(cell).iter().all(|&p| {
                design.pin_dirs[p.0 as usize] != PinDir::Input
                    || design.pin_nets[p.0 as usize]
                        .map(|n| constant[n.0 as usize])
                        .unwrap_or(false)
            });
            if all_constant {
                newly.push(n);
            }
        }
        if newly.is_empty() {
            return constant;
        }
        for n in newly {
            constant[n] = true;
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

/// A propagation candidate: the driver pin's parsed `function` plus the
/// cell's function-referenced input nets, in variable order.
struct Candidate {
    expr: bool_expr::BoolExpr,
    input_nets: Vec<NetId>,
}

/// AP2 combinational propagation: overwrites `Default` seed entries whose
/// driver is a non-sequential non-latch cell output, in Kahn level order
/// over the candidate-net dependency graph. Consumes only resolvable
/// inputs — any `Default`/`Unknown`/`prob_one: None` input taints the
/// output to `Default` (the sequential-feedback cones stay unresolved
/// until AP3). True combinational cycles never dequeue and stay `Default`.
fn propagate_combinational(
    design: &DesignDb,
    lib: &LibDb,
    nets: &mut [ResolvedActivity],
    window_s: f64,
    unresolvable_functions: &mut usize,
) {
    let num_nets = design.num_nets();
    let mut candidates: Vec<Option<Candidate>> = (0..num_nets).map(|_| None).collect();
    for n in 0..num_nets {
        if nets[n].origin != ActivityOrigin::Default {
            continue;
        }
        let Some(drv) = design.net_driver(NetId(n as u32)) else {
            continue;
        };
        let Some(cell) = design.pin_cells[drv.0 as usize] else {
            continue;
        };
        let Some(lib_cell) = design.cell_lib_ids[cell.0 as usize].map(|lc| &lib.cells[lc.0 as usize])
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
            *unresolvable_functions += 1;
            continue;
        };
        let mut input_nets = Vec::with_capacity(vars.len());
        let mut ok = true;
        for v in &vars {
            // A var matching no pin, or a pin with no net, makes the
            // function unresolvable (PG forms like `!VPWR+VGND` land here).
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
            candidates[n] = Some(Candidate { expr, input_nets });
        } else {
            *unresolvable_functions += 1;
        }
    }

    // `indeg(m)` counts `deps(m)` that are themselves candidates — the
    // only edges that can block. Queue pops in `NetId` order for
    // deterministic output.
    let mut indeg = vec![0usize; num_nets];
    let mut dependents: Vec<Vec<NetId>> = (0..num_nets).map(|_| Vec::new()).collect();
    for (n, cand) in candidates.iter().enumerate() {
        let Some(cand) = cand else { continue };
        let mut deps = cand.input_nets.clone();
        deps.sort_unstable();
        deps.dedup();
        for d in deps {
            if candidates[d.0 as usize].is_some() {
                indeg[n] += 1;
                dependents[d.0 as usize].push(NetId(n as u32));
            }
        }
    }
    let mut queue: BTreeSet<NetId> = (0..num_nets)
        .filter(|&n| candidates[n].is_some() && indeg[n] == 0)
        .map(|n| NetId(n as u32))
        .collect();
    while let Some(m) = queue.iter().next().copied() {
        queue.remove(&m);
        let Some(cand) = candidates[m.0 as usize].take() else {
            // A candidate enqueues once — its `indeg` reaches 0 exactly
            // once — so the slot is never empty here.
            debug_assert!(false, "candidate dequeued twice");
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

    let constant = constant_cone(design, lib);

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
            // The accessor reads `P 0.000` on both HI- and LO-side nets
            // (the probe's recorded display limit).
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

    let mut unresolvable_functions = 0usize;
    propagate_combinational(
        design,
        lib,
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
