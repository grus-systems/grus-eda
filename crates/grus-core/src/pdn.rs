//! Per-net PDN model: SPECIALNETS conn-group expansion, power-pin
//! attachment, and a `check_power_grid`-shaped connectivity self-check.
//!
//! Nodes are union components over same-layer wire rects (strict-overlap
//! intersection — the gcell convention) merged by via-stack edges. A pin
//! attaches where its oriented `MacroPin` port rects intersect wire rects
//! on the same layer; the net's top-level `PIN` io rects mark the source
//! components every attachable conn pin must reach.

use crate::design_db::{DesignDb, PlacementStatus, PdnConn, PdnNet, PdnUse};
use crate::interner::{CellId, LayerId, PinId, SymbolId};
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdnNode {
    /// `None` for degenerate via endpoints and multi-layer components.
    pub layer: Option<LayerId>,
    /// Indexes `PdnNet::wire_rects`.
    pub rect_indices: Vec<u32>,
    /// True when the node exists only as a via endpoint.
    pub via_end: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdnGraph {
    /// Connected components after via-edge merging.
    pub nodes: Vec<PdnNode>,
    /// `(PinId, node)` — the pin-attach table.
    pub attachments: Vec<(PinId, u32)>,
    /// Nodes touched by the top-level PIN's io rects.
    pub source_nodes: Vec<u32>,
    /// Conn groups expanded to pins (top-level PINs included).
    pub conn_pins: Vec<PinId>,
    /// Attachable conn pins reaching no wire, or only non-source nodes.
    pub unreachable_pins: Vec<PinId>,
    /// Attachable conn pins touching no wire at all (⊆ `unreachable_pins`).
    pub unattached_pins: Vec<PinId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdnCensus {
    pub net: SymbolId,
    pub use_kind: Option<PdnUse>,
    pub wire_rects: u32,
    pub vias: u32,
    pub nodes: u32,
    pub source_nodes: u32,
    pub conn_pins: u32,
    /// Conn pins with ≥1 routing-layer rect on a placed/fixed cell.
    pub attachable_pins: u32,
    /// Distinct attachable pins with ≥1 wire intersection — one CSV row per
    /// `(instance, terminal)` in PDNSim's voltage map.
    pub attached_terminals: u32,
    pub unattached_pins: u32,
    pub unreachable_pins: u32,
    /// `nodes.len() == 1` — the shape-level verdict `check_power_grid` gives.
    pub all_shapes_connected: bool,
}

struct UnionFind {
    parent: Vec<u32>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n as u32).collect(),
        }
    }

    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            self.parent[x as usize] = self.parent[self.parent[x as usize] as usize];
            x = self.parent[x as usize];
        }
        x
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb as usize] = ra;
        }
    }
}

/// Strict overlap — boundary-touching rects are *not* connected, matching
/// the codebase's geometric convention (`congestion::gcell`).
fn rects_intersect(a: &RectDbu, b: &RectDbu) -> bool {
    a.xl < b.xh && b.xl < a.xh && a.yl < b.yh && b.yl < a.yh
}

/// Closed containment — a via point on a rect's edge still lands on the
/// shape (the cut's own rect is not modelled).
fn rect_contains_point(r: &RectDbu, x: i32, y: i32) -> bool {
    r.xl <= x && x <= r.xh && r.yl <= y && y <= r.yh
}

