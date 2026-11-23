# e6-drt-graph: guide-bounded track graph

E6 ticket 3, depends on `e6-drt-tracks`. Turns the per-net guide
(GCell boxes, already parsed by `congestion::guide`) into the
on-grid graph the maze search walks.

## Scope

1. **Guide intake** — read the committed `<d>.guide` files (same
   format `route --guide-out` emits; the parser already round-trips)
   into `NetGuide`s joined to `DesignDb` nets by name. A guide
   naming an unknown net is an error; a signal net missing from the
   guide is a named gap (report, not silent skip).
2. **Region mapping** — per (net, layer): the union of the net's
   guide boxes on that layer clipped to the track grid, i.e. the
   set of track indices (x and y) covered. Preferred-axis tracks
   with no coverage are excluded; a layer whose boxes miss every
   track contributes a named zero-coverage record.
3. **Graph model** — nodes `(layer, ix, iy)` index into the layer's
   `tracks_x`/`tracks_y` from `drt::tracks` — a node exists when
   BOTH coordinates are real tracks (x-track ∩ y-track crossings).
   Edges: along each axis between adjacent in-region track indices
   (the maze router decides which axis is legal on which layer —
   the graph offers both, cost assignment is later). Via edges are
   NOT edges yet — the layer-transition model is the next ticket's
   (via selection needs LEF via geometry).
4. **Access hookup** — each member pin's access points
   (`drt::access`) map onto graph nodes: an access point (layer, x,
   y) is attached when (x,y) is an in-region track crossing on that
   layer (x ∈ tracks_x ∧ y ∈ tracks_y ∧ node covered). Pins whose
   access points all land outside the region are named
   `region_escape` findings — expected nonzero only where the guide
   genuinely misses a pin (OpenROAD does this on some io/port nets —
   name the count, don't fudge it).

## Non-goals

- Maze search, costs, rip-up/reroute, via model, DEF emission.
- Re-deriving guides (E4 output is input here — the committed files).

## Files

- `crates/grus-core/src/drt/graph.rs` (new — region + node model)
- `crates/grus-core/src/drt.rs` (re-export)
- `crates/grus-core/tests/drt.rs` (region/graph census gate)
- `docs/e6-drt.md` (graph section)
- `docs/results.md` (dated entry)
- `docs/status.md`, `docs/roadmap.md` (progress line)
