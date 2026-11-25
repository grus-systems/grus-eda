# drt-guide-escape: pins with no in-region attach point

External review (S2), confirmed as real by the picorv32 leg
(v0.12.9 recount: unrouted no-stub nets carrying met3+ guide
boxes exist — 48 bridged in the no-stub census; see
`docs/results.md` 2026-11-25): the route graph covers the
guide region plus the topmost layer only. A pin whose metal lies
outside the guide region — or whose in-region attach nodes are
walled by OBS — has no attach point and goes
`region_escape`/`Unattachable`/`NoPath`; no escape mechanism
exists. Global-routing guides are approximate by contract, so
"pin outside its own guide" is a normal occurrence, not an error.

## Scope

1. Re-baseline on post-e2e dev: `e6-drt-e2e` is redesigning pin
   access (`pin-access.md` — off-track/pin-anchored mcon access).
   Measure which escape class remains.
2. Design an escape for the remaining class: extend the per-net
   graph just enough to reach the pin (bounded halo around the pin
   rect, or a region-expansion retry for `region_escape` pins —
   record the choice and its cost).
3. Escape paths are still occupancy-checked — an escaped segment
   is normal committed metal, not a freebie.
4. Named failures stay named: a pin that remains unreachable after
   the escape reports its `FailKind` + the escape attempt in the
   unrouted report (see `drt-unrouted-census`).

## Files

- `crates/grus-core/src/drt/graph.rs` (attach/region_escape)
- `crates/grus-core/src/drt/maze.rs` (escape expansion/retry)
- `crates/grus-core/tests/drt.rs` (fixture: pin outside guide
  region on a lower layer)
- `docs/results.md` dated entry (fixture + gcd unrouted delta)

Note: queue after `e6-drt-e2e` — same pin-access subsystem. This
is the largest of the review tickets; keep it after the smaller
observability ones so its failure modes are measurable.
