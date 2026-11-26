# drt-preemit-spacing: check the emitted shape set before writing DEF

External review (S1): `emit.rs` performs no spacing check at all —
self-DRC exists but is an optional post-emit run
(`--self-drc`). Running the same spacing check over the *emitted*
shape set — including foreign pin shapes the occupancy model
treats as soft — before `write_routed_def` would have caught the
li1 pin-adjacency findings before they reached a file.

## Scope

1. Add a pre-emit spacing check over the exact shape set the emit
   produces (post-`apply_routes` geometry, same rectangles that
   self-DRC enumerates) — including instance pin shapes as
   obstacles, which the maze occupancy deliberately soft-prices.
   Reuse the existing self-DRC checker if its input covers the
   same set; do not build a third geometry engine.
2. Wire it into the `drt` CLI path as an opt-in flag
   (`--preemit-check` or make it part of `--self-drc` semantics —
   record the choice) that reports violations and, per the
   `drt-selfdrc-exit` contract, reflects them in the exit status
   if that ticket has landed (otherwise report-only, flagged as
   the known gap).
3. Report format reuses the self-DRC findings format — same rule
   names, same bbox output.
4. Fail-proof: a fixture DEF whose emit geometry violates spacing
   against a foreign pin must report ≥1 finding.

## Files

- `crates/grus-core/src/drt/emit.rs` (check over emitted shapes)
- `crates/grus-eda/src/main.rs` (flag wiring)
- `crates/grus-core/tests/drt.rs` (fixture + clean-case test)
- `docs/results.md` dated entry

Note: queue after `e6-drt-e2e`. Coordinate with
`drt-selfdrc-exit` — this check detects; that ticket decides how
detection reaches the exit code. If `drt-pin-collision` lands
first, this is the verification net that proves prevention works.
