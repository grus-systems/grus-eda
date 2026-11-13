# E3.6: Make the DMP effective-capacitance solve fast

Design for the E3.6 task in `docs/e3-sta.md` Section 5 (speed: profile, then optimize).
Decisions here are the maintainer's; the implementation ticket is
`docs/design/e36/01-implement.md`.

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

Use it when `v(dt) <= L` (crossing at or after the ramp end) and the result is `>= dt`.
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
fallback for degenerate parameters (eigenvalues equal or ill-separated, non-finite
intermediates) and as the reference in tests. Guards: `rpi` here is already `> 0`
(`pi_charge` is only reached when `pi.rpi > 1e-9` and `rd` is clamped below by 1e-6 in
the caller), `c1` uses the same floor as today (`1e-4 * total`).

### 3d. `solve_dmp`: tolerance-stopped outer bisection

Stop the `ceff` bisection when `hi - lo <= 1e-4 * total` (relative 1e-4 on the load)
instead of always running 24 iterations. The bracket checks and the two early
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
