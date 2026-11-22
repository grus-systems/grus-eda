# Acceptance — e7-drc-geom

- [ ] `parse_def` retains routed geometry for signal nets on
      `tests/designs/{gcd,picorv32}/<d>.routed.def`: per-layer wire-rect
      and via-instance counts recorded; a round-trip/re-derivation test
      asserts the rect set is deterministic.
- [ ] Via instances resolve through `TechDb::via_defs` (same code path as
      `pdn.rs`), not name heuristics; unknown via names error loudly.
- [ ] Layer-shape assembly covers all four sources (signal wires,
      special wires, via layer rects, placed macro pin/obs rects); a test
      on a synthetic DEF+LEF fixture checks each source lands on the
      right layer with orientation applied.
- [ ] Width checker runs on both routed DEFs and reports **0 violations**
      on every implemented width rule; results join `magic_per_rule`'s
      zero expectation per rule name.
- [ ] Mutation fixture: a routed DEF modified to carry one below-min-width
      `met1` (or `met2`) wire produces exactly the expected width-rule
      violation(s) — fail-proof documented in results.md (break the
      checker, watch the fixture test go red, restore).
- [ ] Synthetic orientation fixture: a component at W or FE orient keeps
      its OBS shapes correctly transformed (the E0-era orientation bug
      class covered).
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/results.md` dated entry: what ran, per-layer shape counts on
      both designs, 0-violation result, mutation + fail-proof record;
      `docs/e7-drc.md` gains the LEF-bound provenance note and a family
      status table (width implemented, others pending).
