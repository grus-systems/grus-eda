# drt-occupancy-ds: occupancy scan data structure

External review: `scan_rect`/`scan_wire`'s BTreeMap range scans are
~70% of route CPU — every candidate edge pays a range query per
layer. A rasterized occupancy grid or interval-per-row structure
turns the hot query into O(cells-touched) array reads.

## Scope

1. Measure first: a profile/harness naming the scan share of route
   time on gcd (and picorv32 if cheap enough — an existing census
   run can carry a counter; record the number honestly).
2. Replace or augment the per-layer `BTreeMap`-of-intervals
   occupancy with a structure where `scan_rect`/`scan_wire` answer
   by indexing (per-layer row intervals, raster grid, or interval
   tree — driver's pick after measuring what the queries actually
   look like: long thin wires vs small pin boxes).
3. Occupancy semantics must not change: same Hit sets for every
   query (a differential test driving random rect/wire queries
   against old-vs-new structures, or a recorded corpus replayed).
4. Collision census + route digests must be byte-identical to the
   current implementation on gcd (the occupancy answers the same
   questions — a digest change is a bug, not a tolerance).

## Files

- `crates/grus-eda-core/src/drt/maze.rs` (Occupancy internals — keep
  the public surface)
- `crates/grus-eda-core/tests/drt.rs` (differential/regression pin)
- `docs/results.md` dated entry (measured scan share before/after,
  route_ms on gcd)
