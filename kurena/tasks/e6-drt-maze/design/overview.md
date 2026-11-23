# e6-drt-maze — Overview

E6 ticket 5, the routing core: a sequential maze router that connects
each guided net's member pins inside its guide region on the track
graph built by `e6-drt-graph`, using the via model from `e6-drt-via`.
Non-goals stay as specced: no DEF emission, no full DRC during search,
no timing awareness, no track-assignment stage, no parallelism.

## Resume state (revised after card feedback)

**HEAD is WIP commit `a52d771`; it does not compile.** The checkpoint
contains the full router — occupancy model, attach/projection sets,
Dijkstra tree extension, sequential driver with rip-up, extraction,
collision census, digest — plus all four acceptance test groups in
`tests/drt.rs`, but it stopped mid-refactor: a new edge class
(`RelaxedVia`) was being threaded through the search. The remaining
work is finishing that refactor, then running the gates and writing
the docs. `steps.md` has the exact edit list.

What the checkpoint already landed:

- `drt/maze.rs` (2081 lines): all public types (`WireSeg`,
  `ViaPlace`, `NetRoute`, `UnroutedNet`, `FailKind`, `Collision`,
  `RouteDb`, `MazeConfig`, `OccMode`, cost constants), `Occupancy`
  with rasterized line intervals + rip-up `Mark`s, OBS/pin pre-mark,
  strict + projection attach, `NetRun` CSR + stamped Dijkstra,
  `route_one_net`, `route_nets` with the soft/hard iteration loop,
  `commit_route`, `overlap_census`, `route_digest`, in-module unit
  tests.
- `drt.rs`: `pub mod maze;` + re-exports + doc line. `design_db.rs`:
  `orient_rect` made `pub` for the OBS pre-mark and tests.
- `tests/drt.rs`: `MAZE_LEF`/`MAZE_DEF_*` fixture, `test_maze_cases`
  (all four acceptance cases), `test_maze_census`,
  `test_maze_occupancy`, `test_maze_determinism`, plus a temporary
  `test_maze_debug_gcd` harness (removal specced in `steps.md`).

## Ground truth discovered during implementation

Two mechanisms were added mid-implementation to fix the real-design
`NoPath` flood the first green run exposed — they are design changes
the docs must record:

- **Sparse shared crossings starve via placement.** Strict `ViaEdge`s
  need a same-`(x,y)` crossing on both layers. On sky130 the
  li1–met1 shared-x coincidence repeats every **7820 DBU** (li1
  x-pitch 460 vs met1 x-pitch 340, offsets 230 vs 170) and met1–met2
  is the same 7820 lattice in both axes; met2–met3 has **zero**
  shared crossings (the recorded mod-20 gap), met3–met4 sparse.
  Whole pin areas carry no strict via edge at all, so reachable
  regions around pins were disconnected. The fix: **relaxed via
  edges** — the maze synthesizes a transition wherever a real via can
  sit (a crossing of the two layers' *preferred* wires, not a
  same-layer-node crossing). See `search.md` §3. This also opens the
  met3-bridge nets: met2–met3 has a default candidate
  (`M2M3_PR`), so relaxed transitions exist even though strict ones
  cannot.
- **Stdcell OBS walls in the pins.** Macro `OBS` rects (li1 jumpers)
  cover the whole cell including pin rects, so every li1 pin's
  incident edges were `Obs`-blocked. The fix: the **pin-carve
  exemption** — an `Obs` mark is invisible to a candidate net when
  the marked rect touches a rect of that net's own pins on the same
  layer. See `occupancy.md` §2.

## Decisions (detail in the sibling docs)

1. **Sequential single-net ordering** in `join.nets` (guide-file)
   order — deterministic for free; pin-count ordering rejected.
2. **Pathfinder-style tree extension**: multi-source Dijkstra from
   the routed tree to the nearest unattached pin's node set.
3. **Integer hop costs** — `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 /
   HIST 8 / MAX_ITERS 8`, last iteration hard-conflict.
4. **Occupancy = per-line shape intervals** rasterized within the
   `w/2 + s` band on both axes; `Obs` always hard, foreign nets soft
   then hard, own net free, pin-carve exemption.
5. **Projection attachment** only when strict `pin_nodes` is empty —
   loosening counted and named via `NetRoute::projected`.
6. **Relaxed via edges** per net: every lower-layer node gets one
   transition whose true position sits on both layers' preferred
   wires; emitted in the unified edge index space after strict via
   edges (`search.md` §3).
7. **Rip-up rule**: re-queue unrouted nets plus both parties of every
   provisional collision; per-net `hist` escalation; cap 8.
8. **Named unrouted report** per pin (`Unattachable` vs `NoPath`) —
   never a panic, never an infinite loop.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
