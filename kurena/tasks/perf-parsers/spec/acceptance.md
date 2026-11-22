# Acceptance

- [ ] SPEF/DEF/VCD parse time measurably reduced via
      mechanical scan/tokenize changes; no format-
      semantics changes; error paths (line-numbered
      rejects) unchanged and still tested.
- [ ] All committed goldens green (strict integration
      tests are the identical-parse proof); one
      intentional-break or round-trip check recorded.
- [ ] `docs/results.md` before/after table from
      committed `measure.sh` cells (REPS=5, same
      machine): per-phase and cell-wall speedups vs the
      plan's ~×2 estimate, residual named.
- [ ] `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`
      pass; version bumped (patch) in the same commit.
