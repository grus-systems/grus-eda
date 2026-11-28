//! Root-cause decomposition of the final unrouted population —
//! `kurena/tasks/drt-unrouted-analysis/design/probe.md`. For every
//! `route_db.unrouted` net the probe rebuilds `premark_obs` occupancy
//! minus io-port `Pin` marks (a fresh occupancy has no `Net` marks,
//! and the PDN/`Obs`/`ObsPin` walls stay), rebuilds the net's
//! `NetGraph`/`NetTopo`/`NetAdj` against it, and re-runs one
//! `route_one_net` attempt. The per-pin delta between that outcome
//! and the real outcome — plus the already-collected `blockers`,
//! `stub_outside_cover`, `region_escape`, and guide-union censuses —
//! lands each failed pin in exactly one `RootCause` class. The probe
//! never commits: its occupancy is dropped with the call and the
//! routed output is byte-identical by construction (the route path
//! never calls this module).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use rayon::prelude::*;

use super::access::pin_shapes;
use super::graph::{
    GraphOpts, GuideJoin, GuideUnion, HullMask, NetGraph, PinCover, build_net_graph, guide_union,
};
use super::maze::{
    FailKind, NetAdj, NetOutcome, NetTopo, OccMode, Occupancy, RouteDb, UnroutedNet, covered_range,
    fill_blocked, premark_marks, route_one_net, scan_stub,
};
use super::pool::drt_pool;
use super::tracks::LayerTracks;
use super::via::ViaTable;
use crate::design_db::DesignDb;
use crate::interner::{NetId, PinId};
use crate::tech_db::TechDb;

/// The mutually-exclusive root-cause class of one finally unrouted
/// pin (`taxonomy.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootCause {
    // geometric — fails with foreign `Net`/`Pin` marks absent
    /// No pin shape holds a covered track on either axis — the
    /// `project_attach` empty-window case.
    GeomOffTrack,
    /// Covered tracks exist but zero in-region nodes sit in the ±1
    /// projection window (the pin carries a `region_escape` record).
    GeomNoInRegion,
    /// Probe `Unattachable` via `stub_blocked`, or every rescan
    /// candidate's stub refused by `Obs`/`ObsPin`.
    GeomObsSealed,
    /// The ≤met2 guide union only rejoins at met3+ — the named
    /// edge-domain residual.
    GeomMet3Bridge,
    /// A connected guide union enclosing a fixed pocket — `NoPath`
    /// under both occupancies.
    GeomPocket,
    /// The guide union is fully disjoint at every layer bound —
    /// residual-named ("not possible on real GRT output"), counted
    /// loudly rather than folded into `GeomPocket`.
    GeomDisjoint,
    // negotiation — routes with foreign `Net`/`Pin` marks absent
    /// Real `Unattachable`, probe `Connected`, pin ∈
    /// `stub_outside_cover` — extraction-stage refusal.
    NegAttachStub,
    /// Real `Unattachable`, probe `Connected`, pin ∉
    /// `stub_outside_cover` — topo-stage refusal by a foreign io-port
    /// `Pin` band (the only foreign mark the probe drops that could
    /// refuse at premark time).
    NegAttachPort,
    /// Real `NoPath`, probe `Connected`, with attributable
    /// `(layer, Net|Pin)` wave-death evidence.
    NegWave,
    /// Same delta, all attributable blockers `Obs`/`ObsPin` — the
    /// mark drop was not the mechanism; freed capacity elsewhere in
    /// the pocket was.
    NegWaveObsFrontier,
    /// Same delta, `u.blockers` empty — uninstrumented divergence.
    NegWaveUnattributed,
    // residual named classes — defect signals, never bucketed
    /// A `u.failed` pin absent from the rebuilt graph's `pin_nodes`
    /// — join/graph invariant break.
    ResidualMissingPin,
    /// Probe `Unattachable` whose evidence rescan admits a
    /// candidate — classifier/evidence divergence.
    ResidualAttachInconsistent,
}

