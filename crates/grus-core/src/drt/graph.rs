//! Guide-bounded track graph: per guided net, the union of its guide
//! boxes clipped to the merged track grid becomes the set of covered
//! `(ix, iy)` track-index pairs — the nodes — with in-plane edges
//! between consecutive-index nodes on both axes, and per-pin
//! attachments from `drt::access::pin_shapes` candidate rects, plus
//! the layer-transition (via) edges between nodes at shared track
//! crossings. Costs and direction legality are later tickets. See
//! `docs/e6-drt.md` §5–§6 for the model and claim boundary.

use crate::congestion::guide::{GuideBox, resolve_guide_nets};
use crate::design_db::{DesignDb, PlacementStatus};
use crate::drc::union::rect_covered;
use crate::drt::access::pin_shapes;
use crate::drt::tracks::{Axis, LayerTracks};
use crate::interner::{LayerId, NetId, PinId};
use crate::tech_db::{RectDbu, TechDb};
use std::collections::{BTreeMap, BTreeSet};

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
    pub a: u32, // index into `nodes`
    pub b: u32,
    pub axis: Axis,
}

/// A layer-transition edge between nodes `lower` (layer l) and
/// `upper` (l+1) at the same (x,y) — a track crossing on both layers
/// and guide-covered on both. `lower < upper` always (nodes are
/// emitted layer-ascending).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViaEdge {
    pub lower: u32,
    pub upper: u32,
}

/// One guided net's on-grid graph plus its pin attachments.
#[derive(Clone, Debug, PartialEq)]
pub struct NetGraph {
    pub net: NetId,
    /// Sorted `(layer, ix, iy)`; index = node id.
    pub nodes: Vec<RouteNode>,
    /// In node order, each unordered pair once.
    pub edges: Vec<RouteEdge>,
    /// Layer-transition edges, adjacent pair ascending → lower-layer
    /// node order. Kept separate from in-plane `edges` — the router
    /// prices transitions differently.
    pub via_edges: Vec<ViaEdge>,
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

/// What the bounded pin-halo escape (`escape_extend`) did for one
/// strict-escape pin — the graph-side census joined onto `fail`
/// lines at emit (`docs/e6-drt.md` §5/§8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeKind {
    /// The halo merged into the region — directly (a kept halo pair
    /// is index-adjacent to a pre-existing pair, `corridor_span == 0`)
    /// or through a bounded L-corridor (`corridor_span` index steps).
    Halo,
    /// The nearest in-region pair on every escape layer lay beyond
    /// `EscapeCfg::corridor_lines` — the guide miss is outside the
    /// rescue window; no pairs were added (`corridor_span` is the
    /// measured gap, `u32::MAX` when no layer had a target pair).
    CorridorBound,
    /// Every within-bound corridor died at the hull mask. The halo
    /// pairs themselves were legal and stay — the pin may still
    /// attach to them (an island) or through a route-time access
    /// arc.
    CorridorMasked,
    /// No legal track crossing inside the inflated pin bbox on any
    /// escape layer — nothing to anchor an escape to.
    HaloEmpty,
}

/// One strict-escape pin's escape attempt — recorded for every
/// `region_escape` entry when `GraphOpts::escape` is on, so a failed
/// pin always names what the mechanism tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscapeRecord {
    pub net: NetId,
    pub pin: PinId,
    pub kind: EscapeKind,
    /// Halo + corridor pairs that entered `region` — 0 when the
    /// escape was refused (`CorridorBound`, `HaloEmpty`).
    pub pairs_added: u32,
    /// Index steps of the landed corridor, or of the best
    /// within-bound attempt (`CorridorMasked`) / the measured gap
    /// (`CorridorBound`); 0 when the halo merged without a corridor
    /// or the halo was empty.
    pub corridor_span: u32,
}

impl EscapeRecord {
    /// The `escape=` token of the `fail` line (`docs/e6-drt.md` §8) —
    /// one whitespace-free token per `EscapeKind`.
    pub fn token(&self) -> String {
        match self.kind {
            EscapeKind::Halo if self.corridor_span == 0 => "halo".to_string(),
            EscapeKind::Halo => format!("halo(span={})", self.corridor_span),
            EscapeKind::CorridorBound => format!("corridor_bound(span={})", self.corridor_span),
            EscapeKind::CorridorMasked => "corridor_masked".to_string(),
            EscapeKind::HaloEmpty => "halo_empty".to_string(),
        }
    }
}

/// The bounded pin-halo escape's bounds (`docs/e6-drt.md` §5) —
/// index steps, grid-native like `transit_extend`'s nearest-pair
/// search, never DBU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscapeCfg {
    /// Pin-bbox covered range inflation per axis — ≥ `access_vias`'s
    /// `B_EXT` so `b`-landings exist on the upper stack, one slack
    /// line beyond `project_attach`'s ±1 reach.
    pub halo_lines: i32,
    /// Total Manhattan index steps a rescue corridor may span —
    /// ≈1–2 GCell pitches on the sky130 grids (the "guide stops just
    /// short" window), not an arbitrary guide rewrite.
    pub corridor_lines: i32,
}

impl EscapeCfg {
    pub const DEFAULT: Self = Self {
        halo_lines: 2,
        corridor_lines: 8,
    };
}

/// `build_route_graphs_ext` options — replaces the bare
/// `spare_above` parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphOpts {
    /// Extra routing layers per net, covered on every guide box's XY
    /// (`net_region`) — the OBS-sealed pin-pocket escape.
    pub spare_above: u8,
    /// The bounded pin-halo escape; `None` reproduces the
    /// pre-escape behavior bit-identically (the strict-escape set is
    /// still computed for `region_escape`).
    pub escape: Option<EscapeCfg>,
}

/// All guided nets' graphs plus the cross-net census.
#[derive(Clone, Debug, Default)]
pub struct RouteGraphs {
    pub nets: Vec<NetGraph>, // guide-file order
    pub zero_coverage: Vec<ZeroCoverage>,
    pub region_escape: Vec<RegionEscape>,
    /// Per strict-escape pin's escape attempt, in net/`net_pins`
    /// order — parallel to `region_escape` when the mechanism is on,
    /// empty when `GraphOpts::escape` is `None`.
    pub escapes: Vec<EscapeRecord>,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_via_edges: usize,
    /// Via-edge count per adjacent pair — parallel to
    /// `tech.routing_layers.len() - 1` (index = lower layer). The
    /// census evidence; a pair pinned at 0 is the named edge-domain
    /// tech gap (`docs/e6-drt.md` §6, met2–met3 on sky130).
    pub via_edges_per_pair: Vec<usize>,
    /// Region nodes dropped by the instance-hull mask, per routing
    /// layer — the connectivity cost of the `drc(full)` occupancy
    /// fix (`docs/e6-drt.md` e2e). Zero on layers the mask does not
    /// apply to.
    pub hull_masked: Vec<usize>,
}

/// A member pin counts as signal when it is an instance pin whose
/// macro pin is not `is_power` (an unmapped `pin_macro_pins` entry
/// counts conservatively) or an io port present in `db.io_pins`
/// (sorted — invariant I5 — binary search). A net is signal iff ≥1
/// member pin is signal: `VGND`/`VPWR`-style `NETS` records fail both
/// arms (`+ SPECIAL` ports never join `io_pins`; their `*` wildcard
/// members are `is_power` macro pins).
fn is_signal_pin(db: &DesignDb, tech: &TechDb, p: PinId) -> bool {
    let i = p.0 as usize;
    match db.pin_cells[i] {
        Some(_) => match db.pin_macro_pins[i] {
            None => true,
            Some(mp) => !tech.macro_pins[mp.0 as usize].is_power,
        },
        None => db.io_pins.binary_search(&p).is_ok(),
    }
}

