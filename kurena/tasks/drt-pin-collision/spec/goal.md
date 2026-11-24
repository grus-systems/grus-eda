# drt-pin-collision: foreign-pin hits must cost the wire, not nothing

External review (S1 correctness): a wire may cross a foreign pin
shape with no router-side consequence —

- `Pin` marks are soft-priced even in `hard` mode
  (`maze.rs` `hit_verdict`: "foreign soft-priced — never a hard
  block"), so the final pass does not forbid the overlap.
- `Occupant::Pin` hits are excluded from `Collision` records
  (collision collection keeps only `Occupant::Net`), so the
  offending net is never ripped up for it.
- `project_attach` stubs grow metal to the pin rect — the cheapest
  overlap to produce.

Rationale for the current design is real (rip-up cannot move a
pin, so blocking on it can deadlock a net). The fix is on the
*wire* side: a foreign-pin hit must either become a recorded
collision whose victim is the overlapping net (wire gets ripped,
pin stays), or a hard block on the foreign pin's footprint
(overlapping geometry, not just the node under the pin).

## Scope

1. Re-baseline first: `e6-drt-e2e` may already land part of this
   (instance-hull + pin-rect rules) — measure the remaining gap on
   post-e2e dev before designing.
2. Choose and record the mechanism: foreign-pin hit recorded as a
   `Collision` (needs a net×pin or pseudo-net pairing so rip-up
   targets the wire) vs. hard block on foreign pin footprint in
   `hard` mode. Deadlock-avoidance must be designed (a net whose
   only access passes a foreign pin must fail named, not hang).
3. Emit-side honesty: any pin overlap that survives to emit is a
   named residual (or caught by `drt-preemit-spacing` if that
   landed first).
4. Fixture proof: a synthetic two-net case where the only cheap
   path crosses a foreign pin — router must produce a clean path
   or a named failure, never a silent overlap.

## Files

- `crates/grus-core/src/drt/maze.rs` (verdict/collision path)
- `crates/grus-core/src/drt/graph.rs` (if pin footprints need a
  mark kind)
- `crates/grus-core/tests/drt.rs` (fixture)
- `docs/results.md` dated entry (before/after on the fixture and
  on gcd)

Note: queue after `e6-drt-e2e` — overlapping scope, same files.
Pairs with `drt-preemit-spacing` (belt vs. suspenders: router-side
prevention vs. emit-side detection).