/// Expands `PdnNet::conns` to design pins. Returns `(conn_pins, source_pins)`:
/// top-level `PIN` conns land in both. `AllInstances` wildcards that match
/// nothing are tolerated; a `TopPin` or `Instance` that misses is an error.
fn expand_conns(db: &DesignDb, net: &PdnNet) -> Result<(Vec<PinId>, Vec<PinId>)> {
    let net_name = db.strings.resolve(net.name);
    let mut conn_pins = Vec::new();
    let mut source_pins = Vec::new();
    for conn in &net.conns {
        match conn {
            PdnConn::TopPin(name) => {
                // `+ SPECIAL` bterms are not in `io_pins` (invariant I5), so
                // the scan is on `pin_cells`.
                let p = (0..db.num_pins())
                    .find(|&p| db.pin_cells[p].is_none() && db.pin_names[p] == *name)
                    .with_context(|| {
                        format!(
                            "SPECIALNETS net {net_name} connects to PIN {}, which PINS does not define",
                            db.strings.resolve(*name)
                        )
                    })?;
                let p = PinId(p as u32);
                source_pins.push(p);
                conn_pins.push(p);
            }
            PdnConn::AllInstances(pin) => {
                for c in 0..db.num_cells() {
                    for &p in db.cell_pins(CellId(c as u32)) {
                        if db.pin_names[p.0 as usize] == *pin {
                            conn_pins.push(p);
                        }
                    }
                }
            }
            PdnConn::Instance { inst, pin } => {
                let c = (0..db.num_cells())
                    .find(|&c| db.cell_names[c] == *inst)
                    .with_context(|| {
                        format!(
                            "SPECIALNETS net {net_name} connects to instance {}, which COMPONENTS does not define",
                            db.strings.resolve(*inst)
                        )
                    })?;
                let p = db
                    .cell_pins(CellId(c as u32))
                    .iter()
                    .copied()
                    .find(|&p| db.pin_names[p.0 as usize] == *pin)
                    .with_context(|| {
                        format!(
                            "SPECIALNETS net {net_name} connects to {}/{}, which the instance does not have",
                            db.strings.resolve(*inst),
                            db.strings.resolve(*pin)
                        )
                    })?;
                conn_pins.push(p);
            }
        }
    }
    Ok((conn_pins, source_pins))
}

/// A conn pin can attach iff its `MacroPin` carries routing-layer rects and
/// its cell is placed or fixed. `VNB`/`VPB` (well-layer ports) and pins on
/// unplaced cells are non-attachable by construction.
fn is_attachable(db: &DesignDb, tech_db: &TechDb, p: PinId) -> bool {
    let idx = p.0 as usize;
    let Some(cell) = db.pin_cells[idx] else {
        return false;
    };
    if !matches!(
        db.cell_status[cell.0 as usize],
        PlacementStatus::Placed | PlacementStatus::Fixed
    ) {
        return false;
    }
    match db.pin_macro_pins[idx] {
        Some(mp) => !tech_db.macro_pins[mp.0 as usize].rects.is_empty(),
        None => false,
    }
}