/// Joins parsed guide blocks to design nets: wraps
/// `resolve_guide_nets` (extra names bail there) and classifies the
/// absent `net_names` nets into `missing_signal` /
/// `absent_expected` per `guide-join.md`.
pub fn join_guides(
    db: &DesignDb,
    tech: &TechDb,
    parsed: &[(String, Vec<GuideBox>)],
) -> anyhow::Result<GuideJoin> {
    let nets = resolve_guide_nets(db, parsed)?;
    let guided: BTreeSet<u32> = nets.iter().map(|(n, _)| n.0).collect();
    let mut missing_signal = Vec::new();
    let mut absent_expected = Vec::new();
    for i in 0..db.num_nets() {
        let net = NetId(i as u32);
        if guided.contains(&net.0) {
            continue;
        }
        let pins = db.net_pins(net);
        // OpenROAD emits no block for <2-pin nets or non-signal nets —
        // only a ≥2-pin signal net absent from the guide is a gap.
        let signal = pins.iter().any(|&p| is_signal_pin(db, tech, p));
        if pins.len() >= 2 && signal {
            missing_signal.push(net);
        } else {
            absent_expected.push(net);
        }
    }
    Ok(GuideJoin {
        nets,
        missing_signal,
        absent_expected,
    })
}

/// Index range `[a, b)` of `list` entries inside `[lo, hi]`.
fn covered_range(list: &[i32], lo: i32, hi: i32) -> (usize, usize) {
    (
        list.partition_point(|&t| t < lo),
        list.partition_point(|&t| t <= hi),
    )
}

/// Layer → sorted unique covered `(ix, iy)` pairs.
type RegionMap = BTreeMap<u8, Vec<(u32, u32)>>;

/// One net's region: layer → sorted unique `(ix, iy)` pairs covered by
/// the box union. Per box the covered indices are contiguous, so the
/// work is `O(boxes × tracks-per-box)`, never `O(die)`. Two diagonally
/// disjoint boxes cannot spawn corner nodes — coverage is enumerated
/// per box, not as a bounding cross product.
/// `spare_above` extra routing layers per net, covered within each
/// box's XY on top of the guided layer. A guide box bounds the XY
/// region; the global route's layer choice does not bind detailed
/// routing — OBS-sealed pin pockets are reached one layer above the
/// topmost guided layer (OpenROAD's routed DEF confirms: gcd `_038_`
/// escapes its li1+met1 guide on met2 inside the same box).
fn net_region(
    boxes: &[GuideBox],
    tracks: &[LayerTracks],
    spare_above: u8,
) -> (RegionMap, BTreeSet<u8>) {
    let mut region: BTreeMap<u8, Vec<(u32, u32)>> = BTreeMap::new();
    let mut box_layers: BTreeSet<u8> = BTreeSet::new();
    let spare = boxes
        .iter()
        .map(|b| b.layer.0)
        .max()
        .map_or(0, |m| m.saturating_add(spare_above))
        .min(tracks.len().saturating_sub(1).min(u8::MAX as usize) as u8);
    for b in boxes {
        box_layers.insert(b.layer.0);
        // `spare_above == 0` reproduces the guided-layer-only coverage
        // (the graph stage's pinned census); >0 adds the spare layer
        // on every box's XY for the maze's OBS-seal freedom.
        for l in [b.layer.0, spare]
            .into_iter()
            .take(usize::from(spare_above > 0) + 1)
        {
            let Some(lt) = tracks.get(l as usize) else {
                continue;
            };
            let (x0, x1) = covered_range(&lt.tracks_x, b.rect.xl, b.rect.xh);
            let (y0, y1) = covered_range(&lt.tracks_y, b.rect.yl, b.rect.yh);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let pairs = region.entry(l).or_default();
            for ix in x0..x1 {
                for iy in y0..y1 {
                    pairs.push((ix as u32, iy as u32));
                }
            }
        }
    }
    for pairs in region.values_mut() {
        pairs.sort_unstable();
        pairs.dedup();
    }
    (region, box_layers)
}

// ---------------------------------------------------------------------------
// Instance-hull mask
// ---------------------------------------------------------------------------

/// One net's hull exemption: `cell → absolute pin-rect XY regions`
/// of the member pins the cell hosts — the "own-pin cover" the mask
/// carves out of a host hull. The union is *not* layer-keyed: the
/// delegated routes drop the whole mcon→met1→M1M2 access stack inside
/// a pin rect's XY footprint on layers the pin does not declare
/// (gcd `_001_`: met1 stub + both vias inside the li1 `D` rect's XY),
/// so the cover is the pin's XY landing region — the raw rects
/// `pin_rects` computes — on every masked layer.
/// A `cell` key exists only when the cell hosts a member pin of the
/// net, so host-ness needs no second lookup.
pub type PinCover = BTreeMap<u32, Vec<RectDbu>>;

/// `cover(N)` for the hull mask — `pin_shapes` over `net`'s member
/// instance pins, XY-unioned per host cell. The raw pin rect is the
/// confinement boundary on masked layers: emitted metal inside a hull
/// is legal only where it fully overlaps the pin's own LEF rect —
/// paint in a grown corridor is what Magic flags as subcell-boundary
/// spacing/abutment (the `drc(full)` residual class), and host OBS
/// marks enforce the same `w/2 + s` clearance `premark_obs` knows.
/// A pin rect that holds no track gets its access off-track via the
/// pin-anchored arcs in `maze::access_vias` — the delegated router's
/// model — not by growing the cover.
pub fn pin_rects(db: &DesignDb, tech: &TechDb, net: NetId) -> PinCover {
    let mut cover: PinCover = BTreeMap::new();
    for &pin in db.net_pins(net) {
        let Some(c) = db.pin_cells[pin.0 as usize] else {
            continue;
        };
        for (_, r) in pin_shapes(db, tech, pin) {
            cover.entry(c.0).or_default().push(r);
        }
    }
    cover
}

/// Cell `c`'s placed outline — the conservative hull rect a master
/// may paint internals anywhere inside of on the layers it uses.
pub fn cell_hull_rect(db: &DesignDb, tech: &TechDb, c: u32) -> RectDbu {
    let mac = &tech.macros[db.cell_macros[c as usize].0 as usize];
    let r = DesignDb::orient_rect(
        RectDbu::new(0, 0, mac.width_dbu, mac.height_dbu),
        mac.width_dbu,
        mac.height_dbu,
        db.cell_orient[c as usize],
    );
    RectDbu::new(
        db.cell_x_dbu[c as usize] + r.xl,
        db.cell_y_dbu[c as usize] + r.yl,
        db.cell_x_dbu[c as usize] + r.xh,
        db.cell_y_dbu[c as usize] + r.yh,
    )
}

/// Which routing layers carry master-internal paint the occupancy
/// model cannot see — the shipped L1 variant of the hull ladder
/// (`kurena/tasks/e6-drt-e2e/design/hull-model.md`): sky130_fd_sc_hd
/// masters paint only li1/met1 internals, so the mask applies there;
/// met2+ transit over a cell is legal detailed routing (the strict
/// all-layer variant starves every net — pins become graph islands).
fn hull_masked_layer(tech: &TechDb, l: usize) -> bool {
    matches!(
        tech.strings.resolve(tech.routing_layers[l].name),
        "li1" | "met1"
    )
}

