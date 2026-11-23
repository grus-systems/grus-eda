# e6-drt-maze — API surface

Public surface as landed at `2c3c713`. The remaining work
(routability fix + performance levers) is internal; the public
contract below is expected to survive it unchanged. If the diagnosis
forces a public change (e.g. a third `Occupant`-visible config knob),
it is recorded in `docs/e6-drt.md` §7 and here.

## `drt.rs` (landed)

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

## `drt/graph.rs` — public (added at `2c3c713`)

```rust
pub fn build_route_graphs_ext(db, tech, tracks, join,
                              spare_above: u8) -> RouteGraphs;
// build_route_graphs(..) == build_route_graphs_ext(.., 0)
```

## `drt/maze.rs` — private internals (current signatures)

```rust
enum Occupant { Obs(u32 /* cell */), Net(u32 /* NetId.0 */) }
// Candidate addition if the diagnosis says pin metal must not
// hard-block: Pin(u32 /* NetId.0 */) — see occupancy.md §5.

struct Interval { lo, hi, perp_lo, perp_hi, who: Occupant }
struct Mark { x_axis: bool, layer: u8, line: i32, iv: Interval }
struct Occupancy { lines_x, lines_y, halfw, spc }
// `Hit`/`hit_rect`: the x_line field + fn are dead at HEAD — deleted
// in step 0; if a later lever needs perp info again, re-add then.

struct RelaxedVia { a: u32, b: u32, x: i32, y: i32, lower: u8 }
fn relaxed_vias(g, tech, tracks, vias) -> Vec<RelaxedVia>;

fn edge_cost(g, tracks, tech, vias, occ, relax, own_pins, eidx,
             own, hist, hard, mode, hits) -> Option<u32>;
fn edge_collisions(g, tracks, vias, occ, relax, eidx, own, out, hits);
fn other_end(g, relax, eidx, v) -> u32;

struct NetRun { /* attach sets, tree, CSR incl. relaxed arcs,
                   Dijkstra scratch */ }
impl NetRun {
    fn new(g, vias, relax, attach, projected, attachable) -> Self;
    fn extend_once(&mut self, g, tracks, tech, vias, occ, relax,
                   own_pins, own, hist, hard, mode) -> Option<usize>;
}

fn route_one_net(db, tech, tracks, g, vias, occ, hist, hard, mode)
    -> NetOutcome;   // computes relaxed_vias + NetRun per call
```

**Planned internal split** (perf lever L2, `perf.md`): a `NetTopo`
(attach sets + `relaxed_vias` + CSR adjacency — everything that does
not depend on occupancy/hist/hard) cached per net across iterations,
with `NetRun` reduced to per-attempt search state. No public change;
edge order and the unified index space are preserved bit-for-bit.

## `tech_db.rs` (added at `2c3c713`)

```rust
pub struct MacroDef {
    // ...
    /// OBS rects before `merge_connected_obs` — the maze marks the
    /// unmerged set (merged bboxes seal pin pockets).
    #[serde(default)]
    pub obs_rects_raw: Vec<(LayerId, RectDbu)>,
}
```

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
