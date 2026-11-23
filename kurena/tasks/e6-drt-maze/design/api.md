# e6-drt-maze — API surface

Public surface as landed at `a52d771` (unchanged by the remaining
refactor — `RelaxedVia` and friends stay private).

## `drt.rs` (already landed)

```rust
pub mod maze;
pub use maze::{
    Collision, FailKind, MazeConfig, NetRoute, OccMode, RouteDb,
    UnroutedNet, ViaPlace, WireSeg,
};
```

## `drt/maze.rs` — public

```rust
// Cost constants (docs/e6-drt.md §7).
pub const PREF_COST: u32 = 1;
pub const OFF_AXIS_COST: u32 = 2;
pub const VIA_COST: u32 = 4;
pub const OCC_PENALTY: u32 = 8;
pub const HIST_STEP: u32 = 8;
pub const MAX_ITERS: u32 = 8;

pub enum OccMode { Enabled, Disabled }   // Disabled = fail-proof knob

pub struct MazeConfig { pub occ: OccMode, pub max_iters: u32 }
impl Default for MazeConfig { /* Enabled, MAX_ITERS */ }

pub struct WireSeg  { pub layer: LayerId, pub x1: i32, pub y1: i32,
                      pub x2: i32, pub y2: i32 }   // canonical order
pub struct ViaPlace { pub lower: LayerId, pub x: i32, pub y: i32 }

pub struct NetRoute {
    pub net: NetId,
    pub wires: Vec<WireSeg>,      // merged per (layer,line), sorted
    pub vias: Vec<ViaPlace>,      // sorted (lower,x,y), deduped
    pub connected: Vec<PinId>,    // net_pins order
    pub projected: Vec<PinId>,    // connected-via-projection subset
}

pub enum FailKind { Unattachable, NoPath }

pub struct UnroutedNet {
    pub net: NetId,
    pub connected: Vec<PinId>,
    pub failed: Vec<(PinId, FailKind)>,
}

pub struct Collision { pub a: NetId, pub b: NetId, pub layer: LayerId,
                       pub line: i32, pub lo: i32, pub hi: i32 }

pub struct RouteDb {
    pub nets: Vec<NetRoute>,        // join.nets order
    pub unrouted: Vec<UnroutedNet>,
    pub collisions: Vec<Collision>,
    pub iterations: u32,            // ≤ max_iters
}

pub fn route_nets(db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks],
                  graphs: &RouteGraphs, vias: &ViaTable, cfg: &MazeConfig)
    -> RouteDb;

pub fn overlap_census(routes: &[NetRoute], tech: &TechDb,
                      tracks: &[LayerTracks], vias: &ViaTable)
    -> Vec<Collision>;

pub fn route_digest(routed: &RouteDb) -> u64;
```

## `drt/maze.rs` — private internals (post-refactor signatures)

```rust
struct RelaxedVia { a: u32, b: u32, x: i32, y: i32, lower: u8 }

fn relaxed_vias(g: &NetGraph, tech: &TechDb, tracks: &[LayerTracks],
                vias: &ViaTable) -> Vec<RelaxedVia>;

fn edge_cost(g, tracks, tech, vias, occ, relax: &[RelaxedVia],
             own_pins, eidx, own, hist, hard, mode, hits) -> Option<u32>;

fn edge_collisions(g, tracks, vias, occ, relax: &[RelaxedVia],
                   eidx, own, out, hits);

fn other_end(g: &NetGraph, relax: &[RelaxedVia], eidx: u32, v: u32) -> u32;

struct NetRun { /* attach sets, tree, CSR incl. relaxed arcs, Dijkstra scratch */ }
impl NetRun {
    fn new(g, vias, relax: &[RelaxedVia], attach, projected, attachable) -> Self;
    fn extend_once(&mut self, g, tracks, tech, vias, occ,
                   relax: &[RelaxedVia], own_pins, own, hist, hard, mode)
        -> Option<usize>;
}

fn route_one_net(db, tech, tracks, g, vias, occ, hist, hard, mode)
    -> NetOutcome;   // computes `relaxed_vias` internally, once per call
```

The three `#[doc(hidden)] debug_*` fns (`debug_edge_stats`,
`debug_pin_approach`, `debug_net_wave`) and their test
`test_maze_debug_gcd` are deleted before the ticket lands — their own
doc comments say so; see `steps.md` §1.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
