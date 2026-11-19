# Acceptance

- [ ] `docs/results.md` records the measured OpenSTA `when`
      semantics: which probability model applies, whether
      `leakage_power` `when` states move the number, and the
      marginal(VCD/SAIF)-vs-joint verdict.
- [ ] `when`-qualified `internal_power` blocks evaluate per the
      measured rule on a golden fixture; only inexpressible
      `when` forms stay counted.
- [ ] Golden emitted by the committed script inside the
      LibreLane container; STRICT-tolerance test passes.
- [ ] `cargo test --workspace` and `cargo clippy --workspace
      --all-targets -- -D warnings` pass; `docs/roadmap.md` N7
      updated; version bumped in the same commit.
