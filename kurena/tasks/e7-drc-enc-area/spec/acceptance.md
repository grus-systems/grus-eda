# Acceptance — e7-drc-enc-area

- [ ] Enclosure rules measure cut-vs-metal overlap including the
      one-direction variants; both routed DEFs report 0 on every
      implemented enclosure rule.
- [ ] Min-area rules check merged polygon area per island; 0 violations
      on both designs.
- [ ] Hole-area rules detect holes inside merged metal polygons;
      a synthetic fixture with a sub-minimum hole flags `met1.7` or
      `met2.7`.
- [ ] Mutation fixtures: undersized enclosure around a via flags the
      matching `met*.4`/`via*.4a`-family rule; undersized-area island
      flags the matching `*.6` rule.
- [ ] Fail-proof per family recorded in results.md (break a margin
      comparison, watch the fixture go red, restore).
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/e7-drc.md` family table shows every `lef_expressible` family
      implemented; `docs/results.md` dated entry.
