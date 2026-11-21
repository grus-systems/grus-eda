# AP1 — activity defaults: overview

First ticket of the AP1–AP4 sequence (`kurena/tasks/activity-propagation/design/api.md`
§Plan, the measured semantics; evidence: `docs/results.md` 2026-11-21 N7 entry +
`tests/designs/microbench/mba.activity.reference.json`, 12 sessions × 43 pins).

## What lands

1. **`activity.rs` resolution skeleton** — `ResolvedActivity` + `ActivityOrigin`
   replace `PowerCx.net_activity: Vec<Option<SignalActivity>>` with
   `Vec<ResolvedActivity>`; `net_activity()`/`net_prob_one()` read through it.
   New module `crates/grus-core/src/activity.rs` (not inside `power.rs`): AP2 adds
   the gate-function evaluator and AP3 the sequential rules to the same fill
   pass — one resolution pipeline, `power.rs` stays the consumer.
2. **The measured seed defaults, in precedence order** (api.md §2):
   `constant` → `clock` → `global` → `annotation` → `input` → leftover.
   The leftover takes the **interim** rule: the effective `input` seed — the
   `Default` origin. No combinational propagation (AP2), no sequential rules
   (AP3), no scoped inst-pin annotation (AP4).
3. **`grus-eda power` with no `--vcd`/`--saif`** — the `required_unless_present`
   gate drops; the run is all-defaults.
4. **Golden**: `test_activity_defaults_mba_matches_opensta` in `power.rs`
   `#[cfg(test)]` — `s_global`/`s_global_density` strict on all 43 pins +
   group columns; `s_none`/`s_input` strict on the 15 seeded-class pins and
   the 28 `propagated`/`unknown` pins pinned in an expected-divergence table.
5. **Diagnostics**: per-origin net counts in `PowerDiagnostics`;
   `driven_nets_without_activity` redefined to `Default`/`Unknown`-origin
   driven nets (semantic change recorded in `docs/results.md`).

## What does not land

- Gate-function evaluation (`LibPin.function` beyond the `"0"`/`"1"` constant
  test), levelized propagation, Q=D sequential fill, `unknown` emission —
  AP2/AP3. `ActivityOrigin::{Propagated, Sequential, Unknown}` exist for the
  labels but AP1 never emits them.
- Scoped inst-pin VCD matching, per-pin (not per-net) annotation, x/z
  half-weight *seeds* beyond the existing `SignalActivity::eff_toggles`,
  the gcd `report_power` re-comparison — AP4.
- CLI flags for the `-global`/`-input` knobs — API level only (`ActivityOverrides`);
  the golden test passes the measured values directly.

## Decisions to record in the results entry / commit message

- **Representation (b)**: `eff_toggles` stays a *count over the report window*
  (`f64`), not a rate. Annotated nets keep `sa.eff_toggles()` verbatim — the
  `switching_w`/`pin_internal_power` arithmetic is untouched, so fully-annotated
  fixtures stay **bit-identical** (`mb.power.json` watt columns do not move).
  Seeded nets synthesize `density_per_s × window_s`. The alternative (a), an
  `f64` rate with `/duration_s` dropped, reorders the multiplication/division
  for annotated nets and risks ulp moves on committed JSON. api.md's note is
  satisfied by the defined conversion `count = rate × window_s`.
- **`window_s` / `PowerReport.duration_s`**: `vcd.duration_s` when a dump is
  loaded and `> 0`, else `1.0` — counts equal absolute densities numerically
  and `duration_s` cancels in `switching_w` regardless. A `0`-duration VCD
  therefore behaves as absolute-rate mode (edge rule, recorded).
- **Constant `prob_one` = `Some(0.0)`** on both HI- and LO-side nets: the
  accessor's measured read prints `0.000` for both (the probe's recorded
  display limit). Whether OpenSTA's *internal* `when`-weight uses the truth
  value is unmeasured — named residual.
- **Leftover fill = effective input seed** (`input` override applied), origin
  `Default`. On `s_input` that coincidentally equals many golden `propagated`
  values; the source label differs, so those pins stay in the divergence
  table — the table keys on the *golden* source, not on equality.
- **Per-net, not per-pin**: OpenSTA's activity is per-pin (port `y` can hold
  `vcd` while `u_xor/X` on the same net is `propagated`); our model resolves
  per net. In the four AP1 sessions every seeded class is uniform across its
  net's pins, so a per-net model reproduces the golden exactly. The `vcd`
  per-pin divergence (`s_full` port `y`) is AP4 scope.
- **`driven_nets_without_activity`** counts driven nets whose origin is
  `Default` or `Unknown` — "no measured, seeded or derived source". On
  fully-annotated fixtures it stays 0; on `mba` `s_none` it reads 10.
- **Module split**: `crates/grus-core/src/activity.rs`, exported as
  `pub mod activity` (not glob — `power`/`sta` are not glob-exported either).

## Files

- `crates/grus-core/src/activity.rs` — new module: `ActivityOrigin`,
  `ActivitySetting`, `ActivityOverrides`, `ResolvedActivity`,
  `ActivityResolution`, `resolve_activity`, constant-cone pass
- `crates/grus-core/src/lib.rs` — `pub mod activity;`
- `crates/grus-core/src/power.rs` — `PowerCx` swap, `estimate_power` signature
  (`overrides: &ActivityOverrides`), per-origin diagnostics, `driven_nets_without_activity`
  redefinition, the new golden test, call-site updates
- `crates/grus-eda/src/main.rs` — `--vcd`/`--saif` both optional, `(None, None)`
  arm → `VcdActivity::default()`, activity-origin summary line, CLI parse test
- `tests/designs/microbench/mb.power.json` — regenerated by the committed CLI
  (expected diff: `diagnostics` gains `activity_origin_counts` only)
- `docs/results.md` — dated N7 AP1 entry
- `docs/roadmap.md` — N7 line: AP1 done, AP2–AP4 remain
- `kurena/tasks/ap1-activity-defaults/design/` — these docs
- `Cargo.toml`, `Cargo.lock` — `0.2.13` → `0.2.14` in the same commit
