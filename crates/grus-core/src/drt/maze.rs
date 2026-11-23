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
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
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
    Obs,
    Net(u32), // NetId.0
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

/// Per-layer committed geometry rasterized onto track lines so an
/// edge's conflict check is a single-line interval query. `lines_x[l]`
/// keys vertical lines (x-coords) → y-intervals; `lines_y[l]` keys
/// horizontal lines → x-intervals.
struct Occupancy {
    lines_x: Vec<BTreeMap<i32, Vec<Interval>>>,
    lines_y: Vec<BTreeMap<i32, Vec<Interval>>>,
    /// `width_dbu / 2` per routing layer (integer division — sky130
    /// widths are even).
    halfw: Vec<i32>,
    /// `min_spacing_dbu()` per routing layer.
    spc: Vec<i32>,
}

/// One interval hit by a candidate edge: the rasterized mark (`iv`),
/// the line it was found on, and the overlap of the two shape extents
/// along the line (`lo > hi` encodes a gap). `x_line` separates the
/// two line maps — vertical (`lines_x`) vs horizontal (`lines_y`)
/// lines share the same coord range. The mark's full `iv`/`perp`
/// extents are needed for the pin-carve OBS exemption in
/// `hit_verdict`, which is a 2D rect-overlap test.
#[derive(Clone, Copy, Debug)]
struct Hit {
    iv: Interval,
    layer: u8,
    x_line: bool,
    line: i32,
    lo: i32,
    hi: i32,
}