impl RootCause {
    /// Negotiation classes — the pin routes once foreign `Net`/`Pin`
    /// marks are gone.
    pub fn is_negotiation(&self) -> bool {
        matches!(
            self,
            Self::NegAttachStub
                | Self::NegAttachPort
                | Self::NegWave
                | Self::NegWaveObsFrontier
                | Self::NegWaveUnattributed
        )
    }

    /// Geometric classes — the pin fails even with foreign `Net`/`Pin`
    /// marks absent.
    pub fn is_geometric(&self) -> bool {
        matches!(
            self,
            Self::GeomOffTrack
                | Self::GeomNoInRegion
                | Self::GeomObsSealed
                | Self::GeomMet3Bridge
                | Self::GeomPocket
                | Self::GeomDisjoint
        )
    }

    /// Stable token for the decomposition table / test prints.
    pub fn name(&self) -> &'static str {
        match self {
            Self::GeomOffTrack => "geom_off_track",
            Self::GeomNoInRegion => "geom_no_in_region",
            Self::GeomObsSealed => "geom_obs_sealed",
            Self::GeomMet3Bridge => "geom_met3_bridge",
            Self::GeomPocket => "geom_pocket",
            Self::GeomDisjoint => "geom_disjoint",
            Self::NegAttachStub => "neg_attach_stub",
            Self::NegAttachPort => "neg_attach_port",
            Self::NegWave => "neg_wave",
            Self::NegWaveObsFrontier => "neg_wave_obs_frontier",
            Self::NegWaveUnattributed => "neg_wave_unattributed",
            Self::ResidualMissingPin => "residual_missing_pin",
            Self::ResidualAttachInconsistent => "residual_attach_inconsistent",
        }
    }

    /// All classes — `class_counts` carries every key even at 0 so
    /// residual-zero assertions stay nameless.
    pub const ALL: [Self; 13] = [
        Self::GeomOffTrack,
        Self::GeomNoInRegion,
        Self::GeomObsSealed,
        Self::GeomMet3Bridge,
        Self::GeomPocket,
        Self::GeomDisjoint,
        Self::NegAttachStub,
        Self::NegAttachPort,
        Self::NegWave,
        Self::NegWaveObsFrontier,
        Self::NegWaveUnattributed,
        Self::ResidualMissingPin,
        Self::ResidualAttachInconsistent,
    ];
}

/// One classified `u.failed` entry, in `u.failed` order.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbePin {
    pub pin: PinId,
    /// The real outcome's fail kind (`u.failed` arm).
    pub real_kind: FailKind,
    pub cause: RootCause,
    /// `region_escape` provenance flag (orthogonal to `cause`).
    pub region_escape: bool,
    /// Real `Unattachable` whose probe state is `NoPath` — foreign
    /// marks did block the real attach, but the pin would not have
    /// routed anyway.
    pub probe_attached: bool,
    /// Per-pin wave-blocker evidence when `u.blockers` pairs 1:1 with
    /// the `NoPath` subsequence (`taxonomy.md` §4); `None` means
    /// "net-level set only".
    pub wave_evidence: Option<(u8, u8)>,
}

/// One unrouted net's probe verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeNet {
    pub net: NetId,
    /// The probe re-attempt's outcome for the whole net.
    pub probe_routed: bool,
    /// Guide-union class: `Bridged` (`!conn(≤2) && conn(all)`),
    /// `Met2Connected`, `Disjoint`, `GuideAbsent` (defensive).
    pub union: GuideUnion,
    /// Probe-side wave-death evidence (`u2.blockers`,
    /// `(layer, kind)` — kinds ∈ {Obs, ObsPin} only).
    pub probe_blockers: Vec<(u8, u8)>,
    /// Same length and order as `u.failed`.
    pub pins: Vec<ProbePin>,
}

/// The full decomposition — `nets` mirrors `route_db.unrouted`
/// order; `ProbeReport` is `Send`/`Sync`-safe (no references).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeReport {
    /// `route_db.unrouted` order.
    pub nets: Vec<ProbeNet>,
    /// Io-pin-only nets absent from the guide — the `unguided`
    /// golden class (`probe.md` §5).
    pub unguided: Vec<NetId>,
    /// Wall time of the whole probe call.
    pub elapsed: Duration,
}

