# Acceptance

- [ ] Call-count measurement recorded first (repeat
      share, bisection share); implemented mechanism
      chosen by the counts, named in design docs.
- [ ] Any memoization uses bitwise-exact keys
      (`to_bits`), scoped so no shared-mutable state
      crosses rayon threads; no approximate/quantized
      caching.
- [ ] All committed goldens byte-identical; `--json`
      `cmp`-clean pre/post on jpeg+ibex;
      `RAYON_NUM_THREADS` 1/4/10 `cmp`-clean;
      fail-proof recorded.
- [ ] `docs/results.md` before/after wall + phase table
      from `measure.sh` (REPS=5, same machine); achieved
      speedup and residual named.
- [ ] `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --release -p grus-core sta` pass;
      version bumped (patch) in the same commit.
