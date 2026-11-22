# Acceptance

- [ ] `place.legal.pass1/2/3` cost reduced
      algorithmically (pruning/incremental deltas/scan
      order); no rayon (ordered-move passes stay serial).
- [ ] `verify_legality` passes on all committed designs;
      E2 goldens green; per-design HPWL before/after
      recorded — >0.1% regression named+justified,
      5%-golden breach is hard fail.
- [ ] Deterministic across repeated runs (recorded).
- [ ] `docs/results.md` before/after `place` table +
      per-pass phase table from committed `measure.sh`
      (REPS=5, same machine): speedup vs ~×1.3 ceiling,
      residual named.
- [ ] `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`
      pass; version bumped (patch) in the same commit.
