//! Per-net switching-activity resolution: turns annotation (VCD/SAIF), SDC
//! clocks, and the measured OpenSTA seed defaults into one resolved
//! `{density, prob}` record per net.
//!
//! The fill order is the measured precedence (docs/results.md 2026-11-21 N7):
//! constant cone → clock → `-global` override → annotation → input/undriven →
//! leftover `Default`. Combinational propagation (AP2), sequential rules (AP3),
//! and scoped inst-pin annotation (AP4) are not landed yet — `Default` is the
//! interim fill for every unseeded net and takes the effective input seed.
//!
//! Counts, not rates: `ResolvedActivity::eff_toggles` is the number of
//! effective transitions over the report window (`window_s`), so the power
//! arithmetic downstream (`switching_w`, `pin_internal_power`) is identical
//! for annotated and seeded nets. The defined conversion is
//! `count = rate × window_s`; `window_s` is `vcd.duration_s` when a dump is
//! loaded and positive, else `1.0` (absolute-density mode — counts then equal
//! rates numerically and the window cancels in `switching_w` regardless).

use crate::design_db::DesignDb;
use crate::interner::{NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::readers::vcd::VcdActivity;
use crate::sta::sdc::Sdc;
use serde::Serialize;
use std::collections::BTreeMap;

/// Where a net's resolved activity came from. Labels mirror OpenSTA's
/// `get_property <pin> activity` source column; `Propagated`/`Sequential`/
/// `Unknown` are declared for AP2/AP3 and are never emitted by AP1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ActivityOrigin {
    /// VCD/SAIF flat-name match (scoped inst-pin match: AP4).
    Annotated,
    /// Net driven by an SDC `create_clock` source port.
    Clock,
    /// `-global` override.
    Global,
    /// Input-port-driven or undriven net.
    Input,
    /// Combinational propagation (AP2).
    Propagated,
    /// Sequential rules (AP3).
    Sequential,
    /// `function : "0"/"1"` cone.
    Constant,
    /// Unresolvable sequential cone (AP3).
    Unknown,
    /// AP1 interim fill for unseeded nets.
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

    let mut nets = Vec::with_capacity(design.num_nets());
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let resolved = if constant[n] {
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
                // fall to the leftover (named residual, unmeasured).
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
            } else {
                let name = design.strings.resolve(design.net_names[n]);
                if let Some(sa) = vcd.signals.get(name) {
                    // Flat-name match, verbatim (scoped matching is AP4).
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
                    // Interim fill while AP2/AP3 are unlanded: the effective
                    // input seed (api.md: unannotated nets get the P1-measured
                    // default {P, d} instead of zero).
                    ResolvedActivity {
                        prob_one: Some(input_seed.prob_one),
                        eff_toggles: input_seed.density_per_s * window_s,
                        origin: ActivityOrigin::Default,
                    }
                }
            }
        };
        nets.push(resolved);
    }

    let mut origin_counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &nets {
        *origin_counts.entry(r.origin.label().to_string()).or_insert(0) += 1;
    }
    ActivityResolution {
        nets,
        window_s,
        origin_counts,
    }
}