/// Masked layers whose master internals are sparse enough that the
/// OBS rasterization navigates them — the *host-channel* variant:
/// inside a foreign hull the layer stays fully masked, but inside a
/// hull hosting a member pin the whole interior is open and the
/// maze's `Obs(cell)` marks enforce the channels (the delegated
/// routes run met1 access wires through exactly those OBS-free
/// bands, e.g. gcd `_001_`'s 965-DBU stub inside `_412_`). li1
/// internals fill the master (the OBS list is dense), so li1 keeps
/// the strict own-pin cover.
fn hull_host_open_layer(tech: &TechDb, l: usize) -> bool {
    matches!(tech.strings.resolve(tech.routing_layers[l].name), "met1")
}

/// One y-band of the hull index: instance hulls sharing `[yl, yh]`
/// (a placement row's cells share both), x-intervals sorted by `xl`
/// with a prefix-max `xh` so a backward scan stops at the first
/// provable miss — the same skip pattern `Line::max_hi` uses.
#[derive(Clone, Debug, Default)]
struct HullBand {
    yl: i32,
    yh: i32,
    /// `(xl, xh, cell)` sorted by `xl`.
    ivs: Vec<(i32, i32, u32)>,
    /// `premix[i] = max(ivs[j].1)` over `j <= i`.
    premix: Vec<i32>,
}

/// The placed-instance bboxes whose subcell-internal paint the
/// occupancy model cannot see — the `drc(full)` fix
/// (`kurena/tasks/e6-drt-e2e/design/hull-model.md`). On a `masked`
/// layer a point or axis-aligned segment is legal for a net only
/// outside every hull, or inside an own-pin `PinCover` rect of a
/// containing (host) cell — Magic requires full overlap at the
/// hierarchy boundary; abutment and partial overlap are findings.
/// Built once per design; queried per region node (graph build) and
/// per access stub / relaxed via (route).
pub struct HullMask {
    /// Per routing layer: the mask applies only where masters paint
    /// internals the model cannot see (`hull_masked_layer`).
    masked: Vec<bool>,
    /// Per routing layer: a masked layer whose host-hull interior is
    /// open to the host net (`hull_host_open_layer`) — OBS marks
    /// carry the legality there, not the cover.
    open: Vec<bool>,
    /// Sorted by `yl`; `premiy[i] = max(bands[j].yh)` over `j <= i`.
    bands: Vec<HullBand>,
    premiy: Vec<i32>,
}

impl HullMask {
    /// Index every `{Placed, Fixed, Cover}` instance's placed outline —
    /// the conservative hull: a master may paint internals anywhere
    /// inside its bbox on the layers it uses.
    pub fn new(db: &DesignDb, tech: &TechDb) -> Self {
        let masked: Vec<bool> = (0..tech.routing_layers.len())
            .map(|l| hull_masked_layer(tech, l))
            .collect();
        let open: Vec<bool> = (0..tech.routing_layers.len())
            .map(|l| hull_masked_layer(tech, l) && hull_host_open_layer(tech, l))
            .collect();
        let mut bands: Vec<HullBand> = Vec::new();
        for c in 0..db.num_cells() {
            if !matches!(
                db.cell_status[c],
                PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
            ) {
                continue;
            }
            let r = cell_hull_rect(db, tech, c as u32);
            let (yl, yh) = (r.yl, r.yh);
            let iv = (r.xl, r.xh, c as u32);
            // Cells in a placement row share (yl, yh) exactly; the
            // binary insert keeps `bands` (yl, yh)-sorted.
            match bands.binary_search_by_key(&(yl, yh), |b| (b.yl, b.yh)) {
                Ok(i) => bands[i].ivs.push(iv),
                Err(i) => bands.insert(
                    i,
                    HullBand {
                        yl,
                        yh,
                        ivs: vec![iv],
                        premix: Vec::new(),
                    },
                ),
            }
        }
        for b in &mut bands {
            b.ivs.sort_unstable_by_key(|iv| iv.0);
            b.premix = b
                .ivs
                .iter()
                .scan(i32::MIN, |m, iv| {
                    *m = (*m).max(iv.1);
                    Some(*m)
                })
                .collect();
        }
        let premiy = bands
            .iter()
            .scan(i32::MIN, |m, b| {
                *m = (*m).max(b.yh);
                Some(*m)
            })
            .collect();
        HullMask {
            masked,
            open,
            bands,
            premiy,
        }
    }

    /// Cells whose hull contains `(x, y)` — edge-inclusive, matching
    /// the `pin_nodes` containment convention. Instances abut, never
    /// overlap, so a point meets at most a couple of hulls; `out` is
    /// caller scratch.
    fn hull_cells_at(&self, x: i32, y: i32, out: &mut Vec<u32>) {
        out.clear();
        let pos = self.bands.partition_point(|b| b.yl <= y);
        for i in (0..pos).rev() {
            if self.premiy[i] < y {
                break;
            }
            let b = &self.bands[i];
            if b.yh < y {
                continue;
            }
            let xp = b.ivs.partition_point(|iv| iv.0 <= x);
            for j in (0..xp).rev() {
                if b.premix[j] < x {
                    break;
                }
                let iv = b.ivs[j];
                if iv.1 >= x {
                    out.push(iv.2);
                }
            }
        }
    }

    /// Whether the mask applies on layer `l` (`hull_masked_layer`).
    pub fn masked(&self, l: u8) -> bool {
        self.masked.get(l as usize).copied().unwrap_or(false)
    }

    /// Whether `l` is a host-open masked layer
    /// (`hull_host_open_layer`): inside a hull hosting a member pin
    /// the whole interior is open at graph level.
    pub fn host_open(&self, l: u8) -> bool {
        self.open.get(l as usize).copied().unwrap_or(false)
    }

    /// Cells whose hull intersects `r` with positive area (strict
    /// overlap — a shared edge is not an intersection). `out` is
    /// caller scratch, replaced.
    pub(crate) fn hulls_overlapping(&self, r: &RectDbu, out: &mut Vec<u32>) {
        out.clear();
        let pos = self.bands.partition_point(|b| b.yl < r.yh);
        for i in (0..pos).rev() {
            if self.premiy[i] <= r.yl {
                break;
            }
            let b = &self.bands[i];
            if b.yh <= r.yl || b.yl >= r.yh {
                continue;
            }
            let xp = b.ivs.partition_point(|iv| iv.0 < r.xh);
            for j in (0..xp).rev() {
                if b.premix[j] <= r.xl {
                    break;
                }
                let (xl, xh, c) = b.ivs[j];
                if xl < r.xh && xh > r.xl {
                    out.push(c);
                }
            }
        }
    }

    /// Point legality for a net with `cover` on layer `l`: outside
    /// every hull, or inside an own-pin rect of a containing cell.
    pub fn point_allowed(
        &self,
        cover: &PinCover,
        scratch: &mut Vec<u32>,
        l: u8,
        x: i32,
        y: i32,
    ) -> bool {
        if !self.masked.get(l as usize).copied().unwrap_or(false) {
            return true;
        }
        self.hull_cells_at(x, y, scratch);
        if scratch.is_empty() {
            return true;
        }
        let open = self.open.get(l as usize).copied().unwrap_or(false);
        scratch.iter().any(|&c| {
            cover.get(&c).is_some_and(|rs| {
                open || rs
                    .iter()
                    .any(|r| x >= r.xl && x <= r.xh && y >= r.yl && y <= r.yh)
            })
        })
    }

