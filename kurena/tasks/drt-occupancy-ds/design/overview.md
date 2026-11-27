# drt-occupancy-ds — overview

## Problem (external review)

`Occupancy` (`crates/grus-eda-core/src/drt/maze.rs` ~L283–615 — the
spec's `crates/grus-core` path is stale, the crate is `grus-eda-core`)
keys each layer's track lines in two `BTreeMap<i32, Line>` maps
(`lines_x`, `lines_y`). The review attributes ~70% of route CPU to
the scans: every candidate edge pays a `map.get(&t)` tree descent
(`scan_wire`, one per popped PQ arc in `extend_once`) and every via
footprint pays two `map.range(...)` walks (`scan_rect`), plus
`entry`/`get_mut` tree ops per mark in `rasterize`/`unmark`. The
BTreeMap part is pointer-chasing bookkeeping around the real work —
a per-line `lo`-sorted interval scan that is already cheap.

## What's actually hot (read of the code)

- `scan_wire` is called once per in-plane edge relaxation
  (`edge_cost`, `maze.rs` ~L950) and per committed edge
  (`edge_collisions`, ~L1116), plus relaxed-via stubs
  (`scan_stub`, ~L1035/1041) and the min-area patch walk
  (`patch_component`, ~L3614). It is a **point lookup**, not a range
  query — the review's "range query per layer" is imprecise, but the
  cost model holds: a tree descent + node chase per candidate edge.
- `scan_rect` is called per via-footprint rect per via edge
  (`edge_cost` ~L984, `edge_collisions` ~L1146) and in the
  topo/emit-time admission checks (`access_vias` ~L1567,
  `try_pin_paint` ~L2849, `close_voids` ~L3179, `fill_admitted`
  ~L3289, `try_fill` ~L3363). It is the true `BTreeMap::range` user.
- `rasterize` (`entry` per touched line × 2 axes) and `unmark`
  (`get_mut` per mark) ride the commit/rip-up path — per-net, not
  per-PQ-pop, but not free.

The goal's "measure what the queries look like" matters: if
`scan_rect` windows cover few lines (via footprints are ~2–6 lines
per axis on sky130), the win is killing the per-line tree-node
walk, not the interval scan. That is what the census counters in
`measurement.md` quantify before the swap.

## Decision (driver's pick, justified in `structure.md` §4)

**Dense track-indexed lines.** Marks exist only on track lines —
`rasterize` inserts exclusively at coordinates drawn from
`tracks[l].tracks_{x,y}` — so the key space is exactly
`0..tracks.len()`. Replace `BTreeMap<i32, Line>` with
`Vec<Line>` indexed by track position, keep `Line { ivs, max_hi }`
and its `SKIP_CHUNK` skip index byte-for-byte, and store the sorted
coordinate lists inside `Occupancy` for `Hit.line` reconstruction
and `scan_rect`/`rasterize` windowing. `scan_wire` takes the track
**index** every call site already holds (`RouteNode.ix/.iy`,
`patch_component`'s `line`); `scan_rect` resolves its window with
two `partition_point`s — the same bound math `rasterize` already
runs — then walks a contiguous slice.

Rejected: raster grid (occupancy answers need `Occupant` identity +
interval extents for `Collision`, `exempt_obs`, `hit_bbox` — a bitset
is lossy); interval tree (queries decompose to 1-D per line already —
the tree re-adds the pointer chasing being removed).

## Semantics contract

Occupancy is the router's spacing oracle: identical Hit **sets**
(for `scan_wire`, `scan_rect`) and identical iteration **order**
(dense slice order = BTreeMap ascending-key order) — order even
matches, though both consumers are documented order-insensitive.
Proof obligations, per `tests.md`:

1. Randomized differential test — the pre-change `BTreeMap`
   implementation copied verbatim into `maze.rs`'s `#[cfg(test)]`
   module as `RefOccupancy`, driven by a seeded op stream
   (marks/unmarks/queries) against the new structure.
2. `test_e2e_ours_def_deterministic_gcd` — the existing byte-exact
   routed-DEF golden — stays green (end-to-end cover: every query
   the real route actually issued).
3. A `route_digest` literal pin added to `maze_census` for gcd
   (covers `collisions`/`pin_collisions`/`iter_stats`, which the
   emitted DEF does not).

## Measurement contract

Scan share of route CPU recorded before and after: `sample`
symbol attribution + a committed `ScanStats` counter census under
the existing `GRUS_DRT_TIMING` gate + `route_ms` on gcd ≥5 reps
(min/median/max). picorv32 only if cheap (bounded run); unmeasured
is named, not smoothed. Full protocol: `measurement.md`.

## Non-goals

- No public-surface change: `route_nets`, `route_nets_with_progress`,
  `overlap_census`, `pin_overlap_census`, `route_digest`, `RouteDb`,
  `MazeConfig` keep their signatures. `Occupancy`/`Hit`/`Mark`/`Line`
  stay module-private.
- No `Line`/`Interval`/`Occupant`/`Hit` payload change — the per-line
  interval math (`lo`-sorted insert, `max_hi` skip index, `perp_*`)
  is untouched.
- No parallelism (`drt-rayon` is a queued sibling ticket), no
  behavior-cost tuning (`PREF_COST`, penalties, `OccMode`).

## Files

- `crates/grus-eda-core/src/drt/maze.rs`
- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
