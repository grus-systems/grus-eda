//! Sequential maze routing over the guide-bounded track graph: per
//! guided net, pathfinder-style tree extension under a rasterized
//! interval-occupancy model, in `join.nets` (guide-file) order, with
//! rip-up-and-retry on congestion and a named per-pin unrouted report.
//! See `docs/e6-drt.md` §7 for the model, the cost constants, and the
//! claim boundary.

use crate::design_db::{DesignDb, PlacementStatus};
use crate::drc::DrcLayer;
use crate::drt::access::pin_shapes;
use crate::drt::graph::{NetGraph, RouteGraphs};
use crate::drt::tracks::{Axis, LayerTracks};
use crate::drt::via::ViaTable;
use crate::interner::{LayerId, NetId, PinId};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use xxhash_rust::xxh3::xxh3_64;

// Recorded cost constants (docs/e6-drt.md §7).
pub const PREF_COST: u32 = 1;
pub const OFF_AXIS_COST: u32 = 2;
pub const VIA_COST: u32 = 4;
pub const OCC_PENALTY: u32 = 8;
pub const HIST_STEP: u32 = 8;
pub const MAX_ITERS: u32 = 8;

/// Whether committed foreign-net intervals price/block candidate
/// edges. `Disabled` is the fail-proof knob (`docs/e6-drt.md` §7):
/// foreign-net checks are skipped entirely (a fixed `Obs` blockage is
/// not "occupancy" and keeps blocking) and collision recording is
/// skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccMode {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug)]
pub struct MazeConfig {
    pub occ: OccMode,
    /// Iteration cap; the last iteration runs in hard mode (foreign
    /// hits are blocks, not costs).
    pub max_iters: u32,
}

impl Default for MazeConfig {
    fn default() -> Self {
        Self {
            occ: OccMode::Enabled,
            max_iters: MAX_ITERS,
        }
    }
}

/// One committed wire segment in track coordinates — axis-aligned,
/// `x1 == x2` or `y1 == y2`, and `(x1,y1) <= (x2,y2)` so the order is
/// canonical. Width lives at emit time; occupancy rasterizes `± w/2`
/// itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireSeg {
    pub layer: LayerId,
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
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
    /// named so the loosening is visible (`docs/e6-drt.md` §7).
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

/// One recorded spacing collision between committed routes — a
/// same-line interval pair inside the `w/2 + s` band. `line` is the
/// line's coordinate on the perpendicular axis; `[lo, hi]` is the
/// overlap of the two shape extents along the line (`lo > hi` encodes
/// a gap of `lo - hi` between them).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Nets that never fully connected inside the cap — named per pin,
    /// never a panic or hang.
    pub unrouted: Vec<UnroutedNet>,
    /// Post-hoc census on the final committed routes.
    pub collisions: Vec<Collision>,
    /// Iterations actually run (≤ `max_iters`).
    pub iterations: u32,
}

// ---------------------------------------------------------------------------
// Occupancy
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Occupant {
    /// Macro OBS — the owning cell index lets the pin-access carve
    /// exempt exactly the host cell's blockage (OpenROAD's "access
    /// shapes ignore own-instance OBS" rule). `u32::MAX` marks
    /// blockage with no exempting host: metal of pins that own no net.
    Obs(u32),
    /// A committed route's painted metal — the sequential router's
    /// true occupancy: cost in soft iterations, a hard block on the
    /// last.
    Net(u32), // NetId.0
    /// A member pin's placed metal — free to its own net, a soft
    /// penalty to foreign nets, never a hard block: a pin does not
    /// move under rip-up, so charging it as congestion would churn
    /// the queue forever without clearing the mark; a foreign wire
    /// that lands on it is a pin-access finding for the emit/e2e
    /// DRC, not a maze-time wall (`docs/e6-drt.md` §7).
    Pin(u32), // NetId.0
}

/// A committed shape's extent along one track line, tagged with its
/// occupant. `perp_*` is the shape's extent on the line's
/// perpendicular axis — needed so rect candidates (via footprints) and
/// the post-hoc census can run the exact `R' ∩ S` test, not just the
/// band approximation a wire candidate gets for free.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Interval {
    lo: i32,
    hi: i32,
    perp_lo: i32,
    perp_hi: i32,
    who: Occupant,
}

/// One rasterized insertion — the rip-up log. `x_axis` selects
/// `lines_x` (vertical lines) vs `lines_y` (horizontal).
#[derive(Clone, Copy, Debug)]
struct Mark {
    x_axis: bool,
    layer: u8,
    line: i32,
    iv: Interval,
}

/// Intervals on one track line, kept `lo`-sorted with a chunked
/// `max_hi` skip index (rebuilt eagerly on mutation) so a scan touches
/// only the query window: a chunk with `max_hi ≤ qlo` is skipped, and
/// `lo`-order lets the scan stop at `lo ≥ qhi`. The hit *set* is
/// unchanged vs a linear scan; hit order may differ and both consumers
/// are order-insensitive.
const SKIP_CHUNK: usize = 16;

#[derive(Clone, Debug, Default)]
struct Line {
    ivs: Vec<Interval>,
    /// `max(hi)` per `SKIP_CHUNK` chunk of `ivs` — parallel to
    /// `ivs.chunks(SKIP_CHUNK)`.
    max_hi: Vec<i32>,
}

/// Per-layer committed geometry rasterized onto track lines so an
/// edge's conflict check is a single-line interval query. `lines_x[l]`
/// keys vertical lines (x-coords) → y-intervals; `lines_y[l]` keys
/// horizontal lines → x-intervals.
struct Occupancy {
    lines_x: Vec<BTreeMap<i32, Line>>,
    lines_y: Vec<BTreeMap<i32, Line>>,
    /// `width_dbu / 2` per routing layer (integer division — sky130
    /// widths are even).
    halfw: Vec<i32>,
    /// `min_spacing_dbu()` per routing layer.
    spc: Vec<i32>,
}

/// One interval hit by a candidate edge: the rasterized mark (`iv`),
/// the line it was found on, and the overlap of the two shape extents
/// along the line (`lo > hi` encodes a gap).
#[derive(Clone, Copy, Debug)]
struct Hit {
    iv: Interval,
    line: i32,
    lo: i32,
    hi: i32,
}

impl Occupancy {
    fn new(tech: &TechDb) -> Self {
        let n = tech.routing_layers.len();
        Self {
            lines_x: vec![BTreeMap::new(); n],
            lines_y: vec![BTreeMap::new(); n],
            halfw: tech
                .routing_layers
                .iter()
                .map(|l| l.width_dbu / 2)
                .collect(),
            spc: tech
                .routing_layers
                .iter()
                .map(|l| l.min_spacing_dbu())
                .collect(),
        }
    }

    /// Inserts rect `r`'s shape extents into every track line inside
    /// the strict `w/2 + s` band on both axes: `[r.yl, r.yh]` onto
    /// `lines_x[l][x]` for `x ∈ (r.xl − w/2 − s, r.xh + w/2 + s)`, and
    /// `[r.xl, r.xh]` onto `lines_y[l][y]` for `y ∈ (r.yl − w/2 − s,
    /// r.yh + w/2 + s)`. A rect covering no track line inside a band
    /// contributes nothing on that axis — correct, since wire
    /// candidates only exist on lines.
    fn rasterize(
        &mut self,
        tracks: &[LayerTracks],
        layer: u8,
        r: &RectDbu,
        who: Occupant,
        mut log: Option<&mut Vec<Mark>>,
    ) {
        let l = layer as usize;
        let (w2, s) = (self.halfw[l], self.spc[l]);
        let tx = &tracks[l].tracks_x;
        let a = tx.partition_point(|&t| t <= r.xl - w2 - s);
        let b = tx.partition_point(|&t| t < r.xh + w2 + s);
        for &t in &tx[a..b] {
            let iv = Interval {
                lo: r.yl,
                hi: r.yh,
                perp_lo: r.xl,
                perp_hi: r.xh,
                who,
            };
            Self::insert(self.lines_x[l].entry(t).or_default(), iv);
            if let Some(m) = log.as_deref_mut() {
                m.push(Mark {
                    x_axis: true,
                    layer,
                    line: t,
                    iv,
                });
            }
        }
        let ty = &tracks[l].tracks_y;
        let a = ty.partition_point(|&t| t <= r.yl - w2 - s);
        let b = ty.partition_point(|&t| t < r.yh + w2 + s);
        for &t in &ty[a..b] {
            let iv = Interval {
                lo: r.xl,
                hi: r.xh,
                perp_lo: r.yl,
                perp_hi: r.yh,
                who,
            };
            Self::insert(self.lines_y[l].entry(t).or_default(), iv);
            if let Some(m) = log.as_deref_mut() {
                m.push(Mark {
                    x_axis: false,
                    layer,
                    line: t,
                    iv,
                });
            }
        }
    }