    /// Rect legality on layer `l` — the via-footprint counterpart of
    /// `seg_allowed`: for every hull `r` intersects, the covered part
    /// must sit inside the own-pin rect *union* of that cell
    /// (`rect_covered` — the same union-exact rule `seg_allowed`
    /// uses). A via plate that pokes past the pin edge paints
    /// hull-internal metal Magic flags as a subcell-boundary violation
    /// (the delegated router keeps every access plate inside the pin
    /// rect union).
    pub fn rect_allowed(&self, cover: &PinCover, l: u8, r: &RectDbu) -> bool {
        if !self.masked.get(l as usize).copied().unwrap_or(false) {
            return true;
        }
        let pos = self.bands.partition_point(|b| b.yl <= r.yh);
        for i in (0..pos).rev() {
            if self.premiy[i] < r.yl {
                break;
            }
            let b = &self.bands[i];
            if b.yh < r.yl || b.yl > r.yh {
                continue;
            }
            let xp = b.ivs.partition_point(|iv| iv.0 <= r.xh);
            for j in (0..xp).rev() {
                if b.premix[j] < r.xl {
                    break;
                }
                let (xl, xh, c) = b.ivs[j];
                if xl > r.xh {
                    continue;
                }
                let (sxl, sxh) = (r.xl.max(xl), r.xh.min(xh));
                let (syl, syh) = (r.yl.max(b.yl), r.yh.min(b.yh));
                if sxl > sxh || syl > syh {
                    continue;
                }
                let covered = cover.get(&c).is_some_and(|rs| {
                    self.open.get(l as usize).copied().unwrap_or(false)
                        || rect_covered(rs, &RectDbu::new(sxl, syl, sxh, syh))
                });
                if !covered {
                    return false;
                }
            }
        }
        true
    }

    /// Axis-aligned segment legality on layer `l`: for every hull the
    /// segment intersects, the covered subsegment must sit inside the
    /// own-pin rect *union* of that cell (`rect_covered` — a part
    /// spanning joined same-pin sub-rects is legal; the delegated
    /// plate class that merges two LEF sub-rects of one pin). A
    /// zero-area touch (a stub grazing a hull edge) counts as inside —
    /// consistent with `hull_cells_at`.
    pub fn seg_allowed(&self, cover: &PinCover, l: u8, x1: i32, y1: i32, x2: i32, y2: i32) -> bool {
        if !self.masked.get(l as usize).copied().unwrap_or(false) {
            return true;
        }
        let (xlo, xhi) = (x1.min(x2), x1.max(x2));
        let (ylo, yhi) = (y1.min(y2), y1.max(y2));
        let pos = self.bands.partition_point(|b| b.yl <= yhi);
        for i in (0..pos).rev() {
            if self.premiy[i] < ylo {
                break;
            }
            let b = &self.bands[i];
            if b.yh < ylo || b.yl > yhi {
                continue;
            }
            let xp = b.ivs.partition_point(|iv| iv.0 <= xhi);
            for j in (0..xp).rev() {
                if b.premix[j] < xlo {
                    break;
                }
                let (xl, xh, c) = b.ivs[j];
                if xl > xhi {
                    continue;
                }
                // Hull ∩ segment — contained in one cover rect iff the
                // subsegment's bounding box is (rects are convex, the
                // segment axis-aligned).
                let (sxl, sxh) = (xlo.max(xl), xhi.min(xh));
                let (syl, syh) = (ylo.max(b.yl), yhi.min(b.yh));
                if sxl > sxh || syl > syh {
                    continue;
                }
                let covered = cover.get(&c).is_some_and(|rs| {
                    self.open.get(l as usize).copied().unwrap_or(false)
                        || rect_covered(rs, &RectDbu::new(sxl, syl, sxh, syh))
                });
                if !covered {
                    return false;
                }
            }
        }
        true
    }
}

/// `build_route_graphs_ext`'s transit-layer extension: adds every
/// host hull's interior track-grid points to `pairs`, plus for each
/// hull an index-adjacent L-corridor to the nearest pre-existing
/// pair — the hull's outermost track and the guide mesh's edge node
/// are not always consecutive indices, and without the corridor the
/// extension stays an island. Deterministic: hulls in `cover` key
/// order, ties in the nearest probe broken by first-seen order over
/// sorted sequences.
fn transit_extend(
    pairs: &mut Vec<(u32, u32)>,
    lt: &LayerTracks,
    db: &DesignDb,
    tech: &TechDb,
    cover: &PinCover,
) {
    let base: Vec<(u32, u32)> = pairs.clone();
    let mut extra: BTreeSet<(u32, u32)> = BTreeSet::new();
    for &c in cover.keys() {
        let r = cell_hull_rect(db, tech, c);
        let x0 = lt.tracks_x.partition_point(|&t| t < r.xl);
        let x1 = x0 + lt.tracks_x[x0..].partition_point(|&t| t <= r.xh);
        let y0 = lt.tracks_y.partition_point(|&t| t < r.yl);
        let y1 = y0 + lt.tracks_y[y0..].partition_point(|&t| t <= r.yh);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        for ix in x0..x1 {
            for iy in y0..y1 {
                extra.insert((ix as u32, iy as u32));
            }
        }
        if base.is_empty() {
            continue;
        }
        let mut best: Option<(i64, u32, u32, u32, u32)> = None;
        for ix in x0..x1 {
            for iy in y0..y1 {
                let (ax, ay) = (lt.tracks_x[ix], lt.tracks_y[iy]);
                for &(jx, jy) in &base {
                    let d = (ax - lt.tracks_x[jx as usize]).unsigned_abs() as i64
                        + (ay - lt.tracks_y[jy as usize]).unsigned_abs() as i64;
                    if best.is_none_or(|(bd, ..)| d < bd) {
                        best = Some((d, ix as u32, iy as u32, jx, jy));
                    }
                }
            }
        }
        if let Some((_, ixa, iya, ixb, iyb)) = best {
            for ix in ixa.min(ixb)..=ixa.max(ixb) {
                extra.insert((ix, iya));
            }
            for iy in iya.min(iyb)..=iya.max(iyb) {
                extra.insert((ixb, iy));
            }
        }
    }
    if extra.is_empty() {
        return;
    }
    pairs.extend(extra);
    pairs.sort_unstable();
    pairs.dedup();
}

/// Indices into `pairs` of the covered pairs inside `rect` — the
/// strict-attach kernel shared by `escape_extend`'s pre-halo escape
/// set and the pin-attach loop, so the set pushed to `region_escape`
/// is exactly what emission would fail to attach.
fn strict_hits(pairs: &[(u32, u32)], lt: &LayerTracks, rect: &RectDbu) -> Vec<u32> {
    let (x0, x1) = covered_range(&lt.tracks_x, rect.xl, rect.xh);
    let (y0, y1) = covered_range(&lt.tracks_y, rect.yl, rect.yh);
    if x0 >= x1 || y0 >= y1 {
        return Vec::new();
    }
    // A node inside the rect is an x-track ∩ y-track crossing the
    // pin's shape — the attachment the access point designates (see
    // docs/e6-drt.md §5 midline caveat).
    let lo = pairs.partition_point(|&(ix, _)| ix < x0 as u32);
    let mut out = Vec::new();
    for (k, &(ix, iy)) in pairs[lo..].iter().enumerate() {
        if ix >= x1 as u32 {
            break;
        }
        if iy >= y0 as u32 && iy < y1 as u32 {
            out.push((lo + k) as u32);
        }
    }
    out
}

