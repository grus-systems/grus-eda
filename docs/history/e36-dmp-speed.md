# E3.6: Make the DMP effective-capacitance solve fast

Design for the E3.6 task in `docs/e3-sta.md` Section 5 (speed: profile, then optimize).
Decisions here are the maintainer's; the implementation ticket is
`docs/history/e36/01-implement.md`.

## 1. Baseline (measured 2026-11-13, M4 Mac mini, release build)

- `cargo test --release`: 53 passed, 1 ignored (picorv32 placement RC), 0 failed.
- CLI, picorv32 SPEF STA: **10.45 s** wall (runs: 10.77 / 10.45 / 10.45), the binary's own
  report `elapsed=10440.52ms`, `setup wns=-1.722131 tns=-93.003738, hold wns=0.414952`.
  (The "about 16 s" in `docs/e3-sta.md` Section 5 was measured earlier on other hardware.)
- Target from Section 5: under 2 s on this design, with STA numbers unchanged.

## 2. Profile (macOS `sample`, 6 s window starting 2 s into the run above)

Top of stack, same collapsed:

| samples | symbol |
|---|---|
| 1960 | `exp` (libsystem_m) |
| 1560 | `solve_dmp` residual closure |
| 754 | `rc_slew` |
| 54 | `exp` stub |
| 37 | `nldm::eval_2d` |

The call graph shows essentially the whole sampled time under
`Context::driver_response` -> `solve_dmp`. Parsing, graph building, NLDM lookup and the
Elmore pass are not visible at this scale. **The cost is the DMP solve itself**, as
Section 5 suspected. Per solve (one net, one transition) the current code runs up to
26 residual evaluations; each residual runs `fit_ramp` (40 bisection steps, each step a
`rc_slew` = 2 crossings x 40 bisection steps of `rc_ramp_voltage`, i.e. up to ~3200
`exp` evaluations) plus `pi_charge` (200 implicit-trapezoid ODE steps). No parallelism
or caching is needed until this constant factor is fixed.

## 3. Changes (all inside `crates/grus-core/src/sta/dmp.rs`)

### 3a. `rc_crossing`: closed form after the ramp, tolerance-stopped bisection during it

For `u > dt` the response is `v(u) = 1 - (tau/dt) * exp(-u/tau) * (exp(dt/tau) - 1)`,
so the crossing of level `L` is

```
u = -tau * ln( (1 - L) * dt / (tau * (exp(dt/tau) - 1)) )
```

Use it when `v(dt) <= L` (crossing at or after the ramp end) and the result is
finite and `>= dt`; on a non-finite result, fall back to the bisection below.
Otherwise bisect as today, but stop when the bracket is at most `1e-6` ns wide instead
of always running 40 iterations. 1e-6 ns is 100x below the 1e-4 ns agreement budget in
Section 4, so this cannot move a reported number.

### 3b. `fit_ramp`: tolerance-stopped bisection

Same stop rule: iterate until the `dt` bracket is at most `1e-6` ns wide (today: fixed
40 iterations). The initial bracket logic and the early-return branch stay as they are.

### 3c. `pi_charge`: closed-form solution of the two-node pi circuit

The system

```
C1 V1' = (Vs - V1)/rd - (V1 - V2)/rpi
C2 V2' = (V1 - V2)/rpi
```

with `Vs` a ramp on `[t0, t0 + dt]` and constant 1 afterwards is linear with a
piecewise-affine input, so `V1`, `V2` and the delivered charge
`Q = integral (Vs - V1)/rd dt` all have closed forms: an affine term plus two
exponentials with the eigenvalues of the 2x2 system matrix (one regime for the ramp,
one for the saturated input, matched at `t0 + dt`).

Implement the closed form. Keep the existing 200-step integrator in the code as the
fallback for degenerate parameters (non-finite intermediates or an ill-conditioned
coefficient solve; equal eigenvalues provably cannot occur, see Section 6) and as
the reference in tests. Guards: `rpi` here is already `> 0`
(`pi_charge` is only reached when `pi.rpi > 1e-9` and `rd` is clamped below by 1e-6 in
the caller), `c1` uses the same floor as today (`1e-4 * total`).

### 3d. `solve_dmp`: tolerance-stopped outer bisection

Stop the `ceff` bisection when `hi - lo <= 1e-6 * total` instead of always running
24 iterations (at most 20 iterations from a full-width bracket; tightened from the
originally proposed 1e-4 by the review, see Section 6). The bracket checks and the two early
`lumped(...)` returns stay unchanged.

### 3e. Out of scope for this ticket

No `rayon`, no caching of solves, no change to `RD_SCALE`, the pi reduction, the NLDM
evaluation or `analysis.rs`. If the result is still above 2 s, profile again and write
a second ticket from the new profile.

## 4. Equivalence protocol (the fast version must give the same answer)