    /// `lo`-sorted insert — after existing equal-`lo` entries, so
    /// insertion order stays deterministic — plus the skip-index
    /// rebuild.
    fn insert(line: &mut Line, iv: Interval) {
        let pos = line.ivs.partition_point(|e| e.lo <= iv.lo);
        line.ivs.insert(pos, iv);
        Self::rebuild_from(line, pos / SKIP_CHUNK);
    }

    /// Recomputes the chunk `max_hi` index from chunk `from` onward —
    /// an insert/remove at position `p` can only dirty chunks at
    /// `p / SKIP_CHUNK` and later.
    fn rebuild_from(line: &mut Line, from: usize) {
        line.max_hi.truncate(from);
        line.max_hi.extend(
            line.ivs[from * SKIP_CHUNK..]
                .chunks(SKIP_CHUNK)
                .map(|c| c.iter().map(|iv| iv.hi).max().unwrap_or(i32::MIN)),
        );
    }

    /// Frees one recorded insertion (rip-up).
    fn unmark(&mut self, m: &Mark) {
        let map = if m.x_axis {
            &mut self.lines_x[m.layer as usize]
        } else {
            &mut self.lines_y[m.layer as usize]
        };
        if let Some(line) = map.get_mut(&m.line)
            && let Some(pos) = line.ivs.iter().position(|iv| *iv == m.iv)
        {
            line.ivs.remove(pos);
            Self::rebuild_from(line, pos / SKIP_CHUNK);
        }
    }

    /// Wire-candidate check on line `t` of layer `l`: the candidate
    /// shape spans track coords `[c, d]`, so its extent is
    /// `[c − w/2, d + w/2]` and hits are the line's intervals
    /// overlapping `[c − w/2 − s, d + w/2 + s]` — shape edge-distance
    /// `< s` along the line; the perpendicular `< s` is already
    /// guaranteed because the committed shape rasterized `t` only
    /// inside its `w/2 + s` band.
    fn scan_wire(
        &self,
        axis: Axis,
        layer: u8,
        t: i32,
        c: i32,
        d: i32,
        out: &mut Vec<Hit>,
    ) {
        let l = layer as usize;
        let (w2, s) = (self.halfw[l], self.spc[l]);
        let map = match axis {
            Axis::X => &self.lines_y[l],
            Axis::Y => &self.lines_x[l],
        };
        let Some(line) = map.get(&t) else {
            return;
        };
        let (qlo, qhi) = (c - w2 - s, d + w2 + s);
        let (slo, shi) = (c - w2, d + w2);
        for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
            if line.max_hi[ci] <= qlo {
                continue;
            }
            for iv in chunk {
                if iv.lo >= qhi {
                    return; // `lo`-sorted: no later interval can overlap
                }
                if iv.hi > qlo {
                    out.push(Hit {
                        iv: *iv,
                        line: t,
                        lo: iv.lo.max(slo),
                        hi: iv.hi.min(shi),
                    });
                }
            }
        }
    }

    /// Rect-candidate check (via footprints): hits iff the shape
    /// expanded by `s` intersects a committed shape — the
    /// rect-expand-by-`s`-and-intersect rule. Buckets by the marks
    /// within the `w/2 + s` band on both axes and filters each with
    /// the full 2D test on the stored perpendicular extent; a
    /// collision sliver containing no track line on either axis is the
    /// recorded miss class (spacing-lite).
    fn scan_rect(&self, layer: u8, r: &RectDbu, out: &mut Vec<Hit>) {
        let l = layer as usize;
        let (w2, s) = (self.halfw[l], self.spc[l]);
        let re = RectDbu::new(r.xl - s, r.yl - s, r.xh + s, r.yh + s);
        for (&t, line) in self.lines_y[l].range(re.yl - w2 - s..=re.yh + w2 + s) {
            for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
                if line.max_hi[ci] <= re.xl {
                    continue;
                }
                for iv in chunk {
                    if iv.lo >= re.xh {
                        break;
                    }
                    if iv.hi > re.xl && iv.perp_lo < re.yh && iv.perp_hi > re.yl {
                        out.push(Hit {
                            iv: *iv,
                            line: t,
                            lo: iv.lo.max(r.xl),
                            hi: iv.hi.min(r.xh),
                        });
                    }
                }
            }
        }
        for (&t, line) in self.lines_x[l].range(re.xl - w2 - s..=re.xh + w2 + s) {
            for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
                if line.max_hi[ci] <= re.yl {
                    continue;
                }
                for iv in chunk {
                    if iv.lo >= re.yh {
                        break;
                    }
                    if iv.hi > re.yl && iv.perp_lo < re.xh && iv.perp_hi > re.xl {
                        out.push(Hit {
                            iv: *iv,
                            line: t,
                            lo: iv.lo.max(r.yl),
                            hi: iv.hi.min(r.yh),
                        });
                    }
                }
            }
        }
    }
}

