# Goal

AP1 of the activity-propagation sequence (plan:
`kurena/tasks/activity-propagation/design/api.md` §Plan — it is
authoritative for the measured OpenSTA semantics; the measurement
evidence is `docs/results.md` N7 2026-11-18 + the 12-session golden
`tests/designs/microbench/mba.activity.reference.json`).

Land the resolution skeleton and the measured seed defaults —
**no** combinational propagation, **no** sequential rules
(those are AP2/AP3).

## What lands

1. **Resolved-activity type.** Replace `PowerCx.net_activity:
   Vec<Option<SignalActivity>>` with a per-net resolved record
   (`ResolvedActivity { prob_one: Option<f64>, eff_toggles, origin }`
   + `enum ActivityOrigin`, shape per api.md §1). `net_activity()` /
   `net_prob_one()` read from it. Record the representation choice —
   (a) `eff_toggles` as an `f64` rate vs (b) synthesized integer
   counts = rate × `duration_s` — in the commit message and the
   results entry (api.md §1's note; mixing absolute rates with
   VCD-window counts needs a defined conversion either way).
   Per-origin diagnostic counts (annotated/clock/global/input/
   propagated/constant/unknown) land in `PowerDiagnostics` so the
   report shows OpenSTA's `report_activity_annotation` split shape;
   `driven_nets_without_activity` keeps counting genuinely
   unresolved nets only — the semantic change is recorded in
   `docs/results.md` per api.md §6.

2. **Seed order** (measured precedence, api.md §2):
   1. `constant` — `function : "0"/"1"` output pins **and nets
      reachable through constant-feeding gates** (the golden tags
      `u_bh`/`u_bl` outputs and ports `yh`/`yl` as `constant`, so
      constant-through-buffer is part of this seed, not AP2);
   2. `clock` — SDC clock nets `{2/period, 0.5}`;
   3. `global` — a user activity override hitting every
      non-clock non-constant pin (measured: beats `unknown`
      cones and annotation); API-level is sufficient, CLI flag
      optional;
   4. `annotation` — the existing VCD/SAIF per-net match
      (scoped inst-pin matching is AP4; keep today's name
      match);
   5. `input` — input ports and undriven nets get the measured
      `{0.5 prob, 0.1·f_clk density}`; a `-input`-style
      override replaces it (API level suffices for the
      golden);
   6. everything else unannotated — the interim rule while
      AP2/AP3 are unlanded: fill with the `input` default
      (per api.md's "unannotated nets get the P1-measured
      default {P,d} instead of zero"). The set of pins where
      this diverges from the golden's `propagated`/`unknown`
      values is the pinned expected-divergence list (below).

3. **CLI**: `grus-eda power` runs with neither `--vcd` nor
   `--saif` (today `required_unless_present` rejects it).
   `PowerReport.duration_s` semantics without a VCD window
   must be defined (absolute densities make duration cancel
   in `switching_w`; say what value lands and why).

4. **Golden coverage** — new test
   `test_activity_defaults_mba_matches_opensta`
   (`power.rs` `#[cfg(test)]`, same `require()` pattern as the
   existing power tests):
   - `s_global` and `s_global_density`: **every** pin's
     `{density, prob, source}` matches the golden verbatim,
     and group columns match at the established power bounds
     (internal 2% / switching 5% / total 1% / leakage 25% +
     1.5e-12 W floor — tighten if the honest measured bound is
     tighter). These sessions contain only `global`/`clock`/
     `constant` sources, so a strict full match is achievable
     with AP1's seeds alone.
   - `s_none` and `s_input`: every pin whose golden `source`
     is a seeded class (`input`, `clock`, `constant`, `vcd`,
     `global`) matches verbatim; pins whose golden source is
     `propagated`/`sequential`/`unknown` are asserted against
     an explicit expected-divergence table in the test (pin →
     golden value) so AP2/AP3 shrink it and any *new*
     divergence fails loudly.
   - Fail-proof: disable the fill → red; corrupt one golden
     block → red; restore (record in the entry).

5. **Existing goldens**: any `*.reference.json`/`*.power.json`
   that moves must do so only where a previously-unannotated
   net now gets the default — enumerate the moved files and
   the nets that explain each delta in `docs/results.md`
   (api.md §6: `driven_nets_without_activity` on partial VCDs
   drops by design). Fixtures fully annotated stay
   byte-identical; assert that explicitly.

## Files

- `crates/grus-core/src/power.rs` (type, fill pass, diagnostics,
  tests) — or `crates/grus-core/src/activity.rs` if the module
  split lands here (api.md §"what exists today" leaves the split
  to implement time)
- `crates/grus-eda/src/main.rs` (no-annotation CLI mode)
- `docs/results.md`, `docs/roadmap.md` (N7 line → AP1 done)
- `kurena/tasks/ap1-activity-defaults/` (design docs)
- `Cargo.toml` / `Cargo.lock`