1. New unit tests in `dmp.rs`:
   - closed-form `pi_charge` vs the 200-step integrator over a grid of pi models
     (including a stiff case: `rpi` small but above the caller's floor, `c2 >> c1`),
     ramp durations and `tx` values, on both regimes (`tx` before and after `t0 + dt`):
     agreement within `1e-4` relative (the integrator's own trapezoid error scale).
   - `rc_crossing` closed form vs the current bisection at full 40 iterations for
     levels 0.2 / 0.5 / 0.8 over a grid of `(dt, tau)`: agreement within `1e-6` ns.
   - The three existing `solve_dmp` tests keep passing unchanged.
2. `test_dmp_ceff_matches_opensta_report_dcalc` keeps passing (PDK machine).
3. Full STA comparison tests on the PDK machine (gcd + picorv32, zero wire, placement
   RC, SPEF) keep passing with their existing tolerances.
4. CLI before/after on picorv32 SPEF and gcd placement RC: the printed WNS/TNS must
   agree within `1e-3` ns (expected: identical to ~1e-4).
5. `docs/results.md` gains an E3.6 entry: before/after wall times (same machine, same
   inputs), the profile summary, and what was run. `docs/e3-sta.md` Section 5 marks
   E3.6 done; `docs/status.md` drops "speed (E3.6) not started" from the E3 gaps.

## 5. Expected outcome

The dominant term drops from ~3200 `exp` evaluations plus 200 ODE steps per residual
to a handful of exponentials per residual and roughly half the residuals. Expected end
state: picorv32 SPEF STA well under the 2 s target on the mini. This is an estimate,
not a promise; the acceptance number is the measured one.

## 6. Review resolutions (external review, 2026-11-13)

The full review text is not included in this repository. The maintainer spot-checked the
load-bearing claims: the discriminant identity
`tr^2 - 4det = (a-g)^2 + b^2 + 2b(a+g) > 0` (both review parts derive it
independently), the ramp particular solution `p = [1/dt, 1/dt]`, the overflow bound
(the guard `v(dt) <= L` caps `dt/tau` at about 5.0 / 1.6 / 0.45 for L = 0.8 / 0.5 /
0.2, far below the exp overflow point), and the sensitivity bound in Q3. All check
out. Adopted changes to Sections 3-4:

- **3d stop tightened (Q3, S2):** the outer ceff bisection stops at
  `hi - lo <= 1e-6 * total`, not 1e-4. The reviewer showed the 1e-4 claim needs
  `max |dD/dc| * total < 2 ns`, which the bundle could not establish for the real
  tables; at 1e-6 the condition relaxes 100x and the iteration count is at most 20,
  still below the fixed 24 of today. Section 3d is amended accordingly.
- **3c fallback conditions redefined (Q1 F1/F2, Q5 #2):** equal eigenvalues cannot
  occur for caller-reachable parameters (`Delta > 0` always), so the fallback to the
  numeric integrator triggers on non-finite intermediates or an ill-conditioned
  coefficient solve, never on an eigenvalue-equality test. The integrator stays as
  the test reference, so the fallback path is exercised by the test suite itself.
- **3a finite guard (Q2 F4, Q5 #1):** if the closed-form crossing is not finite,
  fall back to the tolerance-stopped bisection. (An asymptotic form for
  `dt/tau >> 1` was offered and declined: unreachable for the actual levels, and the
  finite check plus fallback covers it.)
- **Test grid made concrete (Q1 F3, Q4 S2/S3, Q5 #3):** the pi_charge grid includes
  `tx = t0` (charge exactly 0), `tx = t0 + dt` (regime boundary; charge continuous
  across it), a large-`rpi` case and a strongly shielded case (`c2/c1 >= 100`); the
  rc_crossing grid includes, per level, cases where the closed form actually
  triggers (`v(dt) <= L`) and the boundary `L = v(dt)`.
- **solve_dmp golden regression (Q4 S2):** outputs of the current implementation
  for 15 (table, pi) cases were recorded by the maintainer before the change and
  are listed in ticket 01; the new implementation must reproduce them within 1e-5
  relative. This replaces the reviewer's "old vs new" comparison, which cannot run
  once the old code is gone.
- **Per-net STA dump declined (Q4 S2):** the existing comparison tests already
  assert every endpoint's arrival, required time and slack against OpenSTA (the
  stronger oracle); the CLI WNS/TNS check in Section 4.4 is an addition, not the
  only guard.
- **Assumptions documented (Q5 S3):** `solve_dmp`'s bisection, old and new, assumes
  the NLDM delay and slew are non-decreasing in load capacitance; `rc_slew` is
  non-decreasing in the ramp duration (the review's derivative argument is in the
  review text). Neither assumption is new.
- **Iteration-count assertions declined (Q4 S3):** brittle across parameter
  ranges; the speed claim is verified by the CLI timing acceptance instead.

## 7. Addendum (2026-11-13, maintainer): the charge must stay the trapezoid's

The ODE closed form of Section 3c was implemented and **rejected at acceptance**,
on measurements (Mac mini, RD_SCALE = 4.5):

- `test_dmp_ceff_matches_opensta_report_dcalc` failed: the most shielded arc
  (`_06198_/X` fall) moved from 5.5% above OpenSTA's Ceff (old code, inside the 6%
  tolerance) to 6.2% above.
- `test_picorv32_placed_spef_matches_opensta` failed with 74 endpoint mismatches
  (setup slack about 50-70 ps more pessimistic than OpenSTA across the design).
- A fine-integration study (trapezoid at 200,000 steps, tight bisection) shows the
  closed form is the *exact* solution of the DMP equations (agreement 6e-6 relative)
  and the old 200-step trapezoid is what deviates (up to 6e-3 relative on Ceff for
  stiff or large-`rpi` cases). The old numbers agree with OpenSTA because
  `RD_SCALE = 4.5` was fitted *through* the old integrator (E3.5): its error is part
  of the fitted model. A rescan (3.5 / 4.0 / 4.25 / 4.75 / 5.0) does not restore the
  picorv32 SPEF agreement (40-74 mismatches at every value), because the
  integrator's error varies per net and is not a scale factor.

Decision: **the model is the 200-step trapezoid as fitted; only its evaluation
cost may change.** Sections 3a, 3b and 3d stand (with them and the numeric charge,
all STA tests pass and the CLI takes 3.9 s, WNS within 3e-6 ns of the baseline).
Section 3c is replaced by 7.1.

### 7.1 `pi_charge`: closed form of the discrete recurrence

The trapezoid step in `pi_charge_numeric` is a constant-coefficient linear
recurrence. With `A` the 2x2 system matrix, `M = I - (h/2) A`, step size
`h = (tx - t0) / 200`:

```
v_{k+1} = M^{-1} (I + (h/2) A) v_k + M^{-1} ((h/2) (g/c1) (vs_k + vs_{k+1}), 0)
```

where `vs_k = clamp(k h / dt, 0, 1)` is affine in `k` up to the saturation index
and 1 after it. The homogeneous modes are geometric with ratios given by the
Cayley transform of `A`'s eigenvalues, `rho_i = (1 + h lam_i / 2) / (1 - h lam_i / 2)`;
the forcing is affine in `k` on each side of the saturation index, so `v_k` and the
trapezoid charge sum `Q = sum_k (h/2) (i_k + i_{k+1})`, `i_k = g (vs_k - v1_k)`,
both have closed forms (geometric and arithmetico-geometric sums). Evaluating them
reproduces the 200-step loop's result up to floating-point summation order
(expected agreement ~1e-12 relative, test tolerance 1e-9), at O(1) cost.

Fallback and reference stay as in Section 6: non-finite intermediates or an
ill-conditioned solve fall back to `pi_charge_numeric`, which also remains the
test reference — now at 1e-9 relative over the Section 6 grid, including cases
where the saturation index falls inside the interval and where `dt` is not a
multiple of `h`.

### 7.2 Equivalence protocol, updated

- The golden regression returns to the values recorded from the pre-E3.6
  implementation (ticket 01's table), tolerance 5e-5 relative (observed stop-rule
  noise is ~1e-5; the tolerance must stay far below the 6e-3 the ODE form shifted
  stiff cases by).
- The mini acceptance (full STA suite, dcalc test, CLI WNS/TNS within 1e-3 ns and
  under 2 s) is unchanged and is now the binding check that the model did not move.

### 7.3 Review resolutions (external review, 2026-11-13)

The full review text is not included in this repository. Verdict: adopt with
changes. The reviewer independently re-derived the recurrence from
`pi_charge_numeric` (step matrix, forcing indices, charge endpoint half-weights)
and found no off-by-one. Adopted:

- **Straddle step is a separate segment (F1/F2, S2/S3):** with
  `k* = ceil(dt / h)` the first saturated grid index, the input sequence
  `u_k = vs_k + vs_{k+1}` is affine for `k <= k* - 2`, a single mixed step at
  `k = k* - 1` (`u = (k* - 1) h / dt + 1`), and 2 from `k*` on. The closed-form
  sums must not run the affine segment through the straddle step.
- **Charge endpoints (F3, S3):** the trapezoid sum keeps its half-weight
  endpoints exactly as in the loop.
- **Fallback conditions extended (Q3, S2):** in addition to non-finite
  intermediates and an ill-conditioned coefficient solve, fall back when any
  `|rho_i| >= 1` (roundoff) or the eigenvector condition number exceeds 1e12.
- **Test tolerance form (Q3):** the 1e-9 agreement test uses
  `|Q_cf - Q_num| <= 1e-9 * max(|Q_num|, 1e-18) + 1e-18` so a near-zero charge is
  not failed on relative error alone.
- **Modal powers by `powi`** (S3), not `exp(k * ln rho)`.
