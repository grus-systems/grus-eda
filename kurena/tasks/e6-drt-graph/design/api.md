# e6-drt-graph — API surface

New module `crates/grus-core/src/drt/graph.rs`, re-exported from
`drt.rs` (`pub mod graph;`). Imports:
`crate::congestion::guide::{GuideBox, resolve_guide_nets}`,
`crate::design_db::DesignDb`, `crate::drt::access::pin_shapes`,
`crate::drt::tracks::{Axis, LayerTracks}`,
`crate::interner::{LayerId, NetId, PinId}`, `crate::tech_db::TechDb`.

```rust
/// One graph node: a track-index crossing inside a net's guide
/// region. `ix`/`iy` index `LayerTracks::tracks_x`/`tracks_y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RouteNode {
    pub layer: LayerId,
    pub ix: u32,
    pub iy: u32,
}

/// An in-plane edge between consecutive-index nodes. `axis` says
/// which track index differs (`X` = ix±1, `Y` = iy±1); both axes are
/// offered — legality and cost are the router's later concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteEdge {
    pub a: u32,          // index into `nodes`
    pub b: u32,
    pub axis: Axis,
}

/// One guided net's on-grid graph plus its pin attachments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetGraph {
    pub net: NetId,
    /// Sorted `(layer, ix, iy)`; index = node id.
    pub nodes: Vec<RouteNode>,
    /// In node order, each unordered pair once.
    pub edges: Vec<RouteEdge>,
    /// `(pin, attached node ids)` for each member pin in `net_pins`
    /// order; node ids sorted/deduped, empty iff the pin escaped.
    pub pin_nodes: Vec<(PinId, Vec<u32>)>,
}

/// Guide blocks resolved to nets plus the missing-net census.
/// `nets` keeps guide-file order.
#[derive(Clone, Debug, Default)]
pub struct GuideJoin {
    pub nets: Vec<(NetId, Vec<GuideBox>)>,
    pub missing_signal: Vec<NetId>,
    pub absent_expected: Vec<NetId>,
}

/// A (net, layer) with guide boxes but zero covered crossings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZeroCoverage {
    pub net: NetId,
    pub layer: LayerId,
}

/// A member pin of a guided net whose shapes contain no in-region
/// node on any layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionEscape {
    pub pin: PinId,
    pub net: NetId,
}

/// All guided nets' graphs plus the cross-net census.
#[derive(Clone, Debug, Default)]
pub struct RouteGraphs {
    pub nets: Vec<NetGraph>,            // guide-file order
    pub zero_coverage: Vec<ZeroCoverage>,
    pub region_escape: Vec<RegionEscape>,
    pub total_nodes: usize,
    pub total_edges: usize,
}

/// Joins parsed guide blocks to design nets: wraps
/// `resolve_guide_nets` (extra names bail there) and classifies the
/// absent `net_names` nets into `missing_signal` /
/// `absent_expected` per `guide-join.md`.
pub fn join_guides(
    db: &DesignDb,
    tech: &TechDb,
    parsed: &[(String, Vec<GuideBox>)],
) -> anyhow::Result<GuideJoin>;

/// Builds every guided net's region graph and pin attachments.
/// `tracks` is `drt::tracks::layer_tracks` output, parallel to
/// `tech.routing_layers`. Emission order is fixed (layer asc →
/// (ix,iy) asc → +x/+y edges in node order): two runs are identical.
pub fn build_route_graphs(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
) -> RouteGraphs;
```

## Implementation shape

- Region: `BTreeMap<u8, BTreeSet<(u32, u32)>>` per net — layer →
  covered pair set. Per box: two `partition_point`s give the covered
  index ranges; insert the cross product.
- Node ids: iterate the map (ascending layer, then sorted pairs) —
  id = push order; a `HashMap<(u8, u32, u32), u32>` or the `BTreeMap`
  itself for the `+1`-index edge lookups.
- `pin_nodes`: per member pin, `pin_shapes` rects → index ranges per
  layer → membership probes into the region set → collect matching
  node ids into a `BTreeSet<u32>` (dedup across rects/layers), then
  `into_iter().collect()`. Empty set → `region_escape` entry.
- `missing_signal`/`absent_expected` iterate `0..num_nets()` in
  `net_names` order; `io_pins` membership by `binary_search` (sorted
  per invariant I5).
- `access::pin_shapes` widens `fn` → `pub(crate) fn` — its contract
  (five-source shape assembly, `Placed|Fixed|Cover` gate) is already
  the right one; no behavior change.
- `drt.rs` gains `pub mod graph;` and one doc line extending the
  module summary to the region/graph model.

## Sizing note

`total_nodes` is the union of covered pairs — bounded by `Σ_box
|covered ix| · |covered iy|`; the test asserts that bound per net
(union ≤ sum of box areas). No `Vec` is sized by track count × layer
count × die.

## Files

- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/access.rs`