/// Index-grid pair `(ix, iy)` — the coordinate of one graph node in a
/// layer's covered-pair set.
type Idx = (u32, u32);

/// Per-layer corridor candidate: BFS span from the layer's halo to
/// its nearest non-halo `region` pair and the attaining endpoints.
struct EscapeCand {
    layer: u8,
    span: u32,
    halo: Idx,
    region: Idx,
}

/// A corridor candidate whose L-path survives the hull mask.
struct Corridor {
    layer: u8,
    span: u32,
    path: Vec<Idx>,
}

/// Min index-grid (Manhattan) distance from `halo` to a `region`
/// pair outside `halo`, plus an attaining `(halo, region)` pair —
/// multi-source BFS over the index grid, each cell visited once and
/// the first target hit in BFS order winning ties. `None` when the
/// layer has no non-halo region pair to anchor to.
fn escape_span(
    halo: &BTreeSet<Idx>,
    region: &[Idx],
    nx: usize,
    ny: usize,
) -> Option<(u32, Idx, Idx)> {
    let seeds: Vec<Idx> = halo.iter().copied().collect();
    // `origin[i]` = index of the seed whose BFS tree reached cell i.
    let mut origin = vec![u32::MAX; nx * ny];
    let mut queue: std::collections::VecDeque<((u32, u32), u32)> = std::collections::VecDeque::new();
    for (i, &s) in seeds.iter().enumerate() {
        origin[s.0 as usize * ny + s.1 as usize] = i as u32;
        queue.push_back((s, 0));
    }
    while let Some((c, d)) = queue.pop_front() {
        for n in [
            (c.0 + 1, c.1),
            (c.0, c.1 + 1),
            (c.0.wrapping_sub(1), c.1),
            (c.0, c.1.wrapping_sub(1)),
        ] {
            if n.0 as usize >= nx || n.1 as usize >= ny {
                continue;
            }
            let o = origin[c.0 as usize * ny + c.1 as usize];
            let slot = &mut origin[n.0 as usize * ny + n.1 as usize];
            if *slot != u32::MAX {
                continue;
            }
            *slot = o;
            if region.binary_search(&n).is_ok() && !halo.contains(&n) {
                return Some((d + 1, seeds[*slot as usize], n));
            }
            queue.push_back((n, d + 1));
        }
    }
    None
}

/// The `transit_extend` L-shape: horizontal leg at `h.1` then
/// vertical leg at `v.0`, interior pairs only (`h` is halo, `v` is
/// already region).
fn l_path(h: Idx, v: Idx) -> Vec<Idx> {
    let mut out = Vec::new();
    for ix in h.0.min(v.0)..=h.0.max(v.0) {
        if (ix, h.1) != h && (ix, h.1) != v {
            out.push((ix, h.1));
        }
    }
    for iy in h.1.min(v.1)..=h.1.max(v.1) {
        if (v.0, iy) != h && (v.0, iy) != v {
            out.push((v.0, iy));
        }
    }
    out
}

/// Merge `extra` into a layer's sorted pair set — the
/// `transit_extend` convention.
fn merge_pairs(pairs: &mut Vec<Idx>, extra: &BTreeSet<Idx>) {
    if extra.is_empty() {
        return;
    }
    pairs.extend(extra.iter().copied());
    pairs.sort_unstable();
    pairs.dedup();
}

