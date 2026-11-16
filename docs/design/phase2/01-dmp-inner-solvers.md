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

- `cargo test --release -p grus-core sta::` — all previously passing tests pass,
  including `dmp::tests::golden_regression_values_are_reproduced` and every
  `*_matches_opensta` STA test (tolerances unchanged).
- `cargo clippy --all-targets -- -D warnings` clean.
- jpeg `sta --parasitics placement` and `--spef`: report the new elapsed and
  confirm `wns`/`tns`/endpoint fields are within the test tolerances (they must
  be bit-identical or within f32 rounding of the pre-change run; record both).
- Prove the new iteration can fail: temporarily perturb a Brent tolerance or
  bracket and show a test goes red, then restore.

## Notes

- `rc_crossing_bisect` brackets `[0, dt + 40*tau]` to `1e-6` ns; `v(u)` is
  smooth and monotone, `dv/du` has a closed form per branch — a safeguarded
  Newton (bisection fallback) is acceptable and preferred over Brent here.
- `fit_ramp_duration` bisects `dt` to `1e-6`; `rc_slew(dt, tau)` is monotone in
  `dt` — Brent's method applies.
- `pi_waveform_slew::crossing` bisects to `1e-7` over `[0, dt + 40*b1]`, 80
  iterations max — Brent applies; `v1` is monotone.
- `solve_dmp_with` residual bisection (`hi - lo > 1e-6 * total`, ~20–26
  residuals/call): the residual is monotone decreasing in `c` per the comment
  at `residual`; Brent applies if sign-change bracket holds — verify, and keep
  bisection if monotonicity cannot be confirmed cheaply.
- `is_lumped_bailout` is evaluated twice per `driver_response` (once inside
  `solve_dmp_with`, once inside `driver_waveform_slew`); computing it once and
  passing it through is allowed if it does not change results.
- Do not change `RD_SCALE`, `TRAPEZOID_STEPS`, tolerances, or the model. This
  ticket is constant-factor only.
- macOS bash 3.2 compatible; no new dependencies.