/// Pre-marks every placed cell's macro `obs_rects` as `Obs` (the
/// `drc::geom` source-4 convention: `{Placed, Fixed, Cover}` gate +
/// `orient_rect` + translate, replicated for OBS only), and every
/// instance pin's shapes as `Pin(own_net)` — a pin's metal is free to
/// its own net, soft-priced for foreign nets, and never a hard block
/// (rip-up cannot move it; a landing wire is an emit-time DRC
/// finding). Pins without a net get `Obs(u32::MAX)` — immovable
/// blockage with no exempting host. io-port rects are not marked
/// (recorded simplification — pin-access DRC is the emit/e2e tickets'
/// job).
fn premark_obs(db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks], occ: &mut Occupancy) {
    for c in 0..db.num_cells() {
        if !matches!(
            db.cell_status[c],
            PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
        ) {
            continue;
        }
        let mac = &tech.macros[db.cell_macros[c].0 as usize];
        let (cx, cy, orient) = (db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_orient[c]);
        let place = |r: RectDbu| {
            let r = DesignDb::orient_rect(r, mac.width_dbu, mac.height_dbu, orient);
            RectDbu::new(cx + r.xl, cy + r.yl, cx + r.xh, cy + r.yh)
        };
        for &(lid, r) in &mac.obs_rects_raw {
            if (lid.0 as usize) < occ.lines_x.len() {
                occ.rasterize(tracks, lid.0, &place(r), Occupant::Obs(c as u32), None);
            }
        }
    }
    for p in 0..db.pin_names.len() {
        let Some(c) = db.pin_cells[p] else {
            continue; // io-port rects unmarked (recorded above)
        };
        if !matches!(
            db.cell_status[c.0 as usize],
            PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
        ) {
            continue;
        }
        let pin = PinId(p as u32);
        let who = db.pin_nets[p].map_or(Occupant::Obs(u32::MAX), |n| Occupant::Pin(n.0));
        for (lid, r) in pin_shapes(db, tech, pin) {
            if (lid.0 as usize) < occ.lines_x.len() {
                occ.rasterize(tracks, lid.0, &r, who, None);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Attach sets
// ---------------------------------------------------------------------------

/// Index range `[a, b)` of `list` entries inside `[lo, hi]` — the
/// `drt::graph` convention, replicated for the projection fallback.
fn covered_range(list: &[i32], lo: i32, hi: i32) -> (usize, usize) {
    (
        list.partition_point(|&t| t < lo),
        list.partition_point(|&t| t <= hi),
    )
}

/// The projection fallback (`docs/e6-drt.md` §7): for each candidate
/// rect `(l, r)` of an escaped pin, the in-region nodes on `l` in the
/// rect's track neighborhood — `ix` and `iy` each within one line of
/// the covered ranges, on at least one covered line. A met2 io-port
/// square holds an x-track but no y-track; the wire that taps it runs
/// along that x-track, so the adjacent-line nodes on it are the
/// connection points. The one-line margin matters: far nodes on a
/// covered line can coincide with *another* pin's attach node, which
/// `absorb_taps` would then claim as a zero-edge connection.
/// Computed on the sorted node order — deterministic, no re-sort.
fn project_attach(g: &NetGraph, shapes: &[(LayerId, RectDbu)], tracks: &[LayerTracks]) -> Vec<u32> {
    let mut out = BTreeSet::new();
    for &(lid, r) in shapes {
        let l = lid.0 as usize;
        let Some(lt) = tracks.get(l) else {
            continue;
        };
        let (x0, x1) = covered_range(&lt.tracks_x, r.xl, r.xh);
        let (y0, y1) = covered_range(&lt.tracks_y, r.yl, r.yh);
        if x0 >= x1 && y0 >= y1 {
            continue;
        }
        let xs = x0.saturating_sub(1)..(x1 + 1).min(lt.tracks_x.len());
        let ys = y0.saturating_sub(1)..(y1 + 1).min(lt.tracks_y.len());
        let lo = g.nodes.partition_point(|n| n.layer.0 < lid.0);
        let hi = g.nodes.partition_point(|n| n.layer.0 <= lid.0);
        for (i, n) in g.nodes[lo..hi].iter().enumerate() {
            let covered = (x0..x1).contains(&(n.ix as usize))
                || (y0..y1).contains(&(n.iy as usize));
            if covered && xs.contains(&(n.ix as usize)) && ys.contains(&(n.iy as usize)) {
                out.insert((lo + i) as u32);
            }
        }
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// A node's absolute coordinates.
fn node_xy(g: &NetGraph, tracks: &[LayerTracks], id: u32) -> (i32, i32) {
    let n = &g.nodes[id as usize];
    let lt = &tracks[n.layer.0 as usize];
    (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize])
}

/// The preferred axis of `dir` — the direction-to-axis flip: a
/// `Horizontal` layer's preferred edges move along `Axis::X` (riding a
/// `tracks_y` line between consecutive `ix`).
fn pref_axis(dir: LayerDir) -> Axis {
    match dir {
        LayerDir::Horizontal => Axis::X,
        LayerDir::Vertical => Axis::Y,
    }
}

/// Classifies one candidate edge's occupancy hits: `None` = blocked
/// (uncarved `Obs` always; foreign `Net` in hard mode), `Some(extra)`
/// = extra cost — `(OCC_PENALTY + hist·HIST_STEP)` per distinct
/// foreign net hit, `Net` and `Pin` pooling into the same set.
///
/// Host-cell carve: an `Obs(cell)` mark is invisible to the candidate
/// net whenever the net has a member pin hosted by `cell` — the
/// rasterized form of OpenROAD's "an access shape ignores
/// own-instance OBS" rule, applied unconditionally (the earlier
/// pin-halo version left pockets walled: the approach edge hit host
/// OBS before reaching the halo — measured on gcd's 48 `NoPath`s).
/// Recorded looseness: the net may cross the host cell's OBS anywhere,
/// not just near the pin — connectivity-level acceptable; the
/// emit/e2e DRC owns the real check. `Obs(u32::MAX)` (net-less pin
/// metal) has no exempting host and always blocks.
///
/// `Pin` marks: own free, foreign soft-priced — never a hard block
/// (rip-up cannot move a pin; a landing wire is an emit-time finding).
fn hit_verdict(
    hits: &[Hit],
    host_cells: &[u32],
    own: u32,
    hist: u32,
    hard: bool,
    mode: OccMode,
) -> Option<u32> {
    for h in hits {
        if let Occupant::Obs(cell) = h.iv.who
            && host_cells.binary_search(&cell).is_err()
        {
            return None;
        }
    }
    if mode == OccMode::Disabled {
        return Some(0);
    }
    let mut foreign: Vec<u32> = Vec::new();
    let mut blocked = false;
    for h in hits {
        let m = match h.iv.who {
            Occupant::Net(m) => {
                blocked |= m != own;
                m
            }
            Occupant::Pin(m) => m,
            _ => continue,
        };
        if m != own && !foreign.contains(&m) {
            foreign.push(m);
        }
    }
    if hard && blocked {
        return None;
    }
    Some((OCC_PENALTY + hist * HIST_STEP) * foreign.len() as u32)
}

/// Total traversal cost of one edge in the unified index space
/// (`< E` in-plane, `≥ E` via), or `None` when the edge is blocked.
#[allow(clippy::too_many_arguments)]
fn edge_cost(
    g: &NetGraph,
    tracks: &[LayerTracks],
    tech: &TechDb,
    vias: &ViaTable,
    occ: &Occupancy,
    relax: &[RelaxedVia],
    host_cells: &[u32],
    eidx: u32,
    own: u32,
    hist: u32,
    hard: bool,
    mode: OccMode,
    hits: &mut Vec<Hit>,
) -> Option<u32> {
    hits.clear();
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    let base;
    if eidx < e_plane {
        let e = &g.edges[eidx as usize];
        let (na, nb) = (&g.nodes[e.a as usize], &g.nodes[e.b as usize]);
        let l = na.layer.0;
        let lt = &tracks[l as usize];
        let (line, c, d) = match e.axis {
            Axis::X => (
                lt.tracks_y[na.iy as usize],
                lt.tracks_x[na.ix as usize].min(lt.tracks_x[nb.ix as usize]),
                lt.tracks_x[na.ix as usize].max(lt.tracks_x[nb.ix as usize]),
            ),
            Axis::Y => (
                lt.tracks_x[na.ix as usize],
                lt.tracks_y[na.iy as usize].min(lt.tracks_y[nb.iy as usize]),
                lt.tracks_y[na.iy as usize].max(lt.tracks_y[nb.iy as usize]),
            ),
        };
        base = if e.axis == pref_axis(tech.routing_layers[l as usize].dir) {
            PREF_COST
        } else {
            OFF_AXIS_COST
        };
        occ.scan_wire(e.axis, l, line, c, d, hits);
    } else {
        let (k, x, y) = if eidx < v_plane {
            let ve = &g.via_edges[(eidx - e_plane) as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            (g.nodes[ve.lower as usize].layer.0, x, y)
        } else {
            let rv = &relax[(eidx - v_plane) as usize];
            // The implied stubs are committed metal — scan them like
            // wires; the footprint check below only covers the via's
            // own rect, so an unscanned stub could price/block
            // nothing while colliding for real.
            scan_stub(g, tracks, occ, rv.a, rv.lower, rv.x, rv.y, hits);
            scan_stub(g, tracks, occ, rv.b, rv.lower + 1, rv.x, rv.y, hits);
            (rv.lower, rv.x, rv.y)
        };
        let pair = &vias.pairs[k as usize];
        let Some(di) = pair.default else {
            return None; // gated at adjacency build — defensive
        };
        let vcand = &vias.candidates[pair.candidates[di as usize] as usize];
        base = VIA_COST;
        for (dl, r) in &vcand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            let abs = RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh);
            occ.scan_rect(lid.0, &abs, hits);
        }
    }
    hit_verdict(hits, host_cells, own, hist, hard, mode).map(|extra| base + extra)
}

/// Scans the wire stub a relaxed via implies between node `n`'s
/// position and the via position `(x, y)` on layer `l` — the
/// transition's real metal outside the via footprint. Each endpoint
/// shares one coordinate with `(x, y)` by construction, so the stub
/// is a single-axis interval (or absent when `n` sits on the via).
#[allow(clippy::too_many_arguments)]
fn scan_stub(
    g: &NetGraph,
    tracks: &[LayerTracks],
    occ: &Occupancy,
    n: u32,
    l: u8,
    x: i32,
    y: i32,
    out: &mut Vec<Hit>,
) {
    let (nx, ny) = node_xy(g, tracks, n);
    if nx == x {
        if ny != y {
            occ.scan_wire(Axis::Y, l, nx, ny.min(y), ny.max(y), out);
        }
    } else if ny == y {
        occ.scan_wire(Axis::X, l, ny, nx.min(x), nx.max(x), out);
    }
}

/// Records every foreign-net spacing hit of one committed edge as a
/// provisional `Collision` (deduped on `(a, b, layer, line, lo, hi)`
/// — `a < b` canonically).
#[allow(clippy::too_many_arguments)]
fn edge_collisions(
    g: &NetGraph,
    tracks: &[LayerTracks],
    vias: &ViaTable,
    occ: &Occupancy,
    relax: &[RelaxedVia],
    eidx: u32,
    own: NetId,
    out: &mut BTreeSet<Collision>,
    hits: &mut Vec<Hit>,
) {
    let mut record = |layer: LayerId, hits: &[Hit]| {
        for h in hits {
            if let Occupant::Net(m) = h.iv.who
                && m != own.0
            {
                out.insert(Collision {
                    a: NetId(own.0.min(m)),
                    b: NetId(own.0.max(m)),
                    layer,
                    line: h.line,
                    lo: h.lo,
                    hi: h.hi,
                });
            }
        }
    };
    hits.clear();
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    if eidx < e_plane {
        let e = &g.edges[eidx as usize];
        let (na, nb) = (&g.nodes[e.a as usize], &g.nodes[e.b as usize]);
        let l = na.layer.0;
        let lt = &tracks[l as usize];
        let (line, c, d) = match e.axis {
            Axis::X => (
                lt.tracks_y[na.iy as usize],
                lt.tracks_x[na.ix as usize].min(lt.tracks_x[nb.ix as usize]),
                lt.tracks_x[na.ix as usize].max(lt.tracks_x[nb.ix as usize]),
            ),
            Axis::Y => (
                lt.tracks_x[na.ix as usize],
                lt.tracks_y[na.iy as usize].min(lt.tracks_y[nb.iy as usize]),
                lt.tracks_y[na.iy as usize].max(lt.tracks_y[nb.iy as usize]),
            ),
        };
        occ.scan_wire(e.axis, l, line, c, d, hits);
        record(LayerId(l), hits);
    } else {
        let (k, x, y) = if eidx < v_plane {
            let ve = &g.via_edges[(eidx - e_plane) as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            (g.nodes[ve.lower as usize].layer.0, x, y)
        } else {
            let rv = &relax[(eidx - v_plane) as usize];
            // Stubs are committed metal — same miss class as in
            // `edge_cost`.
            hits.clear();
            scan_stub(g, tracks, occ, rv.a, rv.lower, rv.x, rv.y, hits);
            record(LayerId(rv.lower), hits);
            hits.clear();
            scan_stub(g, tracks, occ, rv.b, rv.lower + 1, rv.x, rv.y, hits);
            record(LayerId(rv.lower + 1), hits);
            (rv.lower, rv.x, rv.y)
        };
        let pair = &vias.pairs[k as usize];
        let Some(di) = pair.default else {
            return;
        };
        let cand = &vias.candidates[pair.candidates[di as usize] as usize];
        for (dl, r) in &cand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            let shift = RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh);
            hits.clear();
            occ.scan_rect(lid.0, &shift, hits);
            record(*lid, hits);
        }
    }
}

/// A via transition synthesized by the maze outside the strict
/// shared-crossing rule: a real via only needs a *preferred*-track
/// crossing — the vertical layer's x-track and the horizontal layer's
/// y-track — but graph nodes live on same-layer crossings, which for
/// li1–met1 coincide only every 7820 DBU and leave whole pin areas
/// with no via edge at all. `a` is the lower node the edge hangs on;
/// `b` the nearest in-region upper node on the chosen track line;
/// `(x, y)` the true via position used for the footprint scan
/// (`docs/e6-drt.md` §7 records the model change).
#[derive(Clone, Copy, Debug)]
struct RelaxedVia {
    a: u32,
    b: u32,
    x: i32,
    y: i32,
    lower: u8,
}

/// Index of the `list` value nearest `v` (nonempty lists only).
fn nearest_track(list: &[i32], v: i32) -> usize {
    let i = list.partition_point(|&t| t < v);
    if i == 0 {
        0
    } else if i == list.len() {
        list.len() - 1
    } else if v - list[i - 1] <= list[i] - v {
        i - 1
    } else {
        i
    }
}

/// Per-net relaxed via edges: for every node `v` on routing layer `k`
/// (with `k + 1` still a routing layer and `pairs[k].default` set —
/// a `ViaGap` pair emits none), one edge to layer `k + 1`.
///
/// Upper `Horizontal` ⇒ the via sits at `(v.x, upper.ty[jy])` with
/// `jy` the upper y-track nearest `v.y`, and `b` is the in-region
/// upper node on that line nearest `v.x`. Upper `Vertical` mirrors it:
/// `(upper.tx[jx], v.y)`, `b` on that x-line nearest `v.y`. The true
/// position is always on both layers' preferred wires; the hang node
/// is the same wire a track step away at most. Emitted in node order —
/// deterministic; parallel strict/relaxed edges are left alone (the
/// search takes the cheaper, extraction dedupes the placement).
fn relaxed_vias(
    g: &NetGraph,
    tech: &TechDb,
    tracks: &[LayerTracks],
    vias: &ViaTable,
) -> Vec<RelaxedVia> {
    let nl = tracks.len();
    // layer → line → sorted in-region indices: `iy → [ix]` for
    // horizontal landings, `ix → [iy]` for vertical.
    let mut lines_h: Vec<BTreeMap<u32, Vec<u32>>> = vec![BTreeMap::new(); nl];
    let mut lines_v: Vec<BTreeMap<u32, Vec<u32>>> = vec![BTreeMap::new(); nl];
    for n in &g.nodes {
        let l = n.layer.0 as usize;
        lines_h[l].entry(n.iy).or_default().push(n.ix);
        lines_v[l].entry(n.ix).or_default().push(n.iy);
    }
    // Pairs arrive sorted — nodes enumerate (layer, ix, iy) ascending.
    let node_id = |l: u8, ix: u32, iy: u32| -> u32 {
        let lo = g.nodes.partition_point(|n| {
            (n.layer.0, n.ix, n.iy) < (l, ix, iy)
        });
        debug_assert!(lo < g.nodes.len() && {
            let n = &g.nodes[lo];
            n.layer.0 == l && n.ix == ix && n.iy == iy
        });
        lo as u32
    };
    let mut out = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        let k = n.layer.0 as usize;
        if k + 1 >= nl || vias.pairs[k].default.is_none() {
            continue;
        }
        let lt = &tracks[k];
        let up = &tracks[k + 1];
        let (x, y) = (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize]);
        match tech.routing_layers[k + 1].dir {
            LayerDir::Horizontal => {
                if up.tracks_y.is_empty() {
                    continue;
                }
                let jy = nearest_track(&up.tracks_y, y) as u32;
                let Some(ixs) = lines_h[k + 1].get(&jy) else {
                    continue;
                };
                let pos = ixs.partition_point(|&jx| up.tracks_x[jx as usize] < x);
                let jx = [pos.wrapping_sub(1), pos]
                    .into_iter()
                    .filter(|&j| j < ixs.len())
                    .min_by_key(|&j| (up.tracks_x[ixs[j] as usize] - x).abs())
                    .expect("nonempty line");
                out.push(RelaxedVia {
                    a: i as u32,
                    b: node_id((k + 1) as u8, ixs[jx], jy),
                    x,
                    y: up.tracks_y[jy as usize],
                    lower: n.layer.0,
                });
            }
            LayerDir::Vertical => {
                if up.tracks_x.is_empty() {
                    continue;
                }
                let jx = nearest_track(&up.tracks_x, x) as u32;
                let Some(iys) = lines_v[k + 1].get(&jx) else {
                    continue;
                };
                let pos = iys.partition_point(|&jy| up.tracks_y[jy as usize] < y);
                let jy = [pos.wrapping_sub(1), pos]
                    .into_iter()
                    .filter(|&j| j < iys.len())
                    .min_by_key(|&j| (up.tracks_y[iys[j] as usize] - y).abs())
                    .expect("nonempty line");
                out.push(RelaxedVia {
                    a: i as u32,
                    b: node_id((k + 1) as u8, jx, iys[jy]),
                    x: up.tracks_x[jx as usize],
                    y,
                    lower: n.layer.0,
                });
            }
        }
    }
    out
}

/// The other endpoint of edge `eidx` seen from `v`.
fn other_end(g: &NetGraph, relax: &[RelaxedVia], eidx: u32, v: u32) -> u32 {
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    if eidx < e_plane {
        let e = &g.edges[eidx as usize];
        if e.a == v { e.b } else { e.a }
    } else if eidx < v_plane {
        let ve = &g.via_edges[(eidx - e_plane) as usize];
        if ve.lower == v { ve.upper } else { ve.lower }
    } else {
        let rv = &relax[(eidx - v_plane) as usize];
        if rv.a == v { rv.b } else { rv.a }
    }
}

/// One routed net's outcome.
enum NetOutcome {
    Routed(NetRoute, Vec<Collision>),
    Failed(UnroutedNet),
}

/// Per-net topology: everything `route_one_net` needs that does not
/// depend on occupancy, `hist`, or `hard` — built once per net in
/// `route_nets` and reused across rip-up iterations. The unified edge
/// index space `[0,E) | [E,E+V) | [E+V,E+V+R)` and the arc order
/// (in-plane, then strict, then relaxed) are identical to what
/// `NetRun::new` used to build per attempt — byte-identical search.
struct NetTopo {
    /// Per member pin (`pin_nodes` order): strict `pin_nodes`, else the
    /// projection fallback.
    attach: Vec<Vec<u32>>,
    projected: Vec<bool>,
    attachable: Vec<bool>,
    /// Sorted cell indices hosting member pins — the host-cell OBS
    /// carve's domain.
    host_cells: Vec<u32>,
    /// Attach node → member pin indices.
    members: BTreeMap<u32, Vec<u32>>,
    /// Per-net relaxed via edges (`relaxed_vias` output).
    relax: Vec<RelaxedVia>,
    adj_start: Vec<u32>,
    adj_to: Vec<u32>,
    adj_eidx: Vec<u32>,
}

impl NetTopo {
    /// CSR adjacency over the unified edge index space: each in-plane
    /// edge contributes both directions, then each legal via edge
    /// (`pairs[k].default.is_some()` — a `ViaGap` pair forbids the
    /// transition) contributes both directions, then each relaxed via
    /// edge (pre-gated by `relaxed_vias`). Strict arcs come before
    /// relaxed arcs so an equal-`VIA_COST` tie relaxes toward the
    /// strict placement.
    fn new(
        db: &DesignDb,
        tech: &TechDb,
        tracks: &[LayerTracks],
        g: &NetGraph,
        vias: &ViaTable,
    ) -> Self {
        // Attach sets: strict `pin_nodes`, else the projection
        // fallback. `host_cells` collects the cells hosting member
        // pins for the unconditional host-cell OBS carve in
        // `hit_verdict`.
        let m = g.pin_nodes.len();
        let mut attach: Vec<Vec<u32>> = Vec::with_capacity(m);
        let mut projected = vec![false; m];
        let mut attachable = vec![true; m];
        let mut host_cells: Vec<u32> = Vec::new();
        for (pi, &(pin, ref strict)) in g.pin_nodes.iter().enumerate() {
            if let Some(c) = db.pin_cells[pin.0 as usize] {
                host_cells.push(c.0);
            }
            if !strict.is_empty() {
                attach.push(strict.clone());
                continue;
            }
            let shapes = pin_shapes(db, tech, pin);
            let nodes = project_attach(g, &shapes, tracks);
            projected[pi] = !nodes.is_empty();
            attachable[pi] = !nodes.is_empty();
            attach.push(nodes);
        }
        host_cells.sort_unstable();
        host_cells.dedup();
        let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (pi, ids) in attach.iter().enumerate() {
            for &v in ids {
                members.entry(v).or_default().push(pi as u32);
            }
        }

        let relax = relaxed_vias(g, tech, tracks, vias);
        let n = g.nodes.len();
        let e_plane = g.edges.len() as u32;
        let mut deg = vec![0u32; n + 1];
        let mut arcs: Vec<(u32, u32, u32)> = Vec::new();
        for (ei, e) in g.edges.iter().enumerate() {
            deg[e.a as usize + 1] += 1;
            deg[e.b as usize + 1] += 1;
            arcs.push((e.a, e.b, ei as u32));
            arcs.push((e.b, e.a, ei as u32));
        }
        for (vi, ve) in g.via_edges.iter().enumerate() {
            let k = g.nodes[ve.lower as usize].layer.0 as usize;
            if vias.pairs.get(k).and_then(|p| p.default).is_none() {
                continue;
            }
            let ei = e_plane + vi as u32;
            deg[ve.lower as usize + 1] += 1;
            deg[ve.upper as usize + 1] += 1;
            arcs.push((ve.lower, ve.upper, ei));
            arcs.push((ve.upper, ve.lower, ei));
        }
        let v_plane = e_plane + g.via_edges.len() as u32;
        for (ri, rv) in relax.iter().enumerate() {
            let ei = v_plane + ri as u32;
            deg[rv.a as usize + 1] += 1;
            deg[rv.b as usize + 1] += 1;
            arcs.push((rv.a, rv.b, ei));
            arcs.push((rv.b, rv.a, ei));
        }
        for i in 0..n {
            deg[i + 1] += deg[i];
        }
        let mut adj_to = vec![0u32; arcs.len()];
        let mut adj_eidx = vec![0u32; arcs.len()];
        let mut fill = deg.clone();
        for (a, b, ei) in arcs {
            let pos = fill[a as usize] as usize;
            adj_to[pos] = b;
            adj_eidx[pos] = ei;
            fill[a as usize] += 1;
        }
        Self {
            attach,
            projected,
            attachable,
            host_cells,
            members,
            relax,
            adj_start: deg,
            adj_to,
            adj_eidx,
        }
    }
}

/// Per-attempt routing state over a cached `NetTopo`: the grown tree,
/// the committed edge list, and the wave scratch.
struct NetRun<'t> {
    topo: &'t NetTopo,
    connected: Vec<bool>,
    in_tree: Vec<bool>,
    tree_nodes: Vec<u32>,
    used_edges: Vec<u32>,
    dist: Vec<u32>,
    dstamp: Vec<u32>,
    parent: Vec<u32>,
    pstamp: Vec<u32>,
    /// Unweighted hop distance to the nearest still-unconnected
    /// attach node — the goal-direction heuristic (`extend_once`).
    hop: Vec<u32>,
    stamp: u32,
}

