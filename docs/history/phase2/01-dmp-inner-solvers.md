# DMP inner solvers: Brent/secant instead of bisection

## Goal

Cut the per-call cost of `Context::driver_response` on RC-loaded nets. jpeg
placement-RC STA spends ~4.4 s in the level loop over 448,726 calls (~9.8 µs
each); `sample` puts the time in the nested bisections inside `dmp.rs`
(`rc_crossing_bisect`, `fit_ramp_duration`, `pi_waveform_slew::crossing`, and
the `solve_dmp_with` residual loop). Replace them with bracketed
Brent/secant iterations that keep the same stop tolerances, so results move by
at most the existing golden tolerance (~1e-5).

## Files
- crates/grus-core/src/sta/dmp.rs
- crates/grus-core/src/sta/analysis.rs (only if a signature must change; prefer not)

## Interface

No public API change. Internal helpers may change freely. `solve_dmp`,
`solve_dmp_with`, `driver_waveform_slew`, `fit_ramp_duration`,
`ramp_fit_degenerate`, `pi_waveform_slew`, `PiModel`, `DmpResult` keep their
signatures and semantics.

## Acceptance

- `cargo test --release -p grus-core sta::` — every non-`#[ignore]`d test that
  passed before still passes, including
  `dmp::tests::golden_regression_values_are_reproduced` and every
  `*_matches_opensta` STA test (tolerances unchanged). The `#[ignore]`d jpeg
  placement-RC / placed-SPEF tests stay ignored (they are known OpenSTA
  disagreements, unrelated to this change).
- `cargo clippy --all-targets -- -D warnings` clean.
- jpeg `sta --parasitics placement` and `--spef`: report the new elapsed, and
  compare `wns`/`tns`/endpoint fields against the pre-change run — they must
  agree within ~1e-5 relative (or a 0.01 ps floor on near-zero values), the
  same bound as the Goal. Record both runs in `docs/results.md`.
- Prove the new iteration can fail (`AGENTS.md` §7 rule 7): temporarily
  perturb a Brent tolerance or bracket and show a test goes red, then restore.

## Notes

- `rc_crossing_bisect` brackets `[0, dt + 40*tau]` to `1e-6` ns. Edge cases:
  - `dt == 0.0`: `rc_ramp_voltage` divides by `dt` and yields NaN; today the
    bisection hides it (`NaN < level` is false, so `hi` collapses). A
    safeguarded Newton/Brent must special-case `dt <= 0.0` (the response is a
    pure RC step: crossing is `-tau * ln(1 - level)`) or otherwise reproduce
    the current answer exactly — `dmp.rs` tests call `rc_slew(0.0, 0.5)`.
  - `v(u)` is only C^0 at `u = dt` (the derivative jumps), and `dv/du = 0` at
    `u = 0`; a Newton step needs an interior start and a bisection fallback,
    not blind Newton.
- `fit_ramp_duration` bisects `dt` to `1e-6`; `rc_slew(dt, tau)` is monotone
  in `dt` — Brent's method applies.
- `pi_waveform_slew::crossing` bisects to `1e-7` over `[0, dt + 40*b1]`, 80
  iterations max — Brent applies; `v1` is monotone.
- `solve_dmp_with` residual loop: Brent needs only the sign-change bracket
  that is already established (`residual(lo) > 0 > residual(hi)`); strict
  monotonicity is NOT required and must not be assumed — the table is a
  piecewise-linear f32 NLDM interpolation and `delay_slope` is a finite
  difference.
- `is_lumped_bailout` is evaluated twice per `driver_response` (once inside
  `solve_dmp_with`, once inside `driver_waveform_slew`); computing it once and
  passing it through is allowed if it does not change results.
- Do not change `RD_SCALE`, `TRAPEZOID_STEPS`, tolerances, or the model. This
  ticket is constant-factor only.
- macOS bash 3.2 compatible; no new dependencies.
