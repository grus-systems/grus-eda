//! Sequential maze routing over the guide-bounded track graph: per
//! guided net, pathfinder-style tree extension under a rasterized
//! interval-occupancy model, in `join.nets` (guide-file) order, with
//! rip-up-and-retry on congestion and a named per-pin unrouted report.
//! See `docs/e6-drt.md` §7 for the model, the cost constants, and the
//! claim boundary.

use crate::design_db::{DesignDb, PlacementStatus};
use crate::drc::area::is_drawn;
use crate::drc::geom::expand_via;
use crate::drc::union;
use crate::drc::union::{rect_covered, rect_minus};
use crate::drc::{DrcLayer, LayerShapes, ShapeRect, ShapeSrc};
use crate::drt::access::pin_shapes;
use crate::drt::graph::{
    EscapeRecord, GraphOpts, GuideJoin, HullMask, NetGraph, PinCover, RegionEscape, RouteGraphs,
    ZeroCoverage, build_net_graph, cell_hull_rect, net_graph_bytes, pin_rects,
};
use crate::drt::pool::drt_pool;
use crate::drt::tracks::{Axis, LayerTracks};
use crate::drt::via::ViaTable;
use crate::interner::{LayerId, NetId, PinId};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    /// Rayon pool size for the streamed attach pass and the rip-up
    /// search; `0` keeps rayon's default parallelism.
    pub threads: usize,
}

impl Default for MazeConfig {
    fn default() -> Self {
        Self {
            occ: OccMode::Enabled,
            max_iters: MAX_ITERS,
            threads: 0,
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
    /// Self-spacing repair rects (`self-spacing.md` Rule B): notch
    /// fills committed by the extraction closure pass, emitted as
    /// `RECT` path records — absolute DBU geometry, rasterized into
    /// occupancy like wires and plates.
    pub fills: Vec<(LayerId, RectDbu)>,
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
    /// Final hard-iteration classification in `pin_nodes` order —
    /// `route_db.unrouted` is written once on the last iteration, so
    /// this is the emitted DEF's connectivity truth. The ever-failed
    /// census (every pin that saw no legal paint on any iteration,
    /// including pins that later re-attached) is
    /// `RouteDb::stub_outside_cover`, not this field.
    pub failed: Vec<(PinId, FailKind)>,
    /// Wave-death postmortem — `(layer, kind)` of the first blocking
    /// mark found on each failed pin's pocket-reachable attach arcs,
    /// where `kind` is 0=`Obs` 1=`ObsPin` 2=`Net` 3=`Pin`. Empty for
    /// failures that never ran a wave (unattachable / empty region)
    /// or when no blocking mark attributed.
    pub blockers: Vec<(u8, u8)>,
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

/// A connected component that stayed under its layer's `AREA`
/// bound after the patch pass — the named-residual record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaResidual {
    pub net: NetId,
    pub layer: LayerId,
    /// Sub-bound component's bbox.
    pub region: RectDbu,
    pub area_dbu2: i64,
    pub bound_dbu2: i64,
}

/// A same-net sub-spacing pair the closure pass could not repair —
/// every candidate fill was inadmissible and neither prong was a
/// droppable access/stub element (tree-only paint). Recorded, never
/// hidden: Magic's `drc(full)` flags the site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchResidual {
    pub net: NetId,
    pub layer: LayerId,
    pub a: RectDbu,
    pub b: RectDbu,
}

/// An enclosed void under the layer's `MINENCLOSEDAREA` bound that
/// the void-closure pass could not legally fill — the named-residual
/// record for the `*.7` hole rules (`residuals.md` §3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleResidual {
    pub net: NetId,
    pub layer: LayerId,
    /// Enclosed void's bbox.
    pub region: RectDbu,
    pub area_dbu2: i64,
    pub bound_dbu2: i64,
}

/// The closure pass's named residual records for one routed net —
/// reported, never hidden (`residuals.md` §5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClosureResiduals {
    /// Same-net sub-spacing notches (`self-spacing.md`).
    pub notches: Vec<NotchResidual>,
    /// Unfillable under-bound enclosed voids (`residuals.md` §3).
    pub holes: Vec<HoleResidual>,
}

/// One iteration of the `route_nets` rip-up loop — the convergence
/// record behind the census `iter_progress`/`iter_prov` lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IterStat {
    /// Nets queued into the iteration — `bad.len()` at entry;
    /// entry 0 is always `graphs.nets.len()` (every net attempts
    /// once before any rip-up exists).
    pub attempted: u32,
    /// Nets that committed a route this iteration — counts
    /// attempts, not distinct nets (a re-queued net that routes
    /// again counts again). `attempted - routed` is the
    /// iteration's failure count.
    pub routed: u32,
    /// Deduped provisional `Collision`s recorded by this
    /// iteration's committed routes — the re-queue trigger: the
    /// canonical net×net set plus the directed wire→pin set (whose
    /// `a` side alone is re-queued). Always 0 on the hard (last)
    /// iteration, where foreign hits block instead of reporting, and
    /// on every iteration under `OccMode::Disabled`, where recording
    /// is skipped.
    pub prov_collisions: u32,
}

/// One attempted net's completion snapshot — the router's only
/// live-progress channel, emitted once per `bad`-loop attempt to
/// the `route_nets_with_progress` sink. `elapsed` is observational
/// and never feeds a routing decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteProgress {
    /// 1-based rip-up iteration (the `IterStat` index).
    pub iteration: u32,
    /// Attempts finished so far this iteration, `1..=queued`, in
    /// `bad` order — routed or failed alike.
    pub done: u32,
    /// Nets queued into this iteration (`IterStat.attempted`).
    pub queued: u32,
    /// Failed attempts so far this iteration — the "unrouted so
    /// far" count. Resets each iteration: re-queued failures count
    /// again in the iteration where they re-fail.
    pub unrouted: u32,
    /// Wall time since `route_nets` entry.
    pub elapsed: std::time::Duration,
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
    /// Committed routes' wire×foreign-pin overlaps (`Enabled` only) —
    /// directed: `a` is the wire's net, `b` the pin's owning net (the
    /// wire is the only movable party). Expected empty after the
    /// `Pin` verdict change; an entry is a named residual, never
    /// hidden.
    pub pin_collisions: Vec<Collision>,
    /// Iterations actually run (≤ `max_iters`).
    pub iterations: u32,
    /// Per-iteration convergence record — `iter_stats[i]` describes
    /// iteration `i + 1` (1-based like `iterations`); `len() ==
    /// iterations` and ≤ `max_iters` by loop construction.
    pub iter_stats: Vec<IterStat>,
    /// Components the min-area patch brought to bound.
    pub area_patches: usize,
    /// Components it could not — named with layer/bbox/area/bound;
    /// every residual maps to a `*.6` self-DRC finding if the gate
    /// fires, and every `*.6` finding must map back to one of these.
    pub area_residuals: Vec<AreaResidual>,
    /// Same-net notches the closure pass could not fill or reject —
    /// expected empty; each maps to a `*.2`-family Magic finding on
    /// the routed DEF (`self-spacing.md`).
    pub notch_residuals: Vec<NotchResidual>,
    /// Under-bound enclosed voids the void-closure pass could not
    /// fill — expected empty; each maps to a `*.7` Magic hole
    /// finding on the routed DEF (`residuals.md` §3).
    pub hole_residuals: Vec<HoleResidual>,
    /// Pins that got no legal paint — the `Unattachable` census
    /// (`pin-access.md` r3 semantics: projected stubs suppressed by
    /// hull confinement plus access arcs blocked by live occupancy).
    /// Collected across all iterations: every `Unattachable` pin of
    /// an `unrouted` net appears here, alongside pins of nets that
    /// re-attached on a later pass.
    pub stub_outside_cover: Vec<(NetId, PinId)>,
}

// ---------------------------------------------------------------------------
// Occupancy
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Occupant {
    /// Macro OBS outside the host cell's pin rects — a wall for every
    /// net, the host's included. `u32::MAX` marks blockage with no
    /// owning cell: metal of pins that own no net.
    Obs(u32),
    /// A committed route's painted metal — the sequential router's
    /// true occupancy: cost in soft iterations, a hard block on the
    /// last.
    Net(u32), // NetId.0
    /// An io-port's placed metal — free to its own net, a soft
    /// penalty to foreign nets in soft iterations, a hard block on
    /// the last. A pin does not move under rip-up, so the *wire* is
    /// the movable party: a foreign hit is recorded as a directed
    /// provisional `Collision` (wire net → pin net) that re-queues
    /// the wire side only, and a survivor is named in
    /// `RouteDb.pin_collisions` (`docs/e6-drt.md` §7).
    Pin(u32), // NetId.0
    /// An instance pin's placed metal — free to its own net, a hard
    /// block to every other net. Unlike a `Pin` mark (io-port rects,
    /// outside every hull) instance-pin metal sits inside a cell
    /// hull where Magic reports any foreign landing as a subcell
    /// overlap — soft-pricing cannot move a pin, so it must wall.
    ObsPin(u32), // NetId.0
}

/// A committed shape's extent along one track line, tagged with its
/// occupant. `perp_*` is the shape's extent on the line's
/// perpendicular axis — needed so rect candidates (via footprints) and
/// the post-hoc census can run the exact `R' ∩ S` test, not just the
/// band approximation a wire candidate gets for free.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Interval {
    pub(crate) lo: i32,
    pub(crate) hi: i32,
    pub(crate) perp_lo: i32,
    pub(crate) perp_hi: i32,
    pub(crate) who: Occupant,
}

/// One rasterized insertion — the rip-up log. `x_axis` selects
/// `lines_x` (vertical lines) vs `lines_y` (horizontal); `line_idx`
/// indexes that layer's coordinate list (`coords_x`/`coords_y`).
#[derive(Clone, Copy, Debug)]
struct Mark {
    x_axis: bool,
    layer: u8,
    line_idx: u32,
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
/// holds y-intervals on vertical lines indexed by track position into
/// `coords_x[l]`; `lines_y[l]` likewise on horizontal lines. Marks
/// only ever live on track lines (`rasterize` windows are
/// `partition_point`s over `tracks`), so the key domain is dense —
/// a direct slot index replaces a keyed lookup, and `coords_*` resolve
/// a slot back to its line coordinate for `Hit.line` and the
/// `scan_rect`/`rasterize` windows.
pub(crate) struct Occupancy {
    lines_x: Vec<Vec<Line>>,
    lines_y: Vec<Vec<Line>>,
    /// Sorted line coordinates per axis — the `LayerTracks` lists,
    /// owned so windows and `Hit.line` resolution need no `tracks`
    /// parameter.
    coords_x: Vec<Vec<i32>>,
    coords_y: Vec<Vec<i32>>,
    /// `width_dbu / 2` per routing layer (integer division — sky130
    /// widths are even).
    halfw: Vec<i32>,
    /// `min_spacing_dbu()` per routing layer.
    spc: Vec<i32>,
    /// Query census (`GRUS_DRT_TIMING` emission in
    /// `route_nets_with_progress`) — slotted atomics so `&self`
    /// scans count from the parallel search workers without
    /// contending on a shared word; totals stay exact because the
    /// query stream is a pure function of the occupancy state.
    stats: ScanStats,
}

/// Census slots — one per worker ticket, cache-line padded so
/// counter updates from parallel searches never share a word
/// (a shared `AtomicU64` here serialized the inner scan loops to
/// ~7× slower than serial).
const STAT_SLOTS: usize = 64;
static STAT_TICKET: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    /// This thread's census slot — one atomic ticket on first use,
    /// then a TLS read per scan call. Slot reuse after `STAT_SLOTS`
    /// workers is safe (atomics), just mildly contended.
    static STAT_SLOT: usize = STAT_TICKET.fetch_add(1, Ordering::Relaxed);
}

#[repr(align(64))]
#[derive(Default)]
struct ScanStatsSlot {
    wire_calls: AtomicU64,
    rect_calls: AtomicU64,
    /// Lines iterated across both axes inside `scan_rect`.
    rect_lines_visited: AtomicU64,
    /// `ivs` elements inspected across all scans (pre-predicate).
    ivs_visited: AtomicU64,
    hits_pushed: AtomicU64,
    marks_inserted: AtomicU64,
    marks_removed: AtomicU64,
}

/// Scan/mark counters — the query-shape record behind the occupancy
/// structure pick (`docs/results.md` occupancy-ds entry). Counting is
/// unconditional (a handful of slot-local adds per query, below timer
/// noise against the interval work being measured); only the emission
/// is `GRUS_DRT_TIMING`-gated.
struct ScanStats {
    slots: [ScanStatsSlot; STAT_SLOTS],
}

impl Default for ScanStats {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| ScanStatsSlot::default()),
        }
    }
}

impl ScanStats {
    fn slot(&self) -> &ScanStatsSlot {
        STAT_SLOT.with(|&s| &self.slots[s % STAT_SLOTS])
    }
    fn get(&self, f: impl Fn(&ScanStatsSlot) -> &AtomicU64) -> u64 {
        self.slots.iter().map(|s| f(s).load(Ordering::Relaxed)).sum()
    }
}

/// One interval hit by a candidate edge: the rasterized mark (`iv`),
/// the line it was found on, and the overlap of the two shape extents
/// along the line (`lo > hi` encodes a gap). `x_axis` says which map
/// the hit came from — `lines_x` marks are vertical-line intervals
/// (`lo..hi` on y), `lines_y` marks horizontal (`lo..hi` on x) — so a
/// blocking shape's full bbox can be reconstructed for repair
/// decomposition.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub(crate) iv: Interval,
    pub(crate) line: i32,
    pub(crate) layer: u8,
    pub(crate) x_axis: bool,
    pub(crate) lo: i32,
    pub(crate) hi: i32,
}

/// The blocking shape's absolute bbox behind `h` — `iv` stores the
/// mark's extent along its line plus the perpendicular extent.
fn hit_bbox(h: &Hit) -> RectDbu {
    if h.x_axis {
        RectDbu::new(h.iv.perp_lo, h.iv.lo, h.iv.perp_hi, h.iv.hi)
    } else {
        RectDbu::new(h.iv.lo, h.iv.perp_lo, h.iv.hi, h.iv.perp_hi)
    }
}

