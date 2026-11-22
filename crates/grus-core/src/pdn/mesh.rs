//! Resistor network over the extracted PDN geometry (E8 ticket 3).
//!
//! `PdnGraph` answers connectivity — gcd merges each rail into one
//! component. `PdnMesh` is the finer node space the DC solver consumes:
//! feature intervals (rect ends, same-layer overlap spans, via landings,
//! pin-attach and source io-rect intersections) merge into junction
//! pieces, each merged piece set is a node, and the metal run between
//! adjacent nodes on one rect is a `RPERSQ × L/W` resistor. Via stacks
//! price through `TechDb::via_res_ohm`.

use super::{UnionFind, expand_conns, rect_contains_point, rects_intersect, try_build_graph};
use crate::design_db::{DesignDb, PdnNet, PdnUse};
use crate::interner::{LayerId, PinId, SymbolId};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdnMesh {
    /// Net name (indexes `DesignDb::strings`).
    pub net: SymbolId,
    pub nodes: Vec<MeshNode>,
    /// Undirected conductance edges, construction order: per-rect run
    /// order first, then vias in `PdnNet::vias` order.
    pub resistors: Vec<MeshResistor>,
    /// One per distinct attached `PinId` (`graph.attachments` set).
    pub sinks: Vec<MeshSink>,
    /// Node indices under the top-level PIN's io-rect intersections.
    pub sources: Vec<u32>,
    /// Sinks whose pin offered >1 candidate node (resolved to the lowest
    /// index). Recorded at build — a `PdnMesh` alone cannot re-derive the
    /// candidate sets, so `census()` reads the stored count.
    #[serde(default)]
    pub multi_node_sinks: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshNode {
    /// Single-layer by construction — even a dangling via end knows its
    /// layer from `via.lower`/`via.upper`.
    pub layer: LayerId,
    /// Junction-piece bboxes on this layer (empty for dangling via ends).
    pub rects: Vec<RectDbu>,
    pub is_source: bool,
    pub is_sink: bool,
    /// Via endpoint that landed on no rect (mirrors `PdnNode::via_end`).
    pub dangling: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshResistor {
    pub a: u32,
    pub b: u32,
    pub r_ohms: f32,
    pub kind: ResistorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResistorKind {
    /// Run along `PdnNet::wire_rects[idx]` between two junction nodes.
    Wire { rect: u32 },
    /// Stack link of `PdnNet::vias[idx]`.
    Via { via: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshSink {
    pub pin: PinId,
    pub node: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshCensus {
    pub net: SymbolId,
    pub nodes: u32,
    pub source_nodes: u32,
    pub wire_resistors: u32,
    pub via_resistors: u32,
    pub sinks: u32,
    /// Sinks whose pin offered >1 candidate node.
    pub multi_node_sinks: u32,
    /// Via endpoints that landed on no rect.
    pub dangling_via_ends: u32,
    /// Nodes with no resistor at all.
    pub isolated_nodes: u32,
    pub min_r_ohms: f32,
    pub max_r_ohms: f32,
    pub violations: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MeshViolation {
    NonFiniteResistance { resistor: u32 },
    NonPositiveResistance { resistor: u32, r_ohms: f32 },
    DanglingSink { pin: PinId },
    DanglingSource { node: u32 },
    NoSources,
    UnreachableNode { node: u32 },
}

/// One junction piece: a merged feature interval on a wire rect's major
/// axis, spanning the rect's full minor extent.
struct Piece {
    /// Index into `PdnNet::wire_rects`.
    rect: u32,
    dbu: RectDbu,
}

/// Build the resistor mesh for one `PdnNet`. `Err` on conn-expansion
/// failure (forwarded from `try_build_graph`) or when `TechDb::via_res_ohm`
/// has no resistance for a via's cut layer — an unpriceable via cannot be
/// modelled.
pub fn build_mesh(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> Result<PdnMesh> {
    let graph = try_build_graph(db, tech_db, net)?;
    // `PdnGraph` keeps source *nodes*, not pins — the io-rect scan below
    // needs the `TopPin` ids again.
    let (_, source_pins) = expand_conns(db, net)?;
    let n_rects = net.wire_rects.len();

    let major_x: Vec<bool> = net
        .wire_rects
        .iter()
        .map(|&(l, r)| major_axis(tech_db, l, &r))
        .collect();

    // Feature intervals per rect, along its major axis.
    let mut features: Vec<Vec<(i32, i32)>> = vec![Vec::new(); n_rects];
    for (i, &(_, r)) in net.wire_rects.iter().enumerate() {
        let (lo, hi) = major_span(major_x[i], &r);
        features[i].push((lo, lo));
        features[i].push((hi, hi));
    }

    let mut by_layer: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    for (i, &(l, _)) in net.wire_rects.iter().enumerate() {
        by_layer.entry(l.0).or_default().push(i as u32);
    }

    // Same-layer overlap spans — the same sorted-x sweep `build_graph`
    // runs. Each recorded pair later unions the two pieces covering the
    // span; piece strict-overlap implies rect strict-overlap, so these
    // pairs capture every piece-level union a full piece sweep would.
    let mut overlap_pairs: Vec<(u32, u32, RectDbu)> = Vec::new();
    let mut order: Vec<u32> = (0..n_rects as u32).collect();
    order.sort_by_key(|&i| {
        let (l, r) = net.wire_rects[i as usize];
        (l.0, r.xl)
    });
    let mut active: Vec<u32> = Vec::new();
    let mut active_layer = None;
    for &i in &order {
        let (l, r) = net.wire_rects[i as usize];
        if active_layer != Some(l) {
            active.clear();
            active_layer = Some(l);
        }
        active.retain(|&j| net.wire_rects[j as usize].1.xh > r.xl);
        for &j in &active {
            let a = &net.wire_rects[j as usize].1;
            if a.yl < r.yh && r.yl < a.yh {
                let inter = intersect(a, &r);
                overlap_pairs.push((j, i, inter));
                features[i as usize].push(major_span(major_x[i as usize], &inter));
                features[j as usize].push(major_span(major_x[j as usize], &inter));
            }
        }
        active.push(i);
    }

    // Via landing points (closed containment — the cut itself is not
    // modelled).
    for via in &net.vias {
        for layer in [via.lower, via.upper] {
            if let Some(rects) = by_layer.get(&layer.0) {
                for &ri in rects {
                    if rect_contains_point(&net.wire_rects[ri as usize].1, via.x_dbu, via.y_dbu)
                    {
                        let c = if major_x[ri as usize] {
                            via.x_dbu
                        } else {
                            via.y_dbu
                        };
                        features[ri as usize].push((c, c));
                    }
                }
            }
        }
    }

    // The sink set is the ticket-2 attach set verbatim: distinct pins of
    // `graph.attachments` in first-appearance order. Each pin's port ∩
    // wire intersections (the same strict-overlap test the graph runs)
    // add feature intervals and later resolve the sink's node.
    let mut seen = BTreeSet::new();
    let attached_pins: Vec<PinId> = graph
        .attachments
        .iter()
        .map(|&(p, _)| p)
        .filter(|p| seen.insert(p.0))
        .collect();
    let mut pin_hits: Vec<Vec<(u32, RectDbu)>> = Vec::with_capacity(attached_pins.len());
    for &p in &attached_pins {
        let cell = db.pin_cells[p.0 as usize].unwrap();
        let c = cell.0 as usize;
        let mac = &tech_db.macros[db.cell_macros[c].0 as usize];
        let mp = &tech_db.macro_pins[db.pin_macro_pins[p.0 as usize].unwrap().0 as usize];
        let (cx, cy, orient) = (db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_orient[c]);
        let mut hits = Vec::new();
        for &(l, r) in &mp.rects {
            let oriented = DesignDb::orient_rect(r, mac.width_dbu, mac.height_dbu, orient);
            let abs = RectDbu::new(
                cx + oriented.xl,
                cy + oriented.yl,
                cx + oriented.xh,
                cy + oriented.yh,
            );
            if let Some(rects) = by_layer.get(&l.0) {
                for &ri in rects {
                    let w = &net.wire_rects[ri as usize].1;
                    if rects_intersect(&abs, w) {
                        let inter = intersect(&abs, w);
                        hits.push((ri, inter));
                        features[ri as usize].push(major_span(major_x[ri as usize], &inter));
                    }
                }
            }
        }
        pin_hits.push(hits);
    }

    // Source io-rect ∩ wire intersections — every node under one is a
    // source node (a PIN rect may touch several disjoint nodes).
    let mut src_hits: Vec<Vec<(u32, RectDbu)>> = Vec::with_capacity(source_pins.len());
    for &sp in &source_pins {
        let mut hits = Vec::new();
        for &(l, r) in &db.pin_io_rects[sp.0 as usize] {
            if let Some(rects) = by_layer.get(&l.0) {
                for &ri in rects {
                    let w = &net.wire_rects[ri as usize].1;
                    if rects_intersect(&r, w) {
                        let inter = intersect(&r, w);
                        hits.push((ri, inter));
                        features[ri as usize].push(major_span(major_x[ri as usize], &inter));
                    }
                }
            }
        }
        src_hits.push(hits);
    }

    // Closed-union the feature intervals per rect into sorted disjoint
    // junction pieces (a point on an interval's edge merges into it).
    let mut pieces: Vec<Piece> = Vec::new();
    let mut rect_pieces: Vec<Vec<u32>> = vec![Vec::new(); n_rects];
    for (i, feats) in features.iter_mut().enumerate() {
        feats.sort_unstable();
        let (_, r) = net.wire_rects[i];
        let mut k = 0;
        while k < feats.len() {
            let lo = feats[k].0;
            let mut hi = feats[k].1;
            k += 1;
            while k < feats.len() && feats[k].0 <= hi {
                hi = hi.max(feats[k].1);
                k += 1;
            }
            rect_pieces[i].push(pieces.len() as u32);
            pieces.push(Piece {
                rect: i as u32,
                dbu: piece_rect(major_x[i], &r, lo, hi),
            });
        }
    }

    // Nodes: union the pieces covering each recorded overlap span, then
    // one MeshNode per union component in piece order.
    let mut uf = UnionFind::new(pieces.len());
    for &(ra, rb, ref inter) in &overlap_pairs {
        let pa = containing_piece(&rect_pieces, &pieces, &major_x, ra, inter);
        let pb = containing_piece(&rect_pieces, &pieces, &major_x, rb, inter);
        if let (Some(pa), Some(pb)) = (pa, pb) {
            uf.union(pa, pb);
        }
    }
    let mut root_to_node: BTreeMap<u32, u32> = BTreeMap::new();
    let mut piece_node = vec![u32::MAX; pieces.len()];
    let mut nodes: Vec<MeshNode> = Vec::new();
    for i in 0..pieces.len() as u32 {
        let root = uf.find(i);
        let ni = *root_to_node.entry(root).or_insert_with(|| {
            nodes.push(MeshNode {
                layer: net.wire_rects[pieces[i as usize].rect as usize].0,
                rects: Vec::new(),
                is_source: false,
                is_sink: false,
                dangling: false,
            });
            (nodes.len() - 1) as u32
        });
        piece_node[i as usize] = ni;
        nodes[ni as usize].rects.push(pieces[i as usize].dbu);
    }

    // Wire resistors: consecutive pieces along each rect's major axis,
    // center-to-center `L` (junction metal is real — each piece's
    // half-length belongs to the adjacent run), `R = RPERSQ × L / W`.
    let mut resistors: Vec<MeshResistor> = Vec::new();
    for (i, plist) in rect_pieces.iter().enumerate() {
        let (l, r) = net.wire_rects[i];
        let res_per_sq = tech_db.routing_layers[l.0 as usize].res_per_sq;
        let w = minor_extent(major_x[i], &r) as f32;
        for pair in plist.windows(2) {
            let (plo, phi) = major_span(major_x[i], &pieces[pair[0] as usize].dbu);
            let (qlo, qhi) = major_span(major_x[i], &pieces[pair[1] as usize].dbu);
            let len =
                (qlo as i64 + qhi as i64 - plo as i64 - phi as i64).abs() as f32 * 0.5;
            resistors.push(MeshResistor {
                a: piece_node[pair[0] as usize],
                b: piece_node[pair[1] as usize],
                r_ohms: res_per_sq * len / w,
                kind: ResistorKind::Wire { rect: i as u32 },
            });
        }
    }

    // Via resistors: one per `PdnVia`, priced through `via_res_ohm`. An
    // endpoint on no rect becomes a degenerate dangling node — modelled,
    // then flagged by `validate`, not silently dropped.
    let map = PieceMap {
        net,
        by_layer: &by_layer,
        rect_pieces: &rect_pieces,
        pieces: &pieces,
        piece_node: &piece_node,
        major_x: &major_x,
    };
    for (vi, via) in net.vias.iter().enumerate() {
        let r_ohms = tech_db.via_res_ohm(via.cut, via.cut_count).with_context(|| {
            format!(
                "PDN net {} via #{} ({}): cut layer {} carries no RESISTANCE",
                db.strings.resolve(net.name),
                vi,
                db.strings.resolve(via.name),
                db.strings.resolve(via.cut)
            )
        })?;
        let mut end = |layer: LayerId| -> u32 {
            match map.node_at(layer, via.x_dbu, via.y_dbu) {
                Some(n) => n,
                None => {
                    nodes.push(MeshNode {
                        layer,
                        rects: Vec::new(),
                        is_source: false,
                        is_sink: false,
                        dangling: true,
                    });
                    (nodes.len() - 1) as u32
                }
            }
        };
        let (a, b) = (end(via.lower), end(via.upper));
        resistors.push(MeshResistor {
            a,
            b,
            r_ohms,
            kind: ResistorKind::Via { via: vi as u32 },
        });
    }

    // Sinks: lowest-index junction node under each attached pin's
    // intersections; pins offering >1 candidate are counted.
    let mut sinks = Vec::with_capacity(attached_pins.len());
    let mut multi_node_sinks = 0u32;
    for (k, &p) in attached_pins.iter().enumerate() {
        let mut cand: BTreeSet<u32> = BTreeSet::new();
        for &(ri, ref inter) in &pin_hits[k] {
            map.nodes_under(ri, inter, &mut cand);
        }
        // Non-empty by construction: the pin is in the graph attach set.
        let node = *cand
            .iter()
            .next()
            .expect("attached pin under no junction node");
        multi_node_sinks += (cand.len() > 1) as u32;
        sinks.push(MeshSink { pin: p, node });
        nodes[node as usize].is_sink = true;
    }

    // Sources: every junction node under a source PIN's io-rect
    // intersections.
    let mut source_set: BTreeSet<u32> = BTreeSet::new();
    for hits in &src_hits {
        for &(ri, ref inter) in hits {
            map.nodes_under(ri, inter, &mut source_set);
        }
    }
    let sources: Vec<u32> = source_set.iter().copied().collect();
    for &n in &sources {
        nodes[n as usize].is_source = true;
    }

    Ok(PdnMesh {
        net: net.name,
        nodes,
        resistors,
        sinks,
        sources,
        multi_node_sinks,
    })
}

/// Build + census; violations are in `census.violations` and
/// `mesh.validate()` — construction itself does not gate on them.
pub fn check_mesh(
    db: &DesignDb,
    tech_db: &TechDb,
    net: &PdnNet,
) -> Result<(PdnMesh, MeshCensus)> {
    let mesh = build_mesh(db, tech_db, net)?;
    let census = mesh.census();
    Ok((mesh, census))
}

/// Every `+ USE POWER`/`GROUND` net's `(PdnMesh, MeshCensus)` — same net
/// filter as `pdn::census`.
pub fn meshes(db: &DesignDb, tech_db: &TechDb) -> Vec<(PdnMesh, MeshCensus)> {
    db.pdn_nets
        .iter()
        .filter(|n| matches!(n.use_kind, Some(PdnUse::Power) | Some(PdnUse::Ground)))
        .map(|n| check_mesh(db, tech_db, n).expect("PDN mesh build failed"))
        .collect()
}

impl PdnMesh {
    /// Finite+positive R, sink/source index resolution, and source
    /// reachability (BFS over resistor adjacency). Dangling via ends are
    /// reported `UnreachableNode` too — a via endpoint on no metal is a
    /// defect even though its stub stays resistor-linked to the mesh.
    /// Empty on a good mesh.
    pub fn validate(&self) -> Vec<MeshViolation> {
        let n = self.nodes.len() as u32;
        let mut v = Vec::new();
        for (i, r) in self.resistors.iter().enumerate() {
            if !r.r_ohms.is_finite() {
                v.push(MeshViolation::NonFiniteResistance {
                    resistor: i as u32,
                });
            } else if r.r_ohms <= 0.0 {
                v.push(MeshViolation::NonPositiveResistance {
                    resistor: i as u32,
                    r_ohms: r.r_ohms,
                });
            }
        }
        for s in &self.sinks {
            if s.node >= n {
                v.push(MeshViolation::DanglingSink { pin: s.pin });
            }
        }
        for &s in &self.sources {
            if s >= n {
                v.push(MeshViolation::DanglingSource { node: s });
            }
        }
        if self.sources.is_empty() {
            v.push(MeshViolation::NoSources);
        }
        // BFS runs only with ≥1 source — with none, every node would be
        // "unreachable" and `NoSources` already carries the verdict.
        let mut unreachable: BTreeSet<u32> = BTreeSet::new();
        if !self.sources.is_empty() {
            let mut adj: Vec<Vec<u32>> = vec![Vec::new(); n as usize];
            for r in &self.resistors {
                if r.a < n && r.b < n {
                    adj[r.a as usize].push(r.b);
                    adj[r.b as usize].push(r.a);
                }
            }
            let mut seen = vec![false; n as usize];
            let mut stack: Vec<u32> = self
                .sources
                .iter()
                .copied()
                .filter(|&s| s < n)
                .collect();
            while let Some(x) = stack.pop() {
                if seen[x as usize] {
                    continue;
                }
                seen[x as usize] = true;
                stack.extend_from_slice(&adj[x as usize]);
            }
            unreachable.extend((0..n).filter(|&i| !seen[i as usize]));
        }
        unreachable.extend((0..n).filter(|&i| self.nodes[i as usize].dangling));
        v.extend(
            unreachable
                .iter()
                .map(|&node| MeshViolation::UnreachableNode { node }),
        );
        v
    }

    /// Node/resistor/sink/source census for `docs/results.md` and tests;
    /// `violations` counts `validate()`.
    pub fn census(&self) -> MeshCensus {
        let mut wire = 0u32;
        let mut via = 0u32;
        let mut min_r = f32::INFINITY;
        let mut max_r = 0.0f32;
        let mut degree = vec![0u32; self.nodes.len()];
        for r in &self.resistors {
            match r.kind {
                ResistorKind::Wire { .. } => wire += 1,
                ResistorKind::Via { .. } => via += 1,
            }
            min_r = min_r.min(r.r_ohms);
            max_r = max_r.max(r.r_ohms);
            if (r.a as usize) < degree.len() {
                degree[r.a as usize] += 1;
            }
            if (r.b as usize) < degree.len() {
                degree[r.b as usize] += 1;
            }
        }
        MeshCensus {
            net: self.net,
            nodes: self.nodes.len() as u32,
            source_nodes: self.sources.len() as u32,
            wire_resistors: wire,
            via_resistors: via,
            sinks: self.sinks.len() as u32,
            multi_node_sinks: self.multi_node_sinks,
            dangling_via_ends: self.nodes.iter().filter(|n| n.dangling).count() as u32,
            isolated_nodes: degree.iter().filter(|&&d| d == 0).count() as u32,
            min_r_ohms: if self.resistors.is_empty() {
                0.0
            } else {
                min_r
            },
            max_r_ohms: max_r,
            violations: self.validate().len() as u32,
        }
    }
}

/// Major axis of a wire rect: the longer extent; square ties use the
/// layer's preferred routing direction.
fn major_axis(tech_db: &TechDb, layer: LayerId, r: &RectDbu) -> bool {
    match r.width().cmp(&r.height()) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            tech_db.routing_layers[layer.0 as usize].dir == LayerDir::Horizontal
        }
    }
}

fn major_span(major_x: bool, r: &RectDbu) -> (i32, i32) {
    if major_x { (r.xl, r.xh) } else { (r.yl, r.yh) }
}

fn minor_extent(major_x: bool, r: &RectDbu) -> i32 {
    if major_x { r.height() } else { r.width() }
}

fn piece_rect(major_x: bool, r: &RectDbu, lo: i32, hi: i32) -> RectDbu {
    if major_x {
        RectDbu::new(lo, r.yl, hi, r.yh)
    } else {
        RectDbu::new(r.xl, lo, r.xh, hi)
    }
}

fn intersect(a: &RectDbu, b: &RectDbu) -> RectDbu {
    RectDbu::new(
        a.xl.max(b.xl),
        a.yl.max(b.yl),
        a.xh.min(b.xh),
        a.yh.min(b.yh),
    )
}

/// The piece of `rect` whose major-axis interval contains `inter`'s lower
/// bound — the piece covering the rect pair's overlap span.
fn containing_piece(
    rect_pieces: &[Vec<u32>],
    pieces: &[Piece],
    major_x: &[bool],
    rect: u32,
    inter: &RectDbu,
) -> Option<u32> {
    let c = if major_x[rect as usize] {
        inter.xl
    } else {
        inter.yl
    };
    rect_pieces[rect as usize]
        .iter()
        .copied()
        .find(|&pi| {
            let (lo, hi) = major_span(major_x[rect as usize], &pieces[pi as usize].dbu);
            lo <= c && c <= hi
        })
}

/// Piece-level lookup tables shared by the via-endpoint, sink and source
/// resolution passes.
struct PieceMap<'a> {
    net: &'a PdnNet,
    by_layer: &'a BTreeMap<u8, Vec<u32>>,
    rect_pieces: &'a [Vec<u32>],
    pieces: &'a [Piece],
    piece_node: &'a [u32],
    major_x: &'a [bool],
}

impl PieceMap<'_> {
    /// Lowest-index junction node whose piece contains `(x, y)` on `layer`
    /// (closed containment — pieces are disjoint, so at most one per rect;
    /// the min is the spec'd tie-break across rects).
    fn node_at(&self, layer: LayerId, x: i32, y: i32) -> Option<u32> {
        let mut best: Option<u32> = None;
        for &ri in self.by_layer.get(&layer.0)? {
            if !rect_contains_point(&self.net.wire_rects[ri as usize].1, x, y) {
                continue;
            }
            let c = if self.major_x[ri as usize] { x } else { y };
            if let Some(&pi) = self.rect_pieces[ri as usize].iter().find(|&&pi| {
                let (lo, hi) =
                    major_span(self.major_x[ri as usize], &self.pieces[pi as usize].dbu);
                lo <= c && c <= hi
            }) {
                let n = self.piece_node[pi as usize];
                best = Some(best.map_or(n, |b| b.min(n)));
            }
        }
        best
    }

    /// Junction nodes whose pieces strictly intersect `inter` — at most
    /// one piece per `(rect, intersection)` pair by construction (the
    /// feature interval landed inside exactly one merged piece).
    fn nodes_under(&self, ri: u32, inter: &RectDbu, out: &mut BTreeSet<u32>) {
        for &pi in &self.rect_pieces[ri as usize] {
            if rects_intersect(&self.pieces[pi as usize].dbu, inter) {
                out.insert(self.piece_node[pi as usize]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{PDN_DEF, PDN_LEF, load_design, pdn_design, pdn_design_from};
    use super::super::build_graph;
    use super::*;
    use crate::test_support::{design_dir, require};

    /// met1 rail, met2 stripe, one `stack_a` via: 7 junction nodes,
    /// 5 wire runs, 1 via resistor, 2 sinks, 1 source.
    #[test]
    fn test_synthetic_mesh_shape() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let (mesh, c) = check_mesh(&db, &tech, net).unwrap();
        println!("synthetic mesh census: {c:?}");
        assert_eq!(c.nodes, 7);
        assert_eq!(c.wire_resistors, 5);
        assert_eq!(c.via_resistors, 1);
        assert_eq!(c.sinks, 2);
        assert_eq!(c.source_nodes, 1);
        assert_eq!(c.dangling_via_ends, 0);
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
    }

    /// Per-element run arithmetic: `R = res_per_sq × L / W` with
    /// center-to-center `L` — met1 runs L = 500/400/1000, met2 L =
    /// 1800/200, W = 100 on both rects.
    #[test]
    fn test_synthetic_wire_r_arithmetic() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        let met1_r = tech.routing_layers[0].res_per_sq;
        let met2_r = tech.routing_layers[1].res_per_sq;
        let expect: Vec<(u32, f32)> = vec![
            (0, met1_r * 500.0 / 100.0),
            (0, met1_r * 400.0 / 100.0),
            (0, met1_r * 1000.0 / 100.0),
            (1, met2_r * 1800.0 / 100.0),
            (1, met2_r * 200.0 / 100.0),
        ];
        let wires: Vec<(u32, f32)> = mesh
            .resistors
            .iter()
            .filter_map(|r| match r.kind {
                ResistorKind::Wire { rect } => Some((rect, r.r_ohms)),
                _ => None,
            })
            .collect();
        assert_eq!(wires, expect);
    }

    /// `stack_a` resolves its R through the LEF `VIA` block (via12
    /// RESISTANCE 15.0 over 1 cut RECT) — the DEF/LEF name carries
    /// nothing. Respelling it in both files must give the same resistor.
    #[test]
    fn test_synthetic_via_r_from_techdb() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        let via = mesh
            .resistors
            .iter()
            .find(|r| matches!(r.kind, ResistorKind::Via { .. }))
            .unwrap();
        assert_eq!(via.r_ohms, 15.0);
        // Endpoints: the met1 point piece at x = 1000, the met2 point
        // piece at y = 0.
        let (a, b) = (via.a as usize, via.b as usize);
        assert_eq!(mesh.nodes[a].rects.as_slice(), &[RectDbu::new(1000, -50, 1000, 50)]);
        assert_eq!(mesh.nodes[b].rects.as_slice(), &[RectDbu::new(950, 0, 1050, 0)]);

        let (tech2, db2) = pdn_design_from(
            &PDN_LEF.replace("stack_a", "zzz_noname"),
            &PDN_DEF.replace("stack_a", "zzz_noname"),
        );
        let net2 = db2.pdn_net("VPWR").unwrap();
        let mesh2 = build_mesh(&db2, &tech2, net2).unwrap();
        let via2 = mesh2
            .resistors
            .iter()
            .find(|r| matches!(r.kind, ResistorKind::Via { .. }))
            .unwrap();
        assert_eq!(via2.r_ohms, 15.0, "renamed via must price identically");
    }

    /// A via endpoint on no rect becomes a degenerate `dangling` node and
    /// surfaces as `UnreachableNode`; the stranded rail side reports too.
    #[test]
    fn test_synthetic_dangling_via() {
        let (tech, db) = pdn_design();
        let mut net = db.pdn_net("VPWR").unwrap().clone();
        net.vias[0].y_dbu = 1000; // off the met1 rail (y ∈ [-50, 50]), on the stripe
        let mesh = build_mesh(&db, &tech, &net).unwrap();
        let dangling: Vec<u32> = mesh
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.dangling)
            .map(|(i, _)| i as u32)
            .collect();
        assert_eq!(dangling.len(), 1);
        assert_eq!(mesh.nodes[dangling[0] as usize].layer, LayerId(0));
        let v = mesh.validate();
        assert!(
            v.contains(&MeshViolation::UnreachableNode {
                node: dangling[0]
            }),
            "{v:?}"
        );
        // The met1 rail's three junction nodes lost their only stack link.
        let unreach = v
            .iter()
            .filter(|x| matches!(x, MeshViolation::UnreachableNode { .. }))
            .count();
        assert_eq!(unreach, 4, "{v:?}");
    }

    /// Fail-proof: zeroing met1's `RPERSQ` collapses its three runs to
    /// 0 Ω — `NonPositiveResistance` must fire on exactly those.
    #[test]
    fn test_failproof_zero_res_per_sq() {
        let (mut tech, db) = pdn_design();
        tech.routing_layers[0].res_per_sq = 0.0;
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        let v = mesh.validate();
        let nonpos: Vec<u32> = v
            .iter()
            .filter_map(|x| match x {
                MeshViolation::NonPositiveResistance { resistor, .. } => Some(*resistor),
                _ => None,
            })
            .collect();
        assert_eq!(nonpos.len(), 3, "{v:?}");
        assert!(
            nonpos
                .iter()
                .all(|&i| matches!(mesh.resistors[i as usize].kind, ResistorKind::Wire { rect: 0 }))
        );
    }

    /// Fail-proof: with no via edges the met1 rail is stranded from the
    /// met2 source — every rail node reports `UnreachableNode`.
    #[test]
    fn test_failproof_removed_vias() {
        let (tech, db) = pdn_design();
        let mut net = db.pdn_net("VPWR").unwrap().clone();
        net.vias.clear();
        let mesh = build_mesh(&db, &tech, &net).unwrap();
        let v = mesh.validate();
        let unreach: BTreeSet<u32> = v
            .iter()
            .filter_map(|x| match x {
                MeshViolation::UnreachableNode { node } => Some(*node),
                _ => None,
            })
            .collect();
        assert_eq!(unreach.len(), 3, "{v:?}");
        assert!(mesh.sinks.iter().all(|s| unreach.contains(&s.node)));
    }

    /// Golden census for gcd: sinks == the reference CSV row count, the
    /// sink pin set equals the ticket-2 attach set verbatim, and the mesh
    /// validates clean.
    #[test]
    fn test_gcd_mesh_census() {
        let (tech, db) = load_design("gcd", "gcd.placed.def");
        let ref_path = require(design_dir("gcd").join("gcd.irdrop.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).unwrap()).unwrap();

        let all = meshes(&db, &tech);
        assert_eq!(all.len(), 2, "VPWR + VGND");
        for (mesh, c) in &all {
            let name = db.strings.resolve(mesh.net);
            println!("gcd {name} mesh census: {c:?}");
            let nref = &ref_json["nets"][name];
            let csv_rows = nref["conventions"]["embedded"]["csv_rows"]
                .as_u64()
                .unwrap() as u32;
            assert_eq!(c.sinks, csv_rows, "{name} sinks vs CSV rows");
            let net = db.pdn_net(name).unwrap();
            assert_eq!(c.via_resistors, net.vias.len() as u32);
            assert!(c.source_nodes >= 1);
            let graph = build_graph(&db, &tech, net);
            let sink_pins: BTreeSet<u32> = mesh.sinks.iter().map(|s| s.pin.0).collect();
            let attach_pins: BTreeSet<u32> =
                graph.attachments.iter().map(|&(p, _)| p.0).collect();
            assert_eq!(sink_pins, attach_pins, "{name} sink set vs attach set");
            assert!(
                mesh.resistors
                    .iter()
                    .all(|r| r.r_ohms.is_finite() && r.r_ohms > 0.0),
                "{name} non-finite/non-positive R"
            );
            assert!(
                mesh.validate().is_empty(),
                "{name} violations: {:?}",
                mesh.validate()
            );
        }
    }

    /// Orientation coverage on the mesh path: `gcd.orient.def` exercises
    /// W/E/FW/FE inside pin-attach feature collection. Bounded claim, same
    /// as ticket 2: every sink resolves to a valid node, no DanglingSink.
    #[test]
    fn test_gcd_orient_fixture_mesh() {
        let (tech, db) = load_design("gcd", "gcd.orient.def");
        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let (mesh, c) = check_mesh(&db, &tech, net).unwrap();
            println!("gcd.orient {name} mesh census: {c:?}");
            assert!(
                mesh.sinks
                    .iter()
                    .all(|s| (s.node as usize) < mesh.nodes.len())
            );
            assert!(
                !mesh
                    .validate()
                    .iter()
                    .any(|v| matches!(v, MeshViolation::DanglingSink { .. })),
                "{name} dangling sinks"
            );
        }
    }

    /// picorv32 census: 10,205 sinks per rail per the reference CSV row
    /// counts. Scale check — counts are recorded, not gated.
    #[test]
    fn test_picorv32_mesh_census() {
        let (tech, db) = load_design("picorv32", "picorv32.placed.def");
        let ref_path = require(design_dir("picorv32").join("picorv32.irdrop.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).unwrap()).unwrap();

        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let t0 = std::time::Instant::now();
            let (mesh, c) = check_mesh(&db, &tech, net).unwrap();
            let dt = t0.elapsed();
            println!("picorv32 {name} mesh census: {c:?} (build+census {dt:?})");
            let csv_rows = ref_json["nets"][name]["conventions"]["embedded"]["csv_rows"]
                .as_u64()
                .unwrap() as u32;
            assert_eq!(c.sinks, csv_rows, "{name} sinks vs CSV rows");
            assert_eq!(c.via_resistors, net.vias.len() as u32);
            assert!(
                mesh.validate().is_empty(),
                "{name} violations: {:?}",
                mesh.validate()
            );
        }
    }
}
