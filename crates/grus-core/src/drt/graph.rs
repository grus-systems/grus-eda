//! Guide-bounded track graph: per guided net, the union of its guide
//! boxes clipped to the merged track grid becomes the set of covered
//! `(ix, iy)` track-index pairs — the nodes — with in-plane edges
//! between consecutive-index nodes on both axes, and per-pin
//! attachments from `drt::access::pin_shapes` candidate rects, plus
//! the layer-transition (via) edges between nodes at shared track
//! crossings. Costs and direction legality are later tickets. See
//! `docs/e6-drt.md` §5–§6 for the model and claim boundary.

use crate::congestion::guide::{GuideBox, resolve_guide_nets};
use crate::design_db::DesignDb;
use crate::drt::access::pin_shapes;
use crate::drt::tracks::{Axis, LayerTracks};
use crate::interner::{LayerId, NetId, PinId};
use crate::tech_db::TechDb;
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

/// All guided nets' graphs plus the cross-net census.
#[derive(Clone, Debug, Default)]
pub struct RouteGraphs {
    pub nets: Vec<NetGraph>, // guide-file order
    pub zero_coverage: Vec<ZeroCoverage>,
    pub region_escape: Vec<RegionEscape>,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_via_edges: usize,
    /// Via-edge count per adjacent pair — parallel to
    /// `tech.routing_layers.len() - 1` (index = lower layer). The
    /// census evidence; a pair pinned at 0 is the named edge-domain
    /// tech gap (`docs/e6-drt.md` §6, met2–met3 on sky130).
    pub via_edges_per_pair: Vec<usize>,
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
fn net_region(boxes: &[GuideBox], tracks: &[LayerTracks]) -> (RegionMap, BTreeSet<u8>) {
    let mut region: BTreeMap<u8, Vec<(u32, u32)>> = BTreeMap::new();
    let mut box_layers: BTreeSet<u8> = BTreeSet::new();
    for b in boxes {
        box_layers.insert(b.layer.0);
        let Some(lt) = tracks.get(b.layer.0 as usize) else {
            continue;
        };
        let (x0, x1) = covered_range(&lt.tracks_x, b.rect.xl, b.rect.xh);
        let (y0, y1) = covered_range(&lt.tracks_y, b.rect.yl, b.rect.yh);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let pairs = region.entry(b.layer.0).or_default();
        for ix in x0..x1 {
            for iy in y0..y1 {
                pairs.push((ix as u32, iy as u32));
            }
        }
    }
    for pairs in region.values_mut() {
        pairs.sort_unstable();
        pairs.dedup();
    }
    (region, box_layers)
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
    let mut out = RouteGraphs {
        via_edges_per_pair: vec![0; tech.routing_layers.len().saturating_sub(1)],
        ..Default::default()
    };
    for (net, boxes) in &join.nets {
        let (region, box_layers) = net_region(boxes, tracks);
        for &l in &box_layers {
            if region.get(&l).is_none_or(|v| v.is_empty()) {
                out.zero_coverage.push(ZeroCoverage {
                    net: *net,
                    layer: LayerId(l),
                });
            }
        }

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        // layer → first node id — the attachment probes reuse `region`
        // under the same sorted order node ids were assigned.
        let mut layer_base: BTreeMap<u8, u32> = BTreeMap::new();
        for (&l, pairs) in &region {
            layer_base.insert(l, nodes.len() as u32);
            nodes.extend(pairs.iter().map(|&(ix, iy)| RouteNode {
                layer: LayerId(l),
                ix,
                iy,
            }));
            edges.extend(layer_edges(layer_base[&l], pairs));
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
            out.via_edges_per_pair[l as usize] += pair_edges.len();
            via_edges.extend(pair_edges);
        }

        let mut pin_nodes = Vec::new();
        for &pin in db.net_pins(*net) {
            let mut attached: BTreeSet<u32> = BTreeSet::new();
            for (lid, rect) in pin_shapes(db, tech, pin) {
                let (Some(&base), Some(pairs)) = (layer_base.get(&lid.0), region.get(&lid.0))
                else {
                    continue;
                };
                let Some(lt) = tracks.get(lid.0 as usize) else {
                    continue;
                };
                let (x0, x1) = covered_range(&lt.tracks_x, rect.xl, rect.xh);
                let (y0, y1) = covered_range(&lt.tracks_y, rect.yl, rect.yh);
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                // A node inside the rect is an x-track ∩ y-track
                // crossing the pin's shape — the attachment the access
                // point designates (see docs/e6-drt.md §5 midline
                // caveat).
                let lo = pairs.partition_point(|&(ix, _)| ix < x0 as u32);
                for (k, &(ix, iy)) in pairs[lo..].iter().enumerate() {
                    if ix >= x1 as u32 {
                        break;
                    }
                    if iy >= y0 as u32 && iy < y1 as u32 {
                        attached.insert(base + (lo + k) as u32);
                    }
                }
            }
            if attached.is_empty() {
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
        let (region, layers) = net_region(&[gb(0, 50, 50, 250, 150)], &tracks);
        assert_eq!(region[&0], vec![(1, 1), (2, 1)]);
        assert_eq!(layers, BTreeSet::from([0]));

        // Two diagonal boxes: the union is per-box coverage, so the
        // off-diagonal corners (0,2)/(2,0)-class nodes never appear.
        let (region, _) = net_region(&[gb(0, 0, 0, 100, 100), gb(0, 200, 200, 300, 300)], &tracks);
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
        let (region, layers) = net_region(&[gb(0, 500, 500, 600, 600)], &tracks);
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
