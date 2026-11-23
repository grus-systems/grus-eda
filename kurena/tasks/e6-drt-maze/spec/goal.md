# e6-drt-maze: sequential maze router over the track graph

E6 ticket 5, depends on `e6-drt-via`. The routing core: connect
each net's pins inside its guide region on the track graph.

## Scope

1. **Per-net route** — multi-source/multi-sink search over the
   net's region graph (nodes = track crossings, edges = along-track
   + via transitions): shortest path from the routed tree to the
   nearest unconnected access-point set, iterated until all pins
   connect (pathfinder-style tree extension — start from the
   driver pin's access set).
2. **Preferred-direction costs** — traversing along the layer's
   `DIRECTION` axis costs less than the off-axis (the off-axis edge
   must exist for stubs/jogs but is penalized, e.g. 2×); via edge
   cost = the reference-weighted hop (record the constants).
3. **Occupancy + spacing-lite** — a committed route marks its
   track intervals occupied; later nets see the occupancy as cost,
   and a hard-block when the interval + the LEF `SPACING` bound
   collides with the committed one (reusing `TechDb` spacing — the
   full DRC check is the emit/e2e tickets' job). Macro OBS rects
   are pre-marked occupied where they intersect tracks.
4. **Net order + determinism** — fixed net order (design order or
   by pin count — record which and why); fixed iteration cap with
   rip-up-and-retry on congested nets (record the rip-up rule).
   Deterministic output: same input → same routes byte-for-byte.
5. **Unrouted reporting** — nets/pins that fail to connect inside
   the cap are named in a report (expected 0 on the real designs
   under the committed guides; any residual is named per-pin).

## Non-goals

- DEF emission of the routes (next ticket), full DRC-awareness
  during search (enclosure/min-area are post-checks), timing-aware
  routing, antenna rules, parallel search.
- Track assignment stage — access points already fix the pin
  attachment; intra-region TA is folded into the search itself.

## Files

- `crates/grus-core/src/drt/maze.rs` (new — search + occupancy)
- `crates/grus-core/src/drt.rs` (re-export + `Route`/`RouteDb` types)
- `crates/grus-core/tests/drt.rs` (synthetic maze cases + real-design
  connectivity census)
- `docs/e6-drt.md` (search section — costs, order, rip-up rule)
- `docs/results.md` (dated entry — connected/unrouted census, runtime)
