# Acceptance — e7-drc-spacing

- [ ] Same-layer spacing runs on both routed DEFs: **0 violations** on
      every implemented spacing rule; results join `magic_per_rule`
      names 1:1.
- [ ] Same-net merge semantics documented and tested: touching wire
      stubs of one net do not self-flag; separated same-net islands do
      get checked (assert with a fixture).
- [ ] Mutation control: the recorded met2 0.03 µm-gap probe produces
      exactly `met2.2` violation(s) — no other spacing rule fires.
- [ ] Wide-metal rules implemented with named provenance for each
      threshold; both designs still 0.
- [ ] Negative/fail-proof: disable the merge or the edge-distance test,
      fixture goes red; restore — recorded in results.md.
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/e7-drc.md` family table updated; `docs/results.md` dated
      entry with per-rule 0 counts and mutation record.
