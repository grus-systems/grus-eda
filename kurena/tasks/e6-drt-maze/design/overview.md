# e6-drt-maze — Overview

E6 ticket 5, the routing core: a sequential maze router that connects
each guided net's member pins inside its guide region on the track
graph built by `e6-drt-graph`, using the via model from `e6-drt-via`.
Depends on both (landed). Non-goals stay as specced: no DEF emission,
no full DRC during search, no timing awareness, no track-assignment
stage, no parallelism.

## Ground truth discovered while planning

- **The substrate is complete.** `drt::graph::NetGraph` gives per-net
  `nodes` (sorted `(layer, ix, iy)`), `edges` (in-plane,
  consecutive-index, both axes, `axis` recorded), `via_edges`
  (shared-crossing layer transitions, `lower < upper`), and
  `pin_nodes` (strict in-rect attachment, `net_pins` order, driver
  first per invariant I4). `drt::via::ViaTable` gives
  `pairs[k].default` (the policy pick) and per-candidate origin
  `rects` — the via footprint the occupancy model rasterizes.
  `db.net_driver(net)` already returns the driver `PinId`.
- **Region escapes carry over as the attachment problem.** The graph
  ticket pinned `region_escape` = gcd 17 (3 instance pins at GCell
  boundaries + 14 io ports whose ±140 DBU `met2` square holds an
  x-track but no y-track), picorv32 592 (355 inst + 237 io). Under
  the strict crossing rule these pins attach *zero* nodes — every
  net containing one is unroutable by construction. A **projection
  attachment** (same-track-line tap: nodes sharing a covered track
  index with a pin rect on that layer) is required for the
  acceptance's expected-0 census; it is designed in `search.md` §2
  and was pre-announced as needed in `docs/e6-drt.md` §5.
- **met2–met3 stays unroutable — measured bound, not a surprise.**
  The `e6-drt-via` planning recorded the pair's grids are disjoint
  mod-20 domains (0 shared crossings anywhere; the deferred fix is a
  node-model change, out of scope here). Planning probe on the
  committed guides — per net, is the `{li1,met1,met2}` box union
  connected (overlap + adjacent-layer pairing), and does adding
  met3+ boxes rejoin it:

  | design | guided nets | with met3+ boxes | low union disconnected | bridged by met3+ |
  |---|---|---|---|---|
  | gcd | 269 | 28 | **3** (`_126_`, `_155_`, `resp_msg[5]`) | 3 |
  | picorv32 | 6,744 | 313 | **209** | 209 |

  Every disconnected low union reconnects only through met3+ boxes —
  e.g. `resp_msg[5]` has two met1/met2 islands at
  `x ∈ [55200,75900]` and `x ∈ [82800,91950]` bridged solely by met3.
  These are the **upper bound** on unrouted nets: a net fails only if
  its member pins actually split across the components (unknown until
  the router runs — pin positions aren't in the guide). The census
  test names every unrouted net/pin and classifies the cause;
  `search.md` §6 defines the classification.
- **Occupancy inputs exist.** `RoutingLayer::min_spacing_dbu()`
  (plain `SPACING`, else smallest-width table row), `width_dbu`;
  `MacroDef::{obs_rects, pin_obs_rects}`; placed-cell transform is
  the `drc::geom` source-4 convention (gate on
  `{Placed,Fixed,Cover}`, `orient_rect` + translate — the ~10-line
  loop at `geom.rs:166-208`, replicated for OBS only). sky130 note:
  li1's `pitch < width + spacing`, so adjacent-line conflicts are
  real on li1 and dormant on met layers — the band model handles
  both uniformly.
- **Determinism precedent**: fixed emission order + double-run
  asserts with nonzero evidence; PQ ties break on node id.

## Decisions (detail in the sibling docs)

1. **Sequential single-net ordering** in `join.nets` (guide-file)
   order — deterministic for free, matches the census conventions;
   pin-count ordering rejected (arbitrary ties, no measured need).
2. **Pathfinder-style tree extension**: multi-source Dijkstra from
   the routed tree to the nearest unattached pin's node set, one pin
   at a time, starting from the driver's attach set (`search.md` §3).
3. **Integer hop costs** — `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 /
   HIST 8 / MAX_ITERS 8`, last iteration hard-conflict (`search.md` §4).
4. **Occupancy = per-line shape intervals** rasterized onto every
   track line within the spacing band; OBS + via footprints
   pre-marked; foreign-net hits are soft cost in early iterations and
   hard block on the last (`occupancy.md`).
5. **Projection attachment** only when a pin's strict `pin_nodes`
   set is empty — minimal loosening, pins counted and named
   (`search.md` §2).
6. **Rip-up rule**: re-queue unrouted nets plus both parties of every
   recorded collision; per-net history escalation; cap 8
   (`search.md` §5).
7. **Named unrouted report** per pin (`Unattachable` vs `NoPath`) —
   never a panic, never an infinite loop; the synthetic unroutable
   fixture proves the path (`tests.md`).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