/// The strict-escape pre-pass plus the bounded pin-halo escape
/// (`docs/e6-drt.md` §5). Returns the pre-halo escape set — member
/// pins whose shapes hold no strict node in the post-transit region —
/// pushed to `RouteGraphs::region_escape` regardless of post-halo
/// attach so the flag keeps meaning "pin metal outside the guide
/// region". With `escape == None` only the set is computed and no
/// pairs are added — bit-identical to the pre-escape build (the pin
/// still fails `Unattachable`).
///
/// Per escaped pin (in `net_pins` order — deterministic), on every
/// layer from its lowest shape layer up to `max(highest shape layer,
/// first_unmasked)`: the covered pairs of the pin's shape-bbox
/// inflated `halo_lines` index steps per axis, each filtered by
/// `point_allowed` like transit pairs. When no kept pair is
/// index-adjacent to the region, a bounded L-corridor (`≤
/// corridor_lines` Manhattan steps, `point_allowed` per pair)
/// merges the argmin-span layer — refused corridors are recorded,
/// not retried. Escaped pairs are ordinary `region` entries: nodes,
/// edges, via edges, and every occupancy check apply identically —
/// the mechanism adds nodes, never exemptions.
#[allow(clippy::too_many_arguments)]
fn escape_extend(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    region: &mut RegionMap,
    cover: &PinCover,
    hull: &HullMask,
    scratch: &mut Vec<u32>,
    first_unmasked: Option<u8>,
    net: NetId,
    escape: Option<EscapeCfg>,
    escapes: &mut Vec<EscapeRecord>,
) -> BTreeSet<PinId> {
    let mut escaped = BTreeSet::new();
    for &pin in db.net_pins(net) {
        let ok = pin_shapes(db, tech, pin).iter().any(|&(lid, r)| {
            let (Some(pairs), Some(lt)) =
                (region.get(&lid.0), tracks.get(lid.0 as usize))
            else {
                return false;
            };
            !strict_hits(pairs, lt, &r).is_empty()
        });
        if !ok {
            escaped.insert(pin);
        }
    }
    let Some(cfg) = escape else {
        return escaped;
    };
    let bound = cfg.corridor_lines.max(0) as u32;
    for &pin in db.net_pins(net) {
        if !escaped.contains(&pin) {
            continue;
        }
        let mut record = |kind, pairs_added, corridor_span| {
            escapes.push(EscapeRecord {
                net,
                pin,
                kind,
                pairs_added,
                corridor_span,
            });
        };
        let shapes = pin_shapes(db, tech, pin);
        let (mut lo, mut hi) = (u8::MAX, 0u8);
        let mut bbox: Option<RectDbu> = None;
        for &(lid, r) in &shapes {
            lo = lo.min(lid.0);
            hi = hi.max(lid.0);
            bbox = Some(match bbox {
                None => r,
                Some(b) => RectDbu::new(
                    b.xl.min(r.xl),
                    b.yl.min(r.yl),
                    b.xh.max(r.xh),
                    b.yh.max(r.yh),
                ),
            });
        }
        let Some(bbox) = bbox else {
            record(EscapeKind::HaloEmpty, 0, 0);
            continue;
        };
        // The via-descent stack: the pin's own layers up to the
        // corridor-friendly unmasked surface (an li1 pin gets
        // {li1, met1, met2}; a met3 io port gets {met3}).
        hi = hi.max(first_unmasked.unwrap_or(0));
        let h = cfg.halo_lines.max(0) as usize;
        let mut halo: BTreeMap<u8, BTreeSet<(u32, u32)>> = BTreeMap::new();
        for l in lo.min(hi)..=hi {
            let Some(lt) = tracks.get(l as usize) else {
                continue;
            };
            let (x0, x1) = covered_range(&lt.tracks_x, bbox.xl, bbox.xh);
            let (y0, y1) = covered_range(&lt.tracks_y, bbox.yl, bbox.yh);
            let (xa, xb) = (
                x0.saturating_sub(h),
                (x1 + h).min(lt.tracks_x.len()),
            );
            let (ya, yb) = (
                y0.saturating_sub(h),
                (y1 + h).min(lt.tracks_y.len()),
            );
            for ix in xa..xb {
                for iy in ya..yb {
                    let p = (ix as u32, iy as u32);
                    if region
                        .get(&l)
                        .is_some_and(|ps| ps.binary_search(&p).is_ok())
                    {
                        continue;
                    }
                    if !hull.point_allowed(
                        cover,
                        scratch,
                        l,
                        lt.tracks_x[ix],
                        lt.tracks_y[iy],
                    ) {
                        continue;
                    }
                    halo.entry(l).or_default().insert(p);
                }
            }
        }
        let halo_total: usize = halo.values().map(BTreeSet::len).sum();
        if halo_total == 0 {
            record(EscapeKind::HaloEmpty, 0, 0);
            continue;
        }
        // Per-layer argmin (span, halo pair, region pair); a layer
        // with no non-halo region pair offers no anchor.
        let mut cands: Vec<EscapeCand> = Vec::new();
        for (&l, hs) in &halo {
            let (Some(pairs), Some(lt)) = (region.get(&l), tracks.get(l as usize)) else {
                continue;
            };
            if let Some((span, h0, v0)) =
                escape_span(hs, pairs, lt.tracks_x.len(), lt.tracks_y.len())
            {
                cands.push(EscapeCand {
                    layer: l,
                    span,
                    halo: h0,
                    region: v0,
                });
            }
        }
        if cands.iter().any(|c| c.span <= 1) {
            // An adjacent pair already induces an edge at emission —
            // no corridor needed.
            for (l, hs) in halo {
                merge_pairs(region.entry(l).or_default(), &hs);
            }
            record(EscapeKind::Halo, halo_total as u32, 0);
            continue;
        }
        // Try corridors in span order; the unmasked surface wins ties
        // (layers descending).
        cands.sort_by(|a, b| a.span.cmp(&b.span).then(b.layer.cmp(&a.layer)));
        let mut landed: Option<Corridor> = None;
        for c in &cands {
            if c.span > bound {
                break; // sorted: every later candidate also exceeds
            }
            let lt = &tracks[c.layer as usize];
            let path = l_path(c.halo, c.region);
            if path.iter().all(|&(ix, iy)| {
                hull.point_allowed(
                    cover,
                    scratch,
                    c.layer,
                    lt.tracks_x[ix as usize],
                    lt.tracks_y[iy as usize],
                )
            }) {
                landed = Some(Corridor {
                    layer: c.layer,
                    span: c.span,
                    path,
                });
                break;
            }
        }
        let attempted = cands.iter().take_while(|c| c.span <= bound).count();
        match landed {
            Some(Corridor { layer: l, span, path }) => {
                let mut extra: BTreeSet<(u32, u32)> = path.into_iter().collect();
                for (hl, hs) in halo {
                    if hl == l {
                        extra.extend(hs);
                    } else {
                        merge_pairs(region.entry(hl).or_default(), &hs);
                    }
                }
                merge_pairs(region.entry(l).or_default(), &extra);
                record(
                    EscapeKind::Halo,
                    (halo_total + extra.len()) as u32,
                    span,
                );
            }
            None if attempted > 0 => {
                // Every within-bound corridor died at the mask; the
                // legal halo stays — an island the pin still attaches
                // to (and route-time access arcs may land on).
                for (l, hs) in halo {
                    merge_pairs(region.entry(l).or_default(), &hs);
                }
                record(EscapeKind::CorridorMasked, halo_total as u32, cands[0].span);
            }
            None => {
                // Out of the rescue window — leave the graph as if the
                // escape never ran for this pin.
                let gap = cands.first().map(|c| c.span).unwrap_or(u32::MAX);
                record(EscapeKind::CorridorBound, 0, gap);
            }
        }
    }
    escaped
}

/// Edges of one layer's sorted pair set: `(ix, iy)–(ix+1, iy)` on
/// `Axis::X` and `(ix, iy)–(ix, iy+1)` on `Axis::Y` — the induced
/// subgraph of the full grid restricted to covered pairs, so an edge
/// never wires across a one-index corridor gap. `base` is the layer's
/// first node id. Per node the X probe then the Y probe emit in node
/// order; each unordered pair appears once (only the +1 direction is
/// probed).
fn layer_edges(base: u32, pairs: &[(u32, u32)]) -> Vec<RouteEdge> {
    let mut edges = Vec::new();
    for (i, &(ix, iy)) in pairs.iter().enumerate() {
        if let Ok(j) = pairs.binary_search(&(ix + 1, iy)) {
            edges.push(RouteEdge {
                a: base + i as u32,
                b: base + j as u32,
                axis: Axis::X,
            });
        }
        if let Some(&(nx, ny)) = pairs.get(i + 1)
            && nx == ix
            && ny == iy + 1
        {
            edges.push(RouteEdge {
                a: base + i as u32,
                b: base + i as u32 + 1,
                axis: Axis::Y,
            });
        }
    }
    edges
}

/// Via edges between two layers' covered pair sets: for each lower
/// node `(ix,iy)` the crossing's (x,y) must binary-search-hit both
/// upper track lists and land on an upper covered pair — the strict
/// shared-crossing rule of `docs/e6-drt.md` §6. Each lower node emits
/// ≤1 edge and distinct (x,y) map to distinct upper nodes, so ≤1 edge
/// exists per coordinate per pair.
fn layer_via_edges(
    lower_base: u32,
    lower_pairs: &[(u32, u32)],
    lower_tracks: &LayerTracks,
    upper_base: u32,
    upper_pairs: &[(u32, u32)],
    upper_tracks: &LayerTracks,
) -> Vec<ViaEdge> {
    let mut out = Vec::new();
    for (i, &(ix, iy)) in lower_pairs.iter().enumerate() {
        let x = lower_tracks.tracks_x[ix as usize];
        let y = lower_tracks.tracks_y[iy as usize];
        let Ok(jx) = upper_tracks.tracks_x.binary_search(&x) else {
            continue;
        };
        let Ok(jy) = upper_tracks.tracks_y.binary_search(&y) else {
            continue;
        };
        if let Ok(j) = upper_pairs.binary_search(&(jx as u32, jy as u32)) {
            out.push(ViaEdge {
                lower: lower_base + i as u32,
                upper: upper_base + j as u32,
            });
        }
    }
    out
}

/// Builds every guided net's region graph, pin attachments, and
/// layer-transition via edges. `tracks` is `drt::tracks::layer_tracks`
/// output, parallel to `tech.routing_layers`. Emission order is fixed
/// (layer asc → (ix,iy) asc → +x/+y edges in node order; via edges
/// pair asc → lower node order): two runs are identical.
///
/// Via-edge emission is purely geometric — a shared crossing plus
/// guide coverage on both layers — so no `ViaTable` is consulted: a
/// pair with zero candidates is `drt::via`'s `ViaGap` finding, and
/// whatever edges the geometry still yields are the data explaining
/// it. The maze router joins table and graph for legality/pricing.
pub fn build_route_graphs(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
) -> RouteGraphs {
    build_route_graphs_ext(
        db,
        tech,
        tracks,
        join,
        GraphOpts {
            spare_above: 0,
            escape: Some(EscapeCfg::DEFAULT),
        },
    )
}