impl Occupancy {
    pub(crate) fn new(tech: &TechDb, tracks: &[LayerTracks]) -> Self {
        Self {
            lines_x: tracks
                .iter()
                .map(|t| vec![Line::default(); t.tracks_x.len()])
                .collect(),
            lines_y: tracks
                .iter()
                .map(|t| vec![Line::default(); t.tracks_y.len()])
                .collect(),
            coords_x: tracks.iter().map(|t| t.tracks_x.clone()).collect(),
            coords_y: tracks.iter().map(|t| t.tracks_y.clone()).collect(),
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
            stats: ScanStats::default(),
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
        layer: u8,
        r: &RectDbu,
        who: Occupant,
        mut log: Option<&mut Vec<Mark>>,
    ) {
        let l = layer as usize;
        let (w2, s) = (self.halfw[l], self.spc[l]);
        let a = self.coords_x[l].partition_point(|&t| t <= r.xl - w2 - s);
        let b = self.coords_x[l].partition_point(|&t| t < r.xh + w2 + s);
        for i in a..b {
            let iv = Interval {
                lo: r.yl,
                hi: r.yh,
                perp_lo: r.xl,
                perp_hi: r.xh,
                who,
            };
            Self::insert(&mut self.lines_x[l][i], iv);
            self.stats.slot().marks_inserted.fetch_add(1, Ordering::Relaxed);
            if let Some(m) = log.as_deref_mut() {
                m.push(Mark {
                    x_axis: true,
                    layer,
                    line_idx: i as u32,
                    iv,
                });
            }
        }
        let a = self.coords_y[l].partition_point(|&t| t <= r.yl - w2 - s);
        let b = self.coords_y[l].partition_point(|&t| t < r.yh + w2 + s);
        for i in a..b {
            let iv = Interval {
                lo: r.xl,
                hi: r.xh,
                perp_lo: r.yl,
                perp_hi: r.yh,
                who,
            };
            Self::insert(&mut self.lines_y[l][i], iv);
            self.stats.slot().marks_inserted.fetch_add(1, Ordering::Relaxed);
            if let Some(m) = log.as_deref_mut() {
                m.push(Mark {
                    x_axis: false,
                    layer,
                    line_idx: i as u32,
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

    /// Frees one recorded insertion (rip-up). A logged mark always
    /// points at a live slot — indices never move.
    fn unmark(&mut self, m: &Mark) {
        let lines = if m.x_axis {
            &mut self.lines_x[m.layer as usize]
        } else {
            &mut self.lines_y[m.layer as usize]
        };
        let line = &mut lines[m.line_idx as usize];
        if let Some(pos) = line.ivs.iter().position(|iv| *iv == m.iv) {
            line.ivs.remove(pos);
            Self::rebuild_from(line, pos / SKIP_CHUNK);
            self.stats.slot().marks_removed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Wire-candidate check on track index `ti` of layer `l`: the
    /// candidate shape spans track coords `[c, d]`, so its extent is
    /// `[c − w/2, d + w/2]` and hits are the line's intervals
    /// overlapping `[c − w/2 − s, d + w/2 + s]` — shape edge-distance
    /// `< s` along the line; the perpendicular `< s` is already
    /// guaranteed because the committed shape rasterized the line
    /// only inside its `w/2 + s` band.
    fn scan_wire(
        &self,
        axis: Axis,
        layer: u8,
        ti: u32,
        c: i32,
        d: i32,
        out: &mut Vec<Hit>,
    ) {
        let l = layer as usize;
        let (w2, s) = (self.halfw[l], self.spc[l]);
        // Census is bulk-added once per call — a per-element store
        // into the worker's slot costs more than the predicate it
        // measures under parallel search.
        let st = self.stats.slot();
        st.wire_calls.fetch_add(1, Ordering::Relaxed);
        let (mut ivs_seen, mut hits_pushed) = (0u64, 0u64);
        let (lines, coords) = match axis {
            Axis::X => (&self.lines_y[l], &self.coords_y[l]),
            Axis::Y => (&self.lines_x[l], &self.coords_x[l]),
        };
        let line = &lines[ti as usize];
        let t = coords[ti as usize];
        let (qlo, qhi) = (c - w2 - s, d + w2 + s);
        let (slo, shi) = (c - w2, d + w2);
        'scan: for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
            if line.max_hi[ci] <= qlo {
                continue;
            }
            for iv in chunk {
                ivs_seen += 1;
                if iv.lo >= qhi {
                    break 'scan; // `lo`-sorted: no later interval can overlap
                }
                if iv.hi > qlo {
                    hits_pushed += 1;
                    out.push(Hit {
                        iv: *iv,
                        line: t,
                        layer,
                        x_axis: matches!(axis, Axis::Y),
                        lo: iv.lo.max(slo),
                        hi: iv.hi.min(shi),
                    });
                }
            }
        }
        st.ivs_visited.fetch_add(ivs_seen, Ordering::Relaxed);
        st.hits_pushed.fetch_add(hits_pushed, Ordering::Relaxed);
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
        let st = self.stats.slot();
        st.rect_calls.fetch_add(1, Ordering::Relaxed);
        let (mut lines_seen, mut ivs_seen, mut hits_pushed) = (0u64, 0u64, 0u64);
        let re = RectDbu::new(r.xl - s, r.yl - s, r.xh + s, r.yh + s);
        let ay = self.coords_y[l].partition_point(|&t| t < re.yl - w2 - s);
        let by = self.coords_y[l].partition_point(|&t| t <= re.yh + w2 + s);
        for ti in ay..by {
            lines_seen += 1;
            let t = self.coords_y[l][ti];
            let line = &self.lines_y[l][ti];
            for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
                if line.max_hi[ci] <= re.xl {
                    continue;
                }
                for iv in chunk {
                    ivs_seen += 1;
                    if iv.lo >= re.xh {
                        break;
                    }
                    if iv.hi > re.xl && iv.perp_lo < re.yh && iv.perp_hi > re.yl {
                        hits_pushed += 1;
                        out.push(Hit {
                            iv: *iv,
                            line: t,
                            layer,
                            x_axis: false,
                            lo: iv.lo.max(r.xl),
                            hi: iv.hi.min(r.xh),
                        });
                    }
                }
            }
        }
        let ax = self.coords_x[l].partition_point(|&t| t < re.xl - w2 - s);
        let bx = self.coords_x[l].partition_point(|&t| t <= re.xh + w2 + s);
        for ti in ax..bx {
            lines_seen += 1;
            let t = self.coords_x[l][ti];
            let line = &self.lines_x[l][ti];
            for (ci, chunk) in line.ivs.chunks(SKIP_CHUNK).enumerate() {
                if line.max_hi[ci] <= re.yl {
                    continue;
                }
                for iv in chunk {
                    ivs_seen += 1;
                    if iv.lo >= re.yh {
                        break;
                    }
                    if iv.hi > re.yl && iv.perp_lo < re.xh && iv.perp_hi > re.xl {
                        hits_pushed += 1;
                        out.push(Hit {
                            iv: *iv,
                            line: t,
                            layer,
                            x_axis: true,
                            lo: iv.lo.max(r.yl),
                            hi: iv.hi.min(r.yh),
                        });
                    }
                }
            }
        }
        st.rect_lines_visited
            .fetch_add(lines_seen, Ordering::Relaxed);
        st.ivs_visited.fetch_add(ivs_seen, Ordering::Relaxed);
        st.hits_pushed.fetch_add(hits_pushed, Ordering::Relaxed);
    }
}

/// Pre-marks every placed cell's macro `obs_rects` as `Obs` (the
/// `drc::geom` source-4 convention: `{Placed, Fixed, Cover}` gate +
/// `orient_rect` + translate, replicated for OBS only), every pin's
/// shapes via `rasterize_pin_marks` — instance pins `ObsPin(own)`
/// (own-free, foreign hard block), io-port rects `Pin(own)` (own
/// free, foreign soft-priced and recorded, hard-blocked on the last
/// iteration) — and every PDN `SPECIALNETS` wire rect and via stack
/// metal as `Obs(u32::MAX)`. Pins without a net and PDN metal get
/// `Obs(u32::MAX)` — immovable blockage with no exempting host:
/// signal routes must hold the `w/2 + s` band off the power grid.
/// io-port rects mark `Pin(own_net)` — a foreign wire inside a
/// port's spacing band is a real emit-time finding (the gcd `met3.2`
/// `req_msg`/`resp_msg` class), so the band is priced, recorded
/// wire-side, and finally walled.
fn premark_obs(db: &DesignDb, tech: &TechDb, occ: &mut Occupancy) {
    premark_marks(db, tech, occ, true);
}

/// `premark_obs` with the io-port `Pin` arm gated by `io_pin_marks`:
/// `false` drops every io-port mark while keeping the fixed walls
/// (`Obs`, `ObsPin`, net-less `Obs(u32::MAX)`, PDN) — the probe
/// occupancy `drt::probe` re-attempts unrouted nets against. A fresh
/// occupancy carries no `Net` marks, and an own `Pin` mark is free to
/// its own net under every verdict, so dropping *all* `Pin` marks is
/// behaviour-identical to dropping only foreign ones.
pub(crate) fn premark_marks(db: &DesignDb, tech: &TechDb, occ: &mut Occupancy, io_pin_marks: bool) {
    // Cell → its instance pins' placed shapes per layer — OBS paint
    // clipped against them keeps a wall only where it is not pin.
    let mut cell_pins: BTreeMap<u32, Vec<(u8, RectDbu)>> = BTreeMap::new();
    for p in 0..db.pin_names.len() {
        let Some(c) = db.pin_cells[p] else {
            continue;
        };
        for (lid, r) in pin_shapes(db, tech, PinId(p as u32)) {
            cell_pins.entry(c.0).or_default().push((lid.0, r));
        }
    }
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
        let pins = cell_pins.get(&(c as u32));
        for &(lid, r) in mac
            .obs_rects_raw
            .iter()
            .chain(mac.pin_obs_rects.iter())
        {
            if (lid.0 as usize) >= occ.lines_x.len() {
                continue;
            }
            let pr = place(r);
            // OBS clipped against the cell's own pin rects: the
            // pin-area part is already an `ObsPin` mark (own-free to
            // the pin's net), the residual slabs are walls for *every*
            // net, the host's included — real access keeps full
            // `w/2 + s` clearance to non-pin OBS like the delegated
            // routes do. The old whole-cell carve let host wires sit
            // inside the band (the `drc(full)` li-class residual).
            let subs: Vec<RectDbu> = pins
                .map(|ps| {
                    ps.iter()
                        .filter(|&&(pl, _)| pl == lid.0)
                        .map(|&(_, p)| p)
                        .collect()
                })
                .unwrap_or_default();
            for slab in rect_minus(vec![pr], &subs) {
                occ.rasterize(lid.0, &slab, Occupant::Obs(c as u32), None);
            }
        }
    }
    rasterize_pin_marks(db, tech, occ, io_pin_marks);
    // PDN stripes and via stacks: fixed metal every signal net must
    // clear — the emit-time self-DRC measures spacing against these
    // shapes, so the router has to price them.
    for pdn in &db.pdn_nets {
        for &(lid, r) in &pdn.wire_rects {
            if (lid.0 as usize) < occ.lines_x.len() {
                occ.rasterize(lid.0, &r, Occupant::Obs(u32::MAX), None);
            }
        }
        for via in &pdn.vias {
            for (dl, r) in expand_via(via, tech).expect("pdn via resolved at parse") {
                if let DrcLayer::Routing(lid) = dl
                    && (lid.0 as usize) < occ.lines_x.len()
                {
                    occ.rasterize(lid.0, &r, Occupant::Obs(u32::MAX), None);
                }
            }
        }
    }
}

/// Rasterizes every pin's placed shapes with the `premark_obs` mark
/// rule — instance pins `ObsPin(net)`, io-port rects `Pin(net)`,
/// net-less pins `Obs(u32::MAX)`; pins on non-`{Placed, Fixed,
/// Cover}` hosts are skipped. Shared by the router's premark and
/// `pin_overlap_census`'s rebuild so the census measures against
/// exactly the marks the router saw. `io_pin_marks == false` drops
/// the io-port `Pin` arm entirely — the `drt::probe` occupancy.
fn rasterize_pin_marks(db: &DesignDb, tech: &TechDb, occ: &mut Occupancy, io_pin_marks: bool) {
    for p in 0..db.pin_names.len() {
        if let Some(c) = db.pin_cells[p]
            && !matches!(
                db.cell_status[c.0 as usize],
                PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
            )
        {
            continue;
        }
        let pin = PinId(p as u32);
        let who = match db.pin_nets[p] {
            None => Occupant::Obs(u32::MAX),
            // Instance-pin metal sits inside a hull — foreign wires
            // must wall off it, not soft-price past it; io-port rects
            // live outside hulls and keep the soft `Pin` price (the
            // met3.2 port-band finding class).
            Some(n) if db.pin_cells[p].is_some() => Occupant::ObsPin(n.0),
            Some(n) if io_pin_marks => Occupant::Pin(n.0),
            Some(_) => continue,
        };
        for (lid, r) in pin_shapes(db, tech, pin) {
            if (lid.0 as usize) < occ.lines_x.len() {
                occ.rasterize(lid.0, &r, who, None);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Attach sets
// ---------------------------------------------------------------------------

/// Index range `[a, b)` of `list` entries inside `[lo, hi]` — the
/// `drt::graph` convention, replicated for the projection fallback.
pub(crate) fn covered_range(list: &[i32], lo: i32, hi: i32) -> (usize, usize) {
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
///
/// Hull confinement: the stub a projected node implies — node → pin
/// rect clamp — is committed metal, so a candidate whose stub would
/// cross an uncovered hull is never admitted (the emit-side check in
/// `route_one_net` is the matching guard). The stub is also scanned
/// against `occ` — premarks only at topo time (`Net` marks cannot
/// exist yet; the live-occupancy recheck is `try_pin_paint`'s) — so
/// a candidate whose only stub crosses a foreign pin/OBS band is
/// rejected here instead of being claimed and dropped at extraction.
fn project_attach(
    g: &NetGraph,
    shapes: &[(LayerId, RectDbu)],
    tracks: &[LayerTracks],
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    own: u32,
) -> Vec<u32> {
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
            if !covered || !xs.contains(&(n.ix as usize)) || !ys.contains(&(n.iy as usize)) {
                continue;
            }
            let (x, y) = (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize]);
            let (cx, cy) = (x.clamp(r.xl, r.xh), y.clamp(r.yl, r.yh));
            if !mask.seg_allowed(cover, lid.0, x, y, cx, cy) {
                continue;
            }
            let v = (lo + i) as u32;
            let mut hits = Vec::new();
            scan_stub(g, tracks, occ, cover, v, lid.0, cx, cy, &mut hits);
            if hits.iter().any(|h| fill_blocked(h, own)) {
                continue;
            }
            out.insert(v);
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
/// (`Obs` always; foreign `Net` in hard mode), `Some(extra)` = extra
/// cost — `(OCC_PENALTY + hist·HIST_STEP)` per distinct foreign net
/// hit, `Net` and `Pin` pooling into the same set.
///
/// `Obs(cell)` marks block every net, the host's included — access
/// metal must keep full `w/2 + s` clearance to non-pin OBS like the
/// delegated routes do. The old host-cell carve (own-host `Obs`
/// invisible on `mask.carve` layers) is what let own wires sit inside
/// the OBS spacing band; `premark_obs` now scopes the exemption
/// instead: OBS paint inside a pin rect is marked `ObsPin(pin_net)`
/// up front, so what remains as `Obs` never needs carving.
/// `Obs(u32::MAX)` (net-less pin metal, PDN) always blocks.
///
/// `Pin` marks (io-port rects): own free, foreign soft-priced like
/// `Net` — including the hard block on the last iteration. Rip-up
/// cannot move a pin, so the record side is asymmetric instead:
/// `edge_collisions` logs the hit as a *directed* provisional
/// collision and `route_nets` re-queues only the wire net.
/// `ObsPin` marks (instance-pin metal inside hulls): own free,
/// foreign hard-blocked — a soft price there is a Magic
/// subcell-overlap finding the maze cannot otherwise see.
fn hit_verdict(hits: &[Hit], own: u32, hist: u32, hard: bool, mode: OccMode) -> Option<u32> {
    for h in hits {
        match h.iv.who {
            Occupant::Obs(_) => {
                return None;
            }
            Occupant::ObsPin(m) if m != own => {
                return None;
            }
            _ => {}
        }
    }
    if mode == OccMode::Disabled {
        return Some(0);
    }
    let mut foreign: Vec<u32> = Vec::new();
    let mut blocked = false;
    for h in hits {
        let m = match h.iv.who {
            Occupant::Net(m) | Occupant::Pin(m) => {
                blocked |= m != own;
                m
            }
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
    cover: &PinCover,
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
        let (ti, line, c, d) = match e.axis {
            Axis::X => (
                na.iy,
                lt.tracks_y[na.iy as usize],
                lt.tracks_x[na.ix as usize].min(lt.tracks_x[nb.ix as usize]),
                lt.tracks_x[na.ix as usize].max(lt.tracks_x[nb.ix as usize]),
            ),
            Axis::Y => (
                na.ix,
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
        let from = hits.len();
        occ.scan_wire(e.axis, l, ti, c, d, hits);
        let w2 = occ.halfw[l as usize];
        let paint = match e.axis {
            Axis::X => RectDbu::new(c, line - w2, d, line + w2),
            Axis::Y => RectDbu::new(line - w2, c, line + w2, d),
        };
        exempt_obs(cover, &paint, hits, from);
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
            scan_stub(g, tracks, occ, cover, rv.a, rv.lower, rv.x, rv.y, hits);
            scan_stub(g, tracks, occ, cover, rv.b, rv.lower + 1, rv.x, rv.y, hits);
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
            let from = hits.len();
            occ.scan_rect(lid.0, &abs, hits);
            exempt_obs(cover, &abs, hits, from);
        }
    }
    hit_verdict(hits, own, hist, hard, mode).map(|extra| base + extra)
}

/// Drops the hits added since `from` whose `Obs(c)` spacing does not
/// apply: paint fully inside cell `c`'s own-pin cover *union* merges
/// with the pin on connect (the delegated access stacks sit flush
/// inside the pin rect and Magic reports nothing — the
/// subcell-internal abutment is the cell's own geometry), while any
/// paint outside the pin rects keeps the full `w/2 + s` wall.
/// Union-exact (`rect_covered`): paint spanning joined same-pin
/// sub-rects of one pin exempts just like paint inside a single rect.
fn exempt_obs(cover: &PinCover, paint: &RectDbu, hits: &mut Vec<Hit>, from: usize) {
    let kept: Vec<Hit> = hits[from..]
        .iter()
        .copied()
        .filter(|h| {
            !matches!(h.iv.who, Occupant::Obs(c) if cover.get(&c).is_some_and(|rs| {
                rect_covered(rs, paint)
            }))
        })
        .collect();
    hits.truncate(from);
    hits.extend(kept);
}

/// Scans the wire stub a relaxed via implies between node `n`'s
/// position and the via position `(x, y)` on layer `l` — the
/// transition's real metal outside the via footprint. Each endpoint
/// shares one coordinate with `(x, y)` by construction, so the stub
/// is a single-axis interval (or absent when `n` sits on the via).
#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_stub(
    g: &NetGraph,
    tracks: &[LayerTracks],
    occ: &Occupancy,
    cover: &PinCover,
    n: u32,
    l: u8,
    x: i32,
    y: i32,
    out: &mut Vec<Hit>,
) {
    let (nx, ny) = node_xy(g, tracks, n);
    let node = &g.nodes[n as usize];
    let w2 = occ.halfw[l as usize];
    if nx == x {
        if ny != y {
            let from = out.len();
            occ.scan_wire(Axis::Y, l, node.ix, ny.min(y), ny.max(y), out);
            let paint = RectDbu::new(nx - w2, ny.min(y), nx + w2, ny.max(y));
            exempt_obs(cover, &paint, out, from);
        }
    } else if ny == y {
        let from = out.len();
        occ.scan_wire(Axis::X, l, node.iy, nx.min(x), nx.max(x), out);
        let paint = RectDbu::new(nx.min(x), ny - w2, nx.max(x), ny + w2);
        exempt_obs(cover, &paint, out, from);
    }
}

/// Records every foreign spacing hit of one committed-edge scan as a
/// provisional `Collision`. `Net` hits go to `out` deduped on
/// `(a, b, layer, line, lo, hi)` — `a < b` canonically. `Pin` hits
/// go to `out_pins` *directed* — `a` is always the committing (wire)
/// net, `b` the pin's owning net: the wire is the only movable
/// party, so `route_nets` re-queues `a` alone. `ObsPin` stays
/// unrecorded (already a hard block — a hit means the edge was never
/// committed, or a bug).
fn record_hits(
    layer: LayerId,
    hits: &[Hit],
    own: NetId,
    out: &mut BTreeSet<Collision>,
    out_pins: &mut BTreeSet<Collision>,
) {
    for h in hits {
        match h.iv.who {
            Occupant::Net(m) if m != own.0 => {
                out.insert(Collision {
                    a: NetId(own.0.min(m)),
                    b: NetId(own.0.max(m)),
                    layer,
                    line: h.line,
                    lo: h.lo,
                    hi: h.hi,
                });
            }
            Occupant::Pin(m) if m != own.0 => {
                out_pins.insert(Collision {
                    a: own,
                    b: NetId(m),
                    layer,
                    line: h.line,
                    lo: h.lo,
                    hi: h.hi,
                });
            }
            _ => {}
        }
    }
}

/// One committed edge's commit-time collision scan, its parameters
/// resolved at extraction (`edge_probe`) while the attempt's rebuilt
/// `NetGraph`/`NetAdj` are still live — the commit pass replays these
/// against live occupancy with no graph reference. Order mirrors the
/// per-edge `record` sequence `edge_collisions` produced exactly.
#[derive(Clone, Copy)]
pub(crate) enum EdgeProbe {
    /// `scan_wire(axis, l, ti, c, d)` then `record(l)` — the in-plane
    /// `g.edges` arm.
    Wire {
        axis: Axis,
        l: u8,
        ti: u32,
        c: i32,
        d: i32,
    },
    /// `scan_wire` + `exempt_obs(paint)` then `record(l)` — one
    /// `scan_stub` leg of a relaxed via (committed metal on the
    /// endpoint's layer).
    Stub {
        axis: Axis,
        l: u8,
        ti: u32,
        c: i32,
        d: i32,
        paint: RectDbu,
    },
    /// `scan_rect` per routing rect of the pair default then
    /// `record(lid)` — a strict via edge or a relaxed via's head.
    ViaFp { k: u8, x: i32, y: i32 },
}

/// Resolves one used edge's commit-time collision scan into
/// `EdgeProbe`s — the parameter computation of the old
/// `edge_collisions`, moved to extraction.
fn edge_probe(
    g: &NetGraph,
    tracks: &[LayerTracks],
    relax: &[RelaxedVia],
    occ: &Occupancy,
    eidx: u32,
    out: &mut Vec<EdgeProbe>,
) {
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    if eidx < e_plane {
        let e = &g.edges[eidx as usize];
        let (na, nb) = (&g.nodes[e.a as usize], &g.nodes[e.b as usize]);
        let l = na.layer.0;
        let lt = &tracks[l as usize];
        let (ti, c, d) = match e.axis {
            Axis::X => (
                na.iy,
                lt.tracks_x[na.ix as usize].min(lt.tracks_x[nb.ix as usize]),
                lt.tracks_x[na.ix as usize].max(lt.tracks_x[nb.ix as usize]),
            ),
            Axis::Y => (
                na.ix,
                lt.tracks_y[na.iy as usize].min(lt.tracks_y[nb.iy as usize]),
                lt.tracks_y[na.iy as usize].max(lt.tracks_y[nb.iy as usize]),
            ),
        };
        out.push(EdgeProbe::Wire {
            axis: e.axis,
            l,
            ti,
            c,
            d,
        });
    } else if eidx < v_plane {
        let ve = &g.via_edges[(eidx - e_plane) as usize];
        let (x, y) = node_xy(g, tracks, ve.lower);
        out.push(EdgeProbe::ViaFp {
            k: g.nodes[ve.lower as usize].layer.0,
            x,
            y,
        });
    } else {
        let rv = &relax[(eidx - v_plane) as usize];
        // Stubs are committed metal — same miss class as in
        // `edge_cost`. `scan_stub`'s geometry is resolved per
        // endpoint leg, in the same (lower, upper) record order.
        for (n, l) in [(rv.a, rv.lower), (rv.b, rv.lower + 1)] {
            let (nx, ny) = node_xy(g, tracks, n);
            let node = &g.nodes[n as usize];
            let w2 = occ.halfw[l as usize];
            if nx == rv.x {
                if ny != rv.y {
                    out.push(EdgeProbe::Stub {
                        axis: Axis::Y,
                        l,
                        ti: node.ix,
                        c: ny.min(rv.y),
                        d: ny.max(rv.y),
                        paint: RectDbu::new(nx - w2, ny.min(rv.y), nx + w2, ny.max(rv.y)),
                    });
                }
            } else if ny == rv.y {
                out.push(EdgeProbe::Stub {
                    axis: Axis::X,
                    l,
                    ti: node.iy,
                    c: nx.min(rv.x),
                    d: nx.max(rv.x),
                    paint: RectDbu::new(nx.min(rv.x), ny - w2, nx.max(rv.x), ny + w2),
                });
            }
        }
        out.push(EdgeProbe::ViaFp {
            k: rv.lower,
            x: rv.x,
            y: rv.y,
        });
    }
}

/// Replays resolved `EdgeProbe`s against live occupancy at commit —
/// the same scans `edge_collisions` ran, in the same order, so the
/// provisional-collision sets are byte-identical.
#[allow(clippy::too_many_arguments)]
fn probe_collisions(
    occ: &Occupancy,
    vias: &ViaTable,
    cover: &PinCover,
    probes: &[EdgeProbe],
    own: NetId,
    out: &mut BTreeSet<Collision>,
    out_pins: &mut BTreeSet<Collision>,
    hits: &mut Vec<Hit>,
) {
    for p in probes {
        match *p {
            EdgeProbe::Wire { axis, l, ti, c, d } => {
                hits.clear();
                occ.scan_wire(axis, l, ti, c, d, hits);
                record_hits(LayerId(l), hits, own, out, out_pins);
            }
            EdgeProbe::Stub {
                axis,
                l,
                ti,
                c,
                d,
                paint,
            } => {
                hits.clear();
                occ.scan_wire(axis, l, ti, c, d, hits);
                exempt_obs(cover, &paint, hits, 0);
                record_hits(LayerId(l), hits, own, out, out_pins);
            }
            EdgeProbe::ViaFp { k, x, y } => {
                let pair = &vias.pairs[k as usize];
                let Some(di) = pair.default else {
                    continue;
                };
                let cand = &vias.candidates[pair.candidates[di as usize] as usize];
                for (dl, r) in &cand.rects {
                    let DrcLayer::Routing(lid) = dl else {
                        continue;
                    };
                    let shift = RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh);
                    hits.clear();
                    occ.scan_rect(lid.0, &shift, hits);
                    record_hits(*lid, hits, own, out, out_pins);
                }
            }
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

/// Up to `n` elements of `xs` (key-sorted ascending) nearest `v`,
/// nearest first — a bounded candidate list for the relaxed-via
/// confinement probe.
fn nearest_k<T: Copy>(xs: &[T], key: impl Fn(T) -> i32, v: i32, n: usize) -> Vec<T> {
    let p = xs.partition_point(|&x| key(x) < v);
    let (mut a, mut b) = (p, p);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let la = a.checked_sub(1).map_or(i32::MAX, |i| v - key(xs[i]));
        let lb = xs.get(b).map_or(i32::MAX, |&x| key(x) - v);
        if la == i32::MAX && lb == i32::MAX {
            break;
        }
        if la <= lb {
            a -= 1;
            out.push(xs[a]);
        } else {
            out.push(xs[b]);
            b += 1;
        }
    }
    out
}

/// Up to `n` indices into sorted `list` nearest `v`, nearest first.
fn nearest_idx(list: &[i32], v: i32, n: usize) -> Vec<u32> {
    let p = list.partition_point(|&t| t < v);
    let (mut a, mut b) = (p, p);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let la = a.checked_sub(1).map_or(i32::MAX, |i| v - list[i]);
        let lb = list.get(b).map_or(i32::MAX, |&t| t - v);
        if la == i32::MAX && lb == i32::MAX {
            break;
        }
        if la <= lb {
            a -= 1;
            out.push(a as u32);
        } else {
            out.push(b as u32);
            b += 1;
        }
    }
    out
}

/// layer → line → sorted in-region node indices (`layer_line_maps`'s
/// pair).
type LineNodes = Vec<BTreeMap<u32, Vec<u32>>>;

/// layer → line → sorted in-region node indices: `iy → [ix]` for
/// horizontal landings, `ix → [iy]` for vertical.
fn layer_line_maps(g: &NetGraph, nl: usize) -> (LineNodes, LineNodes) {
    let mut lines_h: LineNodes = vec![BTreeMap::new(); nl];
    let mut lines_v: LineNodes = vec![BTreeMap::new(); nl];
    for n in &g.nodes {
        let l = n.layer.0 as usize;
        lines_h[l].entry(n.iy).or_default().push(n.ix);
        lines_v[l].entry(n.ix).or_default().push(n.iy);
    }
    (lines_h, lines_v)
}

/// Per-net relaxed via edges: for every node `v` on routing layer `k`
/// (with `k + 1` still a routing layer and `pairs[k].default` set —
/// a `ViaGap` pair emits none), up to `RELAX_K` confined edges to
/// layer `k + 1`.
///
/// Upper `Horizontal` ⇒ the via sits at `(v.x, upper.ty[jy])` with
/// `jy` the upper y-track nearest `v.y`, and `b` is the in-region
/// upper node on that line nearest `v.x`. Upper `Vertical` mirrors it:
/// `(upper.tx[jx], v.y)`, `b` on that x-line nearest `v.y`. The true
/// position is always on both layers' preferred wires; the hang node
/// is the same wire a track step away at most. Emitted in node order —
/// deterministic; parallel strict/relaxed edges are left alone (the
/// search takes the cheaper, extraction dedupes the placement).
///
/// Hull confinement: a strict `ViaEdge` sits on two surviving nodes,
/// but a relaxed position/stubs can paint metal a strict node could
/// not — the position must be legal on both layers, each implied stub
/// must stay inside the net's own pin rects on masked layers, and
/// every via rect on a masked layer must fit inside one pin rect.
fn relaxed_vias(
    g: &NetGraph,
    tech: &TechDb,
    tracks: &[LayerTracks],
    vias: &ViaTable,
    mask: &HullMask,
    cover: &PinCover,
) -> Vec<RelaxedVia> {
    let nl = tracks.len();
    let (lines_h, lines_v) = layer_line_maps(g, nl);
    // Pairs arrive sorted — nodes enumerate (layer, ix, iy) ascending.
    let node_id = |l: u8, ix: u32, iy: u32| -> u32 {
        let lo = g
            .nodes
            .partition_point(|n| (n.layer.0, n.ix, n.iy) < (l, ix, iy));
        debug_assert!(
            lo < g.nodes.len() && {
                let n = &g.nodes[lo];
                n.layer.0 == l && n.ix == ix && n.iy == iy
            }
        );
        lo as u32
    };
    let mut out = Vec::new();
    let mut scratch = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        let k = n.layer.0 as usize;
        if k + 1 >= nl {
            continue;
        }
        let Some(di) = vias.pairs[k].default else {
            continue;
        };
        let lt = &tracks[k];
        let up = &tracks[k + 1];
        let (x, y) = (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize]);
        let lower = n.layer.0;
        let vcand = &vias.candidates[vias.pairs[k].candidates[di as usize] as usize];
        // Candidate (via position, b) pairs nearest-first, bounded —
        // on a masked layer the nearest track can sit outside a
        // narrow pin corridor while the next one over lands inside
        // it, so the first *confined* candidates win rather than the
        // unconditionally nearest. Up to `RELAX_K` are kept as
        // separate arcs: a pocket whose sole escape position fails
        // `w/2 + s` against sibling `ObsPin` metal stays `NoPath`
        // under a singleton, while another confined position clears
        // (the `_300_/A2` class). Unmasked layers answer `true` at
        // once, so the enumeration cost only appears where the mask
        // actually filters.
        let confined = |scratch: &mut Vec<u32>, vx: i32, vy: i32, bx: i32, by: i32| {
            mask.point_allowed(cover, scratch, lower, vx, vy)
                && mask.point_allowed(cover, scratch, lower + 1, vx, vy)
                && mask.seg_allowed(cover, lower, x, y, vx, vy)
                && mask.seg_allowed(cover, lower + 1, vx, vy, bx, by)
                && vcand.rects.iter().all(|(dl, r)| {
                    let DrcLayer::Routing(lid) = dl else {
                        return true;
                    };
                    mask.rect_allowed(
                        cover,
                        lid.0,
                        &RectDbu::new(vx + r.xl, vy + r.yl, vx + r.xh, vy + r.yh),
                    )
                })
        };
        const RELAX_K: usize = 3;
        let mut found: Vec<(i32, i32, u32, u32)> = Vec::new();
        match tech.routing_layers[k + 1].dir {
            LayerDir::Horizontal => 'enough: {
                for jy in nearest_idx(&up.tracks_y, y, 4) {
                    let Some(ixs) = lines_h[k + 1].get(&jy) else {
                        continue;
                    };
                    let vy = up.tracks_y[jy as usize];
                    for jx in nearest_k(ixs, |jx| up.tracks_x[jx as usize], x, 3) {
                        let bx = up.tracks_x[jx as usize];
                        if confined(&mut scratch, x, vy, bx, vy) {
                            found.push((x, vy, jx, jy));
                            if found.len() >= RELAX_K {
                                break 'enough;
                            }
                        }
                    }
                }
            }
            LayerDir::Vertical => 'enough: {
                for jx in nearest_idx(&up.tracks_x, x, 4) {
                    let Some(iys) = lines_v[k + 1].get(&jx) else {
                        continue;
                    };
                    let vx = up.tracks_x[jx as usize];
                    for jy in nearest_k(iys, |jy| up.tracks_y[jy as usize], y, 3) {
                        let by = up.tracks_y[jy as usize];
                        if confined(&mut scratch, vx, y, vx, by) {
                            found.push((vx, y, jx, jy));
                            if found.len() >= RELAX_K {
                                break 'enough;
                            }
                        }
                    }
                }
            }
        }
        for (vx, vy, bix, biy) in found {
            out.push(RelaxedVia {
                a: i as u32,
                b: node_id((k + 1) as u8, bix, biy),
                x: vx,
                y: vy,
                lower,
            });
        }
    }
    out
}

/// One pin-anchored access candidate — the delegated router's model
/// (`design/pin-access.md`): a `lower→lower+1` via at `(x, y)` whose
/// plate on the pin shape's own layer sits fully inside the pin rect
/// (connectivity to the pin's metal, plus the subcell-boundary rule),
/// reached from node `b` by the single-axis stub `b ↔ (x, y)` on
/// `lower + 1` — `b` shares the position's line by construction.
/// `b` joins the pin's attach set so the wave can claim it; the paint
/// — via + upper stub — is committed at extraction after a live
/// occupancy check, and only when the pin has no already-painted
/// entry.
struct AccessVia {
    x: i32,
    y: i32,
    lower: u8,
    b: u32,
}

/// Per member pin (`pin_nodes` order): confined pin-anchored access
/// candidates on the pin's masked-layer shapes. A pin rect that holds
/// no via-capable track crossing is otherwise a graph island — the
/// grown cover used to bridge it by letting masked-layer paint
/// overrun the rect, which is exactly the `drc(full)` residual; the
/// off-track candidate lands the whole access stack inside the raw
/// rect instead (all 884 delegated mcons sit inside their pin rects).
///
/// Probe space per shape: up to `ACCESS_K` positions — on a
/// `Horizontal` upper, the position shares an upper y-track and its
/// x runs free inside the plate-fit range (edge fits first, matching
/// the delegated right-edge choice); `Vertical` mirrors. Each
/// candidate's `b` is the nearest surviving upper node on that line;
/// positions are confinement-checked (point, stub, every masked via
/// rect) at generation and occupancy-checked at extraction.
#[allow(clippy::too_many_arguments)]
fn access_vias(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    vias: &ViaTable,
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
) -> Vec<Vec<AccessVia>> {
    const ACCESS_K: usize = 8;
    /// A4: upper-line fan-out — index steps past the block of lines
    /// inside the pin's projection (the `relaxed_vias` L-corridor
    /// reach); the stub stays axis-aligned.
    const B_EXT: u32 = 2;
    let nl = tracks.len();
    let (lines_h, lines_v) = layer_line_maps(g, nl);
    let node_id = |l: u8, ix: u32, iy: u32| -> u32 {
        g.nodes.partition_point(|n| (n.layer.0, n.ix, n.iy) < (l, ix, iy)) as u32
    };
    let mut out: Vec<Vec<AccessVia>> = (0..g.pin_nodes.len()).map(|_| Vec::new()).collect();
    let mut scratch = Vec::new();
    for (pi, &(pin, _)) in g.pin_nodes.iter().enumerate() {
        if db.pin_cells[pin.0 as usize].is_none() {
            continue; // io-port rects live outside every hull
        }
        // Per-layer pin shapes merged into overlap-connected
        // components (A1: the plate-fit test is against a component's
        // union, not one sub-rect — a plate spanning joined same-pin
        // rects is the delegated 65-partial class).
        let mut by_layer: BTreeMap<u8, Vec<RectDbu>> = BTreeMap::new();
        for (lid, r) in pin_shapes(db, tech, pin) {
            by_layer.entry(lid.0).or_default().push(r);
        }
        let mut seen: BTreeSet<(u8, i32, i32, u32)> = BTreeSet::new();
        for (l8, rects) in by_layer {
            let k = l8 as usize;
            let lid = LayerId(l8);
            if k + 1 >= nl || !mask.masked(l8) {
                continue;
            }
            let Some(di) = vias.pairs[k].default else {
                continue;
            };
            // The emitted footprint (A2): candidates widen the *probe*
            // enumeration, but placement always drops `default` —
            // confined() prices exactly what emit paints.
            let vcand = &vias.candidates[vias.pairs[k].candidates[di as usize] as usize];
            let Some(dvr) = vcand.rects.iter().find_map(|(dl, vr)| {
                matches!(dl, DrcLayer::Routing(l2) if *l2 == lid).then_some(vr)
            }) else {
                continue;
            };
            let up = &tracks[k + 1];
            let pitch = tech.routing_layers[k].pitch_dbu.max(1);
            for comp in rect_components(&rects) {
                let cb = comp_bbox(&comp);
                // Contact range: the plate at (x,y) touches pin metal —
                // strict-superset of the per-candidate fit ranges.
                let (cx0, cx1) = (cb.xl - dvr.xh, cb.xh - dvr.xl);
                let (cy0, cy1) = (cb.yl - dvr.yh, cb.yh - dvr.yl);
                if cx0 > cx1 || cy0 > cy1 {
                    continue;
                }
                // Free-axis probes (A2 + A3): every candidate's
                // per-sub-rect edge fits, contact-range midpoints and
                // in-range upper tracks, plus a half-pitch raster —
                // DBU-fine where the range is smaller.
                let mut xs: BTreeSet<i32> = BTreeSet::new();
                let mut ys: BTreeSet<i32> = BTreeSet::new();
                for &ci in &vias.pairs[k].candidates {
                    let cand = &vias.candidates[ci as usize];
                    let Some(vr) = cand.rects.iter().find_map(|(dl, vr)| {
                        matches!(dl, DrcLayer::Routing(l2) if *l2 == lid).then_some(vr)
                    }) else {
                        continue;
                    };
                    let (x0, x1) = (cb.xl - vr.xh, cb.xh - vr.xl);
                    let (y0, y1) = (cb.yl - vr.yh, cb.yh - vr.yl);
                    if x0 > x1 || y0 > y1 {
                        continue;
                    }
                    for r in &comp {
                        xs.extend([r.xl - vr.xl, r.xh - vr.xh]);
                        ys.extend([r.yl - vr.yl, r.yh - vr.yh]);
                    }
                    xs.insert((x0 + x1) / 2);
                    ys.insert((y0 + y1) / 2);
                    xs.extend(up.tracks_x.iter().copied().filter(|&t| t >= x0 && t <= x1));
                    ys.extend(up.tracks_y.iter().copied().filter(|&t| t >= y0 && t <= y1));
                    for (lo, hi, set) in [(x0, x1, &mut xs), (y0, y1, &mut ys)] {
                        let span = hi - lo;
                        if span <= 0 {
                            continue;
                        }
                        let step = (pitch / 2).min(span).max(1);
                        let mut t = lo;
                        while t <= hi {
                            set.insert(t);
                            t += step;
                        }
                        set.insert(hi);
                    }
                }
                let (mx, my) = ((cx0 + cx1) / 2, (cy0 + cy1) / 2);
                let mut xprobes: Vec<i32> = xs.into_iter().collect();
                let mut yprobes: Vec<i32> = ys.into_iter().collect();
                xprobes.sort_by_key(|&t| ((t - mx).abs(), t));
                yprobes.sort_by_key(|&t| ((t - my).abs(), t));
                // Connectivity: the emitted (default) lower plate must
                // touch the component's pin metal — confinement alone
                // does not imply it once probes range past the strict
                // fit range.
                let touches_pin = |vx: i32, vy: i32| {
                    let pl = RectDbu::new(vx + dvr.xl, vy + dvr.yl, vx + dvr.xh, vy + dvr.yh);
                    comp.iter().any(|r| overlaps_closed(&pl, r))
                };
                // Confinement (mask) + first-iteration occupancy: a
                // candidate whose paint already collides is never an
                // attach point — that would be a false-claim waiting
                // for extraction. `Net`/`Pin` marks can't exist yet
                // (topo build predates commits) but the check stays
                // uniform with the extraction guard.
                let own = g.net.0;
                let confined = |scratch: &mut Vec<u32>, vx: i32, vy: i32, b: u32| {
                    if !(touches_pin(vx, vy)
                        && mask.point_allowed(cover, scratch, l8, vx, vy)
                        && mask.point_allowed(cover, scratch, l8 + 1, vx, vy)
                        && vcand.rects.iter().all(|(dl, vr)| {
                            let DrcLayer::Routing(l2) = dl else {
                                return true;
                            };
                            mask.rect_allowed(
                                cover,
                                l2.0,
                                &RectDbu::new(vx + vr.xl, vy + vr.yl, vx + vr.xh, vy + vr.yh),
                            )
                        }))
                    {
                        return false;
                    }
                    let mut hits = Vec::new();
                    for (dl, vr) in &vcand.rects {
                        let DrcLayer::Routing(l2) = dl else {
                            continue;
                        };
                        let paint = RectDbu::new(vx + vr.xl, vy + vr.yl, vx + vr.xh, vy + vr.yh);
                        let from = hits.len();
                        occ.scan_rect(l2.0, &paint, &mut hits);
                        exempt_obs(cover, &paint, &mut hits, from);
                    }
                    scan_stub(g, tracks, occ, cover, b, l8 + 1, vx, vy, &mut hits);
                    !hits.iter().any(|h| match h.iv.who {
                        Occupant::Obs(_) => true,
                        Occupant::ObsPin(m) | Occupant::Net(m) | Occupant::Pin(m) => m != own,
                    })
                };
                // Upper lines inside the contact range, extended ±B_EXT
                // index steps (A4): ordered by distance to the range
                // midpoint, in-range lines first by construction.
                let line_ixs = |trks: &[i32], lo: i32, hi: i32| -> Vec<u32> {
                    let n = trks.len() as u32;
                    let ja = trks.partition_point(|&t| t < lo) as u32;
                    let jb = trks.partition_point(|&t| t <= hi) as u32;
                    (ja.saturating_sub(B_EXT)..(jb + B_EXT).min(n)).collect()
                };
                match tech.routing_layers[k + 1].dir {
                    LayerDir::Horizontal => {
                        let mut jys = line_ixs(&up.tracks_y, cy0, cy1);
                        jys.sort_by_key(|&jy| {
                            (
                                (up.tracks_y[jy as usize] - my).abs(),
                                up.tracks_y[jy as usize],
                            )
                        });
                        'enough: for jy in jys {
                            let vy = up.tracks_y[jy as usize];
                            let Some(ixs) = lines_h[k + 1].get(&jy) else {
                                continue;
                            };
                            for &vx in &xprobes {
                                if vx < cx0 || vx > cx1 {
                                    continue;
                                }
                                for jx in
                                    nearest_k(ixs, |jx| up.tracks_x[jx as usize], vx, 4)
                                {
                                    let bx = up.tracks_x[jx as usize];
                                    if !mask
                                        .seg_allowed(cover, l8 + 1, vx, vy, bx, vy)
                                    {
                                        continue;
                                    }
                                    let b = node_id(l8 + 1, jx, jy);
                                    if confined(&mut scratch, vx, vy, b)
                                        && seen.insert((l8, vx, vy, b))
                                    {
                                        out[pi].push(AccessVia {
                                            x: vx,
                                            y: vy,
                                            lower: l8,
                                            b,
                                        });
                                        if out[pi].len() >= ACCESS_K {
                                            break 'enough;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    LayerDir::Vertical => {
                        let mut jxs = line_ixs(&up.tracks_x, cx0, cx1);
                        jxs.sort_by_key(|&jx| {
                            (
                                (up.tracks_x[jx as usize] - mx).abs(),
                                up.tracks_x[jx as usize],
                            )
                        });
                        'enough: for jx in jxs {
                            let vx = up.tracks_x[jx as usize];
                            let Some(iys) = lines_v[k + 1].get(&jx) else {
                                continue;
                            };
                            for &vy in &yprobes {
                                if vy < cy0 || vy > cy1 {
                                    continue;
                                }
                                for jy in
                                    nearest_k(iys, |jy| up.tracks_y[jy as usize], vy, 4)
                                {
                                    let by = up.tracks_y[jy as usize];
                                    if !mask
                                        .seg_allowed(cover, l8 + 1, vx, vy, vx, by)
                                    {
                                        continue;
                                    }
                                    let b = node_id(l8 + 1, jx, jy);
                                    if confined(&mut scratch, vx, vy, b)
                                        && seen.insert((l8, vx, vy, b))
                                    {
                                        out[pi].push(AccessVia {
                                            x: vx,
                                            y: vy,
                                            lower: l8,
                                            b,
                                        });
                                        if out[pi].len() >= ACCESS_K {
                                            break 'enough;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Overlap-connected components of `rects` — boundary contact counts
/// (the Magic merge rule). Component order is canonical (by bbox).
fn rect_components(rects: &[RectDbu]) -> Vec<Vec<RectDbu>> {
    let mut comps: Vec<Vec<RectDbu>> = Vec::new();
    for &r in rects {
        let mut merged = vec![r];
        let mut rest = Vec::with_capacity(comps.len() + 1);
        for c in comps {
            if c.iter().any(|s| overlaps_closed(s, &r)) {
                merged.extend(c);
            } else {
                rest.push(c);
            }
        }
        rest.push(merged);
        comps = rest;
    }
    comps.sort_by_key(|c| comp_bbox(c));
    comps
}

/// The component's bounding box.
fn comp_bbox(comp: &[RectDbu]) -> RectDbu {
    comp.iter().fold(comp[0], |b, r| {
        RectDbu::new(
            b.xl.min(r.xl),
            b.yl.min(r.yl),
            b.xh.max(r.xh),
            b.yh.max(r.yh),
        )
    })
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
pub(crate) enum NetOutcome {
    /// Route, the commit-time collision `EdgeProbe`s, and the closure
    /// pass's named residuals (`self-spacing.md`, `residuals.md`).
    /// The provisional-collision scan runs at commit time against
    /// live occupancy (`route_nets_with_progress`), not at
    /// extraction — under parallel search the extraction-time
    /// occupancy is the iteration-start snapshot, which cannot see
    /// same-iteration stale conflicts. The probes carry the scan
    /// parameters, so the commit needs no `NetGraph`/`NetAdj`
    /// reference (both are per-attempt transients).
    Routed(NetRoute, Vec<EdgeProbe>, ClosureResiduals),
    Failed {
        unrouted: UnroutedNet,
        /// Pins that got no legal paint — the `Unattachable`
        /// diagnostic (`pin-access.md` r3): projected stubs
        /// suppressed by hull confinement plus access arcs blocked
        /// by live occupancy. `route_nets` collects them into
        /// `RouteDb::stub_outside_cover` across all iterations, so a
        /// net that re-attaches on a later pass keeps its record —
        /// every final `Unattachable` pin appears there, but not
        /// every recorded pin belongs to an `unrouted` net.
        stub_blocked: Vec<PinId>,
    },
}

/// Per-net topology: the half of `route_one_net`'s fixed inputs that
/// *reads occupancy* — `project_attach`/`access_vias` probe the
/// post-`premark_obs` marks, so it is built once per net at the
/// attach pass and can never be rebuilt later (a rebuild would see
/// committed routes and change the output). The occupancy-free half
/// — member lookup, relaxed edge domain, CSR adjacency — lives in
/// `NetAdj` and is rebuilt per attempt.
pub(crate) struct NetTopo {
    /// Per member pin (`pin_nodes` order): strict `pin_nodes`, else the
    /// projection fallback.
    attach: Vec<Vec<u32>>,
    projected: Vec<bool>,
    attachable: Vec<bool>,
    /// Per member pin (`pin_nodes` order): pin-anchored access
    /// candidates — their `b` nodes also join `attach`.
    access: Vec<Vec<AccessVia>>,
    /// Own-pin rects for the hull mask — the confinement checks in
    /// `relaxed_vias`/`project_attach` and the stub guard in
    /// `route_one_net`.
    pub(crate) cover: PinCover,
}

impl NetTopo {
    pub(crate) fn new(
        db: &DesignDb,
        tech: &TechDb,
        tracks: &[LayerTracks],
        g: &NetGraph,
        vias: &ViaTable,
        occ: &Occupancy,
        mask: &HullMask,
    ) -> Self {
        // Own-pin rects for the hull mask — every member pin's shape
        // XY unioned per host cell (all member pins, not only those
        // that survived masking into `g.pin_nodes`).
        let cover = pin_rects(db, tech, g.net);
        // Attach sets: strict `pin_nodes`, else the projection
        // fallback.
        let m = g.pin_nodes.len();
        let mut attach: Vec<Vec<u32>> = Vec::with_capacity(m);
        let mut projected = vec![false; m];
        let mut attachable = vec![true; m];
        for (pi, &(pin, ref strict)) in g.pin_nodes.iter().enumerate() {
            if !strict.is_empty() {
                attach.push(strict.clone());
                continue;
            }
            let shapes = pin_shapes(db, tech, pin);
            let nodes = project_attach(g, &shapes, tracks, occ, mask, &cover, g.net.0);
            projected[pi] = !nodes.is_empty();
            attachable[pi] = !nodes.is_empty();
            attach.push(nodes);
        }
        // Pin-anchored access arcs: each candidate's landing node is
        // an extra attach point — the wave claims the pin through it
        // and extraction paints the via + stub.
        let access = access_vias(db, tech, tracks, g, vias, occ, mask, &cover);
        if std::env::var_os("DRT_DEBUG_NET").is_some_and(|v| {
            v.to_string_lossy() == db.strings.resolve(db.net_names[g.net.0 as usize])
        }) {
            for (pi, (&(pin, ref strict), arcs)) in
                g.pin_nodes.iter().zip(access.iter()).enumerate()
            {
                let pname = db.strings.resolve(db.pin_names[pin.0 as usize]);
                eprintln!(
                    "DBG attach pin#{pi} {pname} strict={} access={} total={}",
                    strict.len(),
                    arcs.len(),
                    attach[pi].len()
                );
                for av in arcs {
                    eprintln!(
                        "DBG   access l={} via=({},{}) b={}",
                        av.lower, av.x, av.y, av.b
                    );
                }
            }
        }
        for (pi, arcs) in access.iter().enumerate() {
            for av in arcs {
                if !attach[pi].contains(&av.b) {
                    attach[pi].push(av.b);
                    attachable[pi] = true;
                }
            }
        }
        Self {
            attach,
            projected,
            attachable,
            access,
            cover,
        }
    }
}

/// The occupancy-free half of a net's search context — member-pin
/// lookup, relaxed edge domain, CSR adjacency — a pure function of
/// the kept `NetTopo`, the net's `NetGraph`, and the shared router
/// inputs (`relaxed_vias` and the mask/cover legality checks never
/// read `occ`). Rebuilt per attempt so the rip-up loop holds at most
/// `threads` copies in flight instead of all nets' for the whole
/// run. The unified edge index space `[0,E) | [E,E+V) | [E+V,E+V+R)`
/// and the arc order (in-plane, then strict, then relaxed) are
/// identical to the old once-built fields — byte-identical search.
pub(crate) struct NetAdj {
    /// Attach node → member pin indices, CSR by node id — each node's
    /// pin list in `attach` scan order, the same order the old
    /// `BTreeMap` pushed them.
    member_start: Vec<u32>,
    member_pin: Vec<u32>,
    /// Per-net relaxed via edges (`relaxed_vias` output).
    relax: Vec<RelaxedVia>,
    adj_start: Vec<u32>,
    adj_to: Vec<u32>,
    adj_eidx: Vec<u32>,
}

impl NetAdj {
    /// CSR adjacency over the unified edge index space: each in-plane
    /// edge contributes both directions, then each legal via edge
    /// (`pairs[k].default.is_some()` — a `ViaGap` pair forbids the
    /// transition) contributes both directions, then each relaxed via
    /// edge (pre-gated by `relaxed_vias`). Strict arcs come before
    /// relaxed arcs so an equal-`VIA_COST` tie relaxes toward the
    /// strict placement.
    pub(crate) fn new(
        g: &NetGraph,
        topo: &NetTopo,
        tech: &TechDb,
        tracks: &[LayerTracks],
        vias: &ViaTable,
        mask: &HullMask,
    ) -> Self {
        let n = g.nodes.len();
        // Member-pin CSR — the old `BTreeMap<u32, Vec<u32>>` keyed by
        // attach node, flattened; each node's pin list keeps `attach`
        // scan order.
        let mut member_start = vec![0u32; n + 1];
        for ids in &topo.attach {
            for &v in ids {
                member_start[v as usize + 1] += 1;
            }
        }
        for i in 0..n {
            member_start[i + 1] += member_start[i];
        }
        let mut member_pin = vec![0u32; member_start[n] as usize];
        let mut fill = member_start.clone();
        for (pi, ids) in topo.attach.iter().enumerate() {
            for &v in ids {
                member_pin[fill[v as usize] as usize] = pi as u32;
                fill[v as usize] += 1;
            }
        }

        let relax = relaxed_vias(g, tech, tracks, vias, mask, &topo.cover);
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
            let Some(di) = vias.pairs.get(k).and_then(|p| p.default) else {
                continue;
            };
            // Via-plate footprint on masked layers: inside a hull the
            // plate must land inside an own-pin rect, like the relaxed
            // candidates — an on-track plate poking past the pin edge
            // is a subcell-boundary violation.
            let vcand = &vias.candidates[vias.pairs[k].candidates[di as usize] as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            if vcand.rects.iter().any(|(dl, r)| {
                let DrcLayer::Routing(lid) = dl else {
                    return false;
                };
                !mask.rect_allowed(
                    &topo.cover,
                    lid.0,
                    &RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh),
                )
            }) {
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
            member_start,
            member_pin,
            relax,
            adj_start: deg,
            adj_to,
            adj_eidx,
        }
    }
}

/// Exact heap bytes of one `NetAdj` — the `drt: mem` per-attempt
/// transient census (`drt/mem.rs`).
fn net_adj_bytes(adj: &NetAdj) -> usize {
    use crate::drt::mem::vec_bytes;
    vec_bytes(&adj.member_start)
        + vec_bytes(&adj.member_pin)
        + vec_bytes(&adj.relax)
        + vec_bytes(&adj.adj_start)
        + vec_bytes(&adj.adj_to)
        + vec_bytes(&adj.adj_eidx)
}

// ---------------------------------------------------------------------------
// `GRUS_DRT_MEM` capacity census (drt/mem.rs)
// ---------------------------------------------------------------------------

/// Per-field heap census of one `NetTopo` — the `drt: mem post_topos`
/// split rows (the kept, occupancy-snapshot half — the rebuilt
/// `NetAdj` reports as `attempt_hi` on the `iter` row). `cover` is a
/// `BTreeMap`: entry count and inner-vec bytes are exact, but node
/// overhead has no capacity API — it reports separately as `~est`
/// (`entries × mem::BTREE_NODE_EST`) and is never folded into
/// `exact`.
#[derive(Default)]
struct TopoMem {
    /// `attach` outer + inner lists + `projected`/`attachable` flags.
    attach: usize,
    access: usize,
    cover_entries: usize,
    cover_inner: usize,
}

impl TopoMem {
    fn add(&mut self, f: &TopoMem) {
        self.attach += f.attach;
        self.access += f.access;
        self.cover_entries += f.cover_entries;
        self.cover_inner += f.cover_inner;
    }

    /// Exact bytes — everything but the `~` BTreeMap node estimate.
    fn exact(&self) -> usize {
        self.attach + self.access + self.cover_inner
    }
}

fn net_topo_mem(t: &NetTopo) -> TopoMem {
    use crate::drt::mem::vec_bytes;
    let mut m = TopoMem {
        attach: vec_bytes(&t.attach) + vec_bytes(&t.projected) + vec_bytes(&t.attachable),
        access: vec_bytes(&t.access),
        cover_entries: t.cover.len(),
        ..Default::default()
    };
    for ids in &t.attach {
        m.attach += vec_bytes(ids);
    }
    for av in &t.access {
        m.access += vec_bytes(av);
    }
    for rects in t.cover.values() {
        m.cover_inner += vec_bytes(rects);
    }
    m
}

/// Exact heap bytes of one `NetTopo` (no BTreeMap node estimate).
fn net_topo_bytes(t: &NetTopo) -> usize {
    net_topo_mem(t).exact()
}

/// The rip-up loop's per-net resident pair, measured: builds the
/// pre-route `Occupancy` (`premark_obs`) and every guided net's
/// `NetTopo` exactly as `route_nets` does, then returns the summed
/// capacity-walk bytes. `NetTopo` stays private — this is the
/// census's public handle for tests.
pub fn topos_footprint_bytes(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    graphs: &RouteGraphs,
    vias: &ViaTable,
) -> usize {
    let hull = HullMask::new(db, tech);
    let mut occ = Occupancy::new(tech, tracks);
    premark_obs(db, tech, &mut occ);
    graphs
        .nets
        .iter()
        .map(|g| net_topo_bytes(&NetTopo::new(db, tech, tracks, g, vias, &occ, &hull)))
        .sum()
}

/// `Occupancy`'s heap census: dense `Vec<Line>` per axis per layer
/// (each `Line`'s `ivs` + `max_hi` inner Vecs walked), the line
/// coordinate lists, and the per-layer scalars.
fn occupancy_bytes(o: &Occupancy) -> usize {
    use crate::drt::mem::vec_bytes;
    let mut b = std::mem::size_of::<Occupancy>();
    for lines in o.lines_x.iter().chain(&o.lines_y) {
        b += vec_bytes(lines);
        for l in lines {
            b += vec_bytes(&l.ivs) + vec_bytes(&l.max_hi);
        }
    }
    for c in o.coords_x.iter().chain(&o.coords_y) {
        b += vec_bytes(c);
    }
    b + vec_bytes(&o.halfw) + vec_bytes(&o.spc)
}

/// The rip-up mark log: outer `Vec` slots plus each net's `Mark`s.
fn marks_bytes(m: &[Vec<Mark>]) -> usize {
    use crate::drt::mem::vec_bytes;
    std::mem::size_of_val(m) + m.iter().map(vec_bytes).sum::<usize>()
}

fn net_route_bytes(r: &NetRoute) -> usize {
    use crate::drt::mem::vec_bytes;
    std::mem::size_of::<NetRoute>()
        + vec_bytes(&r.wires)
        + vec_bytes(&r.vias)
        + vec_bytes(&r.connected)
        + vec_bytes(&r.projected)
        + vec_bytes(&r.fills)
}

/// Routes committed so far (`routes[i] = Some`) plus the `Option`
/// slots — the `drt: mem` "routes" row.
fn routes_bytes(r: &[Option<NetRoute>]) -> usize {
    std::mem::size_of_val(r) + r.iter().flatten().map(net_route_bytes).sum::<usize>()
}

fn residuals_bytes(r: &[ClosureResiduals]) -> usize {
    use crate::drt::mem::vec_bytes;
    r.iter()
        .map(|c| {
            std::mem::size_of::<ClosureResiduals>() + vec_bytes(&c.notches) + vec_bytes(&c.holes)
        })
        .sum()
}

/// One outcome's live bytes — the `drt: mem` "outcomes" hi-water row.
fn net_outcome_bytes(o: &NetOutcome) -> usize {
    use crate::drt::mem::vec_bytes;
    std::mem::size_of::<NetOutcome>()
        + match o {
            NetOutcome::Routed(route, used, res) => {
                net_route_bytes(route)
                    + vec_bytes(used)
                    + vec_bytes(&res.notches)
                    + vec_bytes(&res.holes)
            }
            NetOutcome::Failed {
                unrouted,
                stub_blocked,
            } => {
                vec_bytes(&unrouted.connected)
                    + vec_bytes(&unrouted.failed)
                    + vec_bytes(&unrouted.blockers)
                    + vec_bytes(stub_blocked)
            }
        }
}

/// Per-attempt routing state over a cached `NetTopo`/`NetAdj` pair:
/// the grown tree, the committed edge list, and the wave scratch.
struct NetRun<'t> {
    topo: &'t NetTopo,
    adj: &'t NetAdj,
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
    fn new(topo: &'t NetTopo, adj: &'t NetAdj, n_nodes: usize, n_pins: usize) -> Self {
        Self {
            topo,
            adj,
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
        let adj = self.adj;
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
            let range = adj.adj_start[vi] as usize..adj.adj_start[vi + 1] as usize;
            for &to in &adj.adj_to[range] {
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
            let mrange = adj.member_start[vi] as usize..adj.member_start[vi + 1] as usize;
            if let Some(&pi) = adj.member_pin[mrange]
                .iter()
                .find(|&&p| !self.connected[p as usize] && topo.attachable[p as usize])
            {
                found = Some((v, pi as usize));
                break;
            }
            let d = self.dist[vi];
            for pos in adj.adj_start[vi] as usize..adj.adj_start[vi + 1] as usize {
                let to = adj.adj_to[pos];
                if self.in_tree[to as usize] {
                    continue; // tree nodes sit at dist 0 and cannot improve
                }
                let eidx = adj.adj_eidx[pos];
                let Some(ec) = edge_cost(
                    g,
                    tracks,
                    tech,
                    vias,
                    occ,
                    &adj.relax,
                    &topo.cover,
                    eidx,
                    own,
                    hist,
                    hard,
                    mode,
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
            v = other_end(g, &adj.relax, e, v);
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
pub(crate) fn route_one_net(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    topo: &NetTopo,
    adj: &NetAdj,
    vias: &ViaTable,
    occ: &Occupancy,
    mask: &HullMask,
    hist: u32,
    hard: bool,
    mode: OccMode,
) -> NetOutcome {
    let m = g.pin_nodes.len();
    let debug = std::env::var_os("DRT_DEBUG_NET")
        .is_some_and(|v| v.to_string_lossy() == db.strings.resolve(db.net_names[g.net.0 as usize]));
    let fail = |connected: Vec<PinId>, failed: Vec<(PinId, FailKind)>, blockers: Vec<(u8, u8)>| {
        NetOutcome::Failed {
            unrouted: UnroutedNet {
                net: g.net,
                connected,
                failed,
                blockers,
            },
            stub_blocked: Vec::new(),
        }
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
            Vec::new(),
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
        return fail(
            Vec::new(),
            ordered_failed(&vec![false; m], &topo.attachable),
            Vec::new(),
        );
    }

    let mut run = NetRun::new(topo, adj, g.nodes.len(), m);

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
            Vec::new(),
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
            // Graph-only reachability from the tree — separates a
            // disconnected pocket from occupancy-blocked search. Also
            // feeds the blocker census below, so it runs on every
            // wave-death, not only under `DRT_DEBUG_NET`.
            let mut seen = vec![false; g.nodes.len()];
            let mut q: VecDeque<u32> = run.tree_nodes.iter().copied().collect();
            for &v in &q {
                seen[v as usize] = true;
            }
            while let Some(v) = q.pop_front() {
                let r = adj.adj_start[v as usize] as usize..adj.adj_start[v as usize + 1] as usize;
                for &to in &adj.adj_to[r] {
                    if !seen[to as usize] {
                        seen[to as usize] = true;
                        q.push_back(to);
                    }
                }
            }
            if debug {
                let e_plane = g.edges.len() as u32;
                let v_plane = e_plane + g.via_edges.len() as u32;
                for (pi, ids) in topo.attach.iter().enumerate() {
                    if !topo.attachable[pi] {
                        continue;
                    }
                    let reach = ids.iter().filter(|&&v| seen[v as usize]).count();
                    eprintln!(
                        "DBG pin#{pi} attach={} reachable={reach} connected={}",
                        ids.len(),
                        run.connected[pi]
                    );
                    for &v in ids {
                        let n = &g.nodes[v as usize];
                        let (x, y) = node_xy(g, tracks, v);
                        let r = adj.adj_start[v as usize] as usize
                            ..adj.adj_start[v as usize + 1] as usize;
                        let mut cls = [0usize; 3];
                        for &e in &adj.adj_eidx[r.clone()] {
                            let c = if e < e_plane {
                                0
                            } else if e < v_plane {
                                1
                            } else {
                                2
                            };
                            cls[c] += 1;
                        }
                        eprintln!(
                            "DBG   node l={} ({x},{y}) edges={} via={} relax={} seen={}",
                            n.layer.0, cls[0], cls[1], cls[2], seen[v as usize]
                        );
                        for pos in r {
                            let e = adj.adj_eidx[pos];
                            if e < v_plane {
                                continue;
                            }
                            let to = adj.adj_to[pos];
                            let tn = &g.nodes[to as usize];
                            let (tx, ty) = node_xy(g, tracks, to);
                            let rv = &adj.relax[(e - v_plane) as usize];
                            eprintln!(
                                "DBG     relax to l={} ({tx},{ty}) via ({},{}) seen={}",
                                tn.layer.0, rv.x, rv.y, seen[to as usize]
                            );
                            let tr = adj.adj_start[to as usize] as usize
                                ..adj.adj_start[to as usize + 1] as usize;
                            let mut tc = [0usize; 3];
                            for &e2 in &adj.adj_eidx[tr.clone()] {
                                tc[if e2 < e_plane {
                                    0
                                } else if e2 < v_plane {
                                    1
                                } else {
                                    2
                                }] += 1;
                            }
                            eprintln!(
                                "DBG       target edges={} via={} relax={}",
                                tc[0], tc[1], tc[2]
                            );
                            for pos2 in tr {
                                let e2 = adj.adj_eidx[pos2];
                                let t2 = adj.adj_to[pos2];
                                let n2 = &g.nodes[t2 as usize];
                                let (x2, y2) = node_xy(g, tracks, t2);
                                eprintln!(
                                    "DBG       arc cls={} to l={} ({x2},{y2}) seen={}",
                                    if e2 < e_plane {
                                        0
                                    } else if e2 < v_plane {
                                        1
                                    } else {
                                        2
                                    },
                                    n2.layer.0,
                                    seen[t2 as usize]
                                );
                            }
                        }
                    }
                }
            }
            // Blocker attribution: every pocket-reachable attach
            // node's arc toward another reachable node is a wave
            // entry that died — re-price it and name the first
            // blocking hit (Obs / foreign ObsPin / foreign Net in
            // hard mode). Runs for every wave-death, not just
            // `DRT_DEBUG_NET`, so `unrouted_blockers` can census the
            // failure modes.
            let mut blockers = Vec::new();
            for (pi, ids) in topo.attach.iter().enumerate() {
                if run.connected[pi] || !topo.attachable[pi] {
                    continue;
                }
                'nodes: for &v in ids {
                    if !seen[v as usize] {
                        continue;
                    }
                    let r =
                        adj.adj_start[v as usize] as usize..adj.adj_start[v as usize + 1] as usize;
                    for pos in r {
                        let to = adj.adj_to[pos];
                        if !seen[to as usize] {
                            continue;
                        }
                        let eidx = adj.adj_eidx[pos];
                        let mut hits = Vec::new();
                        if edge_cost(
                            g,
                            tracks,
                            tech,
                            vias,
                            occ,
                            &adj.relax,
                            &topo.cover,
                            eidx,
                            g.net.0,
                            hist,
                            hard,
                            mode,
                            &mut hits,
                        )
                        .is_some()
                        {
                            continue;
                        }
                        let h = hits
                            .iter()
                            .find(|h| match h.iv.who {
                                Occupant::Obs(_) => true,
                                Occupant::ObsPin(m) => m != g.net.0,
                                _ => false,
                            })
                            .or_else(|| {
                                hits.iter().find(|h| {
                                    hard && matches!(
                                        h.iv.who,
                                        Occupant::Net(m) | Occupant::Pin(m) if m != g.net.0
                                    )
                                })
                            });
                        let Some(h) = h else {
                            continue;
                        };
                        if debug {
                            let who = match h.iv.who {
                                Occupant::Obs(c) => format!("Obs(cell#{c})"),
                                Occupant::ObsPin(m) => format!("ObsPin(net#{m})"),
                                Occupant::Net(m) => format!("Net(net#{m})"),
                                Occupant::Pin(m) => format!("Pin(net#{m})"),
                            };
                            eprintln!(
                                "DBG block pin#{pi} node={v} eidx={eidx} l={} \
                                 line={} iv=[{},{}] who={who}",
                                h.layer, h.line, h.lo, h.hi
                            );
                        }
                        blockers.push((
                            h.layer,
                            match h.iv.who {
                                Occupant::Obs(_) => 0,
                                Occupant::ObsPin(_) => 1,
                                Occupant::Net(_) => 2,
                                Occupant::Pin(_) => 3,
                            },
                        ));
                        break 'nodes;
                    }
                }
            }
            return fail(
                ordered_pins(&run.connected),
                ordered_failed(&run.connected, &topo.attachable),
                blockers,
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
            Vec::new(),
        );
    }

    // Extraction: merge unit edge spans per (layer, line) into
    // canonical wire intervals; dedupe via placements. Every interval
    // and via placement carries its `PaintSrc` so the self-spacing
    // closure can reject the access element that painted a notch
    // prong (`self-spacing.md`).
    let mut hlines: PaintLines = BTreeMap::new();
    let mut vlines: PaintLines = BTreeMap::new();
    let mut vplaced: BTreeMap<(u8, i32, i32), PaintSrc> = BTreeMap::new();
    let e_plane = g.edges.len() as u32;
    let v_plane = e_plane + g.via_edges.len() as u32;
    for &eidx in &run.used_edges {
        if eidx < e_plane {
            let e = &g.edges[eidx as usize];
            let (ax, ay) = node_xy(g, tracks, e.a);
            let (bx, by) = node_xy(g, tracks, e.b);
            let l = g.nodes[e.a as usize].layer.0;
            match e.axis {
                Axis::X => hlines.entry((l, ay)).or_default().push((
                    ax.min(bx),
                    ax.max(bx),
                    PaintSrc::Tree,
                )),
                Axis::Y => vlines.entry((l, ax)).or_default().push((
                    ay.min(by),
                    ay.max(by),
                    PaintSrc::Tree,
                )),
            }
        } else if eidx < v_plane {
            let ve = &g.via_edges[(eidx - e_plane) as usize];
            let n = &g.nodes[ve.lower as usize];
            let (x, y) = node_xy(g, tracks, ve.lower);
            vplaced.insert((n.layer.0, x, y), PaintSrc::Tree);
        } else {
            // Relaxed transition: the via sits on both layers'
            // preferred wires; its implied stubs — `a.xy → (x,y)` on
            // the lower layer, `(x,y) → b.xy` on the upper — are real
            // metal the committed geometry must carry (each shares
            // one coordinate, so each is a single-axis interval).
            let rv = &adj.relax[(eidx - v_plane) as usize];
            vplaced.insert((rv.lower, rv.x, rv.y), PaintSrc::Tree);
            let (ax, ay) = node_xy(g, tracks, rv.a);
            let (bx, by) = node_xy(g, tracks, rv.b);
            if ax == rv.x {
                if ay != rv.y {
                    vlines.entry((rv.lower, ax)).or_default().push((
                        ay.min(rv.y),
                        ay.max(rv.y),
                        PaintSrc::Tree,
                    ));
                }
            } else if ay == rv.y {
                hlines.entry((rv.lower, ay)).or_default().push((
                    ax.min(rv.x),
                    ax.max(rv.x),
                    PaintSrc::Tree,
                ));
            }
            if bx == rv.x {
                if by != rv.y {
                    vlines.entry((rv.lower + 1, bx)).or_default().push((
                        by.min(rv.y),
                        by.max(rv.y),
                        PaintSrc::Tree,
                    ));
                }
            } else if by == rv.y {
                hlines.entry((rv.lower + 1, by)).or_default().push((
                    bx.min(rv.x),
                    bx.max(rv.x),
                    PaintSrc::Tree,
                ));
            }
        }
    }
    // Painted-connection check per connected pin: a strict attach
    // node in the tree means committed metal inside the pin rect
    // already; a projected pin needs its access stub painted; a pin
    // reached only through an access landing needs the pin-anchored
    // via + stub. Anything left is unattachable-in-fact.
    //
    // The attach pass and the self-spacing closure iterate: a notch
    // whose only droppable prong is pin-attach paint rejects that
    // element (it lands in the pin's tried-set), the pin re-attaches
    // on its next candidate, and the closure reruns.
    let mut stub_blocked: Vec<u32> = Vec::new();
    let mut tried_proj: Vec<BTreeSet<(usize, u32)>> =
        (0..g.pin_nodes.len()).map(|_| BTreeSet::new()).collect();
    let mut tried_arc: Vec<BTreeSet<usize>> =
        (0..g.pin_nodes.len()).map(|_| BTreeSet::new()).collect();
    let mut elem: Vec<Option<AttachElem>> = vec![None; g.pin_nodes.len()];
    let mut fills: Vec<(LayerId, RectDbu)>;
    let residuals: ClosureResiduals;
    'outer: loop {
        for pi in 0..g.pin_nodes.len() {
            if !run.connected[pi] || elem[pi].is_some() {
                continue;
            }
            match try_pin_paint(
                db,
                tech,
                tracks,
                g,
                topo,
                vias,
                occ,
                mask,
                &run,
                debug,
                pi,
                &mut hlines,
                &mut vlines,
                &mut vplaced,
                &mut tried_proj[pi],
                &mut tried_arc[pi],
            ) {
                Some(e) => elem[pi] = Some(e),
                None => stub_blocked.push(pi as u32),
            }
        }
        match close_self_spacing(
            db, tech, g, occ, mask, &topo.cover, vias, &hlines, &vlines, &vplaced,
        ) {
            ClosureResult::Done {
                fills: f,
                residuals: res,
            } => {
                fills = f;
                residuals = res;
                break 'outer;
            }
            ClosureResult::Reject { pin, src } => {
                match elem[pin].take() {
                    Some(AttachElem::Proj(si, v)) => {
                        tried_proj[pin].insert((si, v));
                    }
                    Some(AttachElem::Access(ai)) => {
                        tried_arc[pin].insert(ai);
                    }
                    _ => {}
                }
                remove_paint(src, &mut hlines, &mut vlines, &mut vplaced);
            }
        }
    }
    // A connected pin with no legal paint — every access stub across
    // an uncovered hull, every access via blocked — is
    // unattachable-in-fact: the net lands in the unrouted report with
    // the pin named, and `route_nets` records it in
    // `stub_outside_cover`.
    if !stub_blocked.is_empty() {
        let mut attachable = topo.attachable.clone();
        for &pi in &stub_blocked {
            run.connected[pi as usize] = false;
            attachable[pi as usize] = false;
        }
        return NetOutcome::Failed {
            unrouted: UnroutedNet {
                net: g.net,
                connected: ordered_pins(&run.connected),
                failed: ordered_failed(&run.connected, &attachable),
                blockers: Vec::new(),
            },
            stub_blocked: stub_blocked
                .iter()
                .map(|&pi| g.pin_nodes[pi as usize].0)
                .collect(),
        };
    }
    let mut wires: Vec<WireSeg> = Vec::new();
    let mut emit = |l: u8, t: i32, mut ivs: Vec<(i32, i32, PaintSrc)>, vertical: bool| {
        ivs.sort_unstable();
        let mut merged: Vec<(i32, i32)> = Vec::new();
        for (lo, hi, _src) in ivs {
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
    fills.sort_unstable();
    let vias_out: Vec<ViaPlace> = vplaced
        .into_keys()
        .map(|(l, x, y)| ViaPlace {
            lower: LayerId(l),
            x,
            y,
        })
        .collect();

    // Provisional collisions are recorded at commit time
    // (`route_nets_with_progress`), not here: under snapshot search
    // the scan must see live occupancy — prior-iteration marks plus
    // every earlier same-iteration commit. The scan's per-edge
    // parameters are resolved now, while this attempt's rebuilt
    // `g`/`adj` still live — the commit pass replays `EdgeProbe`s,
    // which hold no graph references.
    let mut probes = Vec::new();
    for &eidx in &run.used_edges {
        edge_probe(g, tracks, &adj.relax, occ, eidx, &mut probes);
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
            fills,
        },
        probes,
        residuals,
    )
}

// ---------------------------------------------------------------------------
// Self-spacing closure (self-spacing.md Rule B)
// ---------------------------------------------------------------------------

/// Which committed element painted a piece — the closure pass's
/// rejection provenance. `Tree`/`PinShape` are immovable (rip-up
/// territory, never a notch rejection); `ProjStub`/`Access` are
/// pin-attach paint that can be dropped so the pin retries its next
/// candidate. `Ord` prefers `ProjStub` over `Access` when a prong
/// holds both — the stub is the smaller blast radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PaintSrc {
    /// Tree paint: edges, strict/relaxed vias, their stubs, fills.
    Tree,
    /// Member pin metal (immovable) — the `pin_nodes` index rides
    /// along so the void pass can tell a cell pin (`MacroPin`) from
    /// an io port (`IoPort`) for the checker's drawn gate.
    PinShape(u32),
    /// Projected access stub of member pin `u32` (`pin_nodes` index).
    ProjStub(u32),
    /// Access-arc via + upper stub of member pin `u32`.
    Access(u32),
}

impl PaintSrc {
    /// The pin-attach paints — the only droppable sources.
    fn droppable(&self) -> bool {
        matches!(self, PaintSrc::ProjStub(_) | PaintSrc::Access(_))
    }
    /// `pin_nodes` index of the pin this paint attaches.
    fn pin(&self) -> usize {
        match self {
            PaintSrc::ProjStub(pi) | PaintSrc::Access(pi) => *pi as usize,
            _ => unreachable!(),
        }
    }
}

/// The extraction ledger: `(layer, line index) → sorted
/// `(lo, hi, src)` intervals on that line — every committed wire
/// segment, stub, fill and via plate carries its `PaintSrc` so the
/// closure can name (and reject) the element that painted a prong.
type PaintLines = BTreeMap<(u8, i32), Vec<(i32, i32, PaintSrc)>>;

/// The attach element a pin's legal paint came from — recorded so a
/// rejected element lands in that pin's tried-set.
#[derive(Clone, Copy, Debug)]
enum AttachElem {
    /// A strict attach node in the tree — no extra paint committed.
    Strict,
    /// Projected attach: `(pin_shapes index, attach node)`.
    Proj(usize, u32),
    /// Access arc: `topo.access[pin]` index.
    Access(usize),
}

/// One closure outcome: `Done` carries the pass's admitted fills and
/// named residuals; `Reject` asks the caller to drop `pin`'s
/// committed attach element (`src`-tagged paint) and re-attach it on
/// the next candidate.
#[derive(Debug)]
enum ClosureResult {
    Done {
        fills: Vec<(LayerId, RectDbu)>,
        residuals: ClosureResiduals,
    },
    Reject {
        pin: usize,
        src: PaintSrc,
    },
}

/// Paints pin `pi`'s first legal attach element — the extraction-time
/// version of `hit_verdict`'s `legal`: strict in-tree node → no paint
/// → `Strict`; projected pin → first untried `(shape, node)` whose
/// stub stays inside cover and clears live occupancy → `Proj`; else
/// first untried access arc whose via + upper stub clears live
/// occupancy → `Access`.
#[allow(clippy::too_many_arguments)]
fn try_pin_paint(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    topo: &NetTopo,
    vias: &ViaTable,
    occ: &Occupancy,
    mask: &HullMask,
    run: &NetRun,
    debug: bool,
    pi: usize,
    hlines: &mut PaintLines,
    vlines: &mut PaintLines,
    vplaced: &mut BTreeMap<(u8, i32, i32), PaintSrc>,
    tried_proj: &mut BTreeSet<(usize, u32)>,
    tried_arc: &mut BTreeSet<usize>,
) -> Option<AttachElem> {
    let (pin, ref strict) = g.pin_nodes[pi];
    if strict.iter().any(|&v| run.in_tree[v as usize]) {
        return Some(AttachElem::Strict);
    }
    // Projected-pin access stubs: `project_attach` attaches at an
    // in-region node within one track of the pin rect, but the pin's
    // metal may not reach the shape committed at that node — paint
    // the single-axis stub closing the gap (the node's x or y lies
    // inside the rect's covered range by `project_attach`
    // construction, so the stub is always axis-aligned).
    if topo.projected[pi] {
        for (si, &(lid, r)) in pin_shapes(db, tech, pin).iter().enumerate() {
            for &v in &topo.attach[pi] {
                if g.nodes[v as usize].layer != lid
                    || !run.in_tree[v as usize]
                    || tried_proj.contains(&(si, v))
                {
                    continue;
                }
                let (x, y) = node_xy(g, tracks, v);
                let (cx, cy) = (x.clamp(r.xl, r.xh), y.clamp(r.yl, r.yh));
                // The access stub is committed metal: on a masked layer
                // it must stay inside this net's own pin rect union —
                // a stub through uncovered hull is the exact class the
                // mask exists to kill, so the candidate is skipped
                // rather than emitted. Live occupancy gates it too —
                // committed `Net` marks did not exist at topo time and
                // emit never gets a soft pass; a blocked candidate lands
                // in the tried-set so the pin advances or exhausts to
                // `stub_blocked` (named), never a hang.
                let mut hits = Vec::new();
                scan_stub(g, tracks, occ, &topo.cover, v, lid.0, cx, cy, &mut hits);
                if hits.iter().any(|h| fill_blocked(h, g.net.0)) {
                    tried_proj.insert((si, v));
                    continue;
                }
                if x == cx {
                    if y != cy {
                        if !mask.seg_allowed(&topo.cover, lid.0, x, y, cx, cy) {
                            continue;
                        }
                        vlines.entry((lid.0, x)).or_default().push((
                            cy.min(y),
                            cy.max(y),
                            PaintSrc::ProjStub(pi as u32),
                        ));
                    }
                    return Some(AttachElem::Proj(si, v));
                }
                if y == cy {
                    if x != cx {
                        if !mask.seg_allowed(&topo.cover, lid.0, x, y, cx, cy) {
                            continue;
                        }
                        hlines.entry((lid.0, y)).or_default().push((
                            cx.min(x),
                            cx.max(x),
                            PaintSrc::ProjStub(pi as u32),
                        ));
                    }
                    return Some(AttachElem::Proj(si, v));
                }
                // This node is not adjacent to this shape — another of
                // the pin's shapes produced it.
            }
        }
    }
    // Pin-anchored access arcs: paint the first untried candidate
    // whose landing node is in the tree and whose via + upper stub
    // clears live occupancy — OBS, foreign pins and committed routes
    // all count here (unlike `edge_cost`, emit never gets a soft
    // pass).
    for (ai, av) in topo.access[pi].iter().enumerate() {
        if !run.in_tree[av.b as usize] || tried_arc.contains(&ai) {
            continue;
        }
        let mut hits = Vec::new();
        let pair = &vias.pairs[av.lower as usize];
        let Some(di) = pair.default else {
            continue;
        };
        let vcand = &vias.candidates[pair.candidates[di as usize] as usize];
        for (dl, r) in &vcand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            let paint = RectDbu::new(av.x + r.xl, av.y + r.yl, av.x + r.xh, av.y + r.yh);
            let from = hits.len();
            occ.scan_rect(lid.0, &paint, &mut hits);
            exempt_obs(&topo.cover, &paint, &mut hits, from);
        }
        scan_stub(
            g,
            tracks,
            occ,
            &topo.cover,
            av.b,
            av.lower + 1,
            av.x,
            av.y,
            &mut hits,
        );
        if let Some(h) = hits.iter().find(|h| match h.iv.who {
            Occupant::Obs(_) => true,
            Occupant::ObsPin(m) | Occupant::Net(m) | Occupant::Pin(m) => m != g.net.0,
        }) {
            if debug {
                let who = match h.iv.who {
                    Occupant::Obs(c) => format!("Obs(cell#{c})"),
                    Occupant::ObsPin(m) => format!("ObsPin(net#{m})"),
                    Occupant::Net(m) => format!("Net(net#{m})"),
                    Occupant::Pin(m) => format!("Pin(net#{m})"),
                };
                eprintln!(
                    "DBG   access-block pin#{pi} via=({},{}) l={} b={} who={who} \
                     iv=[{},{}]@{}",
                    av.x, av.y, av.lower, av.b, h.lo, h.hi, h.line
                );
            }
            continue;
        }
        vplaced.insert((av.lower, av.x, av.y), PaintSrc::Access(pi as u32));
        let (bx, by) = node_xy(g, tracks, av.b);
        if bx == av.x {
            if by != av.y {
                vlines.entry((av.lower + 1, bx)).or_default().push((
                    by.min(av.y),
                    by.max(av.y),
                    PaintSrc::Access(pi as u32),
                ));
            }
        } else {
            hlines.entry((av.lower + 1, by)).or_default().push((
                bx.min(av.x),
                bx.max(av.x),
                PaintSrc::Access(pi as u32),
            ));
        }
        return Some(AttachElem::Access(ai));
    }
    None
}

/// Drops every interval / via placement tagged `src` — the rejected
/// attach element's paint, so the rebuilt piece set no longer holds
/// it.
fn remove_paint(
    src: PaintSrc,
    hlines: &mut PaintLines,
    vlines: &mut PaintLines,
    vplaced: &mut BTreeMap<(u8, i32, i32), PaintSrc>,
) {
    for ivs in hlines.values_mut() {
        ivs.retain(|&(_, _, s)| s != src);
    }
    for ivs in vlines.values_mut() {
        ivs.retain(|&(_, _, s)| s != src);
    }
    vplaced.retain(|_, s| *s != src);
}

/// Same-net self-spacing closure (self-spacing.md Rule B). Builds the
/// committed piece set per routing layer — merged wire rects, via
/// plates, member pin metal — and scans each layer's pieces pairwise
/// for facing-edge disjointness under `s_l`. Each notch is closed
/// with a fill rect admitted under the same mask/occupancy scans as
/// every committed shape, iterated to fixpoint; an inadmissible notch
/// rejects a droppable attach element (`Reject`) or — when both
/// prongs are tree/pin paint — lands in `residuals`, named and
/// visible.
#[allow(clippy::too_many_arguments)]
fn close_self_spacing(
    db: &DesignDb,
    tech: &TechDb,
    g: &NetGraph,
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    vias: &ViaTable,
    hlines: &PaintLines,
    vlines: &PaintLines,
    vplaced: &BTreeMap<(u8, i32, i32), PaintSrc>,
) -> ClosureResult {
    let mut pieces: BTreeMap<u8, Vec<(RectDbu, BTreeSet<PaintSrc>)>> = BTreeMap::new();
    for (vertical, lines) in [(false, hlines), (true, vlines)] {
        for (&(l, t), ivs) in lines {
            let w2 = occ.halfw[l as usize];
            let mut sorted = ivs.clone();
            sorted.sort_unstable();
            let mut cur: Option<(i32, i32, BTreeSet<PaintSrc>)> = None;
            for &(a, b, src) in &sorted {
                match &mut cur {
                    Some((_, chi, cs)) if a <= *chi => {
                        *chi = (*chi).max(b);
                        cs.insert(src);
                    }
                    _ => {
                        if let Some((clo, chi, cs)) = cur.take() {
                            pieces.entry(l).or_default().push((
                                if vertical {
                                    RectDbu::new(t - w2, clo, t + w2, chi)
                                } else {
                                    RectDbu::new(clo, t - w2, chi, t + w2)
                                },
                                cs,
                            ));
                        }
                        cur = Some((a, b, BTreeSet::from([src])));
                    }
                }
            }
            if let Some((clo, chi, cs)) = cur {
                pieces.entry(l).or_default().push((
                    if vertical {
                        RectDbu::new(t - w2, clo, t + w2, chi)
                    } else {
                        RectDbu::new(clo, t - w2, chi, t + w2)
                    },
                    cs,
                ));
            }
        }
    }
    for (&(l, x, y), &src) in vplaced {
        let pair = &vias.pairs[l as usize];
        let Some(di) = pair.default else {
            continue;
        };
        for (dl, r) in &vias.candidates[pair.candidates[di as usize] as usize].rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            pieces.entry(lid.0).or_default().push((
                RectDbu::new(x + r.xl, y + r.yl, x + r.xh, y + r.yh),
                BTreeSet::from([src]),
            ));
        }
    }
    for &(pin, _) in &g.pin_nodes {
        for (lid, r) in pin_shapes(db, tech, pin) {
            pieces
                .entry(lid.0)
                .or_default()
                .push((r, BTreeSet::from([PaintSrc::PinShape(pin.0)])));
        }
    }

    let mut fills: Vec<(LayerId, RectDbu)> = Vec::new();
    let mut residuals = ClosureResiduals::default();
    for (&l, ps) in pieces.iter_mut() {
        let s = occ.spc[l as usize];
        // A prong pair is adjudicated once: an admitted fill bridges
        // it (the prongs stay disjoint pieces, so without this mark
        // the same pair re-fires every fixpoint pass) and a failed one
        // is either a named residual or a Reject. Only fills added
        // since can raise a new notch, so re-scanning proceeds but
        // old pairs stay closed.
        let mut seen: BTreeSet<(RectDbu, RectDbu)> = BTreeSet::new();
        let mut seen_holes: BTreeSet<RectDbu> = BTreeSet::new();
        'fp: loop {
            'fix: loop {
                ps.sort_by_key(|(r, _)| (r.xl, r.yl, r.xh, r.yh));
                let mut added: Option<(RectDbu, BTreeSet<PaintSrc>)> = None;
                'pair: for i in 0..ps.len() {
                    for j in i + 1..ps.len() {
                        let (p, q) = (ps[i].0, ps[j].0);
                        if q.xl >= p.xh + s {
                            break;
                        }
                        let Some(axis) = notch_axis(&p, &q, s) else {
                            continue;
                        };
                        let key = (p.min(q), p.max(q));
                        if !seen.insert(key) {
                            continue;
                        }
                        match try_fill(db, tech, occ, mask, cover, g.net, l, &p, &q, axis)
                        {
                            Some(f) => {
                                fills.push((LayerId(l), f));
                                added = Some((f, BTreeSet::from([PaintSrc::Tree])));
                                break 'pair;
                            }
                            None => {
                                // Smallest droppable src (Ord: ProjStub
                                // before Access — the smaller blast radius).
                                let mut srcs: BTreeSet<PaintSrc> = ps[i]
                                    .1
                                    .iter()
                                    .chain(&ps[j].1)
                                    .copied()
                                    .filter(PaintSrc::droppable)
                                    .collect();
                                if let Some(src) = srcs.pop_first() {
                                    return ClosureResult::Reject {
                                        pin: src.pin(),
                                        src,
                                    };
                                }
                                residuals.notches.push(NotchResidual {
                                    net: g.net,
                                    layer: LayerId(l),
                                    a: p,
                                    b: q,
                                });
                            }
                        }
                    }
                }
                match added {
                    Some(p) => ps.push(p),
                    None => break 'fix,
                }
            }
            // Void-closure (`residuals.md` §3): a void-fill merges
            // into its enclosing walls — it cannot open a new notch —
            // but it can stitch components into a fresh enclosure, so
            // the fixpoint reruns while fills still commit.
            if !close_voids(
                db,
                tech,
                g,
                occ,
                mask,
                cover,
                l,
                ps,
                &mut fills,
                &mut seen_holes,
                &mut residuals.holes,
            ) {
                break 'fp;
            }
        }
    }
    // `residuals.md` §1: identical fill rects — separate prong pairs
    // facing one gap — collapse to one committed `RECT`; overlapping
    // but distinct fills stay.
    fills.sort_unstable();
    fills.dedup();
    ClosureResult::Done { fills, residuals }
}

/// The `MINENCLOSEDAREA` bound the deck applies on layer `l` — only
/// layers carrying a `*.7` hole rule (the checker's `HOLE_RULES`
/// layer set) get the void pass; the bound is the LEF value, else the
/// same deck-pinned fallback `check_hole_report` uses.
fn hole_bound(tech: &TechDb, l: u8) -> Option<i64> {
    let rl = &tech.routing_layers[l as usize];
    let ruled = crate::drc::area::HOLE_RULES
        .iter()
        .any(|r| r.layer == tech.strings.resolve(rl.name));
    if !ruled {
        return None;
    }
    Some(if rl.min_enclosed_area_dbu2 > 0 {
        rl.min_enclosed_area_dbu2
    } else {
        crate::drc::area::HOLE_FALLBACK_DBU2
    })
}

/// Void-closure pass (`residuals.md` §3): enclosed voids under the
/// layer's `MINENCLOSEDAREA` bound get their free-rect decomposition
/// committed as same-net fills iff every covering rect passes
/// `fill_admitted` (all-or-nothing per void); an unfillable
/// under-bound void lands named in `hole_residuals`. The component /
/// solid / obs model mirrors `check_hole_report` exactly: drawn
/// components only (`MacroPin`-only islands are not evaluated),
/// `solid` is every member rect overlapping the component window —
/// foreign committed marks included, `Obs(u32::MAX)` PDN/netless
/// metal counts as real paint — and a void touching an `Obs`
/// abstract is real content: discarded, never filled, never a
/// residual. Returns true when any fill committed, so the caller can
/// re-run the notch fixpoint.
#[allow(clippy::too_many_arguments)]
fn close_voids(
    db: &DesignDb,
    tech: &TechDb,
    g: &NetGraph,
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    l: u8,
    ps: &mut Vec<(RectDbu, BTreeSet<PaintSrc>)>,
    fills: &mut Vec<(LayerId, RectDbu)>,
    seen_holes: &mut BTreeSet<RectDbu>,
    hole_residuals: &mut Vec<HoleResidual>,
) -> bool {
    let Some(bound) = hole_bound(tech, l) else {
        return false;
    };
    let own = g.net.0;
    let sh = |r: &RectDbu, srcs: &BTreeSet<PaintSrc>| ShapeRect {
        rect: *r,
        src: match srcs.iter().next() {
            Some(&PaintSrc::PinShape(pi)) if srcs.len() == 1 => {
                if db.pin_cells[pi as usize].is_some() {
                    ShapeSrc::MacroPin
                } else {
                    ShapeSrc::IoPort
                }
            }
            _ => ShapeSrc::SignalWire,
        },
        net: None,
        group: 0,
    };
    let ls = LayerShapes::from_rects(ps.iter().map(|(r, s)| sh(r, s)).collect());
    let comps = union::components(&ls);
    let mut progress = false;
    let mut hits = Vec::new();
    for members in &comps.members {
        if !members
            .iter()
            .any(|&i| is_drawn(ls.rects[i as usize].src))
        {
            continue;
        }
        let mrects = comp_rects(&ls, members);
        let window = rect_bbox(&mrects);
        let mut solid: Vec<RectDbu> = ps
            .iter()
            .map(|(r, _)| *r)
            .filter(|r| {
                r.xl < window.xh && r.xh > window.xl && r.yl < window.yh && r.yh > window.yl
            })
            .collect();
        let mut obs: Vec<RectDbu> = Vec::new();
        hits.clear();
        occ.scan_rect(l, &window, &mut hits);
        for h in hits.drain(..) {
            match h.iv.who {
                Occupant::Obs(u32::MAX) => solid.push(hit_bbox(&h)),
                Occupant::Obs(_) => obs.push(hit_bbox(&h)),
                Occupant::ObsPin(m) | Occupant::Net(m) | Occupant::Pin(m) if m != own => {
                    solid.push(hit_bbox(&h))
                }
                _ => {}
            }
        }
        for hole in union::enclosed_voids(&solid, window) {
            if hole.area_dbu2 >= bound {
                continue;
            }
            let obs_hit = hole
                .rects
                .iter()
                .any(|f| obs.iter().any(|o| overlaps_closed(f, o)));
            if obs_hit || !seen_holes.insert(hole.bbox) {
                continue;
            }
            if hole
                .rects
                .iter()
                .all(|f| fill_admitted(occ, mask, cover, own, l, f))
            {
                for f in &hole.rects {
                    fills.push((LayerId(l), *f));
                    ps.push((*f, BTreeSet::from([PaintSrc::Tree])));
                }
                progress = true;
            } else {
                hole_residuals.push(HoleResidual {
                    net: g.net,
                    layer: LayerId(l),
                    region: hole.bbox,
                    area_dbu2: hole.area_dbu2,
                    bound_dbu2: bound,
                });
            }
        }
    }
    progress
}

/// The notch signature (`self-spacing.md`): `p`/`q` disjoint, facing
/// edges overlapping or line-touching on the other axis, gap in
/// `(0, s)`. Returns the gap axis.
fn notch_axis(p: &RectDbu, q: &RectDbu, s: i32) -> Option<Axis> {
    let ov_x = p.xh.min(q.xh) - p.xl.max(q.xl);
    let ov_y = p.yh.min(q.yh) - p.yl.max(q.yl);
    if ov_x > 0 && ov_y > 0 {
        return None;
    }
    if ov_x < 0 && ov_x > -s && ov_y >= 0 {
        return Some(Axis::X);
    }
    if ov_y < 0 && ov_y > -s && ov_x >= 0 {
        return Some(Axis::Y);
    }
    None
}

/// The open gap between the facing edges of a `notch_axis` pair —
/// the region `try_fill` paints, minus its ±1 grow into the prongs.
/// Degenerate (line-width) gaps are exact too: `rect_covered` treats
/// them as interval coverage.
fn notch_gap(p: &RectDbu, q: &RectDbu, axis: Axis) -> RectDbu {
    match axis {
        Axis::X => {
            let (lo, hi) = if p.xh <= q.xl { (p.xh, q.xl) } else { (q.xh, p.xl) };
            RectDbu::new(lo, p.yl.max(q.yl), hi, p.yh.min(q.yh))
        }
        Axis::Y => {
            let (lo, hi) = if p.yh <= q.yl { (p.yh, q.yl) } else { (q.yh, p.yl) };
            RectDbu::new(p.xl.max(q.xl), lo, p.xh.min(q.xh), hi)
        }
    }
}

/// Closed-overlap (boundary touch counts — Magic merges on contact).
fn overlaps_closed(a: &RectDbu, b: &RectDbu) -> bool {
    a.xl <= b.xh && a.xh >= b.xl && a.yl <= b.yh && a.yh >= b.yl
}

/// Does `h` block committed paint for `own` net — the emit-time
/// verdict (same predicate as `try_pin_paint`'s arc scan).
pub(crate) fn fill_blocked(h: &Hit, own: u32) -> bool {
    match h.iv.who {
        Occupant::Obs(_) => true,
        Occupant::ObsPin(m) | Occupant::Net(m) | Occupant::Pin(m) => m != own,
    }
}

/// Fill legality — the same scans every committed shape faces:
/// `rect_allowed` (union-exact cover confinement) plus live occupancy
/// with the own-pin OBS exemption.
fn fill_admitted(
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    own: u32,
    l: u8,
    f: &RectDbu,
) -> bool {
    if !mask.rect_allowed(cover, l, f) {
        return false;
    }
    let mut hits = Vec::new();
    occ.scan_rect(l, f, &mut hits);
    exempt_obs(cover, f, &mut hits, 0);
    !hits.iter().any(|h| fill_blocked(h, own))
}

/// Tries to close the `(p, q)` notch on layer `l` (`axis` = the gap
/// axis): the gap-spanning fill rect — grown ±1 into both prongs so
/// the merge is unconditional, pruned to the facing-edge overlap —
/// admitted iff `fill_admitted`. On failure the fill is decomposed
/// against mask blockers (foreign hulls, uncovered member-hull parts)
/// and occupancy blockers; any residual slab still touching both
/// prongs is retried (the shrink-retry of self-spacing.md).
#[allow(clippy::too_many_arguments)]
fn try_fill(
    db: &DesignDb,
    tech: &TechDb,
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    own: NetId,
    l: u8,
    p: &RectDbu,
    q: &RectDbu,
    axis: Axis,
) -> Option<RectDbu> {
    let gap = match axis {
        Axis::X => {
            let (lo, hi) = if p.xh <= q.xl { (p.xh, q.xl) } else { (q.xh, p.xl) };
            let (ylo, yhi) = (p.yl.max(q.yl), p.yh.min(q.yh));
            let e = i32::from(ylo == yhi);
            RectDbu::new(lo - 1, ylo - e, hi + 1, yhi + e)
        }
        Axis::Y => {
            let (lo, hi) = if p.yh <= q.yl { (p.yh, q.yl) } else { (q.yh, p.yl) };
            let (xlo, xhi) = (p.xl.max(q.xl), p.xh.min(q.xh));
            let e = i32::from(xlo == xhi);
            RectDbu::new(xlo - e, lo - 1, xhi + e, hi + 1)
        }
    };
    let mut slabs = vec![gap];
    if mask.masked(l) {
        let mut cells = Vec::new();
        mask.hulls_overlapping(&gap, &mut cells);
        let mut blockers: Vec<RectDbu> = Vec::new();
        for &c in &cells {
            let hull = cell_hull_rect(db, tech, c);
            match cover.get(&c) {
                // Inside a foreign hull: never legal.
                None => blockers.push(hull),
                Some(rs) => {
                    // Host-open layers (`hull_host_open_layer`) leave the
                    // whole member-hull interior legal — the legality rule
                    // `rect_allowed` applies here too, so the uncovered-part
                    // carve is li1-strict only. Carving it on met1 produced
                    // fills that stopped short of the prongs' facing band —
                    // exactly the residual notch Magic then flags.
                    if mask.host_open(l) {
                        continue;
                    }
                    let fin = RectDbu::new(
                        gap.xl.max(hull.xl),
                        gap.yl.max(hull.yl),
                        gap.xh.min(hull.xh),
                        gap.yh.min(hull.yh),
                    );
                    if fin.xl < fin.xh && fin.yl < fin.yh {
                        blockers.extend(rect_minus(vec![fin], rs));
                    }
                }
            }
        }
        slabs = rect_minus(slabs, &blockers);
    }
    let mut hits = Vec::new();
    occ.scan_rect(l, &gap, &mut hits);
    exempt_obs(cover, &gap, &mut hits, 0);
    let blockers: Vec<RectDbu> = hits
        .iter()
        .filter(|h| fill_blocked(h, own.0))
        .map(hit_bbox)
        .collect();
    slabs = rect_minus(slabs, &blockers);
    for f in slabs {
        if !(overlaps_closed(&f, p) && overlaps_closed(&f, q)) {
            continue;
        }
        if fill_admitted(occ, mask, cover, own.0, l, &f) {
            return Some(f);
        }
    }
    None
}

/// Rasterizes one committed route's shapes (merged wire rects + via
/// footprints) as `Net(net)` marks, logging each insertion for rip-up.
fn commit_route(occ: &mut Occupancy, vias: &ViaTable, route: &NetRoute, marks: &mut Vec<Mark>) {
    for w in &route.wires {
        let hw = occ.halfw[w.layer.0 as usize];
        let r = RectDbu::new(w.x1 - hw, w.y1 - hw, w.x2 + hw, w.y2 + hw);
        occ.rasterize(w.layer.0, &r, Occupant::Net(route.net.0), Some(marks));
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
            occ.rasterize(lid.0, &shift, Occupant::Net(route.net.0), Some(marks));
        }
    }
    for (lid, r) in &route.fills {
        occ.rasterize(lid.0, r, Occupant::Net(route.net.0), Some(marks));
    }
}

// ---------------------------------------------------------------------------
// Post-route min-area patch
// ---------------------------------------------------------------------------

/// This net's member metal on routing layer `l`, tagged the way
/// `drc::geom` would — wires `± w/2` as `SignalWire`, pair-default
/// via metal as `Via`, member-pin shapes as `MacroPin`/`IoPort` — so
/// `union::components` + `is_drawn` reproduce `check_area_report`'s
/// component model restricted to one net. That tagging matters: a
/// `MacroPin`-only island is never evaluated by the checker, so it
/// must never become a residual here either.
fn net_layer_shapes(
    db: &DesignDb,
    tech: &TechDb,
    g: &NetGraph,
    vias: &ViaTable,
    route: &NetRoute,
    l: u8,
    w2: i32,
) -> Vec<ShapeRect> {
    let sh = |rect: RectDbu, src: ShapeSrc| ShapeRect {
        rect,
        src,
        net: None,
        group: 0,
    };
    let mut out = Vec::new();
    for w in &route.wires {
        if w.layer.0 != l {
            continue;
        }
        out.push(sh(
            RectDbu::new(
                w.x1.min(w.x2) - w2,
                w.y1.min(w.y2) - w2,
                w.x1.max(w.x2) + w2,
                w.y1.max(w.y2) + w2,
            ),
            ShapeSrc::SignalWire,
        ));
    }
    for vp in &route.vias {
        let pair = &vias.pairs[vp.lower.0 as usize];
        let Some(di) = pair.default else {
            continue;
        };
        let cand = &vias.candidates[pair.candidates[di as usize] as usize];
        for (dl, r) in &cand.rects {
            let DrcLayer::Routing(lid) = dl else {
                continue;
            };
            if lid.0 != l {
                continue;
            }
            out.push(sh(
                RectDbu::new(vp.x + r.xl, vp.y + r.yl, vp.x + r.xh, vp.y + r.yh),
                ShapeSrc::Via,
            ));
        }
    }
    for &(pin, _) in &g.pin_nodes {
        let src = if db.pin_cells[pin.0 as usize].is_some() {
            ShapeSrc::MacroPin
        } else {
            ShapeSrc::IoPort
        };
        for (lid, r) in pin_shapes(db, tech, pin) {
            if lid.0 == l {
                out.push(sh(r, src));
            }
        }
    }
    for (lid, r) in &route.fills {
        if lid.0 == l {
            out.push(sh(*r, ShapeSrc::SignalWire));
        }
    }
    out
}

/// One component's member rects (index list → geometry) — the
/// `union_area`/`bbox` input shared by the probe and the residual.
fn comp_rects(ls: &LayerShapes, members: &[u32]) -> Vec<RectDbu> {
    members.iter().map(|&i| ls.rects[i as usize].rect).collect()
}

fn rect_bbox(rects: &[RectDbu]) -> RectDbu {
    let mut b = rects[0];
    for r in &rects[1..] {
        b = RectDbu::new(
            b.xl.min(r.xl),
            b.yl.min(r.yl),
            b.xh.max(r.xh),
            b.yh.max(r.yh),
        );
    }
    b
}

/// A patch span `± w/2` — the same square-cap expansion wires get.
fn span_rect(axis: Axis, w2: i32, line_t: i32, lo: i32, hi: i32) -> RectDbu {
    match axis {
        Axis::X => RectDbu::new(lo - w2, line_t - w2, hi + w2, line_t + w2),
        Axis::Y => RectDbu::new(line_t - w2, lo - w2, line_t + w2, hi + w2),
    }
}

/// Try to lift one sub-bound component to `bound`: per candidate node
/// inside the component's member rects (g order), walk `+pref` then
/// `−pref` over adjacent covered nodes on its line — every step is one
/// in-plane `g.edges` span, so patch metal stays inside the guide
/// region and on-track by construction. A step commits only when
/// `scan_wire` reports own-net `Net`/`Pin` hits exclusively: the
/// patch is occupancy-strict — any `Obs` (the host-cell carve is not
/// inherited; patch metal is optional) or foreign `Net`/`Pin` hit
/// stops that direction. Once the accumulated span reaches `bound` it
/// must also clear the same-net guard (`residuals.md` §2): it may
/// merge into the net's other components (touch ⇒ merge ⇒ free) but
/// may not stop `< s` short of any own rect it stays disjoint from —
/// the closure's notch signature applied to post-closure paint; a
/// violating reach does not end the walk, a later step can still
/// merge through. The first bound-reaching legal span commits as a
/// single `WireSeg` rasterized as `Net(own)`; returns whether it
/// committed.
#[allow(clippy::too_many_arguments)]
fn patch_component(
    tracks: &[LayerTracks],
    g: &NetGraph,
    occ: &mut Occupancy,
    route: &mut NetRoute,
    l: u8,
    pref: Axis,
    lines: &BTreeMap<u32, BTreeSet<u32>>,
    mrects: &[RectDbu],
    net_rects: &[RectDbu],
    bound: i64,
    w2: i32,
) -> bool {
    let lt = &tracks[l as usize];
    let own = route.net.0;
    let s = occ.spc[l as usize];
    let coord = |pos: u32| -> i32 {
        match pref {
            Axis::X => lt.tracks_x[pos as usize],
            Axis::Y => lt.tracks_y[pos as usize],
        }
    };
    let line_at = |line: u32| -> i32 {
        match pref {
            Axis::X => lt.tracks_y[line as usize],
            Axis::Y => lt.tracks_x[line as usize],
        }
    };
    let mut hits = Vec::new();
    for (ni, n) in g.nodes.iter().enumerate() {
        if n.layer.0 != l {
            continue;
        }
        let (x, y) = node_xy(g, tracks, ni as u32);
        let inside = mrects
            .iter()
            .any(|r| x >= r.xl && x <= r.xh && y >= r.yl && y <= r.yh);
        if !inside {
            continue;
        }
        let (line, pos) = match pref {
            Axis::X => (n.iy, n.ix),
            Axis::Y => (n.ix, n.iy),
        };
        let Some(set) = lines.get(&line) else {
            continue;
        };
        let (mut lo, mut hi) = (pos, pos);
        let mut reached = false;
        for forward in [true, false] {
            loop {
                let (c, d, next) = if forward {
                    let Some(&next) = set.range(hi + 1..).next() else {
                        break;
                    };
                    // An index gap is a corridor the guide does not
                    // cover — not an edge span, so the walk stops.
                    if next != hi + 1 {
                        break;
                    }
                    (coord(hi), coord(next), next)
                } else {
                    let Some(&prev) = set.range(..lo).next_back() else {
                        break;
                    };
                    if prev + 1 != lo {
                        break;
                    }
                    (coord(prev), coord(lo), prev)
                };
                hits.clear();
                occ.scan_wire(pref, l, line, c, d, &mut hits);
                if !hits
                    .iter()
                    .all(|h| {
                        matches!(
                            h.iv.who,
                            Occupant::Net(m) | Occupant::Pin(m) | Occupant::ObsPin(m) if m == own
                        )
                    })
                {
                    break;
                }
                if forward {
                    hi = next;
                } else {
                    lo = next;
                }
                let span = span_rect(pref, w2, line_at(line), coord(lo), coord(hi));
                let mut grown = mrects.to_vec();
                grown.push(span);
                if union::union_area(&grown) >= bound
                    && net_rects.iter().all(|q| {
                        notch_axis(&span, q, s)
                            .is_none_or(|ax| rect_covered(net_rects, &notch_gap(&span, q, ax)))
                    })
                {
                    reached = true;
                    break;
                }
            }
            if reached {
                break;
            }
        }
        if !reached {
            continue;
        }
        let seg = match pref {
            Axis::X => WireSeg {
                layer: LayerId(l),
                x1: coord(lo),
                y1: line_at(line),
                x2: coord(hi),
                y2: line_at(line),
            },
            Axis::Y => WireSeg {
                layer: LayerId(l),
                x1: line_at(line),
                y1: coord(lo),
                x2: line_at(line),
                y2: coord(hi),
            },
        };
        route.wires.push(seg);
        let r = span_rect(pref, w2, line_at(line), coord(lo), coord(hi));
        occ.rasterize(l, &r, Occupant::Net(own), None);
        return true;
    }
    false
}

/// Patch one committed net's sub-bound components toward its layers'
/// `AREA` bounds; returns the number of components fixed. Mirrors
/// `check_area_report`: per layer with `AREA` declared, member rects
/// merge via the checker's own `union::components` and a component is
/// evaluated only when it holds a drawn member — conservative in the
/// checker's direction (`area_ours ≤ area_checker`: foreign metal
/// touching ours can only help, never hide a real violation). The
/// extension walk is preferred-axis, in-region, and occupancy-strict;
/// a component it cannot fix lands in `residuals` with its final
/// geometry — a `*.6` gate finding without one is a model
/// divergence, i.e. a bug, not a tolerance.
#[allow(clippy::too_many_arguments)]
fn patch_net_min_area(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    vias: &ViaTable,
    occ: &mut Occupancy,
    route: &mut NetRoute,
    residuals: &mut Vec<AreaResidual>,
) -> usize {
    let mut patches = 0;
    for l in 0..tech.routing_layers.len() {
        let bound = tech.routing_layers[l].area_dbu2;
        if bound <= 0 {
            continue;
        }
        let l = l as u8;
        let w2 = tech.routing_layers[l as usize].width_dbu / 2;
        let pref = pref_axis(tech.routing_layers[l as usize].dir);
        // Step domain along the preferred axis: line index → sorted
        // covered track indices. Adjacent-index entries are exactly
        // the in-plane `g.edges` spans (built once per call).
        let mut lines: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for n in &g.nodes {
            if n.layer.0 != l {
                continue;
            }
            let (line, pos) = match pref {
                Axis::X => (n.iy, n.ix),
                Axis::Y => (n.ix, n.iy),
            };
            lines.entry(line).or_default().insert(pos);
        }
        'layer: loop {
            let ls = LayerShapes::from_rects(net_layer_shapes(db, tech, g, vias, route, l, w2));
            if ls.rects.is_empty() {
                break;
            }
            let comps = union::components(&ls);
            // The §2 same-net guard's rect set: every member rect of
            // the net on this layer, all components — a span that
            // merges is free, one that stops `< s` short is a notch.
            let net_rects: Vec<RectDbu> = ls.rects.iter().map(|s| s.rect).collect();
            let mut committed = false;
            for members in &comps.members {
                if !members
                    .iter()
                    .any(|&i| is_drawn(ls.rects[i as usize].src))
                {
                    continue;
                }
                let mrects = comp_rects(&ls, members);
                if union::union_area(&mrects) >= bound {
                    continue;
                }
                if patch_component(
                    tracks, g, occ, route, l, pref, &lines, &mrects, &net_rects, bound, w2,
                ) {
                    patches += 1;
                    // Re-derive: the patch can merge two sub-bound
                    // components into one that already clears.
                    committed = true;
                    break;
                }
            }
            if committed {
                continue 'layer;
            }
            // A full sweep accepted nothing — the residuals describe
            // final geometry, so a component a later patch rescued is
            // never stale-recorded.
            for members in &comps.members {
                if !members
                    .iter()
                    .any(|&i| is_drawn(ls.rects[i as usize].src))
                {
                    continue;
                }
                let mrects = comp_rects(&ls, members);
                let area = union::union_area(&mrects);
                if area < bound {
                    residuals.push(AreaResidual {
                        net: route.net,
                        layer: LayerId(l),
                        region: rect_bbox(&mrects),
                        area_dbu2: area,
                        bound_dbu2: bound,
                    });
                }
            }
            break 'layer;
        }
    }
    // Restore canonical `WireSeg` order for emit and `route_digest`.
    route.wires.sort_unstable();
    patches
}

/// Sequential maze routing over streamed per-net graphs: per net,
/// pathfinder-style tree extension under the interval-occupancy
/// model, ordered per `join.nets` (guide-file order). `vias` prices
/// transitions (`pairs[k].default`) and supplies footprints; a
/// `ViaGap` pair's edges are forbidden. Soft iterations price
/// foreign hits (`OCC_PENALTY + hist·HIST_STEP` per net); the last
/// iteration is hard — foreign hits block. `Obs` always blocks. Use
/// `route_nets_with_progress` to observe per-net progress or recover
/// the `RouteGraphs` census.
pub fn route_nets(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    vias: &ViaTable,
    cfg: &MazeConfig,
) -> RouteDb {
    route_nets_with_progress(db, tech, tracks, join, opts, vias, cfg, None).1
}

/// `route_nets` plus an opt-in progress sink: `progress`, when
/// `Some`, receives a `RouteProgress` copy after every attempted
/// net of every rip-up iteration. The sink holds no router-state
/// references — it cannot perturb the route. Returns the
/// `RouteGraphs` census beside the `RouteDb` (its `nets` vec stays
/// empty — the streamed build drops each graph after its `NetTopo`
/// is captured).
#[allow(clippy::too_many_arguments)]
pub fn route_nets_with_progress(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    vias: &ViaTable,
    cfg: &MazeConfig,
    progress: Option<&mut dyn FnMut(RouteProgress)>,
) -> (RouteGraphs, RouteDb) {
    let timing = std::env::var_os("GRUS_DRT_TIMING").is_some();
    let mem_on = crate::drt::mem::enabled();
    let t0 = std::time::Instant::now();
    let n = join.nets.len();
    let hull = HullMask::new(db, tech);
    let mut occ = Occupancy::new(tech, tracks);
    premark_obs(db, tech, &mut occ);
    if timing {
        eprintln!("drt: premark_obs {:?}", t0.elapsed());
    }
    if mem_on {
        eprintln!(
            "drt: mem post_premark rss={} occ={}",
            crate::drt::mem::peak_rss_bytes(),
            occupancy_bytes(&occ)
        );
    }
    // The attach pass is streamed per net: each guided net's
    // `NetGraph` is built, its occupancy-snapshot `NetTopo`
    // extracted, then the graph is dropped. The rip-up loop rebuilds
    // the graph — and the occupancy-free `NetAdj` half of the old
    // once-built topology — per attempt: both are pure functions of
    // the kept `NetTopo` and the shared inputs, so the loop holds at
    // most `threads` copies in flight instead of every net's graph +
    // topology for the whole run (the dominant resident set on
    // large designs).
    // All drt parallel sections run inside a per-call pool sized by
    // `cfg.threads` — the pool size is a call input, so tests force
    // per-call counts in one process without touching shared state.
    let pool = drt_pool(cfg.threads);
    let nthreads = pool.current_num_threads();
    let t1 = std::time::Instant::now();
    // Per-net census partials minus the graph itself — the same
    // records `NetBuild` carries, folded in `join.nets` order below.
    struct NetCensusPart {
        zero_coverage: Vec<ZeroCoverage>,
        region_escape: Vec<RegionEscape>,
        escapes: Vec<EscapeRecord>,
        via_edges_per_pair: Vec<usize>,
        hull_masked: Vec<usize>,
        total_nodes: usize,
        total_edges: usize,
        total_via_edges: usize,
        graph_bytes: usize,
    }
    let built: Vec<(NetTopo, NetCensusPart)> = pool.install(|| {
        join.nets
            .par_iter()
            .map_init(Vec::new, |hull_scratch, &(net, ref boxes)| {
                let nb = build_net_graph(db, tech, tracks, &hull, net, boxes, opts, hull_scratch);
                let topo = NetTopo::new(db, tech, tracks, &nb.graph, vias, &occ, &hull);
                (
                    topo,
                    NetCensusPart {
                        zero_coverage: nb.zero_coverage,
                        region_escape: nb.region_escape,
                        escapes: nb.escapes,
                        via_edges_per_pair: nb.via_edges_per_pair,
                        hull_masked: nb.hull_masked,
                        total_nodes: nb.graph.nodes.len(),
                        total_edges: nb.graph.edges.len(),
                        total_via_edges: nb.graph.via_edges.len(),
                        graph_bytes: net_graph_bytes(&nb.graph),
                    },
                )
            })
            .collect()
    });
    // Serial fold in `join.nets` order — the same append order the
    // bulk builder produced, so the census is byte-identical
    // (`graphs.nets` stays empty by construction).
    let mut graphs = RouteGraphs {
        via_edges_per_pair: vec![0; tech.routing_layers.len().saturating_sub(1)],
        hull_masked: vec![0; tech.routing_layers.len()],
        ..Default::default()
    };
    let mut topos: Vec<NetTopo> = Vec::with_capacity(n);
    let mut graphs_b = 0usize;
    for (topo, part) in built {
        graphs.zero_coverage.extend(part.zero_coverage);
        graphs.region_escape.extend(part.region_escape);
        graphs.escapes.extend(part.escapes);
        for (d, s) in graphs
            .via_edges_per_pair
            .iter_mut()
            .zip(&part.via_edges_per_pair)
        {
            *d += s;
        }
        for (d, s) in graphs.hull_masked.iter_mut().zip(&part.hull_masked) {
            *d += s;
        }
        graphs.total_nodes += part.total_nodes;
        graphs.total_edges += part.total_edges;
        graphs.total_via_edges += part.total_via_edges;
        graphs_b += part.graph_bytes;
        topos.push(topo);
    }
    if timing {
        eprintln!("drt: topos {:?} threads={nthreads}", t1.elapsed());
    }
    if mem_on {
        // `graphs` reports the summed capacity walk of every graph
        // the stream allocated — the same quantity as the old bulk
        // materialization, transiently resident ≤ threads at a time.
        eprintln!(
            "drt: mem post_graphs rss={} graphs={graphs_b}",
            crate::drt::mem::peak_rss_bytes(),
        );
        let mut tm = TopoMem::default();
        for t in &topos {
            tm.add(&net_topo_mem(t));
        }
        let occ_b = occupancy_bytes(&occ);
        let rss = crate::drt::mem::peak_rss_bytes();
        let measured = tm.exact() + occ_b;
        eprintln!(
            "drt: mem post_topos rss={rss} graphs={graphs_b} topos={} \
             attach={} access={} cover_entries={} cover_inner={} cover_est={} \
             occ={occ_b} rest={}",
            tm.exact(),
            tm.attach,
            tm.access,
            tm.cover_entries,
            tm.cover_inner,
            tm.cover_entries * crate::drt::mem::BTREE_NODE_EST,
            rss.saturating_sub(measured as u64),
        );
    }
    let mk_graph = |i: usize, hull_scratch: &mut Vec<u32>| {
        let &(net, ref boxes) = &join.nets[i];
        build_net_graph(db, tech, tracks, &hull, net, boxes, opts, hull_scratch).graph
    };
    let route_db = route_prepared(
        db, tech, tracks, join, &topos, occ, &hull, &mk_graph, vias, cfg, progress, t0,
    );
    (graphs, route_db)
}

/// `route_nets_with_progress` minus the streamed attach pass: drives
/// the rip-up loop over caller-prepared `topos` against the
/// pre-premarked `occ`, rebuilding each attempt's graph through
/// `mk_graph` (tests feed synthetic fixtures; production replays
/// `build_net_graph` per attempt).
#[allow(clippy::too_many_arguments)]
fn route_prepared(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    topos: &[NetTopo],
    mut occ: Occupancy,
    hull: &HullMask,
    mk_graph: &(dyn Fn(usize, &mut Vec<u32>) -> NetGraph + Sync),
    vias: &ViaTable,
    cfg: &MazeConfig,
    mut progress: Option<&mut dyn FnMut(RouteProgress)>,
    t0: std::time::Instant,
) -> RouteDb {
    let timing = std::env::var_os("GRUS_DRT_TIMING").is_some();
    let mem_on = crate::drt::mem::enabled();
    let n = join.nets.len();
    let pool = drt_pool(cfg.threads);
    let nthreads = pool.current_num_threads();
    let net_idx: BTreeMap<u32, usize> = join
        .nets
        .iter()
        .enumerate()
        .map(|(i, (net, _))| (net.0, i))
        .collect();
    let mut routes: Vec<Option<NetRoute>> = vec![None; n];
    let mut res_of: Vec<ClosureResiduals> = (0..n).map(|_| ClosureResiduals::default()).collect();
    let mut marks: Vec<Vec<Mark>> = vec![Vec::new(); n];
    let mut hist = vec![0u32; n];
    let mut bad: Vec<usize> = (0..n).collect();
    let mut unrouted: Vec<UnroutedNet> = Vec::new();
    let mut iterations = 0u32;
    let mut iter_stats: Vec<IterStat> = Vec::new();

    let cap = cfg.max_iters.max(1);
    let mut it = 0u32;
    let mut stub_outside_cover: Vec<(NetId, PinId)> = Vec::new();
    while it < cap && !bad.is_empty() {
        it += 1;
        iterations = it;
        let attempted = bad.len() as u32;
        let hard = it == cap;
        let ti = std::time::Instant::now();
        let mut prov: BTreeSet<Collision> = BTreeSet::new();
        let mut prov_pins: BTreeSet<Collision> = BTreeSet::new();
        let mut failed: Vec<(usize, UnroutedNet)> = Vec::new();
        let mut routed = 0u32;
        let mut search_el = std::time::Duration::ZERO;
        // `drt: mem` "outcomes" row — the transient all-nets buffer
        // exists only in the snapshot arm; the hard arm commits inline
        // (outcomes stays 0). `attempt_hi` is measured in both arms.
        let mut outcomes_b = 0usize;
        // `drt: mem` per-attempt hi-water — the biggest rebuilt
        // (NetGraph, NetAdj) pair this iteration; the transient bound
        // is `threads × attempt_hi`, not `nets × graphs+topos`.
        let mut attempt_hi = 0usize;
        let commit_el = if hard {
            // The hard iteration keeps the serial interleave: foreign
            // hits are blocks *during* search, which requires live
            // occupancy — this iteration does not snapshot, and its
            // provisional scan would have no legal disposition.
            for (bi, &i) in bad.iter().enumerate() {
                let &(net, _) = &join.nets[i];
                let tn = std::time::Instant::now();
                // Per-attempt rebuild — the serial arm holds one
                // (graph, adjacency) pair at a time.
                let mut scratch = Vec::new();
                let g = mk_graph(i, &mut scratch);
                let adj = NetAdj::new(&g, &topos[i], tech, tracks, vias, hull);
                if mem_on {
                    attempt_hi = attempt_hi.max(net_graph_bytes(&g) + net_adj_bytes(&adj));
                }
                match route_one_net(
                    db, tech, tracks, &g, &topos[i], &adj, vias, &occ, hull, hist[i], true, cfg.occ,
                ) {
                    NetOutcome::Routed(route, _probes, res) => {
                        routed += 1;
                        commit_route(&mut occ, vias, &route, &mut marks[i]);
                        res_of[i] = res;
                        routes[i] = Some(route);
                    }
                    NetOutcome::Failed {
                        unrouted,
                        stub_blocked,
                    } => {
                        for &p in &stub_blocked {
                            stub_outside_cover.push((net, p));
                        }
                        res_of[i] = ClosureResiduals::default();
                        failed.push((i, unrouted));
                    }
                }
                if let Some(sink) = progress.as_deref_mut() {
                    sink(RouteProgress {
                        iteration: it,
                        done: bi as u32 + 1,
                        queued: attempted,
                        unrouted: failed.len() as u32,
                        elapsed: t0.elapsed(),
                    });
                }
                if timing {
                    let el = tn.elapsed();
                    if el.as_millis() > 500 || bi % 500 == 0 {
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
            ti.elapsed()
        } else {
            // Snapshot-search / ordered-commit: every net in `bad`
            // searches against the iteration-start occupancy (shared
            // as `&Occupancy` — the borrow checker forbids writers),
            // then outcomes commit one at a time in `bad` order.
            // `collect` on an indexed parallel iterator preserves
            // `bad` order, so the committed stream is a pure function
            // of the inputs for any thread count. Each attempt
            // rebuilds its `NetGraph`/`NetAdj` inside the map — at
            // most `threads` pairs are in flight — and the carried
            // `EdgeProbe`s let the commit scan run with no graph
            // reference.
            let outcomes: Vec<(NetOutcome, usize)> = pool.install(|| {
                bad.par_iter()
                    .map_init(Vec::new, |hull_scratch, &i| {
                        let tn = std::time::Instant::now();
                        let g = mk_graph(i, hull_scratch);
                        let adj = NetAdj::new(&g, &topos[i], tech, tracks, vias, hull);
                        let out = route_one_net(
                            db, tech, tracks, &g, &topos[i], &adj, vias, &occ, hull,
                            hist[i], false, cfg.occ,
                        );
                        if timing {
                            let el = tn.elapsed();
                            if el.as_millis() > 500 || i % 500 == 0 {
                                eprintln!(
                                    "drt:   net[{i}/{}] nodes={} edges={} pins={} {:?} (interleaved)",
                                    bad.len(),
                                    g.nodes.len(),
                                    g.edges.len(),
                                    g.pin_nodes.len(),
                                    el
                                );
                            }
                        }
                        let attempt_b = if mem_on {
                            net_graph_bytes(&g) + net_adj_bytes(&adj)
                        } else {
                            0
                        };
                        (out, attempt_b)
                    })
                    .collect()
            });
            if mem_on {
                outcomes_b = outcomes.iter().map(|(o, _)| net_outcome_bytes(o)).sum();
                attempt_hi = outcomes.iter().map(|(_, b)| *b).max().unwrap_or(0);
            }
            search_el = ti.elapsed();
            let tc = std::time::Instant::now();
            let mut hits = Vec::new();
            for (bi, ((outcome, _), &i)) in outcomes.into_iter().zip(&bad).enumerate() {
                match outcome {
                    NetOutcome::Routed(route, probes, res) => {
                        routed += 1;
                        // Stale-collision detection: parallel searches
                        // may claim the same resource — replay the
                        // resolved probes against live `occ` (prior
                        // marks plus earlier same-iteration commits)
                        // before rasterizing, recording canonical
                        // net×net plus directed wire→pin.
                        if cfg.occ == OccMode::Enabled {
                            probe_collisions(
                                &occ,
                                vias,
                                &topos[i].cover,
                                &probes,
                                join.nets[i].0,
                                &mut prov,
                                &mut prov_pins,
                                &mut hits,
                            );
                        }
                        commit_route(&mut occ, vias, &route, &mut marks[i]);
                        res_of[i] = res;
                        routes[i] = Some(route);
                    }
                    NetOutcome::Failed {
                        unrouted,
                        stub_blocked,
                    } => {
                        for &p in &stub_blocked {
                            stub_outside_cover.push((join.nets[i].0, p));
                        }
                        res_of[i] = ClosureResiduals::default();
                        failed.push((i, unrouted));
                    }
                }
                // The per-attempt progress point is the commit — the
                // only moment an attempt's disposition exists in
                // `bad` order under parallel search.
                if let Some(sink) = progress.as_deref_mut() {
                    sink(RouteProgress {
                        iteration: it,
                        done: bi as u32 + 1,
                        queued: attempted,
                        unrouted: failed.len() as u32,
                        elapsed: t0.elapsed(),
                    });
                }
            }
            tc.elapsed()
        };
        iter_stats.push(IterStat {
            attempted,
            routed,
            prov_collisions: (prov.len() + prov_pins.len()) as u32,
        });
        if mem_on {
            eprintln!(
                "drt: mem iter={it} rss={} occ={} marks={} routed={} outcomes={outcomes_b} attempt_hi={attempt_hi}",
                crate::drt::mem::peak_rss_bytes(),
                occupancy_bytes(&occ),
                marks_bytes(&marks),
                routes_bytes(&routes),
            );
        }
        if hard {
            unrouted = failed.into_iter().map(|(_, u)| u).collect();
            break;
        }
        // bad = unrouted ∪ both parties of every net×net provisional
        // collision ∪ the wire side only of every wire×pin one — a
        // pin's net is never re-queued on its own port being hit: the
        // port cannot move, so re-queuing it would only waste a route
        // attempt and escalate `hist[b]` unfairly.
        let mut next: BTreeSet<usize> = failed.iter().map(|(i, _)| *i).collect();
        for c in &prov {
            if let Some(&i) = net_idx.get(&c.a.0) {
                next.insert(i);
            }
            if let Some(&i) = net_idx.get(&c.b.0) {
                next.insert(i);
            }
        }
        for c in &prov_pins {
            if let Some(&i) = net_idx.get(&c.a.0) {
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
            let st = &occ.stats;
            eprintln!(
                "drt: iter {it} bad={} failed={} prov={} search={:?} commit={:?} threads={} \
                 occ wire={} rect={} rect_lines={} ivs={} hits={} marks+={} marks-={}",
                bad.len(),
                failed.len(),
                prov.len(),
                search_el,
                commit_el,
                nthreads,
                st.get(|s| &s.wire_calls),
                st.get(|s| &s.rect_calls),
                st.get(|s| &s.rect_lines_visited),
                st.get(|s| &s.ivs_visited),
                st.get(|s| &s.hits_pushed),
                st.get(|s| &s.marks_inserted),
                st.get(|s| &s.marks_removed),
            );
        }
    }

    // Post-route min-area patch: `occ` at loop exit holds exactly the
    // committed set (premarks plus `Net(own)` marks of every
    // `routes[i] = Some` net — re-queued nets were unmarked, hard-mode
    // failures never committed), so patches see the final foreign
    // picture, run once per net, and rasterize straight into `occ` so
    // later nets' patches respect earlier patches. Runs here, not in
    // `route_one_net`: the `join.nets[i]` ↔ `routes[i]` pairing is
    // still live and each net is patched once, not once per rip-up.
    let tp = std::time::Instant::now();
    let mut area_residuals = Vec::new();
    let mut area_patches = 0;
    // Patch order stays `join.nets` order (the old `graphs.nets`
    // enumerate); each patched net's graph is rebuilt here — serial
    // per net, bounded — since the loop needs only `g`'s nodes.
    let mut patch_scratch = Vec::new();
    for (i, route) in routes.iter_mut().enumerate() {
        let Some(route) = route else {
            continue;
        };
        let g = mk_graph(i, &mut patch_scratch);
        area_patches += patch_net_min_area(
            db,
            tech,
            tracks,
            &g,
            vias,
            &mut occ,
            route,
            &mut area_residuals,
        );
    }
    if timing {
        eprintln!(
            "drt: min-area patch patches={area_patches} residuals={} {:?}",
            area_residuals.len(),
            tp.elapsed()
        );
        let st = &occ.stats;
        let (wc, rc) = (st.get(|s| &s.wire_calls), st.get(|s| &s.rect_calls));
        let (rl, ivs, hits) = (
            st.get(|s| &s.rect_lines_visited),
            st.get(|s| &s.ivs_visited),
            st.get(|s| &s.hits_pushed),
        );
        eprintln!(
            "drt: occ-scan totals wire={wc} rect={rc} rect_lines={rl} ivs={ivs} \
             hits={hits} marks+={} marks-={} | ivs/wire_call={:.1} \
             lines/rect_call={:.1} hits/scan={:.2}",
            st.get(|s| &s.marks_inserted),
            st.get(|s| &s.marks_removed),
            ivs as f64 / wc.max(1) as f64,
            rl as f64 / rc.max(1) as f64,
            hits as f64 / (wc + rc).max(1) as f64,
        );
    }
    if mem_on {
        eprintln!(
            "drt: mem post_route rss={} occ={} marks={} routes={} residuals={}",
            crate::drt::mem::peak_rss_bytes(),
            occupancy_bytes(&occ),
            marks_bytes(&marks),
            routes_bytes(&routes),
            residuals_bytes(&res_of),
        );
    }

    // One entry per (net, pin) — a net re-queued across iterations
    // can re-report the same blocked pin.
    stub_outside_cover.sort_unstable();
    stub_outside_cover.dedup();

    let nets: Vec<NetRoute> = routes.into_iter().flatten().collect();
    RouteDb {
        collisions: if cfg.occ == OccMode::Enabled {
            overlap_census(&nets, tech, tracks, vias)
        } else {
            Vec::new()
        },
        pin_collisions: if cfg.occ == OccMode::Enabled {
            pin_overlap_census(db, tech, tracks, vias, &nets)
        } else {
            Vec::new()
        },
        nets,
        unrouted,
        iterations,
        iter_stats,
        area_patches,
        area_residuals,
        notch_residuals: res_of
            .iter()
            .flat_map(|r| r.notches.iter().copied())
            .collect(),
        hole_residuals: res_of
            .iter()
            .flat_map(|r| r.holes.iter().copied())
            .collect(),
        stub_outside_cover,
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
    let mut occ = Occupancy::new(tech, tracks);
    for r in routes {
        let mut sink = Vec::new();
        commit_route(&mut occ, vias, r, &mut sink);
    }
    let mut out = BTreeSet::new();
    for l in 0..occ.lines_x.len() {
        let s = occ.spc[l];
        for (coords, lines) in [
            (&occ.coords_x[l], &occ.lines_x[l]),
            (&occ.coords_y[l], &occ.lines_y[l]),
        ] {
            for (&t, line) in coords.iter().zip(lines) {
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

/// Committed-wire × foreign-pin overlaps — the named residual for
/// the `Occupant::Pin`/`ObsPin` classes `overlap_census` cannot see
/// (it rasterizes only `Net` marks). Rebuilds the router's pin marks
/// (`rasterize_pin_marks` — io ports `Pin`, instance pins `ObsPin`)
/// plus every route's `Net` marks into a fresh occupancy, then runs
/// the same per-line `R + s` sweep. Reports are *directed*
/// `Collision`s: `a` is the wire's net, `b` the pin's owning net —
/// the wire is the only movable party, so the direction is the
/// rip-up payload.
pub fn pin_overlap_census(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    vias: &ViaTable,
    routes: &[NetRoute],
) -> Vec<Collision> {
    let mut occ = Occupancy::new(tech, tracks);
    rasterize_pin_marks(db, tech, &mut occ, true);
    for r in routes {
        let mut sink = Vec::new();
        commit_route(&mut occ, vias, r, &mut sink);
    }
    let mut out = BTreeSet::new();
    for l in 0..occ.lines_x.len() {
        let s = occ.spc[l];
        for (coords, lines) in [
            (&occ.coords_x[l], &occ.lines_x[l]),
            (&occ.coords_y[l], &occ.lines_y[l]),
        ] {
            for (&t, line) in coords.iter().zip(lines) {
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
                        let pair = match (a.who, b.who) {
                            (Occupant::Net(w), Occupant::Pin(p) | Occupant::ObsPin(p))
                            | (Occupant::Pin(p) | Occupant::ObsPin(p), Occupant::Net(w)) => (w, p),
                            _ => continue,
                        };
                        if pair.0 == pair.1 {
                            continue;
                        }
                        out.insert(Collision {
                            a: NetId(pair.0),
                            b: NetId(pair.1),
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
        put(r.fills.len() as u32);
        for (lid, f) in &r.fills {
            put(lid.0 as u32);
            put(f.xl as u32);
            put(f.yl as u32);
            put(f.xh as u32);
            put(f.yh as u32);
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
    put(routed.pin_collisions.len() as u32);
    for c in &routed.pin_collisions {
        put(c.a.0);
        put(c.b.0);
        put(c.layer.0 as u32);
        put(c.line as u32);
        put(c.lo as u32);
        put(c.hi as u32);
    }
    put(routed.notch_residuals.len() as u32);
    for nr in &routed.notch_residuals {
        put(nr.net.0);
        put(nr.layer.0 as u32);
        put(nr.a.xl as u32);
        put(nr.a.yl as u32);
        put(nr.a.xh as u32);
        put(nr.a.yh as u32);
        put(nr.b.xl as u32);
        put(nr.b.yl as u32);
        put(nr.b.xh as u32);
        put(nr.b.yh as u32);
    }
    put(routed.hole_residuals.len() as u32);
    for hr in &routed.hole_residuals {
        put(hr.net.0);
        put(hr.layer.0 as u32);
        put(hr.region.xl as u32);
        put(hr.region.yl as u32);
        put(hr.region.xh as u32);
        put(hr.region.yh as u32);
        put(hr.area_dbu2 as u64 as u32);
        put((hr.area_dbu2 as u64 >> 32) as u32);
        put(hr.bound_dbu2 as u64 as u32);
        put((hr.bound_dbu2 as u64 >> 32) as u32);
    }
    put(routed.stub_outside_cover.len() as u32);
    for (n, p) in &routed.stub_outside_cover {
        put(n.0);
        put(p.0);
    }
    put(routed.iterations);
    put(routed.iter_stats.len() as u32);
    for s in &routed.iter_stats {
        put(s.attempted);
        put(s.routed);
        put(s.prov_collisions);
    }
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

    /// Parallel search shares `&Occupancy`, `&NetTopo`, `&ViaTable`,
    /// `&HullMask`, and `&NetGraph` across workers and returns
    /// `NetOutcome`s — the `Sync`/`Send` bounds are the scheme's
    /// stated foundation; a regression to non-atomic interior
    /// mutability fails here at compile time.
    #[test]
    fn drt_search_inputs_are_sync_send() {
        fn assert_sync_send<T: Sync + Send>() {}
        assert_sync_send::<Occupancy>();
        assert_sync_send::<NetTopo>();
        assert_sync_send::<ViaTable>();
        assert_sync_send::<HullMask>();
        assert_sync_send::<NetGraph>();
        assert_sync_send::<NetOutcome>();
    }

    fn toy_tech_area(area_dbu2: i64) -> TechDb {
        let mut tech = TechDb::new();
        let name = tech.strings.intern("li1");
        tech.routing_layers.push(RoutingLayer {
            name,
            dir: LayerDir::Horizontal,
            pitch_dbu: 340,
            width_dbu: 140,
            offset_dbu: 0,
            pitch_x_dbu: 340,
            pitch_y_dbu: 340,
            offset_x_dbu: 0,
            offset_y_dbu: 0,
            tracks_x: vec![],
            tracks_y: vec![],
            res_per_sq: 0.0,
            cap_per_sq_um: 0.0,
            edge_cap_per_um: 0.0,
            spacing_dbu: 140,
            spacing_table: vec![],
            area_dbu2,
            min_enclosed_area_dbu2: 0,
        });
        tech
    }

    fn toy_tech() -> TechDb {
        toy_tech_area(0)
    }

    /// Two io pins `a` (driver) / `b` on net 0 — the square_graph
    /// fixture's design side.
    fn toy_db() -> DesignDb {
        let mut db = DesignDb::new();
        db.pin_names.push(db.strings.intern("a"));
        db.pin_names.push(db.strings.intern("b"));
        db.pin_dirs.push(crate::interner::PinDir::Output);
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
        db
    }

    fn toy_tracks() -> Vec<LayerTracks> {
        vec![LayerTracks {
            tracks_x: vec![0, 1000, 2000, 3000],
            tracks_y: vec![0, 1000, 2000, 3000],
        }]
    }

    fn toy_occ(tech: &TechDb) -> Occupancy {
        Occupancy::new(tech, &toy_tracks())
    }

    fn commit_wire(occ: &mut Occupancy, lo: i32, hi: i32, t: i32) {
        // Horizontal wire of net 7 on line t, span [lo,hi] — shape
        // [lo-70, t-70, hi+70, t+70] with w=140.
        let r = RectDbu::new(lo - 70, t - 70, hi + 70, t + 70);
        occ.rasterize(0, &r, Occupant::Net(7), None);
    }

    #[test]
    fn test_band_math() {
        let tech = toy_tech();
        let mut occ = toy_occ(&tech);
        commit_wire(&mut occ, 1000, 2000, 1000);
        // Committed shape [930,2070] on y=1000. w2=70, s=140.
        let mut hits = Vec::new();
        // Near: candidate span [2210,3000] → shape lo 2140, gap 70 < s → hit.
        occ.scan_wire(Axis::X, 0, 1, 2210, 3000, &mut hits);
        assert!(!hits.is_empty(), "edge distance 70 < s must conflict");
        // Abut-at-s: span [2280,3000] → shape lo 2210, gap == s → legal.
        hits.clear();
        occ.scan_wire(Axis::X, 0, 1, 2280, 3000, &mut hits);
        assert!(hits.is_empty(), "edge distance == s is legal (strict <)");
        // Far: different line entirely.
        hits.clear();
        occ.scan_wire(Axis::X, 0, 2, 1000, 2000, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_perpendicular_crossing() {
        let tech = toy_tech();
        let mut occ = toy_occ(&tech);
        commit_wire(&mut occ, 1000, 2000, 1000);
        // The committed wire's shape [930,930,2070,1070] marks vertical
        // lines x ∈ (720, 3280) → x = 1000, 2000, 3000 with y-interval
        // [930,1070].
        let mut hits = Vec::new();
        // Vertical candidate through the footprint: x=2000, y [0,1500].
        occ.scan_wire(Axis::Y, 0, 2, 0, 1500, &mut hits);
        assert_eq!(hits.len(), 1, "crossing the footprint must hit");
        // Beside it: x=0 holds no mark.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 0, 0, 1500, &mut hits);
        assert!(hits.is_empty());
        // Boundary: y span [1210,2000] → shape lo 1140, gap to 1070 = 70 → hit.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 2, 1210, 2000, &mut hits);
        assert_eq!(hits.len(), 1);
        // Span [1280,2000] → shape lo 1210, gap 140 == s → free.
        hits.clear();
        occ.scan_wire(Axis::Y, 0, 2, 1280, 2000, &mut hits);
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
        let db = DesignDb::new();
        let tech = toy_tech();
        let mask = HullMask::new(&db, &tech);
        let cover = PinCover::new();
        let occ = Occupancy::new(&tech, &tracks);
        let got = project_attach(&g, &shapes, &tracks, &occ, &mask, &cover, 0);
        assert_eq!(got, vec![3, 4], "nodes adjacent to the pin on covered lines");
        // No covered track at all → Unattachable (empty set).
        let shapes = vec![(LayerId(0), RectDbu::new(150, 3100, 250, 3400))];
        assert!(project_attach(&g, &shapes, &tracks, &occ, &mask, &cover, 0).is_empty());
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

        let occ = Occupancy::new(&tech, &tracks);
        let cfg = MazeConfig::default();
        let mask = HullMask::new(&db, &tech);
        let topo = NetTopo::new(&db, &tech, &tracks, &g, &vias, &occ, &mask);
        let adj = NetAdj::new(&g, &topo, &tech, &tracks, &vias, &mask);
        let outcome = route_one_net(
            &db, &tech, &tracks, &g, &topo, &adj, &vias, &occ, &mask, 0, false, cfg.occ,
        );
        let NetOutcome::Routed(route, _, _) = outcome else {
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

    /// Nodes `(0,0)..(3,0)` on the single toy layer — the patch
    /// fixtures' line graph (x-track grid 0/1000/2000/3000).
    fn line_graph() -> NetGraph {
        NetGraph {
            net: NetId(0),
            nodes: (0..4)
                .map(|ix| RouteNode {
                    layer: LayerId(0),
                    ix,
                    iy: 0,
                })
                .collect(),
            edges: vec![
                crate::drt::graph::RouteEdge {
                    a: 0,
                    b: 1,
                    axis: Axis::X,
                },
                crate::drt::graph::RouteEdge {
                    a: 1,
                    b: 2,
                    axis: Axis::X,
                },
                crate::drt::graph::RouteEdge {
                    a: 2,
                    b: 3,
                    axis: Axis::X,
                },
            ],
            via_edges: vec![],
            pin_nodes: vec![],
        }
    }

    fn one_wire_route(x1: i32, x2: i32) -> NetRoute {
        NetRoute {
            net: NetId(0),
            wires: vec![WireSeg {
                layer: LayerId(0),
                x1,
                y1: 0,
                x2,
                y2: 0,
            }],
            vias: vec![],
            connected: vec![],
            projected: vec![],
            fills: vec![],
        }
    }

    /// The component area `patch_net_min_area` would measure on
    /// `route`'s layer 0 — the fixtures' non-vacuity probe (§7.7).
    fn component_area(
        db: &DesignDb,
        tech: &TechDb,
        g: &NetGraph,
        vias: &ViaTable,
        route: &NetRoute,
    ) -> i64 {
        let ls =
            LayerShapes::from_rects(net_layer_shapes(db, tech, g, vias, route, 0, 70));
        union::union_area(&ls.rects.iter().map(|s| s.rect).collect::<Vec<_>>())
    }

    /// Sub-bound stub (wire `[0,1000] @ y=0`, area 159,600 < bound
    /// 250,000): the +x walk from node (0,0) accumulates to (2,0) and
    /// commits one `WireSeg` `(0,0)–(2000,0)` → union 299,600 ≥ bound.
    #[test]
    fn test_min_area_patch_fills() {
        let tech = toy_tech_area(250_000);
        let tracks = toy_tracks();
        let g = line_graph();
        let vias = ViaTable::default();
        let db = DesignDb::new();
        let mut route = one_wire_route(0, 1000);
        assert_eq!(component_area(&db, &tech, &g, &vias, &route), 159_600);
        let mut occ = toy_occ(&tech);
        occ.rasterize(0, &RectDbu::new(-70, -70, 1070, 70), Occupant::Net(0), None);
        let mut residuals = Vec::new();
        let patches = patch_net_min_area(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &mut occ,
            &mut route,
            &mut residuals,
        );
        assert_eq!(patches, 1);
        assert!(residuals.is_empty(), "{residuals:?}");
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
                    x1: 0,
                    y1: 0,
                    x2: 2000,
                    y2: 0
                },
            ]
        );
        // The patch metal is committed: own-net hits on the extension.
        let mut hits = Vec::new();
        occ.scan_wire(Axis::X, 0, 0, 1500, 1800, &mut hits);
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .all(|h| matches!(h.iv.who, Occupant::Net(0)))
        );
    }

    /// A member pin's io-port rect merges into the component like the
    /// checker's plane merge: bound 230,000 sits under `wire + pin`
    /// (233,800) but over wire alone (159,600) — the patch fires only
    /// when the bound clears the pin-inclusive area.
    #[test]
    fn test_min_area_patch_pin_merges() {
        let tracks = toy_tracks();
        let mut g = line_graph();
        g.pin_nodes = vec![(PinId(0), vec![0])];
        let vias = ViaTable::default();
        let mut db = DesignDb::new();
        db.pin_names.push(db.strings.intern("p0"));
        db.pin_dirs.push(crate::interner::PinDir::Input);
        db.pin_cells.push(None);
        db.pin_macro_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_nets.push(Some(NetId(0)));
        db.pin_offset_x_dbu.push(0);
        db.pin_offset_y_dbu.push(0);
        db.pin_io_layer.push(None);
        db.pin_io_rects
            .push(vec![(LayerId(0), RectDbu::new(1070, -70, 1600, 70))]);
        db.pin_is_driver.push(true);
        let route = one_wire_route(0, 1000);
        // wire [-70,1070] + pin [1070,1600] → [-70,1600]×140 = 233,800.
        assert_eq!(component_area(&db, &toy_tech(), &g, &vias, &route), 233_800);

        let run = |area_dbu2: i64, route: &mut NetRoute| {
            let tech = toy_tech_area(area_dbu2);
            let mut occ = toy_occ(&tech);
            occ.rasterize(0, &RectDbu::new(-70, -70, 1070, 70), Occupant::Net(0), None);
            let mut residuals = Vec::new();
            let n = patch_net_min_area(
                &db,
                &tech,
                &tracks,
                &g,
                &vias,
                &mut occ,
                route,
                &mut residuals,
            );
            (n, residuals)
        };

        // Under pin-inclusive area → already at bound, patch is a
        // no-op (without the pin it would have patched).
        let mut route = route.clone();
        let (patches, residuals) = run(230_000, &mut route);
        assert_eq!((patches, residuals.len()), (0, 0));
        assert_eq!(route.wires.len(), 1);
        // Just over → the same two-step extension commits.
        let mut route = one_wire_route(0, 1000);
        let (patches, residuals) = run(240_000, &mut route);
        assert_eq!(patches, 1);
        assert!(residuals.is_empty(), "{residuals:?}");
        assert!(route.wires.contains(&WireSeg {
            layer: LayerId(0),
            x1: 0,
            y1: 0,
            x2: 2000,
            y2: 0
        }));
    }

    /// Occupancy-strict (decision 8): foreign `Net` marks on the +x
    /// span and an `Obs(u32::MAX)` mark covering −x reject every
    /// candidate walk → exactly one named residual, wires untouched.
    #[test]
    fn test_min_area_patch_occupancy_blocked() {
        let tech = toy_tech_area(250_000);
        let tracks = toy_tracks();
        let g = line_graph();
        let vias = ViaTable::default();
        let db = DesignDb::new();
        let mut route = one_wire_route(1000, 2000);
        assert_eq!(component_area(&db, &tech, &g, &vias, &route), 159_600);
        let mut occ = toy_occ(&tech);
        occ.rasterize(0, &RectDbu::new(930, -70, 2070, 70), Occupant::Net(0), None);
        // Foreign metal inside the [2000,3000] step's band; an Obs
        // mark inside every −x step's band.
        occ.rasterize(
            0,
            &RectDbu::new(2280, -70, 3000, 70),
            Occupant::Net(42),
            None,
        );
        occ.rasterize(
            0,
            &RectDbu::new(-500, -70, -100, 70),
            Occupant::Obs(u32::MAX),
            None,
        );
        let mut residuals = Vec::new();
        let patches = patch_net_min_area(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &mut occ,
            &mut route,
            &mut residuals,
        );
        assert_eq!(patches, 0);
        assert_eq!(route.wires.len(), 1, "blocked patch must not emit");
        assert_eq!(
            residuals,
            vec![AreaResidual {
                net: NetId(0),
                layer: LayerId(0),
                region: RectDbu::new(930, -70, 2070, 70),
                area_dbu2: 159_600,
                bound_dbu2: 250_000,
            }]
        );
    }

    /// The patch never leaves the guide region (decision 9): the wire
    /// ends at the last covered node on its line — no in-region step
    /// exists → named residual, wires unchanged.
    #[test]
    fn test_min_area_patch_region_bound() {
        let tech = toy_tech_area(250_000);
        let tracks = toy_tracks();
        let g = NetGraph {
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
            ],
            edges: vec![crate::drt::graph::RouteEdge {
                a: 0,
                b: 1,
                axis: Axis::X,
            }],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        let vias = ViaTable::default();
        let db = DesignDb::new();
        let mut route = one_wire_route(0, 1000);
        assert_eq!(component_area(&db, &tech, &g, &vias, &route), 159_600);
        let mut occ = toy_occ(&tech);
        let mut residuals = Vec::new();
        let patches = patch_net_min_area(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &mut occ,
            &mut route,
            &mut residuals,
        );
        assert_eq!(patches, 0);
        assert_eq!(route.wires.len(), 1);
        assert_eq!(residuals.len(), 1);
        assert_eq!(residuals[0].region, RectDbu::new(-70, -70, 1070, 70));
    }

    /// The post-loop hook, not just the function: `route_prepared` on
    /// the square fixture with a bound the routed L (union 299,600)
    /// cannot meet alone → one patch closes the square's top edge.
    /// `route_nets` itself rebuilds graphs from guides, so the test
    /// enters at the prepared seam with the synthetic square.
    #[test]
    fn test_min_area_patch_in_route_nets() {
        let tech = toy_tech_area(400_000);
        let tracks = toy_tracks();
        let db = toy_db();
        let g = square_graph();
        let join = GuideJoin {
            nets: vec![(NetId(0), Vec::new())],
            ..Default::default()
        };
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
        let mask = HullMask::new(&db, &tech);
        let occ = Occupancy::new(&tech, &tracks);
        let topos = vec![NetTopo::new(&db, &tech, &tracks, &g, &vias, &occ, &mask)];
        let route_db = route_prepared(
            &db,
            &tech,
            &tracks,
            &join,
            &topos,
            occ,
            &mask,
            &|_, _| square_graph(),
            &vias,
            &MazeConfig::default(),
            None,
            std::time::Instant::now(),
        );
        assert_eq!(route_db.area_patches, 1, "{:?}", route_db.area_residuals);
        assert!(route_db.area_residuals.is_empty());
        let route = &route_db.nets[0];
        assert!(route.wires.contains(&WireSeg {
            layer: LayerId(0),
            x1: 0,
            y1: 1000,
            x2: 1000,
            y2: 1000
        }));
    }

    /// `residuals.md` §1 — identical fills collapse: a wire piece and
    /// a via plate sharing their facing edge with a third piece each
    /// admit the same gap rect (`(A,B)` and `(A,C)` pairs), so
    /// `fills` dedups to one committed `RECT`, not two.
    #[test]
    fn test_closure_fill_dedup() {
        let tech = toy_tech();
        let db = DesignDb::new();
        let g = NetGraph {
            net: NetId(0),
            nodes: vec![],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        let occ = toy_occ(&tech);
        let mask = HullMask::new(&db, &tech);
        let cover = PinCover::new();
        let vias = ViaTable {
            candidates: vec![ViaCandidate {
                name: SymbolId(0),
                lower: LayerId(0),
                upper: LayerId(1),
                cut: SymbolId(0),
                cut_count: 1,
                geom: crate::design_db::ViaGeom::Lef(0),
                rects: vec![(DrcLayer::Routing(LayerId(0)), RectDbu::new(0, -70, 100, 70))],
                area_dbu2: 0,
            }],
            pairs: vec![ViaPair {
                lower: LayerId(0),
                upper: LayerId(1),
                cut: Some(SymbolId(0)),
                candidates: vec![0],
                default: Some(0),
            }],
            gaps: vec![],
            unmatched: vec![],
        };
        // Pieces: A [0,930,500,1070], B [600,930,1100,1070] on the
        // t=1000 line; the via at (600,1000) plates
        // C [600,930,700,1070] — C faces A across the identical
        // [500,600]×[930,1070] gap B does (gap 100 < s=140).
        let mut hlines: PaintLines =
            BTreeMap::new();
        hlines.insert(
            (0, 1000),
            vec![(0, 500, PaintSrc::Tree), (600, 1100, PaintSrc::Tree)],
        );
        let vlines = BTreeMap::new();
        let mut vplaced = BTreeMap::new();
        vplaced.insert((0u8, 600, 1000), PaintSrc::Tree);
        let res = close_self_spacing(
            &db, &tech, &g, &occ, &mask, &cover, &vias, &hlines, &vlines, &vplaced,
        );
        let ClosureResult::Done { fills, .. } = res else {
            panic!("no droppable prong exists — must be Done");
        };
        assert_eq!(
            fills,
            vec![(LayerId(0), RectDbu::new(499, 930, 601, 1070))],
        );
    }

    /// `self-spacing.md`'s Reject arm: an unfillable notch on a prong
    /// carrying droppable attach paint rejects that element (smallest
    /// `Ord` first — `ProjStub` before `Access`) so the pin retries
    /// its next candidate, instead of naming a residual.
    #[test]
    fn test_closure_reject_droppable() {
        let tech = toy_tech();
        let db = DesignDb::new();
        let g = NetGraph {
            net: NetId(0),
            nodes: vec![],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        let mut occ = toy_occ(&tech);
        // Foreign committed metal walls the [500,600]×[930,1070] gap —
        // every `try_fill` slab loses it to `fill_blocked`.
        occ.rasterize(
            0,
            &RectDbu::new(500, 930, 600, 1070),
            Occupant::Net(9),
            None,
        );
        let mask = HullMask::new(&db, &tech);
        let cover = PinCover::new();
        let vias = ViaTable::default();
        let mut hlines: PaintLines = BTreeMap::new();
        hlines.insert(
            (0, 1000),
            vec![
                (0, 500, PaintSrc::ProjStub(0)),
                (600, 1100, PaintSrc::Access(1)),
            ],
        );
        let res = close_self_spacing(
            &db,
            &tech,
            &g,
            &occ,
            &mask,
            &cover,
            &vias,
            &hlines,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let ClosureResult::Reject { pin, src } = res else {
            panic!("a droppable prong on a blocked notch must Reject: {res:?}");
        };
        assert_eq!(
            (pin, src),
            (0, PaintSrc::ProjStub(0)),
            "the smaller blast radius goes back first"
        );
    }

    /// The undroppable arm: `Tree`/`PinShape` prongs over a walled
    /// gap cannot reject — the site lands named in
    /// `residuals.notches`, the census a Magic `*.2` finding maps
    /// back to (`self-spacing.md`, `residuals.md` §5).
    #[test]
    fn test_closure_notch_residual() {
        let tech = toy_tech();
        let db = DesignDb::new();
        let g = NetGraph {
            net: NetId(0),
            nodes: vec![],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };
        let mut occ = toy_occ(&tech);
        occ.rasterize(
            0,
            &RectDbu::new(500, 930, 600, 1070),
            Occupant::Net(9),
            None,
        );
        let mask = HullMask::new(&db, &tech);
        let cover = PinCover::new();
        let vias = ViaTable::default();
        let mut hlines: PaintLines =
            BTreeMap::new();
        hlines.insert(
            (0, 1000),
            vec![(0, 500, PaintSrc::Tree), (600, 1100, PaintSrc::PinShape(0))],
        );
        let res = close_self_spacing(
            &db,
            &tech,
            &g,
            &occ,
            &mask,
            &cover,
            &vias,
            &hlines,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let ClosureResult::Done { fills, residuals } = res else {
            panic!("no droppable prong exists — must be Done");
        };
        assert!(fills.is_empty(), "a walled gap must not fill: {fills:?}");
        assert_eq!(
            residuals.notches,
            vec![NotchResidual {
                net: NetId(0),
                layer: LayerId(0),
                a: RectDbu::new(0, 930, 500, 1070),
                b: RectDbu::new(600, 930, 1100, 1070),
            }]
        );
    }

    /// `residuals.md` §2 — the same-net guard admits a merge-through:
    /// the reaching span at hi=2 would stop 60 < s=140 short of the
    /// second own component, so the walk must continue to the hi=3
    /// span that merges it instead of painting the notch.
    #[test]
    fn test_min_area_patch_same_net_guard() {
        let tech = toy_tech_area(250_000);
        let tracks = toy_tracks();
        let g = line_graph();
        let vias = ViaTable::default();
        let db = DesignDb::new();
        let mut route = one_wire_route(0, 1000);
        // Second own component at gap 60 past the hi=2 reach — an
        // unguarded walk commits (0,0)–(2000,0) and leaves a notch.
        route.fills = vec![(LayerId(0), RectDbu::new(2130, -70, 2470, 70))];
        let mut occ = toy_occ(&tech);
        occ.rasterize(0, &RectDbu::new(-70, -70, 1070, 70), Occupant::Net(0), None);
        let mut residuals = Vec::new();
        let patches = patch_net_min_area(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &mut occ,
            &mut route,
            &mut residuals,
        );
        assert_eq!(patches, 1, "{residuals:?}");
        assert!(residuals.is_empty(), "{residuals:?}");
        assert!(
            route.wires.contains(&WireSeg {
                layer: LayerId(0),
                x1: 0,
                y1: 0,
                x2: 3000,
                y2: 0
            }),
            "the commit must merge the facing component, not notch it: {:?}",
            route.wires
        );
    }

    /// The guard's residual arm: flanking own components sit `< s`
    /// off every legal stop, so no span commits — the wire component
    /// lands named in `residuals` rather than painting a notch.
    #[test]
    fn test_min_area_patch_same_net_residual() {
        let tech = toy_tech_area(250_000);
        let tracks = toy_tracks();
        let g = line_graph();
        let vias = ViaTable::default();
        let db = DesignDb::new();
        let mut route = one_wire_route(0, 1000);
        route.fills = vec![
            (LayerId(0), RectDbu::new(2130, -70, 2470, 70)),
            (LayerId(0), RectDbu::new(3130, -70, 3570, 70)),
        ];
        let mut occ = toy_occ(&tech);
        occ.rasterize(0, &RectDbu::new(-70, -70, 1070, 70), Occupant::Net(0), None);
        let mut residuals = Vec::new();
        let patches = patch_net_min_area(
            &db,
            &tech,
            &tracks,
            &g,
            &vias,
            &mut occ,
            &mut route,
            &mut residuals,
        );
        assert_eq!(patches, 0);
        assert_eq!(route.wires.len(), 1, "rejected patch must not emit");
        assert!(
            residuals.contains(&AreaResidual {
                net: NetId(0),
                layer: LayerId(0),
                region: RectDbu::new(-70, -70, 1070, 70),
                area_dbu2: 159_600,
                bound_dbu2: 250_000,
            }),
            "{residuals:?}"
        );
    }

    /// `residuals.md` §3 — void closure: a committed donut's
    /// under-bound enclosed void is decomposed and committed as
    /// same-net fills; a foreign instance hull over the void makes it
    /// unfillable → a named `hole_residuals` entry, not silent paint.
    #[test]
    fn test_close_voids_fill_and_residual() {
        // Wall pieces ± w2 = 70: void (140,140)-(860,860) =
        // 518,400 < bound 600,000.
        let donut = || {
            let mut hlines: PaintLines =
                BTreeMap::new();
            hlines.insert((0, 70), vec![(0, 1000, PaintSrc::Tree)]);
            hlines.insert((0, 930), vec![(0, 1000, PaintSrc::Tree)]);
            let mut vlines: PaintLines =
                BTreeMap::new();
            vlines.insert((0, 70), vec![(0, 1000, PaintSrc::Tree)]);
            vlines.insert((0, 930), vec![(0, 1000, PaintSrc::Tree)]);
            (hlines, vlines)
        };
        let g = NetGraph {
            net: NetId(0),
            nodes: vec![],
            edges: vec![],
            via_edges: vec![],
            pin_nodes: vec![],
        };

        // Filled arm: no hulls, no marks — the void's decomposition
        // commits in full.
        let tech = {
            let mut tech = TechDb::new();
            let name = tech.strings.intern("met1");
            tech.routing_layers.push(RoutingLayer {
                name,
                dir: LayerDir::Horizontal,
                pitch_dbu: 340,
                width_dbu: 140,
                offset_dbu: 0,
                pitch_x_dbu: 340,
                pitch_y_dbu: 340,
                offset_x_dbu: 0,
                offset_y_dbu: 0,
                tracks_x: vec![],
                tracks_y: vec![],
                res_per_sq: 0.0,
                cap_per_sq_um: 0.0,
                edge_cap_per_um: 0.0,
                spacing_dbu: 140,
                spacing_table: vec![],
                area_dbu2: 0,
                min_enclosed_area_dbu2: 600_000,
            });
            tech
        };
        let db = DesignDb::new();
        let occ = toy_occ(&tech);
        let mask = HullMask::new(&db, &tech);
        let cover = PinCover::new();
        let vias = ViaTable::default();
        let (hlines, vlines) = donut();
        let res = close_self_spacing(
            &db,
            &tech,
            &g,
            &occ,
            &mask,
            &cover,
            &vias,
            &hlines,
            &vlines,
            &BTreeMap::new(),
        );
        let ClosureResult::Done { fills, residuals } = res else {
            panic!("no droppable prong exists — must be Done");
        };
        assert!(residuals.holes.is_empty(), "{:?}", residuals.holes);
        assert_eq!(
            union::union_area(
                &fills.iter().map(|(_, r)| *r).collect::<Vec<_>>()
            ),
            518_400,
            "the void's decomposition must commit completely: {fills:?}"
        );

        // Residual arm: a foreign cell hull covers the void — every
        // covering rect fails `fill_admitted`'s mask clause.
        let mut db2 = DesignDb::new();
        db2.cell_names.push(db2.strings.intern("c0"));
        db2.cell_macros.push(crate::interner::MacroId(0));
        db2.cell_lib_ids.push(None);
        db2.cell_status.push(PlacementStatus::Placed);
        db2.cell_x_dbu.push(0);
        db2.cell_y_dbu.push(0);
        db2.cell_orient.push(crate::design_db::Orient::N);
        db2.cell_pin_offsets = vec![0, 0];
        let mut tech2 = tech.clone();
        tech2.macros.push(crate::tech_db::MacroDef {
            name: tech2.strings.intern("BIG"),
            class: crate::tech_db::MacroClass::Block,
            width_dbu: 5000,
            height_dbu: 5000,
            pin_offset: 0,
            pin_count: 0,
            obs_rects: vec![],
            obs_rects_raw: vec![],
            pin_obs_rects: vec![],
        });
        let mask2 = HullMask::new(&db2, &tech2);
        let (hlines, vlines) = donut();
        let res = close_self_spacing(
            &db2,
            &tech2,
            &g,
            &occ,
            &mask2,
            &cover,
            &vias,
            &hlines,
            &vlines,
            &BTreeMap::new(),
        );
        let ClosureResult::Done { fills, residuals } = res else {
            panic!("no droppable prong exists — must be Done");
        };
        assert!(fills.is_empty(), "hull-covered void must not fill");
        assert_eq!(
            residuals.holes,
            vec![HoleResidual {
                net: NetId(0),
                layer: LayerId(0),
                region: RectDbu::new(140, 140, 860, 860),
                area_dbu2: 518_400,
                bound_dbu2: 600_000,
            }]
        );
    }

    // --- Occupancy differential oracle ---------------------------------
    //
    // `RefOccupancy` is the pre-swap per-layer `BTreeMap<i32, Line>`
    // occupancy, verbatim from before the dense track-indexed rewrite
    // (sharing `Line`, `Interval`, `Occupant`, `Hit`, `SKIP_CHUNK`,
    // `insert`, `rebuild_from`). The seeded op stream below must
    // produce identical `Hit` sequences — order included — on both
    // structures; `RefMark` keeps the old coordinate payload while
    // `Mark` keeps the slot index, with `coords_*`/`tracks` resolving
    // between them.

    #[derive(Clone, Copy, Debug)]
    struct RefMark {
        x_axis: bool,
        layer: u8,
        line: i32,
        iv: Interval,
    }

    struct RefOccupancy {
        lines_x: Vec<BTreeMap<i32, Line>>,
        lines_y: Vec<BTreeMap<i32, Line>>,
        halfw: Vec<i32>,
        spc: Vec<i32>,
    }

    impl RefOccupancy {
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

        fn rasterize(
            &mut self,
            tracks: &[LayerTracks],
            layer: u8,
            r: &RectDbu,
            who: Occupant,
            mut log: Option<&mut Vec<RefMark>>,
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
                Occupancy::insert(self.lines_x[l].entry(t).or_default(), iv);
                if let Some(m) = log.as_deref_mut() {
                    m.push(RefMark {
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
                Occupancy::insert(self.lines_y[l].entry(t).or_default(), iv);
                if let Some(m) = log.as_deref_mut() {
                    m.push(RefMark {
                        x_axis: false,
                        layer,
                        line: t,
                        iv,
                    });
                }
            }
        }

        fn unmark(&mut self, m: &RefMark) {
            let map = if m.x_axis {
                &mut self.lines_x[m.layer as usize]
            } else {
                &mut self.lines_y[m.layer as usize]
            };
            if let Some(line) = map.get_mut(&m.line)
                && let Some(pos) = line.ivs.iter().position(|iv| *iv == m.iv)
            {
                line.ivs.remove(pos);
                Occupancy::rebuild_from(line, pos / SKIP_CHUNK);
            }
        }

        fn scan_wire(&self, axis: Axis, layer: u8, t: i32, c: i32, d: i32, out: &mut Vec<Hit>) {
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
                        return;
                    }
                    if iv.hi > qlo {
                        out.push(Hit {
                            iv: *iv,
                            line: t,
                            layer,
                            x_axis: matches!(axis, Axis::Y),
                            lo: iv.lo.max(slo),
                            hi: iv.hi.min(shi),
                        });
                    }
                }
            }
        }

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
                                layer,
                                x_axis: false,
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
                                layer,
                                x_axis: true,
                                lo: iv.lo.max(r.yl),
                                hi: iv.hi.min(r.yh),
                            });
                        }
                    }
                }
            }
        }
    }

    /// xorshift64* — deterministic op stream for the differential
    /// test (no external RNG dependency).
    struct Xor64(u64);

    impl Xor64 {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn span(&mut self) -> i32 {
            // Rect/query coordinate: covers every track list's extent
            // plus off-edge space so empty-band windows occur.
            -1500 + self.below(7500) as i32
        }
    }

    /// Four routing layers with distinct width/spacing pairs so the
    /// band arithmetic differs per layer.
    fn diff_tech() -> TechDb {
        let mut tech = TechDb::new();
        for (i, (w, s)) in [(140, 140), (160, 160), (170, 130), (150, 150)]
            .into_iter()
            .enumerate()
        {
            let name = tech.strings.intern(&format!("l{i}"));
            tech.routing_layers.push(RoutingLayer {
                name,
                dir: LayerDir::Horizontal,
                pitch_dbu: 340,
                width_dbu: w,
                offset_dbu: 0,
                pitch_x_dbu: 340,
                pitch_y_dbu: 340,
                offset_x_dbu: 0,
                offset_y_dbu: 0,
                tracks_x: vec![],
                tracks_y: vec![],
                res_per_sq: 0.0,
                cap_per_sq_um: 0.0,
                edge_cap_per_um: 0.0,
                spacing_dbu: s,
                spacing_table: vec![],
                area_dbu2: 0,
                min_enclosed_area_dbu2: 0,
            });
        }
        tech
    }

    /// Mixed track lists: uniform, irregular-spaced, and one layer
    /// with an empty axis each way — the no-line-window arm.
    fn diff_tracks() -> Vec<LayerTracks> {
        vec![
            LayerTracks {
                tracks_x: vec![0, 1000, 2000, 3000],
                tracks_y: vec![0, 1000, 2000, 3000],
            },
            LayerTracks {
                tracks_x: vec![-320, 150, 930, 2155, 3900, 5150],
                tracks_y: vec![10, 480, 2990],
            },
            LayerTracks {
                tracks_x: vec![],
                tracks_y: vec![0, 170, 340, 510],
            },
            LayerTracks {
                tracks_x: vec![100, 300, 500],
                tracks_y: vec![],
            },
        ]
    }

    fn rand_rect(rng: &mut Xor64) -> RectDbu {
        // Shape classes: degenerate (xl==xh), long-thin both ways,
        // and large — small rects still band-mark lines while off-
        // edge and empty-window rects exercise the zero-line arms.
        let (xl, yl) = (rng.span(), rng.span());
        let (w, h) = match rng.below(4) {
            0 => (0, rng.below(3000) as i32),
            1 => (rng.below(300) as i32, 3000 + rng.below(4000) as i32),
            2 => (3000 + rng.below(4000) as i32, rng.below(300) as i32),
            _ => (rng.below(2000) as i32, rng.below(2000) as i32),
        };
        RectDbu::new(xl, yl, xl + w, yl + h)
    }

    fn rand_who(rng: &mut Xor64) -> Occupant {
        let n = rng.below(64) as u32;
        match rng.below(4) {
            0 => Occupant::Obs(if rng.below(16) == 0 { u32::MAX } else { n }),
            1 => Occupant::Net(n),
            2 => Occupant::Pin(n),
            _ => Occupant::ObsPin(n),
        }
    }

    fn who_key(w: Occupant) -> (u8, u32) {
        match w {
            Occupant::Obs(n) => (0, n),
            Occupant::Net(n) => (1, n),
            Occupant::Pin(n) => (2, n),
            Occupant::ObsPin(n) => (3, n),
        }
    }

    /// Every `Hit` field as a sortable key.
    type HitKey = (i32, u8, bool, i32, i32, i32, i32, i32, i32, (u8, u32));

    /// Ordering the emitted `Vec`s must be identical — a sorted
    /// multiset check would hide a permutation the raw comparison
    /// catches.
    fn hit_key(h: &Hit) -> HitKey {
        (
            h.line,
            h.layer,
            h.x_axis,
            h.lo,
            h.hi,
            h.iv.lo,
            h.iv.hi,
            h.iv.perp_lo,
            h.iv.perp_hi,
            who_key(h.iv.who),
        )
    }

    fn assert_hits_eq(new: &[Hit], oracle: &[Hit], ctx: &str) {
        let a: Vec<_> = new.iter().map(hit_key).collect();
        let b: Vec<_> = oracle.iter().map(hit_key).collect();
        assert_eq!(a, b, "{ctx}: hit sequence differs ({a:?} vs {b:?})");
    }

    /// Differential test: the dense `Occupancy` must reproduce the
    /// BTreeMap oracle's `Hit` sequences exactly — window endpoints,
    /// mark/unmark churn, duplicate intervals, empty axes, and
    /// off-grid rects included.
    #[test]
    fn test_occupancy_differential() {
        let tech = diff_tech();
        let tracks = diff_tracks();
        for &seed in &[0x9E37_79B9_7F4A_7C15u64, 0xDEAD_BEEF_CAFE_F00D] {
            let mut rng = Xor64(seed);
            let mut occ = Occupancy::new(&tech, &tracks);
            let mut oracle = RefOccupancy::new(&tech);
            // Parallel rip-up logs: `marks[i]` and `marks_ref[i]` name
            // the same insertion (slot index vs coordinate).
            let mut marks: Vec<Mark> = Vec::new();
            let mut marks_ref: Vec<RefMark> = Vec::new();
            let mut dead: Vec<(Mark, RefMark)> = Vec::new();
            let (mut ha, mut hb) = (Vec::new(), Vec::new());

            for op in 0..5000u64 {
                let ctx = format!("seed {seed:#x} op {op}");
                match rng.below(20) {
                    // ~40%: rasterize a random rect; ~1 in 8 repeats
                    // the insert so equal `iv`s stack up on a line.
                    0..=7 => {
                        let layer = rng.below(4) as u8;
                        let (r, who) = (rand_rect(&mut rng), rand_who(&mut rng));
                        let reps = 1 + (rng.below(8) == 0) as usize;
                        for _ in 0..reps {
                            occ.rasterize(layer, &r, who, Some(&mut marks));
                            oracle.rasterize(&tracks, layer, &r, who, Some(&mut marks_ref));
                        }
                    }
                    // ~15%: rip up a random live mark; a fifth of the
                    // time re-unmark a dead one (no-op unless a
                    // duplicate `iv` is still live — exercised by the
                    // duplicate-insert arm above).
                    8..=10 => {
                        if !dead.is_empty() && (marks.is_empty() || rng.below(5) == 0) {
                            let i = rng.below(dead.len() as u64) as usize;
                            occ.unmark(&dead[i].0);
                            oracle.unmark(&dead[i].1);
                        } else if !marks.is_empty() {
                            let i = rng.below(marks.len() as u64) as usize;
                            occ.unmark(&marks[i]);
                            oracle.unmark(&marks_ref[i]);
                            dead.push((marks[i], marks_ref[i]));
                            marks.swap_remove(i);
                            marks_ref.swap_remove(i);
                        }
                    }
                    // ~20%: `scan_wire` on a random live track index —
                    // the oracle gets the resolved coordinate.
                    11..=14 => {
                        let layer = rng.below(4) as u8;
                        let axis = [Axis::X, Axis::Y][rng.below(2) as usize];
                        let coords = match axis {
                            Axis::X => &tracks[layer as usize].tracks_y,
                            Axis::Y => &tracks[layer as usize].tracks_x,
                        };
                        // An empty axis can never host a wire
                        // candidate — out of contract, so re-roll as
                        // a rect query on that layer instead.
                        if coords.is_empty() {
                            let r = rand_rect(&mut rng);
                            ha.clear();
                            hb.clear();
                            occ.scan_rect(layer, &r, &mut ha);
                            oracle.scan_rect(layer, &r, &mut hb);
                            assert_hits_eq(&ha, &hb, &format!("{ctx} rect (empty-axis fallback)"));
                            continue;
                        }
                        let ti = rng.below(coords.len() as u64) as usize;
                        let c = rng.span();
                        let d = c + rng.below(4000) as i32;
                        ha.clear();
                        hb.clear();
                        occ.scan_wire(axis, layer, ti as u32, c, d, &mut ha);
                        oracle.scan_wire(axis, layer, coords[ti], c, d, &mut hb);
                        assert_hits_eq(&ha, &hb, &format!("{ctx} wire"));
                    }
                    // ~25%: `scan_rect` on a random rect.
                    _ => {
                        let layer = rng.below(4) as u8;
                        let r = rand_rect(&mut rng);
                        ha.clear();
                        hb.clear();
                        occ.scan_rect(layer, &r, &mut ha);
                        oracle.scan_rect(layer, &r, &mut hb);
                        assert_hits_eq(&ha, &hb, &format!("{ctx} rect"));
                    }
                }
                assert_eq!(marks.len(), marks_ref.len(), "{ctx}: mark logs diverged");
            }

            // Deliberate arms the stream may under-hit.
            // A rect whose bands cover no track line marks nothing.
            let before = marks.len();
            occ.rasterize(
                0,
                &RectDbu::new(-60_000, -60_000, -59_000, -59_000),
                Occupant::Obs(0),
                Some(&mut marks),
            );
            oracle.rasterize(
                &tracks,
                0,
                &RectDbu::new(-60_000, -60_000, -59_000, -59_000),
                Occupant::Obs(0),
                Some(&mut marks_ref),
            );
            assert_eq!(marks.len(), before, "no-line rect must mark nothing");
            assert_eq!(marks_ref.len(), before, "oracle: no-line rect");
            // Unmark-all then reinsert — empty lines must carry no
            // tombstones and reinserted intervals behave identically.
            while let Some(m) = marks.pop() {
                let mr = marks_ref.pop().unwrap();
                occ.unmark(&m);
                oracle.unmark(&mr);
            }
            let dup = RectDbu::new(-500, -500, 500, 500);
            for _ in 0..2 {
                occ.rasterize(0, &dup, Occupant::Net(5), Some(&mut marks));
                oracle.rasterize(&tracks, 0, &dup, Occupant::Net(5), Some(&mut marks_ref));
            }
            // Full sweep: every track index on both axes, plus a
            // whole-space rect per layer.
            for layer in 0..tech.routing_layers.len() as u8 {
                let lt = &tracks[layer as usize];
                for (i, &t) in lt.tracks_y.iter().enumerate() {
                    ha.clear();
                    hb.clear();
                    occ.scan_wire(Axis::X, layer, i as u32, -10_000, 10_000, &mut ha);
                    oracle.scan_wire(Axis::X, layer, t, -10_000, 10_000, &mut hb);
                    assert_hits_eq(
                        &ha,
                        &hb,
                        &format!("seed {seed:#x} sweep wire-x l{layer} i{i}"),
                    );
                }
                for (i, &t) in lt.tracks_x.iter().enumerate() {
                    ha.clear();
                    hb.clear();
                    occ.scan_wire(Axis::Y, layer, i as u32, -10_000, 10_000, &mut ha);
                    oracle.scan_wire(Axis::Y, layer, t, -10_000, 10_000, &mut hb);
                    assert_hits_eq(
                        &ha,
                        &hb,
                        &format!("seed {seed:#x} sweep wire-y l{layer} i{i}"),
                    );
                }
                let all = RectDbu::new(-10_000, -10_000, 10_000, 10_000);
                ha.clear();
                hb.clear();
                occ.scan_rect(layer, &all, &mut ha);
                oracle.scan_rect(layer, &all, &mut hb);
                assert_hits_eq(&ha, &hb, &format!("seed {seed:#x} sweep rect l{layer}"));
            }
        }
    }
}