/// Build the per-net graph: conn expansion, same-layer union, via links,
/// pin attach, source marking. Panics on conn-expansion errors — use
/// [`try_build_graph`] where user input can miss.
pub fn build_graph(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> PdnGraph {
    try_build_graph(db, tech_db, net).expect("PDN conn expansion failed")
}

pub fn try_build_graph(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> Result<PdnGraph> {
    let (conn_pins, source_pins) = expand_conns(db, net)?;
    let n_rects = net.wire_rects.len();

    // Same-layer union: sorted-x sweep, strict `<` overlap on both axes.
    let mut uf = UnionFind::new(n_rects);
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
                uf.union(i, j);
            }
        }
        active.push(i);
    }

    // Same-layer components → proto-nodes.
    let mut comp_of_rect = vec![u32::MAX; n_rects];
    let mut root_to_comp: BTreeMap<u32, u32> = BTreeMap::new();
    let mut comp_layer: Vec<LayerId> = Vec::new();
    let mut comp_rects: Vec<Vec<u32>> = Vec::new();
    for i in 0..n_rects as u32 {
        let root = uf.find(i);
        let comp = *root_to_comp.entry(root).or_insert_with(|| {
            comp_layer.push(net.wire_rects[i as usize].0);
            comp_rects.push(Vec::new());
            (comp_rects.len() - 1) as u32
        });
        comp_of_rect[i as usize] = comp;
        comp_rects[comp as usize].push(i);
    }

    // Rects grouped by layer, for point/rect queries.
    let mut by_layer: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    for (i, (l, _)) in net.wire_rects.iter().enumerate() {
        by_layer.entry(l.0).or_default().push(i as u32);
    }
    let comp_at = |layer: LayerId, x: i32, y: i32| -> Option<u32> {
        by_layer.get(&layer.0)?.iter().find_map(|&i| {
            rect_contains_point(&net.wire_rects[i as usize].1, x, y)
                .then(|| comp_of_rect[i as usize])
        })
    };

    // Via edges union the components their two endpoints land in. An
    // endpoint outside every rect is a dangling via — an unconnected shape
    // that must stay its own component and flag `all_shapes_connected`.
    let mut proto_nodes: Vec<PdnNode> = comp_rects
        .into_iter()
        .enumerate()
        .map(|(c, rect_indices)| PdnNode {
            layer: Some(comp_layer[c]),
            rect_indices,
            via_end: false,
        })
        .collect();
    let mut nuf = UnionFind::new(proto_nodes.len() + 2 * net.vias.len());
    for via in &net.vias {
        let lo = comp_at(via.lower, via.x_dbu, via.y_dbu);
        let hi = comp_at(via.upper, via.x_dbu, via.y_dbu);
        match (lo, hi) {
            (Some(a), Some(b)) => nuf.union(a, b),
            _ => {
                if lo.is_none() {
                    proto_nodes.push(PdnNode {
                        layer: None,
                        rect_indices: Vec::new(),
                        via_end: true,
                    });
                }
                if hi.is_none() {
                    proto_nodes.push(PdnNode {
                        layer: None,
                        rect_indices: Vec::new(),
                        via_end: true,
                    });
                }
            }
        }
    }

    // Final components: merge proto-nodes by node-level union root.
    let mut root_to_node: BTreeMap<u32, u32> = BTreeMap::new();
    let mut proto_to_node = vec![u32::MAX; proto_nodes.len()];
    let mut nodes: Vec<PdnNode> = Vec::new();
    let mut node_layer: Vec<Option<LayerId>> = Vec::new();
    for p in 0..proto_nodes.len() as u32 {
        let root = nuf.find(p);
        let ni = *root_to_node.entry(root).or_insert_with(|| {
            nodes.push(PdnNode {
                layer: None,
                rect_indices: Vec::new(),
                via_end: false,
            });
            node_layer.push(proto_nodes[p as usize].layer);
            (nodes.len() - 1) as u32
        });
        proto_to_node[p as usize] = ni;
        let proto = &proto_nodes[p as usize];
        if node_layer[ni as usize] != proto.layer {
            node_layer[ni as usize] = None;
        }
        let node = &mut nodes[ni as usize];
        node.rect_indices.extend_from_slice(&proto.rect_indices);
        node.via_end |= proto.via_end;
    }
    for (n, &l) in nodes.iter_mut().zip(&node_layer) {
        n.layer = l;
    }

    // rect → final node.
    let mut rect_node = vec![u32::MAX; n_rects];
    for (i, &c) in comp_of_rect.iter().enumerate() {
        rect_node[i] = proto_to_node[c as usize];
    }

    // Source nodes: wire components touched by a top-level PIN's io rects.
    let mut source_set: BTreeSet<u32> = BTreeSet::new();
    for &sp in &source_pins {
        for &(l, r) in &db.pin_io_rects[sp.0 as usize] {
            if let Some(rects) = by_layer.get(&l.0) {
                for &ri in rects {
                    if rects_intersect(&r, &net.wire_rects[ri as usize].1) {
                        source_set.insert(rect_node[ri as usize]);
                    }
                }
            }
        }
    }
    let source_nodes: Vec<u32> = source_set.iter().copied().collect();

    // Attach instance power/ground pins: each routing-layer port rect,
    // oriented and translated by the cell's placement, intersected against
    // same-layer wire rects.
    let mut attachments: Vec<(PinId, u32)> = Vec::new();
    let mut unattached_pins: Vec<PinId> = Vec::new();
    let mut unreachable_pins: Vec<PinId> = Vec::new();
    for &p in &conn_pins {
        if !is_attachable(db, tech_db, p) {
            continue;
        }
        let cell = db.pin_cells[p.0 as usize].unwrap();
        let c = cell.0 as usize;
        let mac = &tech_db.macros[db.cell_macros[c].0 as usize];
        let mp = &tech_db.macro_pins[db.pin_macro_pins[p.0 as usize].unwrap().0 as usize];
        let (cx, cy, orient) = (db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_orient[c]);

        let mut pin_nodes: Vec<u32> = Vec::new();
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
                    if rects_intersect(&abs, &net.wire_rects[ri as usize].1) {
                        let node = rect_node[ri as usize];
                        if !pin_nodes.contains(&node) {
                            pin_nodes.push(node);
                            attachments.push((p, node));
                        }
                    }
                }
            }
        }

        if pin_nodes.is_empty() {
            unattached_pins.push(p);
            unreachable_pins.push(p);
        } else if !pin_nodes.iter().any(|n| source_set.contains(n)) {
            unreachable_pins.push(p);
        }
    }

    Ok(PdnGraph {
        nodes,
        attachments,
        source_nodes,
        conn_pins,
        unreachable_pins,
        unattached_pins,
    })
}