impl ProbeReport {
    /// class → pin count, all 13 classes present (zero kept — a
    /// named class at 0 is itself evidence).
    pub fn class_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts: BTreeMap<&'static str, usize> =
            RootCause::ALL.iter().map(|c| (c.name(), 0)).collect();
        for pn in &self.nets {
            for pp in &pn.pins {
                *counts.get_mut(pp.cause.name()).unwrap() += 1;
            }
        }
        counts
    }

    /// class → distinct-net count (nets carrying ≥1 pin of the
    /// class).
    pub fn net_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts: BTreeMap<&'static str, usize> =
            RootCause::ALL.iter().map(|c| (c.name(), 0)).collect();
        for pn in &self.nets {
            for name in pn
                .pins
                .iter()
                .map(|p| p.cause.name())
                .collect::<BTreeSet<_>>()
            {
                *counts.get_mut(name).unwrap() += 1;
            }
        }
        counts
    }
}

/// Occupancy state of `pin` in the probe re-attempt — the
/// classification input the taxonomy's cascade reads.
#[derive(Clone, Copy)]
enum ProbeState {
    /// Probe `Routed`, or `pin ∈ u2.connected`.
    Connected,
    /// `pin ∈ u2.failed` — the kind it failed under the probe.
    Failed(FailKind),
    /// `pin ∉ g.pin_nodes` — the graph built without it.
    MissingPin,
}

/// The `project_attach` rescan breakdown for one pin under probe
/// occupancy (`design/probe.md` §3) — the evidence split between
/// `geom_off_track`, `geom_no_in_region`, and `geom_obs_sealed` when
/// the net's re-attempt still reports `Unattachable`.
struct AttachEvidence {
    /// Any pin shape holds a covered track on some layer
    /// (`x0 < x1 || y0 < y1` under `covered_range`).
    covered_tracks: bool,
    /// In-region nodes inside the ±1 window on a covered line whose
    /// stub passes `seg_allowed` — `project_attach` candidates.
    candidates: usize,
    /// Candidates whose `scan_stub` carries a `fill_blocked` hit.
    refused: usize,
}

/// `project_attach`'s candidate enumeration plus the `scan_stub`
/// verdict it does not record — read-only against probe occupancy.
/// Replicates its windows and `seg_allowed` gate so the refused set
/// is exactly the candidate set minus the admitted set.
#[allow(clippy::too_many_arguments)]
fn attach_evidence(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    occ: &Occupancy,
    mask: &HullMask,
    cover: &PinCover,
    pin: PinId,
) -> AttachEvidence {
    let mut ev = AttachEvidence {
        covered_tracks: false,
        candidates: 0,
        refused: 0,
    };
    let mut hits = Vec::new();
    for (lid, r) in pin_shapes(db, tech, pin) {
        let Some(lt) = tracks.get(lid.0 as usize) else {
            continue;
        };
        let (x0, x1) = covered_range(&lt.tracks_x, r.xl, r.xh);
        let (y0, y1) = covered_range(&lt.tracks_y, r.yl, r.yh);
        if x0 >= x1 && y0 >= y1 {
            continue;
        }
        ev.covered_tracks = true;
        let xs = x0.saturating_sub(1)..(x1 + 1).min(lt.tracks_x.len());
        let ys = y0.saturating_sub(1)..(y1 + 1).min(lt.tracks_y.len());
        // Nodes sort by (layer, ix, iy) — the same `layer_base`
        // convention `access_vias` uses.
        let lo = g
            .nodes
            .partition_point(|n| (n.layer.0, n.ix, n.iy) < (lid.0, 0, 0));
        let hi = g
            .nodes
            .partition_point(|n| (n.layer.0, n.ix, n.iy) < (lid.0 + 1, 0, 0));
        for id in lo..hi {
            let node = &g.nodes[id];
            let (ix, iy) = (node.ix as usize, node.iy as usize);
            if !xs.contains(&ix)
                || !ys.contains(&iy)
                || !((x0..x1).contains(&ix) || (y0..y1).contains(&iy))
            {
                continue;
            }
            let (nx, ny) = (lt.tracks_x[ix], lt.tracks_y[iy]);
            let (cx, cy) = (nx.clamp(r.xl, r.xh), ny.clamp(r.yl, r.yh));
            if !mask.seg_allowed(cover, lid.0, nx, ny, cx, cy) {
                continue;
            }
            ev.candidates += 1;
            hits.clear();
            scan_stub(g, tracks, occ, cover, id as u32, lid.0, cx, cy, &mut hits);
            if hits.iter().any(|h| fill_blocked(h, g.net.0)) {
                ev.refused += 1;
            }
        }
    }
    ev
}

