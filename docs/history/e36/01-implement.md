# Ticket 01 (E3.6): speed up the DMP solve per the design

## Goal

Implement `docs/history/e36-dmp-speed.md` Sections 3a-3d **as amended by the review
resolutions in its Section 6** (external review):
closed-form post-ramp crossing with a finite guard, tolerance-stopped bisections,
closed-form `pi_charge` with the numeric integrator as fallback (trigger: non-finite
intermediates or an ill-conditioned coefficient solve — never an eigenvalue-equality
test) and test reference, and the outer ceff bisection stopped at
`hi - lo <= 1e-6 * total`.

## Files

- `crates/grus-core/src/sta/dmp.rs` (implementation and unit tests)
- `docs/results.md` (one E3.6 entry, appended to the log — the maintainer fills in
  the mini timings from the acceptance run; write the entry with the before/after
  fields and leave the measured values to be completed at acceptance if you cannot
  run them)
- `docs/e3-sta.md` (Section 5: mark E3.6 done)
- `docs/status.md` (E3 row: remove the "speed (E3.6) not started" gap)
- `Cargo.toml` (version bump 0.1.21 -> 0.1.22) and `Cargo.lock` (version lines only)

## Interface

No public API changes: `PiModel`, `DmpResult` and `solve_dmp` keep their signatures.
`pi_charge` stays private; the kept numeric integrator may be renamed
(e.g. `pi_charge_numeric`) with the closed form taking the `pi_charge` name, or the
other way round — one name, one fallback path, no dead code.

## Golden regression values (recorded from the current implementation, 2026-11-13)

`solve_dmp(pi, 1/ln 2, table)` for three tables — `lin1`: `(0.7c + 0.05, 2.2c + 0.02)`,
`lin2`: `(1.4c + 0.05, 4.4c + 0.02)`, `nl`: `(0.05 + 0.9 c^0.8, 0.02 + 1.4 c^0.9)` —
and five pi models: pi0 `(0.01, 5.0, 0.04)`, pi1 `(0.001, 2.0, 0.2)` (strongly
shielded, c2/c1 = 200), pi2 `(0.005, 0.05, 0.03)` (stiff), pi3 `(0.02, 50.0, 0.01)`
(large rpi), pi4 `(0.002, 0.5, 0.002)` — as `(c1, rpi, c2)`:

| case | ceff | delay | slew |
|---|---|---|---|
| lin1 pi0 | 1.355989813805e-2 | 5.949192869663e-2 | 4.983177590370e-2 |
| lin1 pi1 | 9.480459451675e-3 | 5.663632161617e-2 | 4.085701079369e-2 |
| lin1 pi2 | 3.421102732420e-2 | 7.394771912694e-2 | 9.526426011324e-2 |
| lin1 pi3 | 2.045850068331e-2 | 6.432095047832e-2 | 6.500870150328e-2 |
| lin1 pi4 | 3.899842679501e-3 | 5.272988987565e-2 | 2.857965389490e-2 |
| lin2 pi0 | 1.634207129478e-2 | 7.287889981270e-2 | 9.190511369705e-2 |
| lin2 pi1 | 3.239255642891e-2 | 9.534957900047e-2 | 1.625272482872e-1 |
| lin2 pi2 | 3.454947739840e-2 | 9.836926835775e-2 | 1.720177005529e-1 |
| lin2 pi3 | 2.076532274485e-2 | 7.907145184278e-2 | 1.113674200773e-1 |
| lin2 pi4 | 3.917413175106e-3 | 5.548437844515e-2 | 3.723661797047e-2 |
| nl pi0 | 1.250149369240e-2 | 7.702788325274e-2 | 4.712649929345e-2 |
| nl pi1 | 8.382279634476e-3 | 6.963053904824e-2 | 3.893014611227e-2 |
| nl pi2 | 3.337034612894e-2 | 1.092831236217e-1 | 8.563759741969e-2 |
| nl pi3 | 2.011763006449e-2 | 8.954612763281e-2 | 6.162432191976e-2 |
| nl pi4 | 3.884719073772e-3 | 6.061036206277e-2 | 2.947439946701e-2 |

## Acceptance

Run by you (this machine has no PDK; the PDK-dependent tests panic on missing files
here — that is expected, do not treat it as your failure, and do not skip them
silently in code):

- [ ] Golden regression test exists with the 15 cases above and passes within
      1e-5 relative on ceff, delay and slew.
- [ ] New unit tests from design Section 4.1, with the review's grid additions:
      closed-form vs numeric `pi_charge` (grid incl. pi1/pi2/pi3 above, `tx = t0`
      giving exactly 0, `tx = t0 + dt` and points just either side of it with charge
      continuous across the boundary, both ramp regimes, 1e-4 relative), and
      `rc_crossing` closed form vs the full 40-iteration bisection (levels
      0.2/0.5/0.8; per level, cases where the closed form triggers — `v(dt) <= L` —
      and the boundary `L = v(dt)`; 1e-6 ns).
      Prove each new test can fail: break the closed form on purpose once
      (e.g. drop a term), see red, restore, and say so in the job report.
- [ ] The three pre-existing `dmp.rs` tests pass unchanged.
- [ ] All PDK-free tests pass (`cargo test` filtered to the ones that do not need
      the PDK or design files: at minimum the whole `sta::dmp`, `sta::nldm`,
      `congestion` and `placement` synthetic tests); `cargo clippy -- -D warnings`
      and `cargo build --release` pass.

Run by the maintainer on the PDK machine (Mac mini) from your patch — not your job,
listed so you know what will be checked:

- Full `cargo test --release` (53 passed + new tests, 1 ignored), including
  `test_dmp_ceff_matches_opensta_report_dcalc` and all STA comparison tests.
- CLI picorv32 SPEF STA under 2 s with setup WNS/TNS and hold WNS within 1e-3 ns of
  the baseline (setup wns=-1.722131 tns=-93.003738, hold wns=0.414952).

## Notes

- Design first: if the closed form in 3c turns out numerically unstable for a case
  in the test grid, do not paper over it — report the case and the numbers back in
  the job report and keep the fallback for that regime; the maintainer decides.
- Do not touch `RD_SCALE`, `analysis.rs`, `nldm.rs` or `parasitics.rs`.
- Do not commit; the maintainer commits after the mini acceptance run.
