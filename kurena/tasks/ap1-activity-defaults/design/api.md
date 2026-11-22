# api — `activity.rs`, the fill pass, and the `power.rs`/`main.rs` deltas

## New module `crates/grus-core/src/activity.rs`

```rust
/// Where a net's resolved activity came from. Labels mirror OpenSTA's
/// `get_property <pin> activity` source column; `Propagated`/`Sequential`/
/// `Unknown` are declared for AP2/AP3 and are never emitted by AP1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ActivityOrigin {
    Annotated,   // vcd/saif flat-name match (scoped inst-pin match: AP4)
    Clock,       // net driven by an SDC `create_clock` source port
    Global,      // `-global` override
    Input,       // input-port-driven or undriven net
    Propagated,  // AP2
    Sequential,  // AP3
    Constant,    // `function : "0"/"1"` cone
    Unknown,     // AP3
    Default,     // AP1 interim fill for unseeded nets
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
    /// defined the state (matches today's `prob_one() == None` semantics:
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

pub fn resolve_activity(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    vcd: &VcdActivity,
    overrides: &ActivityOverrides,
) -> ActivityResolution;
```

`origin` → label for diagnostics/test comparison: `Annotated → "annotated"`,
`Clock → "clock"`, `Global → "global"`, `Input → "input"`,
`Propagated → "propagated"`, `Sequential → "sequential"`,
`Constant → "constant"`, `Unknown → "unknown"`, `Default → "default"`.
The test maps `Annotated` onto the golden's `vcd`/`saif`/`user` labels
(AP1 sessions never produce it).

## Fill pass (`resolve_activity`)

`window_s = if vcd.duration_s > 0.0 { vcd.duration_s } else { 1.0 }`.
`f_clk = if sdc.period_ns() > 0.0 { 1e9 / period_ns } else { 0.0 }`
(first port-bearing clock row, per `Sdc::period_ns`; single-domain, matching
the probe). `input_seed = overrides.input.unwrap_or({f_clk / 10.0, 0.5})` —
`f_clk / 10.0`, not `0.1 * f_clk`, keeps `5e7 → 5e6` bit-exact.

1. **Constant cone** — fixed-point pass, evaluated before everything:
   - Seed: a net whose driver pin's `LibPin.function` resolves (trimmed) to
     `"0"` or `"1"` → `Constant`, `{prob_one: Some(0.0), eff_toggles: 0.0}`.
   - Grow: a net driven by a cell output pin where the cell is *not*
     `is_sequential`/`is_latch` and **every** input pin's net is already
     `Constant` → `Constant`. Worklist until no change (monotone; cycles
     converge). Covers `u_c` outputs, the `u_bh`/`u_bl` buf cones, and
     ports `yh`/`yl` — constant-through-buffer is this seed, not AP2.
   - Mixed-input gates (one constant, one toggling input) are **not**
     resolved — unmeasured; they fall through to the per-net pass. Named
     residual (the mba fixture has no such case).
2. **Per net, first match wins** (each unresolved net, `net_names` lookup):
   - `driver` is a port pin (`pin_cells[drv].is_none()`) whose name satisfies
     `sdc.is_clock_port(name)` → `Clock`,
     `{prob: Some(0.5), eff: (2 × 1e9 / clock.period_ns) × window_s}` —
     the *own* clock row's period, not `period_ns()` (multi-clock ready).
     Cell-driven downstream clock nets (clock trees) are **not** `Clock` —
     they fall to the leftover; named residual, unmeasured.
   - `overrides.global` → `Global`, `{prob: Some(p), eff: d × window_s}`.
   - `vcd.signals.get(net_name)` → `Annotated`,
     `{prob: sa.prob_one(), eff: sa.eff_toggles()}` — today's flat-name
     match, verbatim (scoped matching is AP4).
   - `driver` has `pin_cells == None` (port-driven) **or**
     `net_driver(net).is_none()` (undriven) → `Input`, `input_seed × window_s`.
   - else → `Default`, `input_seed × window_s`.

`origin_counts` folds `nets` by `label()` (nonzero entries only).

## `power.rs` changes

- `PowerCx.net_activity: Vec<ResolvedActivity>` (field name stays);
  `net_activity()` returns `ResolvedActivity`, `net_prob_one()` returns
  `resolved.prob_one`. `net_eff_toggles`/`switching_w`/
  `pin_internal_power` consume `resolved.eff_toggles` — **no arithmetic
  change** (counts over `window_s`; `duration_s` stays the window).
- `estimate_power(..., vcd: &VcdActivity, pin_slews: &[PinSlew],
  overrides: &ActivityOverrides)` — new last parameter. Builds
  `resolve_activity` first; `cx.duration_s = resolution.window_s`;
  `PowerReport.duration_s` = `window_s` (semantics: annotation window, or
  `1.0` = absolute-density mode — `duration_s` cancels in `switching_w`,
  so the value only labels the JSON/print).
- `PowerDiagnostics` gains `pub activity_origin_counts: BTreeMap<String, usize>`
  (serde map; keys are the labels above). `driven_nets_without_activity`
  counts driven nets with `origin ∈ {Default, Unknown}` — doc comment
  updated ("unresolved" = no measured, seeded, or derived source).
- `conditional_power_blocks` doc note: a `when` literal on an unannotated
  net now evaluates at the seed probability instead of counting
  inexpressible (`prob_one` is `Some` on every AP1 origin except
  never-defined `Annotated` dumps).
- Module doc: activity is resolved by the measured seed order
  (`docs/results.md` 2026-11-21), counts are `window_s`-equivalents.

## `main.rs` changes

- `--vcd` loses `required_unless_present = "saif"` (keeps
  `conflicts_with = "saif"`); `--saif` unchanged. `(None, None)` arm →
  `(VcdActivity::default(), None)` replacing the `unreachable!`.
- `estimate_power` call gains `&ActivityOverrides::default()` (no CLI
  flags for the knobs in AP1 — recorded decision).
- The `Power (...)` headline prints the activity source: keep the window
  field when a dump was loaded (`activity window {:.3e} s`); with no
  annotation file print `activity: defaults (no annotation file)` instead
  of a misleading `1.000e+00 s` window.
- After the group rows, one unconditional line prints the origin split,
  e.g. `activity: clock=1 constant=4 input=3 default=10` (ordered
  annotated/clock/global/input/propagated/sequential/constant/unknown/
  default, zero entries skipped) — the `report_activity_annotation`
  analogue. The gated diagnostics `eprintln` is unchanged otherwise.
- `mod tests`: extend the clap parse test — `power --lib a --tlef b
  --lef c --def d --sdc e` (no `--vcd`/`--saif`) parses `is_ok()`; with
  both `--vcd` and `--saif` it stays `is_err()`.