/// `build_route_graphs` plus `opts`: `spare_above` extra routing
/// layers per net, covered on every guide box's XY (see
/// `net_region`) — the maze router passes 1, detailed routing may
/// need one layer above the global route's choice to reach
/// OBS-sealed pin pockets — and the bounded pin-halo escape
/// (`escape_extend`).
pub fn build_route_graphs_ext(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
) -> RouteGraphs {
    let mut out = RouteGraphs {
        via_edges_per_pair: vec![0; tech.routing_layers.len().saturating_sub(1)],
        hull_masked: vec![0; tech.routing_layers.len()],
        ..Default::default()
    };
    let hull = HullMask::new(db, tech);
    let mut hull_scratch = Vec::new();
    for (net, boxes) in &join.nets {
        let (mut region, box_layers) = net_region(boxes, tracks, opts.spare_above);
        // Instance-hull mask: on masked layers a node inside a cell
        // bbox survives only inside the net's own pin rects on that
        // cell — foreign transit there would paint over subcell
        // internals the occupancy model cannot see (`drc(full)`
        // finding class). Attachment is unaffected: a pin's own
        // nodes sit inside its cover.
        let cover = pin_rects(db, tech, *net);
        for (&l, pairs) in region.iter_mut() {
            if !hull.masked.get(l as usize).copied().unwrap_or(false) {
                continue;
            }
            let lt = &tracks[l as usize];
            let before = pairs.len();
            pairs.retain(|&(ix, iy)| {
                hull.point_allowed(
                    &cover,
                    &mut hull_scratch,
                    l,
                    lt.tracks_x[ix as usize],
                    lt.tracks_y[iy as usize],
                )
            });
            out.hull_masked[l as usize] += before - pairs.len();
        }
        for &l in &box_layers {
            if region.get(&l).is_none_or(|v| v.is_empty()) {
                out.zero_coverage.push(ZeroCoverage {
                    net: *net,
                    layer: LayerId(l),
                });
            }
        }

        // Transit-layer hull coverage: the lowest unmasked layer may
        // legally paint metal anywhere over a cell, but the guide's
        // box can stop at a host hull's edge — the pin pocket then
        // has no node for its escape via to land on (gcd `_037_`:
        // the met2 box starts at 75900 DBU while the nearest met2
        // track inside the driver's hull sits at 75670). Extend the
        // layer's region to each host hull's track grid plus an
        // L-corridor to the nearest pre-existing pair — `layer_edges`
        // only links index-adjacent nodes, so a gap would leave the
        // extension an island. Past `zero_coverage` so a guide hole
        // still reports.
        let first_unmasked = (0..tech.routing_layers.len())
            .find(|&l| !hull.masked.get(l).copied().unwrap_or(false))
            .map(|l| l as u8);
        if let Some(l) = first_unmasked
            && !cover.is_empty()
        {
            transit_extend(
                region.entry(l).or_default(),
                &tracks[l as usize],
                db,
                tech,
                &cover,
            );
        }

        // Pin-halo escape: for each strict-escape pin (no in-region
        // node under its metal) add the bounded halo + corridor on
        // its own stack — pre-emission, so nodes, edges, via edges,
        // and the maze-side probes see escaped pairs unchanged.
        let escaped = escape_extend(
            db,
            tech,
            tracks,
            &mut region,
            &cover,
            &hull,
            &mut hull_scratch,
            first_unmasked,
            *net,
            opts.escape,
            &mut out.escapes,
        );

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        // layer → first node id — the attachment probes reuse `region`
        // under the same sorted order node ids were assigned.
        let mut layer_base: BTreeMap<u8, u32> = BTreeMap::new();
        for (&l, pairs) in &region {
            layer_base.insert(l, nodes.len() as u32);
            let base = layer_base[&l];
            nodes.extend(pairs.iter().map(|&(ix, iy)| RouteNode {
                layer: LayerId(l),
                ix,
                iy,
            }));
            let mut es = layer_edges(base, pairs);
            if hull.masked.get(l as usize).copied().unwrap_or(false) {
                // Node filtering keeps endpoints inside the pin rects,
                // but an edge between two rects of one cell still
                // paints the hull-internal strip between them —
                // `seg_allowed` is the committed-paint check.
                let lt = &tracks[l as usize];
                es.retain(|e| {
                    let (ia, ib) = ((e.a - base) as usize, (e.b - base) as usize);
                    let (ax, ay) = (
                        lt.tracks_x[pairs[ia].0 as usize],
                        lt.tracks_y[pairs[ia].1 as usize],
                    );
                    let (bx, by) = (
                        lt.tracks_x[pairs[ib].0 as usize],
                        lt.tracks_y[pairs[ib].1 as usize],
                    );
                    hull.seg_allowed(&cover, l, ax, ay, bx, by)
                });
            }
            edges.extend(es);
        }

        let mut via_edges = Vec::new();
        for (&l, pairs_lo) in &region {
            let u = l + 1;
            let Some(pairs_hi) = region.get(&u) else {
                continue;
            };
            let (Some(lt_lo), Some(lt_hi)) =
                (tracks.get(l as usize), tracks.get(u as usize))
            else {
                continue;
            };
            let pair_edges = layer_via_edges(
                layer_base[&l],
                pairs_lo,
                lt_lo,
                layer_base[&u],
                pairs_hi,
                lt_hi,
            );
            if let Some(slot) = out.via_edges_per_pair.get_mut(l as usize) {
                *slot += pair_edges.len();
            }
            via_edges.extend(pair_edges);
        }

        let mut pin_nodes = Vec::new();
        for &pin in db.net_pins(*net) {
            let mut attached: BTreeSet<u32> = BTreeSet::new();
            for (lid, rect) in pin_shapes(db, tech, pin) {
                let (Some(&base), Some(pairs), Some(lt)) = (
                    layer_base.get(&lid.0),
                    region.get(&lid.0),
                    tracks.get(lid.0 as usize),
                ) else {
                    continue;
                };
                for k in strict_hits(pairs, lt, &rect) {
                    attached.insert(base + k);
                }
            }
            // `escaped` is the pre-halo escape set — recorded even
            // when the halo later re-attached the pin, so the flag
            // keeps meaning "pin metal outside the guide region".
            if escaped.contains(&pin) {
                out.region_escape.push(RegionEscape { pin, net: *net });
            }
            pin_nodes.push((pin, attached.into_iter().collect()));
        }

        out.total_nodes += nodes.len();
        out.total_edges += edges.len();
        out.total_via_edges += via_edges.len();
        out.nets.push(NetGraph {
            net: *net,
            nodes,
            edges,
            via_edges,
            pin_nodes,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::PlacementStatus;
    use crate::interner::{CellId, MacroId, MacroPinId, PinDir};
    use crate::tech_db::{LayerDir, MacroDef, MacroPin, RectDbu, RoutingLayer};

    fn lt() -> LayerTracks {
        LayerTracks {
            tracks_x: vec![0, 100, 200, 300],
            tracks_y: vec![0, 100, 200, 300],
        }
    }

    fn gb(layer: u8, xl: i32, yl: i32, xh: i32, yh: i32) -> GuideBox {
        GuideBox {
            layer: LayerId(layer),
            rect: RectDbu::new(xl, yl, xh, yh),
        }
    }

    /// Region mapping: box-edge boundaries are inclusive, off-grid
    /// boxes contribute nothing, and disjoint boxes spawn no corner
    /// nodes.
    #[test]
    fn test_net_region() {
        let tracks = [lt()];
        let (region, layers) = net_region(&[gb(0, 50, 50, 250, 150)], &tracks, 0);
        assert_eq!(region[&0], vec![(1, 1), (2, 1)]);
        assert_eq!(layers, BTreeSet::from([0]));

        // Two diagonal boxes: the union is per-box coverage, so the
        // off-diagonal corners (0,2)/(2,0)-class nodes never appear.
        let (region, _) = net_region(&[gb(0, 0, 0, 100, 100), gb(0, 200, 200, 300, 300)], &tracks, 0);
        assert_eq!(
            region[&0],
            vec![
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
                (2, 2),
                (2, 3),
                (3, 2),
                (3, 3)
            ]
        );

        // A box that misses every track contributes no entry.
        let (region, layers) = net_region(&[gb(0, 500, 500, 600, 600)], &tracks, 0);
        assert!(region.is_empty());
        assert_eq!(layers, BTreeSet::from([0]));
    }

    /// Induced-subgraph edges: an L-shaped set emits one X and one Y
    /// edge, and a one-index gap in a row wires nothing across it.
    #[test]
    fn test_layer_edges() {
        let edges = layer_edges(10, &[(0, 0), (0, 1), (1, 0)]);
        assert_eq!(
            edges,
            vec![
                RouteEdge {
                    a: 10,
                    b: 12,
                    axis: Axis::X
                },
                RouteEdge {
                    a: 10,
                    b: 11,
                    axis: Axis::Y
                },
            ]
        );
        assert!(layer_edges(0, &[(0, 0), (2, 0)]).is_empty());
    }

    /// Via-edge emission on two hand-built pair sets: a lower node at
    /// a shared crossing covered on both layers gets an edge; a lower
    /// node off the upper grid or uncovered on the upper side gets
    /// none; `lower < upper` always.
    #[test]
    fn test_layer_via_edges() {
        let lo_tracks = LayerTracks {
            tracks_x: vec![0, 100, 200],
            tracks_y: vec![0, 100, 200],
        };
        // Upper grid shares (0,0) and (200,200) coordinates only.
        let hi_tracks = LayerTracks {
            tracks_x: vec![0, 200],
            tracks_y: vec![0, 200],
        };
        let lo_pairs = vec![(0, 0), (0, 1), (1, 1), (2, 2)];
        // Upper covers (0,0) but not (200,200).
        let hi_pairs = vec![(0, 0), (0, 1)];

        let edges = layer_via_edges(10, &lo_pairs, &lo_tracks, 20, &hi_pairs, &hi_tracks);
        assert_eq!(edges, vec![ViaEdge { lower: 10, upper: 20 }]);
        assert!(edges[0].lower < edges[0].upper);

        // Both shared crossings covered → two edges, lower node order.
        let hi_full = vec![(0, 0), (1, 1)];
        let edges = layer_via_edges(10, &lo_pairs, &lo_tracks, 20, &hi_full, &hi_tracks);
        assert_eq!(
            edges,
            vec![
                ViaEdge { lower: 10, upper: 20 },
                ViaEdge { lower: 13, upper: 21 },
            ]
        );
    }

    /// Join fixture: `guided` (two signal instance pins) is the only
    /// net with a guide block; `n_miss` (two signal pins), `n_pwr`
    /// (two power members), and `n_one` (single pin) are absent.
    fn join_fixture() -> (TechDb, DesignDb) {
        let mut tech = TechDb::new();
        let sym = tech.strings.intern("met1");
        tech.routing_layers.push(RoutingLayer {
            name: sym,
            dir: LayerDir::Horizontal,
            pitch_dbu: 100,
            width_dbu: 50,
            offset_dbu: 0,
            pitch_x_dbu: 100,
            pitch_y_dbu: 100,
            offset_x_dbu: 0,
            offset_y_dbu: 0,
            tracks_x: Vec::new(),
            tracks_y: Vec::new(),
            res_per_sq: 0.0,
            cap_per_sq_um: 0.0,
            edge_cap_per_um: 0.0,
            spacing_dbu: 0,
            spacing_table: Vec::new(),
            area_dbu2: 0,
            min_enclosed_area_dbu2: 0,
        });
        let mac_sym = tech.strings.intern("cell");
        tech.macros.push(MacroDef {
            name: mac_sym,
            class: crate::tech_db::MacroClass::Core,
            width_dbu: 100,
            height_dbu: 100,
            pin_offset: 0,
            pin_count: 2,
            obs_rects: Vec::new(),
            obs_rects_raw: Vec::new(),
            pin_obs_rects: Vec::new(),
        });
        for (pname, power) in [("SIG", false), ("PWR", true)] {
            tech.macro_pins.push(MacroPin {
                name: tech.strings.intern(pname),
                dir: PinDir::Input,
                is_power: power,
                offset_x_dbu: 0,
                offset_y_dbu: 0,
                offset_x_frac: false,
                offset_y_frac: false,
                layer: Some(LayerId(0)),
                rects: Vec::new(),
            });
        }
        tech.rebuild_maps();

        let mut db = DesignDb::new();
        db.cell_names.push(db.strings.intern("u0"));
        db.cell_macros.push(MacroId(0));
        db.cell_lib_ids.push(None);
        db.cell_status.push(PlacementStatus::Placed);
        db.cell_x_dbu.push(0);
        db.cell_y_dbu.push(0);
        db.cell_orient.push(crate::design_db::Orient::N);
        db.cell_pin_offsets = vec![0, 6];
        db.cell_pin_ids = (0..6).map(PinId).collect();

        // pins 0-3: signal instance pins; 4-5: power instance pins.
        for i in 0..6 {
            db.pin_names.push(db.strings.intern("p"));
            db.pin_dirs.push(PinDir::Input);
            db.pin_cells.push(Some(CellId(0)));
            db.pin_macro_pins
                .push(Some(MacroPinId(if i < 4 { 0 } else { 1 })));
            db.pin_lib_pins.push(None);
            db.pin_nets.push(None);
            db.pin_offset_x_dbu.push(0);
            db.pin_offset_y_dbu.push(0);
            db.pin_io_layer.push(None);
            db.pin_io_rects.push(Vec::new());
            db.pin_is_driver.push(false);
        }

        for name in ["guided", "n_miss", "n_pwr", "n_one"] {
            db.net_names.push(db.strings.intern(name));
        }
        db.net_pin_offsets = vec![0, 2, 4, 6, 7];
        db.net_pin_ids = vec![
            PinId(0),
            PinId(1),
            PinId(2),
            PinId(3),
            PinId(4),
            PinId(5),
            PinId(0),
        ];
        (tech, db)
    }

    /// Absent nets classify by membership: `n_miss` is a named gap,
    /// `n_pwr`/`n_one` expected absences; an unknown guide name bails.
    #[test]
    fn test_join_classification() {
        let (tech, db) = join_fixture();
        let parsed = vec![("guided".to_string(), vec![gb(0, 0, 0, 100, 100)])];
        let join = join_guides(&db, &tech, &parsed).unwrap();
        assert_eq!(join.nets.len(), 1);
        assert_eq!(join.nets[0].0, NetId(0));
        assert_eq!(join.missing_signal, vec![NetId(1)]);
        assert_eq!(join.absent_expected, vec![NetId(2), NetId(3)]);

        let err = join_guides(&db, &tech, &[("no_such_net".to_string(), Vec::new())]).unwrap_err();
        assert!(err.to_string().contains("no_such_net"));
    }
}