/// Rebuilds `premark_obs` occupancy minus io-port `Pin` marks and
/// re-attempts every `route_db.unrouted` net against it — the
/// root-cause probe. `opts` must match the options the route ran
/// under (the same graphs are rebuilt); `threads` sizes the probe's
/// rayon pool (`0` = rayon default).
#[allow(clippy::too_many_arguments)]
pub fn probe_unrouted(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    vias: &ViaTable,
    route_db: &RouteDb,
    threads: usize,
) -> ProbeReport {
    let t0 = Instant::now();
    let hull = HullMask::new(db, tech);
    let mut occ = Occupancy::new(tech, tracks);
    premark_marks(db, tech, &mut occ, false);

    let net_idx: BTreeMap<u32, usize> = join
        .nets
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (n.0, i))
        .collect();
    // `absent_expected` ∩ single-member ∩ io-pin-only — the golden
    // verifier's own rule (`check_drt_ours.sh` step-E):
    // `( PIN <port> )`-only conns the guide never covered.
    let unguided: Vec<NetId> = join
        .absent_expected
        .iter()
        .copied()
        .filter(|n| {
            let np = db.net_pins(*n);
            np.len() == 1 && db.pin_cells[np[0].0 as usize].is_none()
        })
        .collect();

    let stub_outside: BTreeSet<(NetId, PinId)> =
        route_db.stub_outside_cover.iter().copied().collect();
    let nets: Vec<ProbeNet> = drt_pool(threads).install(|| {
        route_db
            .unrouted
            .par_iter()
            .map_init(Vec::new, |scratch, u| {
                probe_net(
                    db,
                    tech,
                    tracks,
                    join,
                    opts,
                    vias,
                    &occ,
                    &hull,
                    &net_idx,
                    &stub_outside,
                    u,
                    scratch,
                )
            })
            .collect()
    });

    ProbeReport {
        nets,
        unguided,
        elapsed: t0.elapsed(),
    }
}

