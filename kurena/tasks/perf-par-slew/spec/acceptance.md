# Acceptance

- [ ] `propagate_slew` computes each Kahn level's vertex
      slews in parallel (`par_iter`) and applies results
      in deterministic order; no shared mutable state
      inside the parallel bodies.
- [ ] All committed STA goldens byte-identical;
      `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --release -p grus-core sta` pass.
- [ ] Determinism: merge/apply order independent of thread
      scheduling (no HashMap-order or arrival-order
      dependence); recorded how proven.
- [ ] `docs/results.md` before/after table from committed
      `measure.sh` (picorv32/aes/ibex/jpeg sta+power
      parasitics cells, REPS=5, same machine): achieved
      speedup vs plan ceiling, residual named.
- [ ] Fail-proof evidence recorded (intentional-break run
      or equivalent proof the tests exercise the pass).
- [ ] Version bumped (patch) in the same commit.
