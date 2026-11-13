# Ticket 01 (E3.6): speed up the DMP solve per the design

## Goal

Implement `docs/design/e36-dmp-speed.md` Sections 3a-3d exactly: closed-form
post-ramp crossing and tolerance-stopped bisections in `rc_crossing` / `fit_ramp`,
closed-form `pi_charge` with the numeric integrator kept as fallback and test
reference, and a tolerance-stopped outer bisection in `solve_dmp`. Then run the
equivalence protocol of Section 4 of the design on this machine (it has the PDK and
the generated picorv32 inputs) and record the outcome.

## Files

- `crates/grus-core/src/sta/dmp.rs` (implementation and unit tests)
- `docs/results.md` (one E3.6 entry, appended to the log)
- `docs/e3-sta.md` (Section 5: mark E3.6 done, with the measured time)
- `docs/status.md` (E3 row: remove the "speed (E3.6) not started" gap)
- `Cargo.toml` (version bump 0.1.21 -> 0.1.22) and `Cargo.lock` (version lines only)

## Interface

No public API changes: `PiModel`, `DmpResult` and `solve_dmp` keep their signatures.
`pi_charge` stays private; the kept numeric integrator may be renamed
(e.g. `pi_charge_numeric`) with the closed form taking the `pi_charge` name, or the
other way round — one name, one fallback path, no dead code.

## Acceptance

- [ ] New unit tests from design Section 4.1 exist and pass: closed-form vs numeric
      `pi_charge` (grid incl. a stiff case, both ramp regimes, 1e-4 relative) and
      closed-form vs full-bisection `rc_crossing` (levels 0.2/0.5/0.8, 1e-6 ns).
      Prove each can fail: at least once, break the closed form on purpose (e.g. drop
      a term), see the test go red, restore.
- [ ] The three pre-existing `dmp.rs` tests pass unchanged.
- [ ] `cargo test --release` on this machine: everything passes (53 passed, 1 ignored
      at baseline; the count may grow with the new tests), including
      `test_dmp_ceff_matches_opensta_report_dcalc` and all gcd/picorv32 STA
      comparison tests.
- [ ] CLI picorv32 SPEF STA (command in `docs/e3-sta.md` Section 6 style, using
      `grus-eda sta --parasitics spef --spef ...`): wall time under 2 s, and the
      printed setup WNS/TNS and hold WNS agree with the pre-change baseline
      (setup wns=-1.722131 tns=-93.003738, hold wns=0.414952) within 1e-3 ns.
      Measure the baseline yourself before changing code if you want a second
      confirmation; the numbers above are the maintainer's, same machine.
- [ ] `cargo clippy -- -D warnings` passes; `cargo build --release` passes.
- [ ] `docs/results.md` E3.6 entry records: machine, before/after wall times (CLI,
      3 runs each, median), the profile summary from the design Section 2, and the
      exact tests run.
- [ ] Commit with a Conventional Commit message (`perf(sta): ...`), the agent
      trailer, and the version bump included.

## Notes

- Design first: if the closed form in 3c turns out numerically unstable for a case
  in the test grid, do not paper over it — report the case and the numbers back in
  the job report and keep the fallback for that regime; the maintainer decides.
- Do not touch `RD_SCALE`, `analysis.rs`, `nldm.rs` or `parasitics.rs`.
- Run tests on this machine only; the picorv32 inputs under `tests/designs/picorv32/`
  are already generated here (git-ignored, do not commit them).
