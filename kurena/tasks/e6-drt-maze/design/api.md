# e6-drt-maze — API surface

Public surface as landed at `c1832dc`, unchanged at HEAD `cf34607`
(a docs-only checkpoint). The remaining work (docs, clippy, verify,
commit) touches no signatures; the internal notes below are the
code's current truth for whoever writes `docs/e6-drt.md` §7.

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

pub enum OccMode { Enabled, Disabled }   // Disabled = fail-proof knob:
                                         // foreign-Net checks + collision
                                         // recording skipped; Obs still blocks

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
// line = the mark line's coord on the perpendicular axis; [lo,hi] =
// overlap of the two shape extents along the line (lo > hi = a gap)

pub struct RouteDb {
    pub nets: Vec<NetRoute>,        // join.nets order
    pub unrouted: Vec<UnroutedNet>,
    pub collisions: Vec<Collision>, // post-hoc census (Enabled only)
    pub iterations: u32,            // ≤ max_iters
}

pub fn route_nets(db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks],
                  graphs: &RouteGraphs, vias: &ViaTable, cfg: &MazeConfig)
    -> RouteDb;

pub fn overlap_census(routes: &[NetRoute], tech: &TechDb,
                      tracks: &[LayerTracks], vias: &ViaTable)
    -> Vec<Collision>;

pub fn route_digest(routed: &RouteDb) -> u64;   // xxh3, nonzero evidence
```

## `drt/graph.rs` — public (landed)

```rust
pub fn build_route_graphs_ext(db, tech, tracks, join,
                              spare_above: u8) -> RouteGraphs;
// build_route_graphs(..) == build_route_graphs_ext(.., 0); the maze
// test calls it with 1.
```

## `drt/maze.rs` — private internals (current signatures)

```rust
enum Occupant { Obs(u32 /* cell; u32::MAX = no host */),
                Net(u32 /* NetId.0 */),
                Pin(u32 /* NetId.0 */) }

struct Interval { lo, hi, perp_lo, perp_hi, who: Occupant }
struct Mark { x_axis: bool, layer: u8, line: i32, iv: Interval }
struct Line { ivs: Vec<Interval>, max_hi: Vec<i32> }  // SKIP_CHUNK=16
struct Occupancy { lines_x, lines_y, halfw, spc }
struct Hit { iv: Interval, line: i32, lo: i32, hi: i32 }

fn hit_verdict(hits, host_cells, own, hist, hard, mode) -> Option<u32>;
fn edge_cost(g, tracks, tech, vias, occ, relax, host_cells, eidx,
             own, hist, hard, mode, hits) -> Option<u32>;  // #[allow(too_many_arguments)]
fn scan_stub(g, tracks, occ, n, l, x, y, out);             // needs the same allow
fn edge_collisions(g, tracks, vias, occ, relax, eidx, own, out, hits);

struct RelaxedVia { a: u32, b: u32, x: i32, y: i32, lower: u8 }
fn relaxed_vias(g, tech, tracks, vias) -> Vec<RelaxedVia>;
fn other_end(g, relax, eidx, v) -> u32;

struct NetTopo { attach, projected, attachable, host_cells, members,
                 relax, adj_start, adj_to, adj_eidx }
// built once per net in route_nets; occupancy/hist/hard-independent.

struct NetRun<'t> { topo, connected, in_tree, tree_nodes, used_edges,
                    dist, dstamp, parent, pstamp, hop, stamp }
impl NetRun<'_> {
    fn absorb_taps(&mut self);
    fn extend_once(&mut self, g, tracks, tech, vias, occ, own, hist,
                   hard, mode) -> Option<usize>;  // #[allow(too_many_arguments)]
}

fn route_one_net(db, tech, tracks, g, topo, vias, occ, hist, hard,
                 mode) -> NetOutcome;              // needs the same allow
fn commit_route(occ, tracks, vias, route, marks);
fn premark_obs(db, tech, tracks, occ);
fn project_attach(g, shapes, tracks) -> Vec<u32>;
```

## `tech_db.rs` (landed)

```rust
pub struct MacroDef {
    // ...
    /// OBS rects before `merge_connected_obs` — the maze marks the
    /// unmerged set (merged bboxes seal pin pockets).
    #[serde(default)]
    pub obs_rects_raw: Vec<(LayerId, RectDbu)>,
}
```

## Environment knobs

`GRUS_DRT_TIMING` (any value): `route_nets` prints premark/topo/
per-iteration/per-slow-net timings to stderr. Opt-in; stays in the
committed code — feeds `results.md` runtime evidence.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/tech_db.rs`
