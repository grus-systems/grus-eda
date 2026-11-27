# Acceptance — drt-test-consolidation

- [ ] Same-config gcd `route_nets` executions in `tests/drt.rs`
      are consolidated to one run per fixture/config pair; runs
      with distinct inputs are not shared.
- [ ] Assertion count and digest/census pins unchanged; a recorded
      red-check (perturbed digest pin) proves the tests still fail.
- [ ] `docs/results.md` dated entry: before/after dev-profile
      drt-test wall time and per-run route counts.
- [ ] `with_rayon_threads` unsoundness resolved: either no test
      mutates the process environment, or the unsafe justification
      is honest about coverage, panic-safe, and restores any
      pre-existing `RAYON_NUM_THREADS`.
- [ ] `cargo test --workspace --profile verify` and clippy
      `-D warnings` pass.