/// One unrouted net under the probe occupancy — graph rebuild,
/// topo/adj rebuild, one `route_one_net`, then per-pin
/// classification in `u.failed` order.
#[allow(clippy::too_many_arguments)]
fn probe_net(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    vias: &ViaTable,
    occ: &Occupancy,
    hull: &HullMask,
    net_idx: &BTreeMap<u32, usize>,
    stub_outside: &BTreeSet<(NetId, PinId)>,
    u: &UnroutedNet,
    scratch: &mut Vec<u32>,
) -> ProbeNet {
    let i = *net_idx.get(&u.net.0).expect("unrouted net must be guided");
    let (net, boxes) = &join.nets[i];
    let nb = build_net_graph(db, tech, tracks, hull, *net, boxes, opts, scratch);
    let g = &nb.graph;
    let ptopo = NetTopo::new(db, tech, tracks, g, vias, occ, hull);
    let padj = NetAdj::new(g, &ptopo, tech, tracks, vias, hull);
    // `hard=false, mode=Enabled` — no `Net`/`Pin` marks exist in the
    // probe occupancy, so `hit_verdict`'s `blocked` can never set;
    // `Obs`/`foreign ObsPin` block in every mode (the geometric
    // walls). `hist=0` — negotiation pricing is moot with no
    // negotiable marks.
    let out = route_one_net(
        db,
        tech,
        tracks,
        g,
        &ptopo,
        &padj,
        vias,
        occ,
        hull,
        0,
        false,
        OccMode::Enabled,
    );

    let pin_index: BTreeMap<PinId, usize> = g
        .pin_nodes
        .iter()
        .enumerate()
        .map(|(pi, &(p, _))| (p, pi))
        .collect();
    let (probe_routed, connected_set, failed_map, stub_set, probe_blockers) = match &out {
        NetOutcome::Routed(..) => (
            true,
            BTreeSet::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            Vec::new(),
        ),
        NetOutcome::Failed {
            unrouted: u2,
            stub_blocked,
        } => (
            false,
            u2.connected.iter().copied().collect::<BTreeSet<_>>(),
            u2.failed
                .iter()
                .copied()
                .collect::<BTreeMap<PinId, FailKind>>(),
            stub_blocked.iter().copied().collect::<BTreeSet<_>>(),
            u2.blockers.clone(),
        ),
    };

    let union = guide_union(boxes);
    // `blockers` pairs 1:1 with `NoPath` failed pins in `u.failed`
    // order — the wave-death census pushes at most one entry per
    // such pin; a length mismatch degrades to the unpaired
    // (net-level) evidence read (`taxonomy.md` §4).
    let nopath = u
        .failed
        .iter()
        .filter(|(_, k)| *k == FailKind::NoPath)
        .count();
    let paired = nopath > 0 && u.blockers.len() == nopath;
    let mut wave_rank = 0usize;
    let pins = u
        .failed
        .iter()
        .map(|&(pin, real_kind)| {
            let wave_evidence = if real_kind == FailKind::NoPath {
                let e = paired.then(|| u.blockers[wave_rank]);
                wave_rank += 1;
                e
            } else {
                None
            };
            let state = if !pin_index.contains_key(&pin) {
                ProbeState::MissingPin
            } else if probe_routed || connected_set.contains(&pin) {
                ProbeState::Connected
            } else if let Some(&k) = failed_map.get(&pin) {
                ProbeState::Failed(k)
            } else {
                ProbeState::MissingPin
            };
            let probe_attached =
                real_kind == FailKind::Unattachable
                    && matches!(state, ProbeState::Failed(FailKind::NoPath));
            let cause = match state {
                ProbeState::MissingPin => RootCause::ResidualMissingPin,
                ProbeState::Connected => match real_kind {
                    FailKind::Unattachable => {
                        if stub_outside.contains(&(u.net, pin)) {
                            RootCause::NegAttachStub
                        } else {
                            RootCause::NegAttachPort
                        }
                    }
                    FailKind::NoPath => {
                        // Paired evidence reads this pin's own census
                        // slot; unpaired falls back to the net-level
                        // set — either way kinds 2/3 are `Net`/`Pin`.
                        let ev = wave_evidence
                            .as_ref()
                            .map(std::slice::from_ref)
                            .unwrap_or(&u.blockers);
                        if ev.iter().any(|&(_, k)| k == 2 || k == 3) {
                            RootCause::NegWave
                        } else if !ev.is_empty() {
                            RootCause::NegWaveObsFrontier
                        } else {
                            RootCause::NegWaveUnattributed
                        }
                    }
                },
                ProbeState::Failed(FailKind::NoPath) => match union {
                    GuideUnion::Bridged => RootCause::GeomMet3Bridge,
                    GuideUnion::Met2Connected => RootCause::GeomPocket,
                    GuideUnion::Disjoint | GuideUnion::GuideAbsent => RootCause::GeomDisjoint,
                },
                ProbeState::Failed(FailKind::Unattachable) => {
                    if stub_set.contains(&pin) {
                        RootCause::GeomObsSealed
                    } else {
                        let ev =
                            attach_evidence(db, tech, tracks, g, occ, hull, &ptopo.cover, pin);
                        if !ev.covered_tracks {
                            RootCause::GeomOffTrack
                        } else if ev.candidates == 0 {
                            RootCause::GeomNoInRegion
                        } else if ev.refused == ev.candidates {
                            RootCause::GeomObsSealed
                        } else {
                            RootCause::ResidualAttachInconsistent
                        }
                    }
                }
            };
            ProbePin {
                pin,
                real_kind,
                cause,
                region_escape: nb.region_escape.iter().any(|re| re.pin == pin),
                probe_attached,
                wave_evidence,
            }
        })
        .collect();

    ProbeNet {
        net: *net,
        probe_routed,
        union,
        probe_blockers,
        pins,
    }
}