/// Connectivity verdict + census for one net.
pub fn check_net(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> (PdnGraph, PdnCensus) {
    let graph = build_graph(db, tech_db, net);
    let attachable = graph
        .conn_pins
        .iter()
        .filter(|&&p| is_attachable(db, tech_db, p))
        .count() as u32;
    let attached_terminals = {
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        graph
            .attachments
            .iter()
            .filter(|&&(p, _)| seen.insert(p.0))
            .count() as u32
    };
    let census = PdnCensus {
        net: net.name,
        use_kind: net.use_kind,
        wire_rects: net.wire_rects.len() as u32,
        vias: net.vias.len() as u32,
        nodes: graph.nodes.len() as u32,
        source_nodes: graph.source_nodes.len() as u32,
        conn_pins: graph.conn_pins.len() as u32,
        attachable_pins: attachable,
        attached_terminals,
        unattached_pins: graph.unattached_pins.len() as u32,
        unreachable_pins: graph.unreachable_pins.len() as u32,
        all_shapes_connected: graph.nodes.len() == 1,
    };
    (graph, census)
}

/// Every `+ USE POWER`/`GROUND` net's census.
pub fn census(db: &DesignDb, tech_db: &TechDb) -> Vec<PdnCensus> {
    db.pdn_nets
        .iter()
        .filter(|n| matches!(n.use_kind, Some(PdnUse::Power) | Some(PdnUse::Ground)))
        .map(|n| check_net(db, tech_db, n).1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::def::{parse_def, parse_def_bytes};
    use crate::readers::lef::{load_tech_db, parse_lef_bytes};
    use crate::test_support::{design_dir, require, sky130_hd};

    /// Two routing layers, one cut layer, a LEF `VIA` with a non-descriptive
    /// name, and a cell with a `met1` power pin.
    const PDN_LEF: &str = r#"
VERSION 5.8 ;
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
SITE unithd
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
END unithd
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
END met1
LAYER via12
  TYPE CUT ;
  RESISTANCE 15.0 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
VIA stack_a
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END stack_a
MACRO CELL
  CLASS CORE ;
  SIZE 1.0 BY 1.0 ;
  PIN VPWR
    DIRECTION INOUT ;
    USE POWER ;
    PORT
      LAYER met1 ;
        RECT 0.0 -0.05 0.2 0.05 ;
    END
  END VPWR
END CELL
END LIBRARY
"#;

    /// One met1 rail, one met2 stripe over the top-level pad, bridged by one
    /// `stack_a` via at (1000, 0). u1/u2 power pins touch the rail.
    const PDN_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN pdn ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 2000 2000 ) ;
ROW ROW_0 unithd 0 0 N DO 4 BY 1 STEP 460 2720 ;
TRACKS X 170 DO 100 STEP 340 LAYER met1 ;
COMPONENTS 2 ;
- u1 CELL + PLACED ( 0 0 ) N ;
- u2 CELL + PLACED ( 500 0 ) N ;
END COMPONENTS
PINS 1 ;
- VPWR + NET VPWR + SPECIAL + DIRECTION INOUT + USE POWER
  + PORT
    + LAYER met2 ( -50 -50 ) ( 50 50 )
    + PLACED ( 1000 1800 ) N ;
END PINS
VIAS 1 ;
- via12_inst + VIA stack_a ;
END VIAS
SPECIALNETS 1 ;
- VPWR ( PIN VPWR ) ( * VPWR ) + USE POWER
  + ROUTED met1 100 + SHAPE FOLLOWPIN ( 0 0 ) ( 2000 0 )
  NEW met2 100 + SHAPE STRIPE ( 1000 0 ) ( 1000 2000 )
  NEW met2 0 + SHAPE STRIPE ( 1000 0 ) via12_inst
  ;
END SPECIALNETS
END DESIGN
"#;

    fn pdn_design() -> (TechDb, DesignDb) {
        let mut interner = StringInterner::new();
        let mut tech = TechDb::new();
        parse_lef_bytes(PDN_LEF.as_bytes(), &mut tech, &mut interner).expect("LEF");
        tech.strings = interner.clone();
        tech.rebuild_maps();
        let db = parse_def_bytes(PDN_DEF.as_bytes(), &tech, None).expect("DEF");
        (tech, db)
    }

    fn load_design(design: &str, def_file: &str) -> (TechDb, DesignDb) {
        let lib_dir = sky130_hd();
        let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
        let def = require(design_dir(design).join(def_file));
        let mut interner = StringInterner::new();
        let tech = load_tech_db(&tlef, &lef, &mut interner).expect("LEF").0;
        let db = parse_def(&def, &tech, None).expect("DEF");
        (tech, db)
    }

    #[test]
    fn test_synthetic_pdn_connectivity() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let (graph, c) = check_net(&db, &tech, net);

        // rail + stripe + via = one component.
        assert_eq!(graph.nodes.len(), 1);
        assert!(c.all_shapes_connected);
        assert_eq!(c.source_nodes, 1);
        // The net expands to PIN + 2 instance pins; both instance pins attach.
        assert_eq!(c.conn_pins, 3);
        assert_eq!(c.attachable_pins, 2);
        assert_eq!(c.attached_terminals, 2);
        assert!(graph.unreachable_pins.is_empty());
        assert!(graph.unattached_pins.is_empty());
        assert_eq!(graph.attachments.len(), 2);
    }

    /// Fail-proof: the connectivity verdict must depend on the via edges —
    /// removing them must flip it, and a dangling via endpoint must leave a
    /// disconnected component.
    #[test]
    fn test_failproof_via_edges_carry_connectivity() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();

        // Baseline: all connected.
        let (_, c) = check_net(&db, &tech, net);
        assert!(c.all_shapes_connected && c.unreachable_pins == 0);

        // Remove every via edge: the rail (both pins) loses the source.
        let mut broken = net.clone();
        broken.vias.clear();
        let (graph, c) = check_net(&db, &tech, &broken);
        assert_eq!(graph.nodes.len(), 2, "rail and stripe must split");
        assert!(!c.all_shapes_connected);
        assert_eq!(c.unreachable_pins, 2);
        // The pins still touch the rail — they are unreachable, not
        // unattached.
        assert_eq!(c.unattached_pins, 0);

        // Move the via so its met1 endpoint lands off the rail: a dangling
        // via is an unconnected shape.
        let mut dangling = net.clone();
        dangling.vias[0].y_dbu = 1000; // rail occupies y ∈ [-50, 50]
        let (graph, c) = check_net(&db, &tech, &dangling);
        assert!(!c.all_shapes_connected);
        assert!(graph.nodes.iter().any(|n| n.via_end), "degenerate via node missing");
        assert_eq!(c.unreachable_pins, 2);
    }

    /// A pin under no wire must report unattached *and* unreachable.
    #[test]
    fn test_pin_under_no_wire_is_unattached() {
        let (tech, db) = pdn_design();
        let mut net = db.pdn_net("VPWR").unwrap().clone();
        // Shrink the rail's left edge past u1's pin (x ∈ [0, 200]) while the
        // via point at x = 1000 stays on it.
        net.wire_rects[0].1.xl = 300;
        let (graph, c) = check_net(&db, &tech, &net);
        assert_eq!(c.unattached_pins, 1);
        assert_eq!(c.unreachable_pins, 1);
        assert_eq!(graph.attachments.len(), 1);
        assert!(c.all_shapes_connected, "the wire itself stays connected");
    }

    /// Golden census for gcd: every instance power pin reaches the pad,
    /// matching `check_power_grid` in `gcd.irdrop.reference.json`, and the
    /// attached-terminal count equals the voltage CSV's row count.
    #[test]
    fn test_gcd_pdn_census_matches_reference() {
        let (tech, db) = load_design("gcd", "gcd.placed.def");
        let ref_path = require(design_dir("gcd").join("gcd.irdrop.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).unwrap()).unwrap();

        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let (graph, c) = check_net(&db, &tech, net);
            let nref = &ref_json["nets"][name];
            println!("gcd {name} census: {c:?}");

            assert_eq!(
                c.all_shapes_connected,
                nref["connectivity_result"].as_str().unwrap() == "1",
                "{name} connectivity disagrees with check_power_grid"
            );
            assert!(graph.unreachable_pins.is_empty(), "{name} unreachable");
            assert!(graph.unattached_pins.is_empty(), "{name} unattached");
            assert_eq!(c.nodes, 1, "{name} must be one component");
            assert_eq!(
                c.attached_terminals,
                nref["conventions"]["embedded"]["csv_rows"].as_u64().unwrap() as u32,
                "{name} attached terminals must equal embedded CSV rows"
            );
        }
    }

    /// Per-element golden check: deserialize the committed PDNSim voltage
    /// CSVs and compare — not just row counts. `(Instance, Terminal)` sets
    /// must equal our attach table's pin set, and each pin's `pin_pos_dbu`
    /// must sit within 1 DBU of the CSV's X/Y (µm * dbu_per_micron).
    /// (Project lesson 5: compare per element, not only the total.)
    #[test]
    fn test_gcd_attach_matches_embedded_csv() {
        let (tech, db) = load_design("gcd", "gcd.placed.def");
        let dbu = db.dbu_per_micron as f64;

        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let (graph, _) = check_net(&db, &tech, net);
            let csv_path = require(
                design_dir("gcd")
                    .join(format!("gcd.irdrop.voltage.{}.embedded.csv", name.to_lowercase())),
            );
            let text = std::fs::read_to_string(&csv_path).unwrap();

            // CSV rows: Instance,Terminal,Layer,X location,Y location,Voltage.
            let mut csv_terms: BTreeMap<(String, String), (f64, f64)> = BTreeMap::new();
            for (n, l) in text.lines().enumerate() {
                if n == 0 {
                    assert_eq!(
                        l, "Instance,Terminal,Layer,X location,Y location,Voltage",
                        "{name} CSV header"
                    );
                    continue;
                }
                let f: Vec<&str> = l.split(',').collect();
                assert_eq!(f.len(), 6, "{name} CSV row {n}: {l}");
                csv_terms.insert(
                    (f[0].to_string(), f[1].to_string()),
                    (f[3].parse().unwrap(), f[4].parse().unwrap()),
                );
            }

            // Our attach set, as (instance, terminal) names.
            let mut ours: BTreeMap<(String, String), Vec<(i32, i32)>> = BTreeMap::new();
            for &(p, _node) in &graph.attachments {
                let cell = db.pin_cells[p.0 as usize].expect("attached pin on a cell");
                let inst = db.strings.resolve(db.cell_names[cell.0 as usize]);
                let term = db.strings.resolve(db.pin_names[p.0 as usize]);
                ours.entry((inst.to_string(), term.to_string()))
                    .or_default()
                    .push(db.pin_pos_dbu(p, &tech));
            }
            let ours_set: BTreeSet<_> = ours.keys().cloned().collect();
            let csv_set: BTreeSet<_> = csv_terms.keys().cloned().collect();
            assert_eq!(
                ours_set, csv_set,
                "{name}: (instance, terminal) sets differ (ours-only {:?}, csv-only {:?})",
                ours_set.difference(&csv_set).take(5).collect::<Vec<_>>(),
                csv_set.difference(&ours_set).take(5).collect::<Vec<_>>()
            );

            // Each CSV coordinate must sit within 1 DBU of a pin position.
            for ((inst, term), (x_um, y_um)) in &csv_terms {
                let positions = &ours[&(inst.clone(), term.clone())];
                let (cx, cy) = (x_um * dbu, y_um * dbu);
                assert!(
                    positions.iter().any(|&(px, py)| {
                        (px as f64 - cx).abs() <= 1.0 && (py as f64 - cy).abs() <= 1.0
                    }),
                    "{name}: {inst}/{term} csv=({x_um},{y_um})um vs ours {positions:?} dbu"
                );
            }
        }
    }

    /// Orientation coverage: `gcd.orient.def` places cells under every
    /// orient so `orient_rect`'s W/E/FW/FE arms run inside pin attach.
    /// Assert the attach accounting invariants (every attachable conn pin is
    /// attached-or-unattached exactly once) rather than golden counts — the
    /// fixture's rotations move pins off rails by construction.
    #[test]
    fn test_gcd_orient_fixture_attach() {
        let (tech, db) = load_design("gcd", "gcd.orient.def");
        let orients_used: BTreeSet<u8> =
            db.cell_orient.iter().map(|o| *o as u8).collect();
        assert!(
            orients_used.contains(&(crate::design_db::Orient::W as u8))
                && orients_used.contains(&(crate::design_db::Orient::FE as u8)),
            "orient fixture lost W/FE coverage: {orients_used:?}"
        );
        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let (graph, c) = check_net(&db, &tech, net);
            println!("gcd.orient {name} census: {c:?}");
            let attached: BTreeSet<u32> = graph
                .attachments
                .iter()
                .map(|&(p, _)| p.0)
                .collect();
            let unattached: BTreeSet<u32> =
                graph.unattached_pins.iter().map(|&p| p.0).collect();
            assert!(attached.is_disjoint(&unattached));
            // Every attachable pin gets exactly one verdict.
            assert_eq!(
                attached.len() + unattached.len(),
                c.attachable_pins as usize,
                "{name}: attach verdicts do not partition attachable pins"
            );
        }
    }

    /// picorv32 census: 10,205 attached terminals per rail per the reference
    /// voltage CSV row counts.
    #[test]
    fn test_picorv32_pdn_census_matches_reference() {
        let (tech, db) = load_design("picorv32", "picorv32.placed.def");
        let ref_path = require(design_dir("picorv32").join("picorv32.irdrop.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).unwrap()).unwrap();

        for name in ["VPWR", "VGND"] {
            let net = db.pdn_net(name).expect(name);
            let (graph, c) = check_net(&db, &tech, net);
            let nref = &ref_json["nets"][name];
            println!("picorv32 {name} census: {c:?}");
            assert_eq!(
                c.all_shapes_connected,
                nref["connectivity_result"].as_str().unwrap() == "1",
                "{name} connectivity disagrees with check_power_grid"
            );
            assert!(graph.unreachable_pins.is_empty(), "{name} unreachable");
            assert_eq!(
                c.attached_terminals,
                nref["conventions"]["embedded"]["csv_rows"].as_u64().unwrap() as u32,
                "{name} attached terminals must equal embedded CSV rows"
            );
        }
    }
}