/// The marked shape's rect, reconstructed from its line interval:
/// `lines_x` marks store y-extent with x perp extent, `lines_y` the
/// opposite.
fn hit_rect(h: &Hit) -> RectDbu {
    if h.x_line {
        RectDbu::new(h.iv.perp_lo, h.iv.lo, h.iv.perp_hi, h.iv.hi)
    } else {
        RectDbu::new(h.iv.lo, h.iv.perp_lo, h.iv.hi, h.iv.perp_hi)
    }
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
            self.lines_x[l].entry(t).or_default().push(iv);
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
            self.lines_y[l].entry(t).or_default().push(iv);
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

    /// Frees one recorded insertion (rip-up).
    fn unmark(&mut self, m: &Mark) {
        let map = if m.x_axis {
            &mut self.lines_x[m.layer as usize]
        } else {
            &mut self.lines_y[m.layer as usize]
        };
        if let Some(ivs) = map.get_mut(&m.line)
            && let Some(pos) = ivs.iter().position(|iv| *iv == m.iv)
        {
            ivs.remove(pos);
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
        let Some(ivs) = map.get(&t) else {
            return;
        };
        let (qlo, qhi) = (c - w2 - s, d + w2 + s);
        let (slo, shi) = (c - w2, d + w2);
        for iv in ivs {
            if iv.lo < qhi && iv.hi > qlo {
                out.push(Hit {
                    iv: *iv,
                    layer,
                    x_line: axis == Axis::Y,
                    line: t,
                    lo: iv.lo.max(slo),
                    hi: iv.hi.min(shi),
                });
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
        for (&t, ivs) in self.lines_y[l].range(re.yl - w2 - s..=re.yh + w2 + s) {
            for iv in ivs {
                if iv.lo < re.xh && iv.hi > re.xl && iv.perp_lo < re.yh && iv.perp_hi > re.yl {
                    out.push(Hit {
                        iv: *iv,
                        layer,
                        x_line: false,
                        line: t,
                        lo: iv.lo.max(r.xl),
                        hi: iv.hi.min(r.xh),
                    });
                }
            }
        }
        for (&t, ivs) in self.lines_x[l].range(re.xl - w2 - s..=re.xh + w2 + s) {
            for iv in ivs {
                if iv.lo < re.yh && iv.hi > re.yl && iv.perp_lo < re.xh && iv.perp_hi > re.xl {
                    out.push(Hit {
                        iv: *iv,
                        layer,
                        x_line: true,
                        line: t,
                        lo: iv.lo.max(r.yl),
                        hi: iv.hi.min(r.yh),
                    });
                }
            }
        }
    }
}

/// Pre-marks every placed cell's macro `obs_rects` as `Obs` (the
/// `drc::geom` source-4 convention: `{Placed, Fixed, Cover}` gate +
/// `orient_rect` + translate, replicated for OBS only), and every
/// instance pin's shapes as `Net(own_net)` — a pin's metal is a
/// blockage to foreign nets but free to the net it belongs to, so
/// routing can reach its own attach nodes. io-port rects are not
/// marked (recorded simplification — pin-access DRC is the emit/e2e
/// tickets' job).
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
        for &(lid, r) in &mac.obs_rects {
            if (lid.0 as usize) < occ.lines_x.len() {
                occ.rasterize(tracks, lid.0, &place(r), Occupant::Obs, None);
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
        let who = db.pin_nets[p].map_or(Occupant::Obs, |n| Occupant::Net(n.0));
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
/// rect `(l, r)` of an escaped pin, the in-region nodes on `l` sharing
/// a covered track index — `ix ∈ covered_range(tracks_x, r)` **or**
/// `iy ∈ covered_range(tracks_y, r)`. A met2 io-port square holds an
/// x-track but no y-track; the wire that taps it runs along that
/// x-track, so any in-region node on the line is a connection point.
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
        let lo = g.nodes.partition_point(|n| n.layer.0 < lid.0);
        let hi = g.nodes.partition_point(|n| n.layer.0 <= lid.0);
        for (i, n) in g.nodes[lo..hi].iter().enumerate() {
            if (x0..x1).contains(&(n.ix as usize)) || (y0..y1).contains(&(n.iy as usize)) {
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
/// (`Obs` hit always; foreign-net hit in hard mode), `Some(extra)` =
/// soft-mode extra cost — `(OCC_PENALTY + hist·HIST_STEP)` per
/// distinct foreign net hit (numerically `OCC_PENALTY × (1+hist)` at
/// the current constants).
///
/// Pin carve-out: an `Obs` mark is invisible to the candidate net when
/// the marked shape's rect touches a rect of the net's own pins on the
/// same layer — the rasterized form of "a net may touch the blockage
/// its own pin already sits on". Stdcell li1 OBS jumpers cover the
/// whole cell including the pin rects, so without this every li1 pin
/// is walled in; the carve-out is what OpenROAD's gcd solution shows
/// (the mcon's li1 plate lands inside the pin rect, inside the OBS
/// jumper). The side effect — the whole OBS rect of a cell hosting
/// the net's pin becomes transparent to that net — is a recorded
/// simplification of spacing-lite.
fn hit_verdict(
    hits: &[Hit],
    own_pins: &[Vec<RectDbu>],
    own: u32,
    hist: u32,
    hard: bool,
    mode: OccMode,
) -> Option<u32> {
    for h in hits {
        if h.iv.who != Occupant::Obs {
            continue;
        }
        let r = hit_rect(h);
        let carved = own_pins
            .get(h.layer as usize)
            .is_some_and(|pins| {
                pins.iter()
                    .any(|p| r.xl <= p.xh && p.xl <= r.xh && r.yl <= p.yh && p.yl <= r.yh)
            });
        if !carved {
            return None;
        }
    }
    if mode == OccMode::Disabled {
        return Some(0);
    }
    let mut foreign: Vec<u32> = Vec::new();
    for h in hits {
        if let Occupant::Net(m) = h.iv.who
            && m != own
            && !foreign.contains(&m)
        {
            foreign.push(m);
        }
    }
    if hard && !foreign.is_empty() {
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
    own_pins: &[Vec<RectDbu>],
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
            (rv.lower, rv.x, rv.y)
        };
        let pair = &vias.pairs[k as usize];
        let Some(di) = pair.default else {
            return None; // gated at adjacency build — defensive
        };
        let cand = &vias.candidates[pair.candidates[di as usize] as usize];
        base = VIA_COST;
        for (dl, r) in &cand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            occ.scan_rect(
                lid.0,
                &RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh),
                hits,
            );
        }
    }
    hit_verdict(hits, own_pins, own, hist, hard, mode).map(|extra| base + extra)
}

/// Records every foreign-net spacing hit of one committed edge as a
/// provisional `Collision` (deduped on `(a, b, layer, line, lo, hi)`
/// — `a < b` canonically).
fn edge_collisions(
    g: &NetGraph,
    tracks: &[LayerTracks],
    vias: &ViaTable,
    occ: &Occupancy,
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
                    b: node_id((k + 1) as u8, jx, jy),
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
                    b: node_id((k + 1) as u8, jx, jy),
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

/// Per-net routing state: attach sets, the grown tree, the committed
/// edge list, and the CSR adjacency + Dijkstra scratch.
struct NetRun {
    attach: Vec<Vec<u32>>, // per member pin (`pin_nodes` order)
    projected: Vec<bool>,
    attachable: Vec<bool>,
    connected: Vec<bool>,
    members: BTreeMap<u32, Vec<u32>>, // attach node → pin indices
    in_tree: Vec<bool>,
    tree_nodes: Vec<u32>,
    used_edges: Vec<u32>,
    adj_start: Vec<u32>,
    adj_to: Vec<u32>,
    adj_eidx: Vec<u32>,
    dist: Vec<u32>,
    dstamp: Vec<u32>,
    parent: Vec<u32>,
    pstamp: Vec<u32>,
    stamp: u32,
}

impl NetRun {
    /// CSR adjacency over the unified edge index space: each in-plane
    /// edge contributes both directions, then each legal via edge
    /// (`pairs[k].default.is_some()` — a `ViaGap` pair forbids the
    /// transition) contributes both directions.
    fn new(
        g: &NetGraph,
        vias: &ViaTable,
        attach: Vec<Vec<u32>>,
        projected: Vec<bool>,
        attachable: Vec<bool>,
    ) -> Self {
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
        let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (pi, ids) in attach.iter().enumerate() {
            for &v in ids {
                members.entry(v).or_default().push(pi as u32);
            }
        }
        Self {
            attach,
            projected,
            attachable,
            connected: vec![false; g.pin_nodes.len()],
            members,
            in_tree: vec![false; n],
            tree_nodes: Vec::new(),
            used_edges: Vec::new(),
            adj_start: deg,
            adj_to,
            adj_eidx,
            dist: vec![u32::MAX; n],
            dstamp: vec![0; n],
            parent: vec![u32::MAX; n],
            pstamp: vec![0; n],
            stamp: 0,
        }
    }

    /// Marks still-unconnected pins whose attach set already touches
    /// the tree — shared attach nodes connect for free.
    fn absorb_taps(&mut self) {
        for pi in 0..self.connected.len() {
            if !self.connected[pi]
                && self.attachable[pi]
                && self.attach[pi].iter().any(|&v| self.in_tree[v as usize])
            {
                self.connected[pi] = true;
            }
        }
    }

    /// One multi-source Dijkstra round from the tree to the nearest
    /// unattached pin (`(cost, node)` PQ order — equal costs pop the
    /// lower node id; neighbors enumerate in edge-list order). On a
    /// hit, backtracks the `parent` chain to the tree and appends the
    /// simple path. Returns the connected pin index, or `None` when
    /// the wave exhausted with targets left.
    #[allow(clippy::too_many_arguments)]
    fn extend_once(
        &mut self,
        g: &NetGraph,
        tracks: &[LayerTracks],
        tech: &TechDb,
        vias: &ViaTable,
        occ: &Occupancy,
        own_pins: &[Vec<RectDbu>],
        own: u32,
        hist: u32,
        hard: bool,
        mode: OccMode,
    ) -> Option<usize> {
        self.stamp += 1;
        let stamp = self.stamp;
        let mut pq: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for &v in &self.tree_nodes {
            self.dist[v as usize] = 0;
            self.dstamp[v as usize] = stamp;
            pq.push(Reverse((0, v)));
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut found = None;
        while let Some(Reverse((d, v))) = pq.pop() {
            let vi = v as usize;
            if self.dstamp[vi] == stamp && self.dist[vi] < d {
                continue;
            }
            if let Some(pins) = self.members.get(&v)
                && let Some(&pi) = pins
                    .iter()
                    .find(|&&p| !self.connected[p as usize] && self.attachable[p as usize])
            {
                found = Some((v, pi as usize));
                break;
            }
            for pos in self.adj_start[vi] as usize..self.adj_start[vi + 1] as usize {
                let to = self.adj_to[pos];
                if self.in_tree[to as usize] {
                    continue; // tree nodes sit at dist 0 and cannot improve
                }
                let eidx = self.adj_eidx[pos];
                let Some(ec) = edge_cost(
                    g, tracks, tech, vias, occ, own_pins, eidx, own, hist, hard, mode,
                    &mut hits,
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
                    pq.push(Reverse((nd, to)));
                }
            }
        }
        let (mut v, pi) = found?;
        // Backtracked path is simple: parents record the arriving edge
        // of a strictly decreasing `dist` chain and terminate on the
        // tree; the tree only grows (nodes are added, never removed).
        while !self.in_tree[v as usize] {
            self.in_tree[v as usize] = true;
            self.tree_nodes.push(v);
            let e = self.parent[v as usize];
            self.used_edges.push(e);
            v = other_end(g, e, v);
        }
        self.connected[pi] = true;
        Some(pi)
    }
}

/// Routes one `NetGraph`: seed at the driver's attach set (else the
/// first attachable member in `pin_nodes` order), extend the tree one
/// pin at a time, and either return the committed `NetRoute` plus the
/// provisional collisions its edges took, or the per-pin named
/// failure.
fn route_one_net(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
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

    // Attach sets: strict `pin_nodes`, else the projection fallback.
    // `own_pins` collects the member pin rects per routing layer for
    // the pin-carve OBS exemption in `hit_verdict`.
    let mut attach: Vec<Vec<u32>> = Vec::with_capacity(m);
    let mut projected = vec![false; m];
    let mut attachable = vec![true; m];
    let mut own_pins: Vec<Vec<RectDbu>> = vec![Vec::new(); tracks.len()];
    for (pi, &(pin, ref strict)) in g.pin_nodes.iter().enumerate() {
        let shapes = pin_shapes(db, tech, pin);
        for (lid, r) in &shapes {
            if (lid.0 as usize) < own_pins.len() {
                own_pins[lid.0 as usize].push(*r);
            }
        }
        if !strict.is_empty() {
            attach.push(strict.clone());
            continue;
        }
        let nodes = project_attach(g, &shapes, tracks);
        projected[pi] = !nodes.is_empty();
        attachable[pi] = !nodes.is_empty();
        attach.push(nodes);
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

    if !attachable.iter().any(|&a| a) {
        return fail(Vec::new(), ordered_failed(&vec![false; m], &attachable));
    }

    let mut run = NetRun::new(g, vias, attach, projected, attachable);

    // Seed: the driver pin's attach set (`net_driver`, else the first
    // attachable member in `net_pins` order).
    let drv = db.net_driver(g.net);
    let seed = g
        .pin_nodes
        .iter()
        .position(|&(p, _)| Some(p) == drv)
        .filter(|&pi| run.attachable[pi])
        .or_else(|| (0..m).find(|&pi| run.attachable[pi]));
    let Some(seed) = seed else {
        return fail(Vec::new(), ordered_failed(&run.connected, &run.attachable));
    };
    for &v in &run.attach[seed].clone() {
        run.in_tree[v as usize] = true;
        run.tree_nodes.push(v);
    }
    run.connected[seed] = true;
    run.absorb_taps();

    while run
        .connected
        .iter()
        .enumerate()
        .any(|(pi, &c)| !c && run.attachable[pi])
    {
        if run
            .extend_once(g, tracks, tech, vias, occ, &own_pins, g.net.0, hist, hard, mode)
            .is_none()
        {
            return fail(
                ordered_pins(&run.connected),
                ordered_failed(&run.connected, &run.attachable),
            );
        }
        run.absorb_taps();
    }

    // The loop only sweeps attachable pins — a leftover `Unattachable`
    // member still lands the net in the named report.
    if !run.connected.iter().all(|&c| c) {
        return fail(
            ordered_pins(&run.connected),
            ordered_failed(&run.connected, &run.attachable),
        );
    }

    // Extraction: merge unit edge spans per (layer, line) into
    // canonical wire intervals; dedupe via placements.
    let mut hlines: BTreeMap<(u8, i32), Vec<(i32, i32)>> = BTreeMap::new();
    let mut vlines: BTreeMap<(u8, i32), Vec<(i32, i32)>> = BTreeMap::new();
    let mut vplaced: BTreeSet<(u8, i32, i32)> = BTreeSet::new();
    let e_plane = g.edges.len() as u32;
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
        } else {
            let ve = &g.via_edges[(eidx - e_plane) as usize];
            let n = &g.nodes[ve.lower as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            vplaced.insert((n.layer.0, x, y));
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
        edge_collisions(g, tracks, vias, occ, eidx, g.net, &mut prov, &mut hits);
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
                .filter(|(pi, _)| run.connected[*pi] && run.projected[*pi])
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

/// Temporary diagnosis for the real-design NoPath investigation —
/// counts, per guided net, how many in-plane edges are `Obs`-blocked
/// so the wall-around-pins hypothesis is measurable. Removed before
/// the ticket lands.
#[doc(hidden)]
pub fn debug_edge_stats(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
) -> Vec<(u32, usize, usize, usize)> {
    let mut occ = Occupancy::new(tech);
    premark_obs(db, tech, tracks, &mut occ);
    let mut out = Vec::new();
    for g in &graphs.nets {
        let mut blocked = 0usize;
        let mut hits = Vec::new();
        for eidx in 0..g.edges.len() as u32 {
            if edge_cost(g, tracks, tech, vias, &occ, &[], eidx, u32::MAX, 0, false, OccMode::Enabled, &mut hits).is_none() {
                blocked += 1;
            }
        }
        out.push((g.net.0, g.edges.len(), blocked, g.nodes.len()));
    }
    out
}

/// Temporary diagnosis: for one guided net, dump every attach node's
/// incident in-plane edge with its occupancy verdict — `OBS` blocks
/// hard, `FOREIGN n` soft-costs, `FREE` is clean or own-net.
#[doc(hidden)]
pub fn debug_pin_approach(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
    net: u32,
) -> String {
    let mut occ = Occupancy::new(tech);
    premark_obs(db, tech, tracks, &mut occ);
    let g = graphs.nets.iter().find(|g| g.net.0 == net).expect("net");
    let mut s = String::new();
    for (pi, &(pin, ref strict)) in g.pin_nodes.iter().enumerate() {
        let attach = if strict.is_empty() {
            project_attach(g, &pin_shapes(db, tech, pin), tracks)
        } else {
            strict.clone()
        };
        s.push_str(&format!("pin#{pi} attach={attach:?}\n"));
        for &v in &attach {
            let n = &g.nodes[v as usize];
            let lt = &tracks[n.layer.0 as usize];
            let (x, y) = (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize]);
            s.push_str(&format!("  node {v} (l{} x{x} y{y}):\n", n.layer.0));
            let mut hits = Vec::new();
            for (ei, e) in g.edges.iter().enumerate() {
                if e.a != v && e.b != v {
                    continue;
                }
                hits.clear();
                let verdict = match edge_cost(
                    g,
                    tracks,
                    tech,
                    vias,
                    &occ,
                    &[],
                    ei as u32,
                    g.net.0,
                    0,
                    false,
                    OccMode::Enabled,
                    &mut hits,
                ) {
                    None => "OBS".to_string(),
                    Some(c) if hits.is_empty() => format!("FREE c{c}"),
                    Some(c) => {
                        let who: Vec<String> = hits
                            .iter()
                            .map(|h| match h.iv.who {
                                Occupant::Obs => "Obs".into(),
                                Occupant::Net(m) => format!("N{m}"),
                            })
                            .collect();
                        format!("HIT c{c} {who:?}")
                    }
                };
                s.push_str(&format!("    e{ei} {verdict}\n"));
            }
        }
    }
    s
}

/// Temporary diagnosis: for one guided net, run the full
/// `route_one_net` Dijkstra setup under `OccMode::Disabled` and report
/// per-pin attach-node coordinates, how many tree nodes each layer
/// reached, and which attach nodes the wave touched. Removed before
/// the ticket lands.
#[doc(hidden)]
pub fn debug_net_wave(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
    net: u32,
) -> String {
    let mut occ = Occupancy::new(tech);
    premark_obs(db, tech, tracks, &mut occ);
    let g = graphs.nets.iter().find(|g| g.net.0 == net).expect("net");
    let m = g.pin_nodes.len();
    let mut attach: Vec<Vec<u32>> = Vec::with_capacity(m);
    let mut projected = vec![false; m];
    let mut attachable = vec![true; m];
    let mut own_pins: Vec<Vec<RectDbu>> = vec![Vec::new(); tracks.len()];
    for (pi, &(pin, ref strict)) in g.pin_nodes.iter().enumerate() {
        let shapes = pin_shapes(db, tech, pin);
        for (lid, r) in &shapes {
            if (lid.0 as usize) < own_pins.len() {
                own_pins[lid.0 as usize].push(*r);
            }
        }
        if strict.is_empty() {
            let nodes = project_attach(g, &shapes, tracks);
            projected[pi] = !nodes.is_empty();
            attachable[pi] = !nodes.is_empty();
            attach.push(nodes);
        } else {
            attach.push(strict.clone());
        }
    }
    let mut s = String::new();
    for (pi, (&(pin, _), att)) in g.pin_nodes.iter().zip(&attach).enumerate() {
        let i = pin.0 as usize;
        let label = match db.pin_cells[i] {
            Some(c) => format!(
                "{}/{}",
                db.strings.resolve(db.cell_names[c.0 as usize]),
                db.strings.resolve(db.pin_names[i])
            ),
            None => db.strings.resolve(db.pin_names[i]).to_string(),
        };
        s.push_str(&format!(
            "pin#{pi} {label} attachable={} projected={} attach={att:?}\n",
            attachable[pi],
            projected[pi],
        ));
        for &v in att {
            let n = &g.nodes[v as usize];
            let (x, y) = node_xy(g, tracks, v);
            s.push_str(&format!("  node {v} (l{} x{x} y{y})\n", n.layer.0));
            let mut hits = Vec::new();
            let dump = |s: &mut String, hits: &mut Vec<Hit>, eidx: u32| {
                hits.clear();
                let c = edge_cost(
                    g,
                    tracks,
                    tech,
                    vias,
                    &occ,
                    &own_pins,
                    eidx,
                    g.net.0,
                    0,
                    false,
                    OccMode::Disabled,
                    hits,
                );
                let who: Vec<String> = hits
                    .iter()
                    .map(|h| match h.iv.who {
                        Occupant::Obs => format!("Obs@l{}", h.layer),
                        Occupant::Net(mm) => format!("N{mm}@l{}", h.layer),
                    })
                    .collect();
                let to = other_end(g, eidx, v);
                let tn = &g.nodes[to as usize];
                let (tx, ty) = node_xy(g, tracks, to);
                s.push_str(&format!(
                    "    e{eidx} -> l{} ({tx},{ty}) node {to} cost={c:?} {who:?}\n",
                    tn.layer.0
                ));
            };
            let e_plane = g.edges.len() as u32;
            for ei in 0..g.edges.len() as u32 {
                let e = &g.edges[ei as usize];
                if e.a == v || e.b == v {
                    dump(&mut s, &mut hits, ei);
                }
            }
            for vi in 0..g.via_edges.len() as u32 {
                let ve = &g.via_edges[vi as usize];
                if ve.lower == v || ve.upper == v {
                    dump(&mut s, &mut hits, e_plane + vi);
                }
            }
        }
    }
    let mut run = NetRun::new(g, vias, attach, projected, attachable);
    let drv = db.net_driver(g.net);
    let seed = g
        .pin_nodes
        .iter()
        .position(|&(p, _)| Some(p) == drv)
        .filter(|&pi| run.attachable[pi])
        .or_else(|| (0..m).find(|&pi| run.attachable[pi]));
    let Some(seed) = seed else {
        s.push_str("no attachable pin\n");
        return s;
    };
    s.push_str(&format!("seed=pin#{seed}\n"));
    for &v in &run.attach[seed].clone() {
        run.in_tree[v as usize] = true;
        run.tree_nodes.push(v);
    }
    run.connected[seed] = true;
    run.absorb_taps();
    while run
        .connected
        .iter()
        .enumerate()
        .any(|(pi, &c)| !c && run.attachable[pi])
    {
        if run
            .extend_once(g, tracks, tech, vias, &occ, &own_pins, g.net.0, 0, false, OccMode::Disabled)
            .is_none()
        {
            break;
        }
        run.absorb_taps();
    }
    let mut per_layer = vec![0usize; tracks.len()];
    for &v in &run.tree_nodes {
        per_layer[g.nodes[v as usize].layer.0 as usize] += 1;
    }
    s.push_str(&format!("tree={} per_layer={per_layer:?}\n", run.tree_nodes.len()));
    let mut explored = vec![0usize; tracks.len()];
    for (i, n) in g.nodes.iter().enumerate() {
        if run.dstamp[i] == run.stamp {
            explored[n.layer.0 as usize] += 1;
        }
    }
    let mut nodes_per_layer = vec![0usize; tracks.len()];
    for n in &g.nodes {
        nodes_per_layer[n.layer.0 as usize] += 1;
    }
    s.push_str(&format!(
        "explored_last={explored:?} nodes_per_layer={nodes_per_layer:?}\n"
    ));
    for (pi, att) in run.attach.iter().enumerate() {
        let hit: Vec<u32> = att.iter().copied().filter(|&v| run.in_tree[v as usize]).collect();
        s.push_str(&format!(
            "pin#{pi} connected={} reached_attach={hit:?}\n",
            run.connected[pi]
        ));
    }
    s
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
    let n = graphs.nets.len();
    let mut occ = Occupancy::new(tech);
    premark_obs(db, tech, tracks, &mut occ);
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
        let mut prov: BTreeSet<Collision> = BTreeSet::new();
        let mut failed: Vec<(usize, UnroutedNet)> = Vec::new();
        for &i in &bad {
            let g = &graphs.nets[i];
            match route_one_net(db, tech, tracks, g, vias, &occ, hist[i], hard, cfg.occ) {
                NetOutcome::Routed(route, hits) => {
                    commit_route(&mut occ, tracks, vias, &route, &mut marks[i]);
                    if !hard && cfg.occ == OccMode::Enabled {
                        prov.extend(hits);
                    }
                    routes[i] = Some(route);
                }
                NetOutcome::Failed(u) => failed.push((i, u)),
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
            for (&t, ivs) in map {
                let mut sorted: Vec<&Interval> = ivs.iter().collect();
                sorted.sort_by_key(|iv| iv.lo);
                for i in 0..sorted.len() {
                    for b in sorted.iter().skip(i + 1) {
                        let a = sorted[i];
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
            ],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        // Escaped pin: rect covers x-tracks 2000..3000 but no y-track
        // (y range 3000..3500 holds no line) → same-line nodes only.
        let shapes = vec![(LayerId(0), RectDbu::new(1900, 3100, 3100, 3400))];
        let got = project_attach(&g, &shapes, &tracks);
        assert_eq!(got, vec![1, 2], "nodes on the covered x-track line");
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
        let outcome = route_one_net(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &occ,
            0,
            false,
            cfg.occ,
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
