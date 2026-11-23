# e6-drt-maze — API surface

New module `crates/grus-core/src/drt/maze.rs`, wired from `drt.rs`
as `pub mod maze;` with `pub use` re-exports for the route types
(the spec's `Route`/`RouteDb` surface lives at `drt::` level).
Imports: `crate::design_db::DesignDb`, `crate::drt::{access::pin_shapes,
graph::{NetGraph, RouteGraphs}, tracks::{Axis, LayerTracks},
via::ViaTable}`, `crate::interner::{LayerId, NetId, PinId}`,
`crate::tech_db::{RectDbu, TechDb}`, `std::collections::{BTreeMap,
BinaryHeap}`, `std::cmp::Reverse`.

## `drt/maze.rs`

```rust
// Recorded cost constants (docs/e6-drt.md §7).
pub const PREF_COST: u32 = 1;
pub const OFF_AXIS_COST: u32 = 2;
pub const VIA_COST: u32 = 4;
pub const OCC_PENALTY: u32 = 8;
pub const HIST_STEP: u32 = 8;
pub const MAX_ITERS: u32 = 8;

/// Whether committed foreign-net intervals price/block candidate
/// edges. `Disabled` is the fail-proof knob (`occupancy.md` §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccMode { Enabled, Disabled }

#[derive(Clone, Copy, Debug)]
pub struct MazeConfig {
    pub occ: OccMode,
    /// Iteration cap; the last iteration runs in hard mode.
    pub max_iters: u32,
}
impl Default for MazeConfig { /* Enabled, MAX_ITERS */ }

/// One committed wire segment in track coordinates — axis-aligned,
/// `x1 == x2` or `y1 == y2`, and `(x1,y1) <= (x2,y2)` so the order
/// is canonical. Width lives at emit time; occupancy rasterizes
/// `± w/2` itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireSeg {
    pub layer: LayerId,
    pub x1: i32, pub y1: i32,
    pub x2: i32, pub y2: i32,
}

/// One placed transition: the default via candidate of pair
/// `lower..lower+1` dropped at `(x, y)` (a shared crossing — the
/// position is a node on both layers by `ViaEdge` construction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViaPlace {
    pub lower: LayerId,
    pub x: i32,
    pub y: i32,
}

/// A fully connected net's committed geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct NetRoute {
    pub net: NetId,
    /// Merged per `(layer, line)` intervals, sorted canonically.
    pub wires: Vec<WireSeg>,
    /// Used via edges, sorted `(lower, x, y)`.
    pub vias: Vec<ViaPlace>,
    /// Member pins reached (`net_pins` order).
    pub connected: Vec<PinId>,
    /// Subset of `connected` attached via the projection fallback —
    /// named so the loosening is visible (search.md §2).
    pub projected: Vec<PinId>,
}

/// Why a member pin did not connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// No strict or projected attach node (carryover of
    /// `region_escape`/`off_track`).
    Unattachable,
    /// Attachable, but unreachable from the tree this round.
    NoPath,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnroutedNet {
    pub net: NetId,
    pub connected: Vec<PinId>,
    pub failed: Vec<(PinId, FailKind)>,
}

/// One recorded spacing collision between committed routes —
/// same-line interval overlap inside the `w/2 + s` band
/// (`occupancy.md` §4). `line` is the line's coordinate on the
/// perpendicular axis; `[lo, hi]` the shared extent along it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    pub a: NetId,
    pub b: NetId,
    pub layer: LayerId,
    pub line: i32,
    pub lo: i32,
    pub hi: i32,
}

/// The router's complete deterministic output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteDb {
    /// Connected nets, `join.nets` order.
    pub nets: Vec<NetRoute>,
    /// Nets that never fully connected inside the cap — named
    /// per pin, never a panic or hang.
    pub unrouted: Vec<UnroutedNet>,
    /// Post-hoc census on the final committed routes.
    pub collisions: Vec<Collision>,
    /// Iterations actually run (≤ `max_iters`).
    pub iterations: u32,
}

/// Sequential maze routing over `graphs`: per net, pathfinder-style
/// tree extension under the `occupancy.md` model, ordered per
/// `search.md` §5. `vias` prices transitions (`pairs[k].default`)
/// and supplies footprints; a `ViaGap` pair's edges are forbidden.
pub fn route_nets(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
    cfg: &MazeConfig,
) -> RouteDb;

/// Exact cross-net spacing violations among committed routes
/// (`occupancy.md` §4) — the reported collision list and the
/// fail-proof's measuring stick.
pub fn overlap_census(
    routes: &[NetRoute],
    tech: &TechDb,
    tracks: &[LayerTracks],
    vias: &ViaTable,
) -> Vec<Collision>;

/// `xxh3` digest over the canonical `RouteDb` — nonzero-evidence
/// determinism checking (`xxhash-rust` is a workspace dep).
pub fn route_digest(routed: &RouteDb) -> u64;
```

Internal (not exported): `Occupancy`, `Occupant`, the CSR adjacency
build, the Dijkstra round, commit/rip-up bookkeeping, and
`attach_set` (strict + projection fallback). In-module unit tests
cover the interval-band math, projection fallback on an escaped
pin, PQ tie order, and the two Dijkstra invariants (tree only grows,
backtracked path is simple).

## `drt.rs` delta

```rust
pub mod maze;
pub use maze::{Collision, FailKind, MazeConfig, NetRoute, OccMode,
               RouteDb, UnroutedNet, ViaPlace, WireSeg};
```

plus a module-doc line: `maze` routes each guided net inside its
region by sequential tree extension under interval occupancy, and
names whatever it cannot connect.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
