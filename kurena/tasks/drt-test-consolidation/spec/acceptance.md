# Acceptance — drt-test-consolidation

- [ ] Same-config gcd `route_nets` executions in `tests/drt.rs`
      are consolidated to one run per fixture/config pair; runs
      with distinct inputs are not shared.
- [ ] Assertion count and digest/census pins unchanged; a recorded
      red-check (perturbed digest pin) proves the tests still fail.
- [ ] `docs/results.md` dated entry: before/after dev-profile
      drt-test wall time and per-run route counts.
- [ ] `cargo test --workspace --profile verify` and clippy
      `-D warnings` pass.
