# Ticket 03 (E3.6): closed form of the trapezoid recurrence for `pi_charge`

## Goal

Replace the ODE closed form from ticket 01 (`pi_charge_closed`, rejected at
acceptance — see `docs/history/e36-dmp-speed.md` Section 7) with the closed form of
the **discrete** 200-step trapezoid recurrence (design Section 7.1), so the charge
evaluation costs O(1) while reproducing the numeric integrator's results to
floating-point summation accuracy. Everything else from ticket 01 stays:
closed-form `rc_crossing`, tolerance-stopped bisections, the outer stop at
`1e-6 * total`, the version bump, and the docs updates listed there.

The working tree already contains ticket 01's patch plus the maintainer's golden
restoration (original recorded values, 5e-5 relative). Build on it; do not restart
from HEAD.

## Files

- `crates/grus-core/src/sta/dmp.rs` (implementation and unit tests)
- `docs/results.md` (extend the E3.6 entry: the Section 7 outcome — ODE form
  rejected at acceptance with the measured reasons, discrete form adopted; the
  maintainer completes the mini timings)

## Review resolutions (design Section 7.3, binding)

- The input sequence `u_k = vs_k + vs_{k+1}` has three pieces: affine up to
  `k* - 2`, the single straddle step at `k* - 1` with `u = (k* - 1) h / dt + 1`
  (where `k* = ceil(dt / h)`), and 2 from `k*` on. Do not run the affine segment
  through the straddle step.
- Keep the charge sum's endpoint half-weights exactly as the loop has them.
- Fallback triggers: non-finite intermediates, an ill-conditioned coefficient
  solve, any `|rho_i| >= 1`, or an eigenvector condition number above 1e12.
- Evaluate modal powers with `powi` (or equivalent repeated squaring), not
  `exp(k * ln rho)`.
- The 1e-9 agreement test compares as
  `|Q_cf - Q_num| <= 1e-9 * max(|Q_num|, 1e-18) + 1e-18`.

## Interface

`pi_charge` dispatches to the discrete closed form, falling back to
`pi_charge_numeric` on non-finite intermediates or an ill-conditioned solve
(same trigger policy as design Section 6). Names may follow the pattern
`pi_charge_discrete` / `pi_charge_numeric`. No public API changes.

## Acceptance

Run by you (no PDK on this machine; PDK-dependent failures are expected and must
be reported as such, not hidden):

- [ ] Discrete vs numeric charge agreement **1e-9 relative** over the ticket-01
      grid (6 pi models incl. stiff and strongly shielded, rd 0.2/1/5,
      dt 0.02/0.1/0.4, tx offsets incl. the regime boundary), **plus** cases where
      the ramp saturates inside the interval at a step that is not a multiple of
      the step size (choose dt / (tx - t0) ratios that are not 1/200 multiples,
      e.g. dt = 0.03 with tx - t0 = 0.07), and a case with `tx - t0 < dt`
      (charge evaluated entirely inside the ramp).
- [ ] `charge_is_exactly_zero_at_the_ramp_start_and_continuous_across_its_end`
      still passes (adapted to the discrete form where needed).
- [ ] The golden regression (original values, 5e-5 relative) now **passes**.
- [ ] The three pre-existing `dmp.rs` tests pass unchanged; all ticket-01 crossing
      tests pass unchanged.
- [ ] Red check: perturb the discrete form once (e.g. use the ODE eigenvalue
      instead of its Cayley ratio in one mode), see the 1e-9 test go red, restore,
      and report it.
- [ ] `cargo clippy -- -D warnings` and `cargo build --release` pass; PDK-free
      test subsets pass (report exactly which, and which failures are the known
      missing-PDK ones).

Run by the maintainer on the Mac mini from your patch: full `cargo test
--release`, dcalc and STA comparison tests, CLI picorv32 SPEF under 2 s with
WNS/TNS within 1e-3 ns of the pre-E3.6 baseline.

## Notes

- Design first: if the discrete form cannot reproduce the numeric integrator at
  1e-9 on some grid case, stop and report the case with numbers; do not loosen
  the tolerance or special-case it.
- The recurrence in the current `pi_charge_numeric` is the specification. Note
  its exact forcing discretization: at step k the input term uses
  `(vs(t-h) + vs(t)) / 2` with `vs` the clamped ramp — replicate that, including
  the charge sum's endpoint weights.
- Do not commit; the maintainer commits after the mini acceptance run.