impl<'t> NetRun<'t> {
    fn new(topo: &'t NetTopo, n_nodes: usize, n_pins: usize) -> Self {
        Self {
            topo,
            connected: vec![false; n_pins],
            in_tree: vec![false; n_nodes],
            tree_nodes: Vec::new(),
            used_edges: Vec::new(),
            dist: vec![u32::MAX; n_nodes],
            dstamp: vec![0; n_nodes],
            parent: vec![u32::MAX; n_nodes],
            pstamp: vec![0; n_nodes],
            hop: vec![u32::MAX; n_nodes],
            stamp: 0,
        }
    }

    /// Marks still-unconnected pins whose attach set already touches
    /// the tree — shared attach nodes connect for free.
    fn absorb_taps(&mut self) {
        for pi in 0..self.connected.len() {
            if !self.connected[pi]
                && self.topo.attachable[pi]
                && self.topo.attach[pi].iter().any(|&v| self.in_tree[v as usize])
            {
                self.connected[pi] = true;
            }
        }
    }

    /// One goal-directed multi-source round from the tree to the
    /// nearest unattached pin. `hop` is the unweighted hop distance
    /// to the nearest still-unconnected attach node, recomputed per
    /// round by BFS over every arc class (occupancy-blind, so it
    /// undercounts — with every edge costing ≥1 hop it is a
    /// consistent heuristic: the `(g + hop, node)` PQ pops the same
    /// minimum-g target a plain Dijkstra would, just without
    /// flooding the whole graph. `u32::MAX` = no target reachable
    /// in the unweighted sense; such nodes sort last and the wave
    /// still exhausts before reporting `None`, so the `NoPath`
    /// verdict is unchanged). On a hit, backtracks the `parent`
    /// chain to the tree and appends the simple path. Returns the
    /// connected pin index, or `None` when the wave exhausted with
    /// targets left.
    #[allow(clippy::too_many_arguments)]
    fn extend_once(
        &mut self,
        g: &NetGraph,
        tracks: &[LayerTracks],
        tech: &TechDb,
        vias: &ViaTable,
        occ: &Occupancy,
        own: u32,
        hist: u32,
        hard: bool,
        mode: OccMode,
    ) -> Option<usize> {
        let topo = self.topo;
        // Hop field: BFS from every unconnected attachable pin's
        // attach set, all arcs unit weight.
        self.hop.fill(u32::MAX);
        let mut bfs: VecDeque<u32> = VecDeque::new();
        for (pi, ids) in topo.attach.iter().enumerate() {
            if self.connected[pi] || !topo.attachable[pi] {
                continue;
            }
            for &v in ids {
                if self.hop[v as usize] == u32::MAX {
                    self.hop[v as usize] = 0;
                    bfs.push_back(v);
                }
            }
        }
        while let Some(v) = bfs.pop_front() {
            let vi = v as usize;
            let h = self.hop[vi] + 1;
            let range = topo.adj_start[vi] as usize..topo.adj_start[vi + 1] as usize;
            for &to in &topo.adj_to[range] {
                let ti = to as usize;
                if self.hop[ti] == u32::MAX {
                    self.hop[ti] = h;
                    bfs.push_back(to);
                }
            }
        }

        self.stamp += 1;
        let stamp = self.stamp;
        let mut pq: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for &v in &self.tree_nodes {
            self.dist[v as usize] = 0;
            self.dstamp[v as usize] = stamp;
            pq.push(Reverse((self.hop[v as usize], v)));
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut found = None;
        while let Some(Reverse((f, v))) = pq.pop() {
            let vi = v as usize;
            if self.dstamp[vi] == stamp && self.dist[vi].saturating_add(self.hop[vi]) < f {
                continue; // a better `g + hop` was pushed after this entry
            }
            if let Some(pins) = topo.members.get(&v)
                && let Some(&pi) = pins
                    .iter()
                    .find(|&&p| !self.connected[p as usize] && topo.attachable[p as usize])
            {
                found = Some((v, pi as usize));
                break;
            }
            let d = self.dist[vi];
            for pos in topo.adj_start[vi] as usize..topo.adj_start[vi + 1] as usize {
                let to = topo.adj_to[pos];
                if self.in_tree[to as usize] {
                    continue; // tree nodes sit at dist 0 and cannot improve
                }
                let eidx = topo.adj_eidx[pos];
                let Some(ec) = edge_cost(
                    g, tracks, tech, vias, occ, &topo.relax, &topo.host_cells, eidx, own,
                    hist, hard, mode, &mut hits,
                ) else {
                    continue;
                };
                let nd = d + ec;
                let ti = to as usize;
                let cur = if self.dstamp[ti] == stamp {
                    self.dist[ti]
                } else {
                    u32::MAX
                };
                if nd < cur {
                    self.dist[ti] = nd;
                    self.dstamp[ti] = stamp;
                    self.parent[ti] = eidx;
                    self.pstamp[ti] = stamp;
                    pq.push(Reverse((nd.saturating_add(self.hop[ti]), to)));
                }
            }
        }
        let (mut v, pi) = found?;
        // Backtracked path is simple: parents record the arriving edge
        // of a strictly decreasing `dist` chain and terminate on the
        // tree; the tree only grows (nodes are added, never removed).
        // The bound is an invariant guard, not a heuristic — a broken
        // parent chain must surface as a named `NoPath`, never a hang.
        for _ in 0..g.nodes.len() {
            if self.in_tree[v as usize] {
                self.connected[pi] = true;
                return Some(pi);
            }
            self.in_tree[v as usize] = true;
            self.tree_nodes.push(v);
            let e = self.parent[v as usize];
            self.used_edges.push(e);
            v = other_end(g, &topo.relax, e, v);
        }
        None
    }
}

/// Routes one `NetGraph` over its cached `NetTopo`: seed at the
/// driver's attach set (else the first attachable member in
/// `pin_nodes` order), extend the tree one pin at a time, and either
/// return the committed `NetRoute` plus the provisional collisions its
/// edges took, or the per-pin named failure.
#[allow(clippy::too_many_arguments)]
fn route_one_net(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    topo: &NetTopo,
    vias: &ViaTable,
    occ: &Occupancy,
    hist: u32,
    hard: bool,
    mode: OccMode,
) -> NetOutcome {
    let m = g.pin_nodes.len();
    let fail = |connected: Vec<PinId>, failed: Vec<(PinId, FailKind)>| {
        NetOutcome::Failed(UnroutedNet {
            net: g.net,
            connected,
            failed,
        })
    };
    if g.nodes.is_empty() || m == 0 {
        // Defensive — `join.nets` only holds guided ≥2-pin nets with
        // nonzero coverage, but an empty region is `NoPath`, never a
        // panic.
        return fail(
            Vec::new(),
            g.pin_nodes
                .iter()
                .map(|&(p, _)| (p, FailKind::NoPath))
                .collect(),
        );
    }

    let ordered_pins = |connected: &[bool]| -> Vec<PinId> {
        g.pin_nodes
            .iter()
            .enumerate()
            .filter(|(pi, _)| connected[*pi])
            .map(|(_, &(p, _))| p)
            .collect()
    };
    let ordered_failed = |connected: &[bool], attachable: &[bool]| -> Vec<(PinId, FailKind)> {
        g.pin_nodes
            .iter()
            .enumerate()
            .filter(|(pi, _)| !connected[*pi])
            .map(|(pi, &(p, _))| {
                (
                    p,
                    if attachable[pi] {
                        FailKind::NoPath
                    } else {
                        FailKind::Unattachable
                    },
                )
            })
            .collect()
    };

    if !topo.attachable.iter().any(|&a| a) {
        return fail(Vec::new(), ordered_failed(&vec![false; m], &topo.attachable));
    }

    let mut run = NetRun::new(topo, g.nodes.len(), m);

    // Seed: the driver pin's attach set (`net_driver`, else the first
    // attachable member in `net_pins` order).
    let drv = db.net_driver(g.net);
    let seed = g
        .pin_nodes
        .iter()
        .position(|&(p, _)| Some(p) == drv)
        .filter(|&pi| topo.attachable[pi])
        .or_else(|| (0..m).find(|&pi| topo.attachable[pi]));
    let Some(seed) = seed else {
        return fail(
            Vec::new(),
            ordered_failed(&run.connected, &topo.attachable),
        );
    };
    for &v in &topo.attach[seed] {
        run.in_tree[v as usize] = true;
        run.tree_nodes.push(v);
    }
    run.connected[seed] = true;
    run.absorb_taps();

    while run
        .connected
        .iter()
        .enumerate()
        .any(|(pi, &c)| !c && topo.attachable[pi])
    {
        if run
            .extend_once(g, tracks, tech, vias, occ, g.net.0, hist, hard, mode)
            .is_none()
        {
            return fail(
                ordered_pins(&run.connected),
                ordered_failed(&run.connected, &topo.attachable),
            );
        }
        run.absorb_taps();
    }

    // The loop only sweeps attachable pins — a leftover `Unattachable`
    // member still lands the net in the named report.
    if !run.connected.iter().all(|&c| c) {
        return fail(
            ordered_pins(&run.connected),
            ordered_failed(&run.connected, &topo.attachable),
        );
    }

    // Extraction: merge unit edge spans per (layer, line) into
    // canonical wire intervals; dedupe via placements.
    let mut hlines: BTreeMap<(u8, i32), Vec<(i32, i32)>> = BTreeMap::new();
    let mut vlines: BTreeMap<(u8, i32), Vec<(i32, i32)>> = BTreeMap::new();
    let mut vplaced: BTreeSet<(u8, i32, i32)> = BTreeSet::new();
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    for &eidx in &run.used_edges {
        if eidx < e_plane {
            let e = &g.edges[eidx as usize];
            let (ax, ay) = node_xy(g, tracks, e.a);
            let (bx, by) = node_xy(g, tracks, e.b);
            let l = g.nodes[e.a as usize].layer.0;
            match e.axis {
                Axis::X => hlines
                    .entry((l, ay))
                    .or_default()
                    .push((ax.min(bx), ax.max(bx))),
                Axis::Y => vlines
                    .entry((l, ax))
                    .or_default()
                    .push((ay.min(by), ay.max(by))),
            }
        } else if eidx < v_plane {
            let ve = &g.via_edges[(eidx - e_plane) as usize];
            let n = &g.nodes[ve.lower as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            vplaced.insert((n.layer.0, x, y));
        } else {
            // Relaxed transition: the via sits on both layers'
            // preferred wires; its implied stubs — `a.xy → (x,y)` on
            // the lower layer, `(x,y) → b.xy` on the upper — are real
            // metal the committed geometry must carry (each shares
            // one coordinate, so each is a single-axis interval).
            let rv = &topo.relax[(eidx - v_plane) as usize];
            vplaced.insert((rv.lower, rv.x, rv.y));
            let (ax, ay) = node_xy(g, tracks, rv.a);
            let (bx, by) = node_xy(g, tracks, rv.b);
            if ax == rv.x {
                if ay != rv.y {
                    vlines
                        .entry((rv.lower, ax))
                        .or_default()
                        .push((ay.min(rv.y), ay.max(rv.y)));
                }
            } else if ay == rv.y {
                hlines
                    .entry((rv.lower, ay))
                    .or_default()
                    .push((ax.min(rv.x), ax.max(rv.x)));
            }
            if bx == rv.x {
                if by != rv.y {
                    vlines
                        .entry((rv.lower + 1, bx))
                        .or_default()
                        .push((by.min(rv.y), by.max(rv.y)));
                }
            } else if by == rv.y {
                hlines
                    .entry((rv.lower + 1, by))
                    .or_default()
                    .push((bx.min(rv.x), bx.max(rv.x)));
            }
        }
    }
    let mut wires: Vec<WireSeg> = Vec::new();
    let mut emit = |l: u8, t: i32, mut ivs: Vec<(i32, i32)>, vertical: bool| {
        ivs.sort_unstable();
        let mut merged: Vec<(i32, i32)> = Vec::new();
        for (lo, hi) in ivs {
            match merged.last_mut() {
                // Touching counts: a wire passing through a node is one
                // continuous segment.
                Some(e) if lo <= e.1 => e.1 = e.1.max(hi),
                _ => merged.push((lo, hi)),
            }
        }
        for (a, b) in merged {
            wires.push(if vertical {
                WireSeg {
                    layer: LayerId(l),
                    x1: t,
                    y1: a,
                    x2: t,
                    y2: b,
                }
            } else {
                WireSeg {
                    layer: LayerId(l),
                    x1: a,
                    y1: t,
                    x2: b,
                    y2: t,
                }
            });
        }
    };
    for ((l, t), ivs) in hlines {
        emit(l, t, ivs, false);
    }
    for ((l, t), ivs) in vlines {
        emit(l, t, ivs, true);
    }
    wires.sort_unstable();
    let vias_out: Vec<ViaPlace> = vplaced
        .into_iter()
        .map(|(l, x, y)| ViaPlace {
            lower: LayerId(l),
            x,
            y,
        })
        .collect();

    // Provisional collisions: the foreign hits the committed edges
    // actually took (soft-mode bookkeeping for the rip-up set).
    let mut prov = BTreeSet::new();
    let mut hits = Vec::new();
    for &eidx in &run.used_edges {
        edge_collisions(g, tracks, vias, occ, &topo.relax, eidx, g.net, &mut prov, &mut hits);
    }

    NetOutcome::Routed(
        NetRoute {
            net: g.net,
            wires,
            vias: vias_out,
            connected: ordered_pins(&run.connected),
            projected: g
                .pin_nodes
                .iter()
                .enumerate()
                .filter(|(pi, _)| run.connected[*pi] && topo.projected[*pi])
                .map(|(_, &(p, _))| p)
                .collect(),
        },
        prov.into_iter().collect(),
    )
}

/// Rasterizes one committed route's shapes (merged wire rects + via
/// footprints) as `Net(net)` marks, logging each insertion for rip-up.
fn commit_route(
    occ: &mut Occupancy,
    tracks: &[LayerTracks],
    vias: &ViaTable,
    route: &NetRoute,
    marks: &mut Vec<Mark>,
) {
    for w in &route.wires {
        let hw = occ.halfw[w.layer.0 as usize];
        let r = RectDbu::new(w.x1 - hw, w.y1 - hw, w.x2 + hw, w.y2 + hw);
        occ.rasterize(tracks, w.layer.0, &r, Occupant::Net(route.net.0), Some(marks));
    }
    for v in &route.vias {
        let pair = &vias.pairs[v.lower.0 as usize];
        let Some(di) = pair.default else {
            continue;
        };
        let cand = &vias.candidates[pair.candidates[di as usize] as usize];
        for (dl, r) in &cand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            if (lid.0 as usize) >= occ.lines_x.len() {
                continue;
            }
            let shift = RectDbu::new(v.x + r.xl, v.y + r.yl, v.x + r.xh, v.y + r.yh);
            occ.rasterize(tracks, lid.0, &shift, Occupant::Net(route.net.0), Some(marks));
        }
    }
}

/// Sequential maze routing over `graphs`: per net, pathfinder-style
/// tree extension under the interval-occupancy model, ordered per
/// `join.nets` (guide-file order). `vias` prices transitions
/// (`pairs[k].default`) and supplies footprints; a `ViaGap` pair's
/// edges are forbidden. Soft iterations price foreign hits
/// (`OCC_PENALTY + hist·HIST_STEP` per net); the last iteration is
/// hard — foreign hits block. `Obs` always blocks.
pub fn route_nets(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
    cfg: &MazeConfig,
) -> RouteDb {
    let timing = std::env::var_os("GRUS_DRT_TIMING").is_some();
    let t0 = std::time::Instant::now();
    let n = graphs.nets.len();
    let mut occ = Occupancy::new(tech);
    premark_obs(db, tech, tracks, &mut occ);
    if timing {
        eprintln!("drt: premark_obs {:?}", t0.elapsed());
    }
    // Per-net topology (attach sets, relaxed vias, CSR) is built once —
    // occupancy/history/hard-mode vary per iteration, the graph does
    // not. Arc order is fixed → determinism untouched.
    let t1 = std::time::Instant::now();
    let topos: Vec<NetTopo> = graphs
        .nets
        .iter()
        .map(|g| NetTopo::new(db, tech, tracks, g, vias))
        .collect();
    if timing {
        eprintln!("drt: topos {:?}", t1.elapsed());
    }
    let net_idx: BTreeMap<u32, usize> = graphs
        .nets
        .iter()
        .enumerate()
        .map(|(i, g)| (g.net.0, i))
        .collect();
    let mut routes: Vec<Option<NetRoute>> = vec![None; n];
    let mut marks: Vec<Vec<Mark>> = vec![Vec::new(); n];
    let mut hist = vec![0u32; n];
    let mut bad: Vec<usize> = (0..n).collect();
    let mut unrouted: Vec<UnroutedNet> = Vec::new();
    let mut iterations = 0u32;

    let cap = cfg.max_iters.max(1);
    let mut it = 0u32;
    while it < cap && !bad.is_empty() {
        it += 1;
        iterations = it;
        let hard = it == cap;
        let ti = std::time::Instant::now();
        let mut prov: BTreeSet<Collision> = BTreeSet::new();
        let mut failed: Vec<(usize, UnroutedNet)> = Vec::new();
        for (bi, &i) in bad.iter().enumerate() {
            let g = &graphs.nets[i];
            let tn = std::time::Instant::now();
            match route_one_net(db, tech, tracks, g, &topos[i], vias, &occ, hist[i], hard, cfg.occ)
            {
                NetOutcome::Routed(route, hits) => {
                    commit_route(&mut occ, tracks, vias, &route, &mut marks[i]);
                    if !hard && cfg.occ == OccMode::Enabled {
                        prov.extend(hits);
                    }
                    routes[i] = Some(route);
                }
                NetOutcome::Failed(u) => failed.push((i, u)),
            }
            if timing {
                let el = tn.elapsed();
                if timing && (el.as_millis() > 500 || bi % 500 == 0) {
                    eprintln!(
                        "drt:   net[{bi}/{}] nodes={} edges={} pins={} {:?}",
                        bad.len(),
                        g.nodes.len(),
                        g.edges.len(),
                        g.pin_nodes.len(),
                        el
                    );
                }
            }
        }
        if hard {
            unrouted = failed.into_iter().map(|(_, u)| u).collect();
            break;
        }
        // bad = unrouted ∪ both parties of every provisional collision.
        let mut next: BTreeSet<usize> = failed.iter().map(|(i, _)| *i).collect();
        for c in &prov {
            if let Some(&i) = net_idx.get(&c.a.0) {
                next.insert(i);
            }
            if let Some(&i) = net_idx.get(&c.b.0) {
                next.insert(i);
            }
        }
        bad = next.into_iter().collect();
        // Rip up every re-queued net (free its occupancy) and escalate
        // per-net history — deterministic, and it breaks the
        // ping-pong cycle where two nets keep colliding on one line.
        for &i in &bad {
            for m in std::mem::take(&mut marks[i]) {
                occ.unmark(&m);
            }
            routes[i] = None;
            hist[i] = hist[i].saturating_add(1);
        }
        if timing {
            eprintln!(
                "drt: iter {it} bad={} failed={} prov={} {:?}",
                bad.len(),
                failed.len(),
                prov.len(),
                ti.elapsed()
            );
        }
    }

    let nets: Vec<NetRoute> = routes.into_iter().flatten().collect();
    RouteDb {
        collisions: if cfg.occ == OccMode::Enabled {
            overlap_census(&nets, tech, tracks, vias)
        } else {
            Vec::new()
        },
        nets,
        unrouted,
        iterations,
    }
}

/// Exact cross-net spacing violations among committed routes — the
/// reported collision list and the fail-proof's measuring stick.
/// Rebuilds every route's wire-shape intervals and via footprints into
/// the same per-line model (routes only, no OBS), sweeps each line's
/// sorted intervals, and reports each cross-net pair whose
/// `R + s`-intersect rule violates.
pub fn overlap_census(
    routes: &[NetRoute],
    tech: &TechDb,
    tracks: &[LayerTracks],
    vias: &ViaTable,
) -> Vec<Collision> {
    let mut occ = Occupancy::new(tech);
    for r in routes {
        let mut sink = Vec::new();
        commit_route(&mut occ, tracks, vias, r, &mut sink);
    }
    let mut out = BTreeSet::new();
    for l in 0..occ.lines_x.len() {
        let s = occ.spc[l];
        for map in [&occ.lines_x[l], &occ.lines_y[l]] {
            for (&t, line) in map {
                let sorted = &line.ivs;
                for i in 0..sorted.len() {
                    for b in sorted.iter().skip(i + 1) {
                        let a = &sorted[i];
                        if b.lo >= a.hi + s {
                            break;
                        }
                        let pgap = (a.perp_lo - b.perp_hi).max(b.perp_lo - a.perp_hi).max(0);
                        if pgap >= s {
                            continue;
                        }
                        if let (Occupant::Net(m1), Occupant::Net(m2)) = (a.who, b.who)
                            && m1 != m2
                        {
                            out.insert(Collision {
                                a: NetId(m1.min(m2)),
                                b: NetId(m1.max(m2)),
                                layer: LayerId(l as u8),
                                line: t,
                                lo: a.lo.max(b.lo),
                                hi: a.hi.min(b.hi),
                            });
                        }
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}

/// `xxh3` digest over the canonical `RouteDb` — nonzero-evidence
/// determinism checking.
pub fn route_digest(routed: &RouteDb) -> u64 {
    let mut buf = Vec::new();
    let mut put = |v: u32| buf.extend_from_slice(&v.to_le_bytes());
    for r in &routed.nets {
        put(r.net.0);
        put(r.wires.len() as u32);
        for w in &r.wires {
            put(w.layer.0 as u32);
            put(w.x1 as u32);
            put(w.y1 as u32);
            put(w.x2 as u32);
            put(w.y2 as u32);
        }
        put(r.vias.len() as u32);
        for v in &r.vias {
            put(v.lower.0 as u32);
            put(v.x as u32);
            put(v.y as u32);
        }
        put(r.connected.len() as u32);
        for p in &r.connected {
            put(p.0);
        }
        put(r.projected.len() as u32);
        for p in &r.projected {
            put(p.0);
        }
    }
    put(routed.unrouted.len() as u32);
    for u in &routed.unrouted {
        put(u.net.0);
        put(u.connected.len() as u32);
        for p in &u.connected {
            put(p.0);
        }
        put(u.failed.len() as u32);
        for (p, k) in &u.failed {
            put(p.0);
            put(match k {
                FailKind::Unattachable => 0,
                FailKind::NoPath => 1,
            });
        }
    }
    put(routed.collisions.len() as u32);
    for c in &routed.collisions {
        put(c.a.0);
        put(c.b.0);
        put(c.layer.0 as u32);
        put(c.line as u32);
        put(c.lo as u32);
        put(c.hi as u32);
    }
    put(routed.iterations);
    xxh3_64(&buf)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drt::graph::{NetGraph, RouteNode};
    use crate::drt::tracks::LayerTracks;
    use crate::drt::via::{ViaCandidate, ViaPair, ViaTable};
    use crate::interner::SymbolId;
    use crate::tech_db::RoutingLayer;

    fn toy_tech() -> TechDb {
        let mut tech = TechDb::new();
        tech.routing_layers.push(RoutingLayer {
            name: SymbolId(0),
            dir: LayerDir::Horizontal,
            pitch_dbu: 340,
            width_dbu: 140,
            offset_dbu: 0,
            tracks_x: vec![],
            tracks_y: vec![],
            res_per_sq: 0.0,
            cap_per_sq_um: 0.0,
            edge_cap_per_um: 0.0,
            spacing_dbu: 140,
            spacing_table: vec![],
            area_dbu2: 0,
            min_enclosed_area_dbu2: 0,
        });
        tech
    }

    fn toy_tracks() -> Vec<LayerTracks> {
        vec![LayerTracks {
            tracks_x: vec![0, 1000, 2000, 3000],
            tracks_y: vec![0, 1000, 2000, 3000],
        }]
    }

    fn toy_occ(tech: &TechDb) -> Occupancy {
        Occupancy::new(tech)
    }

    fn commit_wire(occ: &mut Occupancy, tracks: &[LayerTracks], lo: i32, hi: i32, t: i32) {
        // Horizontal wire of net 7 on line t, span [lo,hi] — shape
        // [lo-70, t-70, hi+70, t+70] with w=140.
        let r = RectDbu::new(lo - 70, t - 70, hi + 70, t + 70);
        occ.rasterize(tracks, 0, &r, Occupant::Net(7), None);
    }

    #[test]
    fn test_band_math() {
        let tech = toy_tech();
        let tracks = toy_tracks();
        let mut occ = toy_occ(&tech);
        commit_wire(&mut occ, &tracks, 1000, 2000, 1000);
        // Committed shape [930,2070] on y=1000. w2=70, s=140.
        let mut hits = Vec::new();
        // Near: candidate span [2210,3000] → shape lo 2140, gap 70 < s → hit.
        occ.scan_wire(Axis::X, 0, 1000, 2210, 3000, &mut hits);
        assert!(!hits.is_empty(), "edge distance 70 < s must conflict");
        // Abut-at-s: span [2280,3000] → shape lo 2210, gap == s → legal.
        hits.clear();
        occ.scan_wire(Axis::X, 0, 1000, 2280, 3000, &mut hits);
        assert!(hits.is_empty(), "edge distance == s is legal (strict <)");
        // Far: different line entirely.
        hits.clear();
        occ.scan_wire(Axis::X, 0, 2000, 1000, 2000, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_perpendicular_crossing() {
        let tech = toy_tech();
        let tracks = toy_tracks();
        let mut occ = toy_occ(&tech);
        commit_wire(&mut occ, &tracks, 1000, 2000, 1000);
        // The committed wire's shape [930,930,2070,1070] marks vertical
        // lines x ∈ (720, 3280) → x = 1000, 2000, 3000 with y-interval
        // [930,1070].
        let mut hits = Vec::new();
        // Vertical candidate through the footprint: x=2000, y [0,1500].
        occ.scan_wire(Axis::Y, 0, 2000, 0, 1500, &mut hits);
        assert_eq!(hits.len(), 1, "crossing the footprint must hit");
        // Beside it: x=0 holds no mark.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 0, 0, 1500, &mut hits);
        assert!(hits.is_empty());
        // Boundary: y span [1210,2000] → shape lo 1140, gap to 1070 = 70 → hit.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 2000, 1210, 2000, &mut hits);
        assert_eq!(hits.len(), 1);
        // Span [1280,2000] → shape lo 1210, gap 140 == s → free.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 2000, 1280, 2000, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_projection_fallback() {
        let tracks = toy_tracks();
        let g = NetGraph {
            net: NetId(0),
            nodes: vec![
                RouteNode {
                    layer: LayerId(0),
                    ix: 1,
                    iy: 1,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 2,
                    iy: 1,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 3,
                    iy: 1,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 2,
                    iy: 3,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 3,
                    iy: 3,
                },
            ],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        // Escaped pin: rect covers x-tracks 2000..3000 but no y-track
        // (y range 3100..3400 holds no line) → nodes on the covered
        // x-lines at the adjacent y-line; the same x-lines two rows
        // away are out of the pin's neighborhood.
        let shapes = vec![(LayerId(0), RectDbu::new(1900, 3100, 3100, 3400))];
        let got = project_attach(&g, &shapes, &tracks);
        assert_eq!(got, vec![3, 4], "nodes adjacent to the pin on covered lines");
        // No covered track at all → Unattachable (empty set).
        let shapes = vec![(LayerId(0), RectDbu::new(150, 3100, 250, 3400))];
        assert!(project_attach(&g, &shapes, &tracks).is_empty());
    }

    #[test]
    fn test_pq_tie_order() {
        let mut pq: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for (c, v) in [(1, 9), (1, 3), (0, 7), (1, 3), (2, 0)] {
            pq.push(Reverse((c, v)));
        }
        let order: Vec<u32> = std::iter::from_fn(|| pq.pop().map(|r| (r.0).1)).collect();
        assert_eq!(order, vec![7, 3, 3, 9, 0]);
    }

    fn square_graph() -> NetGraph {
        // 2×2 met1 grid; driver at (0,0), sink at (1,1).
        NetGraph {
            net: NetId(0),
            nodes: vec![
                RouteNode {
                    layer: LayerId(0),
                    ix: 0,
                    iy: 0,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 1,
                    iy: 0,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 0,
                    iy: 1,
                },
                RouteNode {
                    layer: LayerId(0),
                    ix: 1,
                    iy: 1,
                },
            ],
            edges: vec![
                crate::drt::graph::RouteEdge {
                    a: 0,
                    b: 1,
                    axis: Axis::X,
                },
                crate::drt::graph::RouteEdge {
                    a: 2,
                    b: 3,
                    axis: Axis::X,
                },
                crate::drt::graph::RouteEdge {
                    a: 0,
                    b: 2,
                    axis: Axis::Y,
                },
                crate::drt::graph::RouteEdge {
                    a: 1,
                    b: 3,
                    axis: Axis::Y,
                },
            ],
            via_edges: vec![],
            pin_nodes: vec![(PinId(0), vec![0]), (PinId(1), vec![3])],
        }
    }

    #[test]
    fn test_tree_extension_simple_path() {
        let tech = toy_tech();
        let tracks = toy_tracks();
        let g = square_graph();
        let vias = ViaTable {
            candidates: Vec::<ViaCandidate>::new(),
            pairs: vec![ViaPair {
                lower: LayerId(0),
                upper: LayerId(1),
                cut: None,
                candidates: vec![],
                default: None,
            }],
            gaps: vec![],
            unmatched: vec![],
        };
        let mut db = DesignDb::new();
        db.pin_names.push(db.strings.intern("a"));
        db.pin_names.push(db.strings.intern("b"));
        db.pin_dirs
            .push(crate::interner::PinDir::Output);
        db.pin_dirs.push(crate::interner::PinDir::Input);
        db.pin_cells.push(None);
        db.pin_cells.push(None);
        db.pin_macro_pins.push(None);
        db.pin_macro_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_nets.push(Some(NetId(0)));
        db.pin_nets.push(Some(NetId(0)));
        db.pin_offset_x_dbu.push(0);
        db.pin_offset_x_dbu.push(0);
        db.pin_offset_y_dbu.push(0);
        db.pin_offset_y_dbu.push(0);
        db.pin_io_layer.push(None);
        db.pin_io_layer.push(None);
        db.pin_io_rects.push(vec![]);
        db.pin_io_rects.push(vec![]);
        db.pin_is_driver.push(true);
        db.pin_is_driver.push(false);
        db.net_names.push(db.strings.intern("n1"));
        db.net_pin_offsets = vec![0, 2];
        db.net_pin_ids = vec![PinId(0), PinId(1)];

        let occ = Occupancy::new(&tech);
        let cfg = MazeConfig::default();
        let topo = NetTopo::new(&db, &tech, &tracks, &g, &vias);
        let outcome = route_one_net(
            &db, &tech, &tracks, &g, &topo, &vias, &occ, 0, false, cfg.occ,
        );
        let NetOutcome::Routed(route, _) = outcome else {
            panic!("square must route");
        };
        // Deterministic tie: node 1 reached before node 2, so the path
        // is (0,0)→(1,0)→(1,1) — horizontal then vertical.
        assert_eq!(
            route.wires,
            vec![
                WireSeg {
                    layer: LayerId(0),
                    x1: 0,
                    y1: 0,
                    x2: 1000,
                    y2: 0
                },
                WireSeg {
                    layer: LayerId(0),
                    x1: 1000,
                    y1: 0,
                    x2: 1000,
                    y2: 1000
                },
            ]
        );
        assert_eq!(route.connected, vec![PinId(0), PinId(1)]);
    }
}
