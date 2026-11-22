# Acceptance

- [ ] `docs/results.md` records, per endpoint kind (async-check,
      latch-D), the measured OpenSTA verdict: applied / ignored /
      errored, with the req rule if applied.
- [ ] Any kind OpenSTA applies is implemented and covered by a
      golden scenario at STRICT tolerance; any kind it does not
      is left counted with the measured evidence recorded.
- [ ] `docs/path-delays.md` §5 shows async/latch resolved and
      keeps two-clock-domain open with its blocker.
- [ ] `cargo test --workspace` and `cargo clippy --workspace
      --all-targets -- -D warnings` pass; version bumped in the
      same commit.
