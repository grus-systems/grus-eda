use super::dmp::PiModel;
use crate::congestion::gcell::GCellGrid;
use crate::congestion::guide::GuideBox;
use crate::congestion::rsmt::Pt;
use crate::design_db::{DesignDb, Orient};
use crate::interner::{LayerId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::readers::spef::{Spef, SpefNet, unescape_spef_name};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use anyhow::{Result, bail};
use rayon::prelude::*;
use std::collections::HashMap;

/// Unit-length wire parasitics of one routing layer, as OpenROAD's `set_wire_rc -layer` derives them
/// from the LEF: `R = RPERSQ / width`, `C = CPERSQDIST * width + 2 * EDGECAPACITANCE`.
#[derive(Copy, Clone, Debug)]
pub struct WireRc {
    pub r_kohm_per_um: f32,
    pub c_pf_per_um: f32,
}

impl WireRc {
    pub fn from_layer(tech: &TechDb, layer_name: &str) -> Result<Self> {
        let Some(layer) = tech
            .routing_layers
            .iter()
            .find(|l| tech.strings.resolve(l.name) == layer_name)
        else {
            bail!("routing layer {layer_name} not found in LEF");
        };
        let width_um = layer.width_dbu as f32 / tech.dbu_per_micron as f32;
        Ok(Self {
            r_kohm_per_um: layer.res_per_sq / width_um / 1000.0,
            c_pf_per_um: layer.cap_per_sq_um * width_um + 2.0 * layer.edge_cap_per_um,
        })
    }
}

/// RC tree of one net, rooted at the driver. Nodes are in BFS order, so a parent always
/// precedes its children and both Elmore passes are plain loops. Components the
/// driver's BFS cannot reach (guide-parasitics dead islands) are appended as
/// dead subtrees: the component root carries `parent[v] == v` and `dead[v]`,
/// keeping the island's caps in `total_wire_cap_pf` without joining it to the
/// driver's Elmore path — the self-loop/disconnected shape OpenROAD's
/// `-global_routing` SPEF writes.
#[derive(Clone, Debug)]
pub struct NetRcTree {
    parent: Vec<u32>,
    edge_r_kohm: Vec<f32>,
    wire_cap_pf: Vec<f32>,
    /// Nodes of components unreachable from the driver. Dead pins get no
    /// wire delay; dead caps only load the driver when `lumped` is set.
    dead: Vec<bool>,
    /// Unpaired zero-length stub segments as `(tree node, kohm)` — emitted
    /// as `n n r` self-loop resistors, matching OpenROAD's SPEF.
    self_loops: Vec<(u32, f32)>,
    /// OpenSTA falls back to a lumped parasitic (pin-capacitance load, zero
    /// wire delay) for nets whose dead island is large enough that the
    /// pi/elmore reduction is not worth building, and whenever a `*P` port
    /// sits on an island. `report_dcalc` probes on the gcd/picorv32 guide
    /// SPEFs put the boundary at roughly a third of total wire capacitance;
    /// the band itself is inconsistent between nets, so the fraction is an
    /// approximation, not an exact replica of OpenSTA's internals.
    pub lumped: bool,
    /// Wire capacitance on dead (unreachable) nodes. A live-part pi model
    /// never sees these caps, so `net_load` subtracts them for non-lumped
    /// nets.
    pub dead_wire_cap_pf: f32,
    /// Tree node of each pin of the net, in `DesignDb::net_pins` order.
    pin_node: Vec<u32>,
    pub total_wire_cap_pf: f32,
}

impl NetRcTree {
    /// Driving-point pi model seen by the driver, from the admittance moments of the whole tree.
    /// `pin_caps_pf[i]` is the pin capacitance of pin `i` for the transition being analysed.
    pub fn pi_model(&self, pin_caps_pf: &[f32]) -> PiModel {
        let n = self.parent.len();
        let mut y1: Vec<f64> = self.wire_cap_pf.iter().map(|&c| c as f64).collect();
        for (pin, &node) in self.pin_node.iter().enumerate() {
            y1[node as usize] += pin_caps_pf[pin] as f64;
        }
        let (mut y2, mut y3) = (vec![0.0f64; n], vec![0.0f64; n]);
        // Children come after their parent, so a reverse sweep sees every child's final moments.
        // A self-parented dead-component root is skipped: nothing flows from a
        // dead subtree to the driver node.
        for v in (1..n).rev() {
            let (p, r) = (self.parent[v] as usize, self.edge_r_kohm[v] as f64);
            if p == v {
                continue;
            }
            let (a1, a2, a3) = (y1[v], y2[v], y3[v]);
            y2[p] += a2 - r * a1 * a1;
            y3[p] += a3 - 2.0 * r * a1 * a2 + r * r * a1 * a1 * a1;
            y1[p] += a1;
        }
        PiModel::from_moments(y1[0], y2[0], y3[0])
    }

    /// Elmore delay (ns) from the driver to each pin of the net, in `net_pins` order.
    /// `pin_caps_pf[i]` is the pin capacitance of pin `i` for the transition being analysed.
    /// Pins on dead (unreachable) nodes get 0 — OpenSTA gives island pins no
    /// wire delay.
    pub fn elmore_ns(&self, pin_caps_pf: &[f32]) -> Vec<f32> {
        let n = self.parent.len();
        let mut downstream = self.wire_cap_pf.clone();
        for (pin, &node) in self.pin_node.iter().enumerate() {
            downstream[node as usize] += pin_caps_pf[pin];
        }
        for v in (1..n).rev() {
            let p = self.parent[v] as usize;
            if p == v {
                continue;
            }
            downstream[p] += downstream[v];
        }
        let mut tau = vec![0.0f32; n];
        for v in 1..n {
            let p = self.parent[v] as usize;
            if p == v {
                continue;
            }
            tau[v] = tau[p] + self.edge_r_kohm[v] * downstream[v];
        }
        self.pin_node
            .iter()
            .map(|&node| {
                if self.dead[node as usize] {
                    0.0
                } else {
                    tau[node as usize]
                }
            })
            .collect()
    }
}

/// One RC tree per net (from placement or from a SPEF), `None` for nets without a driver,
/// with a single pin, or absent from the SPEF.
#[derive(Clone, Debug)]
pub struct NetParasitics {
    nets: Vec<Option<NetRcTree>>,
    /// Nets whose placement PD tree failed OpenROAD's `checkTree`: OpenROAD would
    /// build those with FLUTE, we still use the PD tree, so our estimate diverges
    /// from `estimate_parasitics -placement` on them. Always 0 for SPEF parasitics
    /// and for the A/B tree builders; 0 on all five public designs.
    pub pd_check_failed_nets: usize,
}

impl NetParasitics {
    pub fn net(&self, net: NetId) -> Option<&NetRcTree> {
        self.nets[net.0 as usize].as_ref()
    }
}

pub fn estimate_placement_parasitics(
    design: &DesignDb,
    tech: &TechDb,
    rc: WireRc,
) -> NetParasitics {
    let failed = std::sync::atomic::AtomicUsize::new(0);
    let mut parasitics = estimate_placement_parasitics_with_tree(design, tech, rc, |pts| {
        let tree = crate::congestion::pdr::build_pd_tree_for_points(pts);
        if !tree.check_passed {
            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        tree.segments
    });
    parasitics.pd_check_failed_nets = failed.load(std::sync::atomic::Ordering::Relaxed);
    parasitics
}

/// The tree builder is a parameter so the measurement can compare the greedy and exact trees against
/// routed truth; the shipped estimate is the PD builder used by `estimate_placement_parasitics`.
pub(crate) fn estimate_placement_parasitics_with_tree<F>(
    design: &DesignDb,
    tech: &TechDb,
    rc: WireRc,
    segments: F,
) -> NetParasitics
where
    F: Fn(&[Pt]) -> Vec<(Pt, Pt)> + Sync,
{
    let n_nets = design.num_nets();
    let nets = if n_nets < 512 {
        (0..n_nets)
            .map(|n| build_net_tree(design, tech, rc, NetId(n as u32), &segments))
            .collect()
    } else {
        (0..n_nets)
            .into_par_iter()
            .map(|n| build_net_tree(design, tech, rc, NetId(n as u32), &segments))
            .collect()
    };
    NetParasitics {
        nets,
        pd_check_failed_nets: 0,
    }
}

fn build_net_tree<F: Fn(&[Pt]) -> Vec<(Pt, Pt)>>(
    design: &DesignDb,
    tech: &TechDb,
    rc: WireRc,
    net: NetId,
    segments: &F,
) -> Option<NetRcTree> {
    design.net_driver(net)?;
    let pins: &[PinId] = design.net_pins(net);
    if pins.len() < 2 {
        return None;
    }
    let dbu_per_um = tech.dbu_per_micron as f32;
    let pin_pts: Vec<Pt> = pins.iter().map(|&p| design.pin_pos_dbu(p, tech)).collect();

    let mut points: Vec<Pt> = Vec::with_capacity(pin_pts.len() * 2);
    let mut index: Option<HashMap<Pt, usize>> = None;
    let mut intern = |p: Pt, points: &mut Vec<Pt>| {
        if let Some(map) = index.as_mut() {
            return *map.entry(p).or_insert_with(|| {
                points.push(p);
                points.len() - 1
            });
        }
        if let Some(pos) = points.iter().position(|&q| q == p) {
            return pos;
        }
        points.push(p);
        let id = points.len() - 1;
        if points.len() >= 24 {
            let mut map = HashMap::with_capacity(points.len() * 2);
            for (idx, &pt) in points.iter().enumerate() {
                map.insert(pt, idx);
            }
            index = Some(map);
        }
        id
    };
    let pin_point: Vec<usize> = pin_pts.iter().map(|&p| intern(p, &mut points)).collect();
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (a, b) in segments(&pin_pts) {
        edges.push((intern(a, &mut points), intern(b, &mut points)));
    }
    adjacency.resize(points.len(), Vec::new());
    for &(a, b) in &edges {
        adjacency[a].push(b);
        adjacency[b].push(a);
    }

    // BFS from the driver; `order[i]` is the point that becomes tree node `i`.
    let root = pin_point[0];
    let mut node_of = vec![u32::MAX; points.len()];
    let mut order = vec![root];
    node_of[root] = 0;
    let mut parent = vec![0u32];
    let mut edge_r = vec![0.0f32];
    let mut wire_cap = vec![0.0f32];
    let mut total_cap = 0.0f32;
    let mut head = 0;
    while head < order.len() {
        let u = order[head];
        let u_node = head;
        head += 1;
        for &v in &adjacency[u] {
            if node_of[v] != u32::MAX {
                continue;
            }
            let length_um = (points[u].0.abs_diff(points[v].0) + points[u].1.abs_diff(points[v].1))
                as f32
                / dbu_per_um;
            let cap = rc.c_pf_per_um * length_um;
            node_of[v] = order.len() as u32;
            order.push(v);
            parent.push(u_node as u32);
            edge_r.push(rc.r_kohm_per_um * length_um);
            // Half of the segment capacitance sits at each end (pi model), as OpenROAD writes it.
            wire_cap.push(cap / 2.0);
            wire_cap[u_node] += cap / 2.0;
            total_cap += cap;
        }
    }
    let pin_node = pin_point.iter().map(|&p| node_of[p]).collect();
    Some(NetRcTree {
        dead: vec![false; parent.len()],
        self_loops: Vec::new(),
        lumped: false,
        dead_wire_cap_pf: 0.0,
        parent,
        edge_r_kohm: edge_r,
        wire_cap_pf: wire_cap,
        pin_node,
        total_wire_cap_pf: total_cap,
    })
}

/// Builds the RC trees from an extracted SPEF. SPEF capacitance is wire-only (`PIN_CAP NONE`), so
/// sink pin capacitance is added later, exactly as for the placement estimate. Resistor loops are
/// broken by a BFS spanning tree from the driver; `n n r` self-loops are kept for
/// re-emission and parallel resistors are dropped. Components the driver's BFS
/// cannot reach (guide-SPEF dead islands) are appended as dead subtrees instead
/// of failing — OpenSTA accepts such nets and gives their pins no wire delay.
pub fn spef_parasitics(design: &DesignDb, spef: &Spef) -> Result<NetParasitics> {
    let n_nets = design.num_nets();
    let build_one = |n: usize| -> Result<Option<NetRcTree>> {
        let net = NetId(n as u32);
        let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
        let usable = design.net_driver(net).is_some() && design.net_pins(net).len() >= 2;
        match (usable, spef.nets.get(&name)) {
            (true, Some(spef_net)) => Ok(Some(spef_net_tree(design, net, &name, spef_net)?)),
            _ => Ok(None),
        }
    };
    let nets: Vec<Option<NetRcTree>> = if n_nets < 512 {
        (0..n_nets).map(build_one).collect::<Result<Vec<_>>>()?
    } else {
        (0..n_nets)
            .into_par_iter()
            .map(build_one)
            .collect::<Result<Vec<_>>>()?
    };
    Ok(NetParasitics {
        nets,
        pd_check_failed_nets: 0,
    })
}

fn spef_pin_node_name(design: &DesignDb, pin: PinId) -> String {
    let p = pin.0 as usize;
    let pin_name = design.strings.resolve(design.pin_names[p]);
    match design.pin_cells[p] {
        Some(cell) => {
            let cell_name = design.strings.resolve(design.cell_names[cell.0 as usize]);
            unescape_spef_name(&format!("{cell_name}:{pin_name}"))
        }
        None => unescape_spef_name(pin_name),
    }
}

fn spef_net_tree(
    design: &DesignDb,
    net: NetId,
    net_name: &str,
    spef_net: &SpefNet,
) -> Result<NetRcTree> {
    let pins = design.net_pins(net);
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut intern = |name: &str| -> usize {
        let next = index.len();
        *index.entry(name.to_string()).or_insert(next)
    };
    let resistor_ends: Vec<(usize, usize, f32)> = spef_net
        .resistors
        .iter()
        .map(|(a, b, r)| (intern(a), intern(b), *r))
        .collect();
    let cap_nodes: Vec<(usize, f32)> = spef_net.caps.iter().map(|(n, c)| (intern(n), *c)).collect();
    let node_count = index.len();
    let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); node_count];
    let mut self_loop_ends: Vec<(usize, f32)> = Vec::new();
    for &(a, b, r) in &resistor_ends {
        if a == b {
            self_loop_ends.push((a, r));
        } else {
            adjacency[a].push((b, r));
            adjacency[b].push((a, r));
        }
    }
    let mut node_cap = vec![0.0f32; node_count];
    for &(i, cap) in &cap_nodes {
        node_cap[i] += cap;
    }
    let pin_names: Vec<String> = pins
        .iter()
        .map(|&p| spef_pin_node_name(design, p))
        .collect();
    let mut pin_point = Vec::with_capacity(pins.len());
    for name in &pin_names {
        let Some(&i) = index.get(name) else {
            bail!("SPEF net {net_name} has no node for pin {name}");
        };
        pin_point.push(i);
    }

    // `net_pins` stores the driver pin first (the DEF parser enforces it and
    // `net_driver` returns `pins[0]`), so `pin_point[0]` is the driver node —
    // the same root `build_net_tree` uses.
    let root = pin_point[0];
    let mut node_of = vec![u32::MAX; adjacency.len()];
    let mut order = vec![root];
    node_of[root] = 0;
    let mut parent = vec![0u32];
    let mut edge_r = vec![0.0f32];
    let mut wire_cap = vec![node_cap[root]];
    let mut dead = vec![false];
    let mut head = 0;
    let mut dead_component = false;
    while order.len() < adjacency.len() {
        while head < order.len() {
            let u = order[head];
            let u_node = head;
            head += 1;
            for &(v, r) in &adjacency[u] {
                if node_of[v] != u32::MAX {
                    continue;
                }
                node_of[v] = order.len() as u32;
                order.push(v);
                parent.push(u_node as u32);
                edge_r.push(r);
                wire_cap.push(node_cap[v]);
                dead.push(dead_component);
            }
        }
        if order.len() == adjacency.len() {
            break;
        }
        // A component the driver cannot reach: the lowest-index unreached
        // node becomes a self-parented dead root (deterministic), its
        // subtree BFSs in with `dead` set. Pins on it keep their nodes but
        // get no wire delay — OpenSTA's island semantics.
        let v = (0..adjacency.len())
            .find(|&v| node_of[v] == u32::MAX)
            .expect("fewer nodes than order length");
        node_of[v] = order.len() as u32;
        parent.push(order.len() as u32);
        order.push(v);
        edge_r.push(0.0);
        wire_cap.push(node_cap[v]);
        dead.push(true);
        dead_component = true;
    }
    let pin_node: Vec<u32> = pin_point.iter().map(|&p| node_of[p]).collect();
    let self_loops = self_loop_ends
        .iter()
        .map(|&(v, r)| (node_of[v], r))
        .collect();
    let (lumped, dead_wire_cap_pf) = lumped_flag(design, pins, &dead, &wire_cap, &pin_node);
    let total = wire_cap.iter().sum();
    Ok(NetRcTree {
        parent,
        edge_r_kohm: edge_r,
        wire_cap_pf: wire_cap,
        dead,
        self_loops,
        lumped,
        dead_wire_cap_pf,
        pin_node,
        total_wire_cap_pf: total,
    })
}

/// One guide route segment between two `(x, y, layer)` grid points, as
/// OpenROAD's `boxToGlobalRouting`/`updateVias` produce it: endpoints at
/// tile centers, zero-length segments as via candidates.
struct GSeg {
    x0: i32,
    y0: i32,
    l0: u8,
    x1: i32,
    y1: i32,
    l1: u8,
}

impl GSeg {
    fn is_via(&self) -> bool {
        self.x0 == self.x1 && self.y0 == self.y1
    }
    fn init_pt(&self) -> (i32, i32) {
        (self.x0, self.y0)
    }
    fn final_pt(&self) -> (i32, i32) {
        (self.x1, self.y1)
    }
}

/// Route-point graph of one guide net: nodes keyed by `(x, y, layer)`
/// (pin leaves intern by position only), half-split caps per node,
/// resistor edges as `(neighbor, kohm)` adjacency. Same-layer zero-length
/// segments (unpaired stubs) record `(node, kohm)` self-loops instead of
/// adjacency — OpenROAD emits them as `n n r` resistors.
#[derive(Default)]
struct RouteGraph {
    node_of: HashMap<(i32, i32, u8), usize>,
    pt: Vec<(i32, i32)>,
    cap: Vec<f32>,
    adj: Vec<Vec<(usize, f32)>>,
    self_loops: Vec<(usize, f32)>,
}

impl RouteGraph {
    fn intern(&mut self, x: i32, y: i32, l: u8) -> usize {
        *self.node_of.entry((x, y, l)).or_insert_with(|| {
            self.pt.push((x, y));
            self.cap.push(0.0);
            self.adj.push(Vec::new());
            self.pt.len() - 1
        })
    }

    /// Pin leaves never merge with a RoutePt at the same coordinates —
    /// OpenROAD keeps a named node per pin — so they index by position.
    fn intern_pos(&mut self, x: i32, y: i32) -> usize {
        self.pt.push((x, y));
        self.cap.push(0.0);
        self.adj.push(Vec::new());
        self.pt.len() - 1
    }

    fn edge(&mut self, a: usize, b: usize, r: f32) {
        if a == b {
            self.self_loops.push((a, r));
        } else {
            self.adj[a].push((b, r));
            self.adj[b].push((a, r));
        }
    }
}

/// The node a pin attaches to: `(grid_pt, conn+1)` first (the cut via is
/// lumped into the returned via R), then `(grid_pt, conn)`.
fn pin_node_lookup(
    g: &RouteGraph,
    num_layers: usize,
    via_kohm: &[f32],
    gx: i32,
    gy: i32,
    conn: LayerId,
) -> Option<(usize, u8, f32)> {
    let above = LayerId(conn.0 + 1);
    if (above.0 as usize) < num_layers
        && let Some(&node) = g.node_of.get(&(gx, gy, above.0))
    {
        return Some((
            node,
            above.0,
            via_kohm.get(conn.0 as usize).copied().unwrap_or(0.0),
        ));
    }
    g.node_of
        .get(&(gx, gy, conn.0))
        .map(|&node| (node, conn.0, 0.0))
}

/// Which die edge a pin sits on, OpenROAD `Pin::PinEdge`.
#[derive(Copy, Clone, PartialEq, Eq)]
enum PinEdge {
    None,
    North,
    South,
    East,
    West,
}

/// Everything `makeParasiticsToPin` needs about one pin: `pt` is the
/// lower-left corner of the pin's last routing-layer box, `grid_pt` the
/// on-grid position OpenROAD computes in `findPins`, `conn` its connection
/// layer.
#[derive(Copy, Clone)]
struct PinAttach {
    px: i32,
    py: i32,
    gx: i32,
    gy: i32,
    conn: LayerId,
}

/// A macro-coord pin box placed at `(cx, cy)` under `orient`, corner-wise
/// with `DesignDb::pin_pos_dbu`'s point transform.
fn place_rect(r: RectDbu, w: i32, h: i32, o: Orient, cx: i32, cy: i32) -> RectDbu {
    let t = |x: i32, y: i32| match o {
        Orient::N => (cx + x, cy + y),
        Orient::S => (cx + w - x, cy + h - y),
        Orient::W => (cx + h - y, cy + x),
        Orient::E => (cx + y, cy + w - x),
        Orient::FN => (cx + w - x, cy + y),
        Orient::FS => (cx + x, cy + h - y),
        Orient::FW => (cx + y, cy + x),
        Orient::FE => (cx + h - y, cy + w - x),
    };
    let (ax, ay) = t(r.xl, r.yl);
    let (bx, by) = t(r.xh, r.yh);
    RectDbu::new(ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
}

/// Absolute routing-layer shapes of a pin in file order — OpenDB's
/// `pin.getBoxes()`. Instance pins transform `MacroPin::rects`; I/O ports
/// carry their PINS record rects directly.
fn pin_rects_dbu(design: &DesignDb, tech: &TechDb, pin: PinId) -> Vec<(LayerId, RectDbu)> {
    let p = pin.0 as usize;
    match design.pin_cells[p] {
        None => design.pin_io_rects[p].clone(),
        Some(cell) => {
            let c = cell.0 as usize;
            let mac = &tech.macros[design.cell_macros[c].0 as usize];
            design.pin_macro_pins[p]
                .map(|mp| {
                    tech.macro_pins[mp.0 as usize]
                        .rects
                        .iter()
                        .map(|&(l, r)| {
                            (
                                l,
                                place_rect(
                                    r,
                                    mac.width_dbu,
                                    mac.height_dbu,
                                    design.cell_orient[c],
                                    design.cell_x_dbu[c],
                                    design.cell_y_dbu[c],
                                ),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
    }
}

/// `Pin::determineEdge`: every box votes for its nearest die edge; ties go
/// to the first of N, S, E, W like `std::max` on the initializer list.
fn pin_edge(rects: &[(LayerId, RectDbu)], die: &RectDbu) -> PinEdge {
    let mut votes = [0; 4];
    for (_, r) in rects {
        let d = [
            die.yh - r.yh,
            r.yl - die.yl,
            die.xh - r.xh,
            r.xl - die.xl,
        ];
        let m = d.iter().min().copied().unwrap();
        let idx = if d[0] == m {
            0
        } else if d[1] == m {
            1
        } else if d[2] == m {
            2
        } else {
            3
        };
        votes[idx] += 1;
    }
    match votes.iter().position(|&v| v == *votes.iter().max().unwrap_or(&0)) {
        Some(0) if votes[0] > 0 => PinEdge::North,
        Some(1) => PinEdge::South,
        Some(2) => PinEdge::East,
        Some(3) => PinEdge::West,
        _ => PinEdge::None,
    }
}

/// `Pin::connection_layer`: regular pins use their highest routing layer;
/// edge pins prefer the highest layer whose direction matches the edge.
fn pin_conn_layer(rects: &[(LayerId, RectDbu)], edge: PinEdge, tech: &TechDb) -> Option<LayerId> {
    let top = || rects.iter().map(|&(l, _)| l).max_by_key(|l| l.0);
    if edge == PinEdge::None {
        return top();
    }
    let dir = match edge {
        PinEdge::North | PinEdge::South => LayerDir::Vertical,
        _ => LayerDir::Horizontal,
    };
    rects
        .iter()
        .filter(|&&(l, _)| tech.routing_layers[l.0 as usize].dir == dir)
        .map(|&(l, _)| l)
        .max_by_key(|l| l.0)
        .or_else(top)
}

/// `Grid::getPositionOnGrid`: the covered GCell's center.
fn on_grid_center(grid: &GCellGrid, x: i32, y: i32) -> (i32, i32) {
    (
        grid.origin_x_dbu + grid.gcell_x(x) as i32 * grid.gcell_w_dbu + grid.gcell_w_dbu / 2,
        grid.origin_y_dbu + grid.gcell_y(y) as i32 * grid.gcell_h_dbu + grid.gcell_h_dbu / 2,
    )
}

/// `pinOverlapsWithSingleTrack`: a pin spanning at most three pitches of
/// its connection layer snaps its on-grid position onto the single
/// covered track. Returns the track-snapped position, if the rule fires.
fn single_track_snap(
    conn_rects: &[RectDbu],
    dir: LayerDir,
    track_loc: i32,
    track_pitch: i32,
    pos_on_grid: (i32, i32),
) -> Option<(i32, i32)> {
    if track_pitch <= 0 {
        return None;
    }
    let (min, max) = if dir == LayerDir::Horizontal {
        (
            conn_rects.iter().map(|r| r.yl).min()?,
            conn_rects.iter().map(|r| r.yh).max()?,
        )
    } else {
        (
            conn_rects.iter().map(|r| r.xl).min()?,
            conn_rects.iter().map(|r| r.xh).max()?,
        )
    };
    if (max - min) as f32 / track_pitch as f32 > 3.0 {
        return None;
    }
    let track = |back: i32| {
        ((max - track_loc) as f32 / track_pitch as f32 - back as f32).floor() as i32 * track_pitch
            + track_loc
    };
    let (t1, t2) = (track(0), track(1));
    let inside = |t: i32| t >= min && t <= max;
    if inside(t1) && inside(t2) {
        return None;
    }
    let strict = |t: i32| t > min && t < max;
    let t = if strict(t1) {
        Some(t1)
    } else if strict(t2) {
        Some(t2)
    } else {
        None
    }?;
    Some(if dir == LayerDir::Horizontal {
        (pos_on_grid.0, t)
    } else {
        (t, pos_on_grid.1)
    })
}

/// `findOnGridPositions` + `computePinPositionOnGrid` without access
/// points: each connection-layer box middle votes for its GCell center
/// (first box wins ties); pad/macro edge pins would snap to the instance
/// edge on oversized boxes but are not modeled (test designs have none).
/// A single-track pin re-snaps onto the covered track along the layer's
/// preferred direction.
fn pin_attach(design: &DesignDb, tech: &TechDb, grid: &GCellGrid, pin: PinId, met1: LayerId) -> PinAttach {
    let p = pin.0 as usize;
    let rects = pin_rects_dbu(design, tech, pin);
    let (px, py) = rects
        .last()
        .map(|&(_, r)| (r.xl, r.yl))
        .unwrap_or_else(|| design.pin_pos_dbu(pin, tech));
    let edge = if design.pin_cells[p].is_none() {
        pin_edge(&rects, &design.die_area)
    } else {
        PinEdge::None
    };
    let conn = pin_conn_layer(&rects, edge, tech)
        .or_else(|| {
            design.pin_io_layer[p].or_else(|| {
                design.pin_macro_pins[p].and_then(|mp| tech.macro_pins[mp.0 as usize].layer)
            })
        })
        .unwrap_or(met1);
    let conn_rects: Vec<RectDbu> = rects
        .iter()
        .filter(|&&(l, _)| l == conn)
        .map(|&(_, r)| r)
        .collect();
    let tile = grid.gcell_w_dbu.min(grid.gcell_h_dbu);
    let mut votes: Vec<(i32, i32)> = Vec::with_capacity(conn_rects.len());
    let mut pos_on_grid = on_grid_center(grid, px, py);
    for r in &conn_rects {
        let (mx, my) = ((r.xl + r.xh) / 2, (r.yl + r.yh) / 2);
        // Edge pins with a box longer than a tile snap to the box side
        // nearest the die edge before cell lookup.
        let pos = match edge {
            PinEdge::None => (mx, my),
            _ if r.width().max(r.height()) >= tile => match edge {
                PinEdge::North => (mx, r.yh),
                PinEdge::South => (mx, r.yl),
                PinEdge::East => (r.xh, my),
                PinEdge::West => (r.xl, my),
                PinEdge::None => unreachable!(),
            },
            _ => (mx, my),
        };
        pos_on_grid = on_grid_center(grid, pos.0, pos.1);
        votes.push(pos_on_grid);
    }
    let mut grid_pt = votes.first().copied().unwrap_or(pos_on_grid);
    let mut best = -1i32;
    for &v in &votes {
        let eq = votes.iter().filter(|&&q| q == v).count() as i32;
        if eq > best {
            grid_pt = v;
            best = eq;
        }
    }
    let dir = tech.routing_layers[conn.0 as usize].dir;
    let track = design.tracks.iter().find(|tg| {
        tg.is_vertical_tracks == (dir == LayerDir::Vertical) && tg.layers.contains(&conn)
    });
    if let Some(tg) = track
        && let Some(tp) =
            single_track_snap(&conn_rects, dir, tg.start_dbu, tg.step_dbu, pos_on_grid)
    {
        let snapped = on_grid_center(grid, tp.0, tp.1);
        let moved = match dir {
            LayerDir::Horizontal => snapped.1 != grid_pt.1,
            LayerDir::Vertical => snapped.0 != grid_pt.0,
        };
        if snapped != grid_pt && moved {
            grid_pt = snapped;
        }
    }
    PinAttach {
        px,
        py,
        gx: grid_pt.0,
        gy: grid_pt.1,
        conn,
    }
}

/// Nearest route node to `(px, py)` by Manhattan distance, optionally
/// restricted to the driver's reachable component.
fn nearest_route_node(
    g: &RouteGraph,
    route_nodes: usize,
    reached: Option<&[bool]>,
    px: i32,
    py: i32,
) -> usize {
    (0..route_nodes)
        .filter(|&v| reached.is_none_or(|r| r[v]))
        .min_by_key(|&v| {
            let (nx, ny) = g.pt[v];
            (nx - px).abs() + (ny - py).abs()
        })
        .expect("route nodes exist")
}

/// Builds one `NetRcTree` per net from route-guide boxes — the E4.4
/// estimator (`estimate_parasitics -global_routing` analogue). `None` for
/// nets without a driver, with < 2 pins, or whose boxes emit no route
/// nodes; semantics in `docs/e4-guides.md` §10. `grid` maps box rects
/// back to covered GCells — `GCellGrid::build` for parsed guides,
/// `routed.grid` when the caller routed internally. `net_boxes` carries
/// only nets that have boxes; the result indexes all `num_nets()`.
pub fn estimate_guide_parasitics(
    design: &DesignDb,
    tech: &TechDb,
    grid: &GCellGrid,
    net_boxes: &[(NetId, Vec<GuideBox>)],
) -> NetParasitics {
    let dbu = tech.dbu_per_micron as f32;
    let layer_rc: Vec<WireRc> = tech
        .routing_layers
        .iter()
        .map(|l| {
            let width_um = l.width_dbu as f32 / dbu;
            WireRc {
                r_kohm_per_um: l.res_per_sq / width_um / 1000.0,
                c_pf_per_um: l.cap_per_sq_um * width_um + 2.0 * l.edge_cap_per_um,
            }
        })
        .collect();
    let via_kohm: Vec<f32> = tech
        .via_res_ohm_above
        .iter()
        .map(|r| r / 1000.0)
        .collect();

    let mut nets = vec![None; design.num_nets()];
    for (net, boxes) in net_boxes {
        nets[net.0 as usize] = guide_net_tree(design, tech, grid, &layer_rc, &via_kohm, *net, boxes);
    }
    NetParasitics {
        nets,
        pd_check_failed_nets: 0,
    }
}

fn guide_net_tree(
    design: &DesignDb,
    tech: &TechDb,
    grid: &GCellGrid,
    layer_rc: &[WireRc],
    via_kohm: &[f32],
    net: NetId,
    boxes: &[GuideBox],
) -> Option<NetRcTree> {
    design.net_driver(net)?;
    let pins = design.net_pins(net);
    if pins.len() < 2 || grid.gcell_w_dbu <= 0 || grid.gcell_h_dbu <= 0 {
        return None;
    }
    let dbu = tech.dbu_per_micron as f32;
    let tw = grid.gcell_w_dbu;
    let th = grid.gcell_h_dbu;

    // boxToGlobalRouting: segment endpoints are tile centers computed
    // 0-anchored (`tile*(xMin/tile) + tile/2`), the high end one cell in
    // (`tile*(xMax/tile) - tile/2`); a box covering more than one cell in
    // both dims emits nothing.
    let mut segs: Vec<GSeg> = Vec::new();
    for b in boxes {
        let l = b.layer.0;
        let mut x0 = tw * (b.rect.xl / tw) + tw / 2;
        let mut y0 = th * (b.rect.yl / th) + th / 2;
        let x1 = tw * (b.rect.xh / tw) - tw / 2;
        let y1 = th * (b.rect.yh / th) - th / 2;
        if x0 == x1 && y0 == y1 {
            segs.push(GSeg { x0, y0, l0: l, x1, y1, l1: l });
        }
        while y0 == y1 && x0 + tw <= x1 {
            segs.push(GSeg { x0, y0, l0: l, x1: x0 + tw, y1: y0, l1: l });
            x0 += tw;
        }
        while x0 == x1 && y0 + th <= y1 {
            segs.push(GSeg { x0, y0, l0: l, x1: x0, y1: y0 + th, l1: l });
            y0 += th;
        }
    }

    // updateVias: one forward pass over consecutive pairs; a zero-length
    // segment adjacent to a wire one layer up becomes a via.
    for i in 0..segs.len().saturating_sub(1) {
        let (head, tail) = segs.split_at_mut(i + 1);
        let s1 = &mut head[i];
        let s2 = &mut tail[0];
        if s1.is_via()
            && s1.l0 + 1 == s2.l0
            && (s1.init_pt() == s2.init_pt() || s1.init_pt() == s2.final_pt())
        {
            s1.l1 = s2.l0;
        } else if s2.is_via()
            && s2.l0 + 1 == s1.l0
            && (s2.init_pt() == s1.init_pt() || s2.init_pt() == s1.final_pt())
        {
            s2.l0 = s1.l1;
        }
    }

    // Graph nodes keyed by RoutePt (x, y, layer); pin leaves are extra nodes.
    let mut g = RouteGraph::default();
    for s in &segs {
        let a = g.intern(s.x0, s.y0, s.l0);
        let b = g.intern(s.x1, s.y1, s.l1);
        if s.is_via() {
            // Cut-layer resistance above the lower endpoint layer; a
            // same-layer zero-length segment is a self-loop (R only).
            let r = via_kohm
                .get(s.l0.min(s.l1) as usize)
                .copied()
                .unwrap_or(0.0);
            g.edge(a, b, r);
        } else {
            let len_um = (s.x0.abs_diff(s.x1) + s.y0.abs_diff(s.y1)) as f32 / dbu;
            let rc = &layer_rc[s.l0 as usize];
            let cap = rc.c_pf_per_um * len_um;
            g.cap[a] += cap / 2.0;
            g.cap[b] += cap / 2.0;
            g.edge(a, b, rc.r_kohm_per_um * len_um);
        }
    }
    if g.pt.is_empty() {
        return None;
    }
    let route_nodes = g.pt.len();

    // Pin attachment (makeParasiticsToPin): the node at the pin's on-grid
    // position on conn+1 is preferred — the cut via is lumped into the edge
    // R — then the node on conn. The pin wire runs the Manhattan distance
    // pin position → grid point on the chosen layer.
    let met1 = LayerId((tech.routing_layers.len() > 1) as u8);
    let pin_locs: Vec<PinAttach> = pins
        .iter()
        .map(|&pin| pin_attach(design, tech, grid, pin, met1))
        .collect();
    // The driver's seed component is where the tree starts; pins whose
    // lookup misses a node entirely fall back to its nearest node.
    let seed = {
        let loc = pin_locs[0];
        pin_node_lookup(&g, layer_rc.len(), via_kohm, loc.gx, loc.gy, loc.conn)
            .map(|(node, _, _)| node)
            .unwrap_or_else(|| nearest_route_node(&g, route_nodes, None, loc.px, loc.py))
    };
    let mut reached = vec![false; route_nodes];
    let mut stack = vec![seed];
    reached[seed] = true;
    while let Some(u) = stack.pop() {
        for &(v, _) in &g.adj[u] {
            if v < route_nodes && !reached[v] {
                reached[v] = true;
                stack.push(v);
            }
        }
    }

    let mut pin_leaf = Vec::with_capacity(pins.len());
    for loc in &pin_locs {
        let (px, py, gx, gy, conn) = (loc.px, loc.py, loc.gx, loc.gy, loc.conn);
        let hit = pin_node_lookup(&g, layer_rc.len(), via_kohm, gx, gy, conn);
        // The pin resistor always lands on the looked-up node — OpenROAD
        // attaches it there even when that node sits on a disconnected
        // stub island; the island links into the tree afterwards.
        let (cap_node, wire_layer, via_r, edge_target) = match hit {
            Some((node, layer, via)) => (node, layer, via, node),
            None => {
                let t = nearest_route_node(&g, route_nodes, Some(&reached), px, py);
                (t, conn.0, 0.0, t)
            }
        };
        let dist_um = ((px - gx).abs() + (py - gy).abs()) as f32 / dbu;
        let rc = &layer_rc[wire_layer as usize];
        // EST-0026 mirror: OpenROAD charges no pin wire when the pin cell
        // carries no node on conn/conn+1 — the edge is topology only, so
        // it adds no cap to the total.
        let cap = if hit.is_some() {
            rc.c_pf_per_um * dist_um
        } else {
            0.0
        };
        let leaf = g.intern_pos(px, py);
        g.cap[leaf] += cap / 2.0;
        g.cap[cap_node] += cap / 2.0;
        g.edge(leaf, edge_target, rc.r_kohm_per_um * dist_um + via_r);
        pin_leaf.push(leaf);
    }

    // BFS spanning tree from the driver pin leaf — the same shape the
    // SPEF path builds. OpenROAD's segment graph can be genuinely
    // disconnected (a stub box that never pairs into a via stays a
    // self-loop island, and whole detour branches can detach). Its SPEF
    // leaves such components electrically dead: `*CAP` entries stay on the
    // island nodes and unpaired stubs become `n n r` self-loops, but no
    // resistor joins an island to the driver's component — island pins
    // see no wire delay and island caps never load the driver's path.
    // Each remaining component is therefore appended as a dead subtree:
    // the lowest-index unreached node roots it as a self-parented
    // sentinel, and the component BFSs in with `dead` set.
    let root = pin_leaf[0];
    let mut tree_of = vec![u32::MAX; g.pt.len()];
    let mut order = vec![root];
    tree_of[root] = 0;
    let mut parent = vec![0u32];
    let mut edge_r = vec![0.0f32];
    let mut wire_cap = vec![g.cap[root]];
    let mut dead = vec![false];
    let mut head = 0;
    let mut dead_component = false;
    while order.len() < g.pt.len() {
        while head < order.len() {
            let u = order[head];
            let u_node = head;
            head += 1;
            for &(v, r) in &g.adj[u] {
                if tree_of[v] != u32::MAX {
                    continue;
                }
                tree_of[v] = order.len() as u32;
                order.push(v);
                parent.push(u_node as u32);
                edge_r.push(r);
                wire_cap.push(g.cap[v]);
                dead.push(dead_component);
            }
        }
        if order.len() == g.pt.len() {
            break;
        }
        let v = (0..g.pt.len())
            .find(|&v| tree_of[v] == u32::MAX)
            .expect("fewer nodes than order length");
        tree_of[v] = order.len() as u32;
        parent.push(order.len() as u32);
        order.push(v);
        edge_r.push(0.0);
        wire_cap.push(g.cap[v]);
        dead.push(true);
        dead_component = true;
    }
    let pin_node: Vec<u32> = pin_leaf.iter().map(|&leaf| tree_of[leaf]).collect();
    let self_loops = g.self_loops.iter().map(|&(v, r)| (tree_of[v], r)).collect();
    let (lumped, dead_wire_cap_pf) = lumped_flag(design, pins, &dead, &wire_cap, &pin_node);
    Some(NetRcTree {
        parent,
        edge_r_kohm: edge_r,
        wire_cap_pf: wire_cap,
        dead,
        self_loops,
        lumped,
        dead_wire_cap_pf,
        pin_node,
        total_wire_cap_pf: g.cap.iter().sum(),
    })
}

/// OpenSTA's lumped fallback decision for partially connected parasitics,
/// as probed by `report_dcalc` on the gcd/picorv32 guide SPEFs: a dead
/// `*P` port always forces the fallback (OpenSTA needs an elmore delay to
/// every port, so the reduction cannot succeed); with only dead `*I` pins
/// the net goes lumped once the island carries roughly a third of the wire
/// capacitance. The band itself is inconsistent between structurally
/// identical nets, so the fraction is an approximation of OpenSTA's
/// behavior, not a literal replica.
fn lumped_flag(
    design: &DesignDb,
    pins: &[PinId],
    dead: &[bool],
    wire_cap: &[f32],
    pin_node: &[u32],
) -> (bool, f32) {
    let dead_cap: f32 = wire_cap
        .iter()
        .zip(dead)
        .map(|(&c, &d)| if d { c } else { 0.0 })
        .sum();
    let dead_pin = pin_node.iter().any(|&n| dead[n as usize]);
    if !dead_pin {
        return (false, dead_cap);
    }
    let total: f32 = wire_cap.iter().sum();
    let dead_port = pins
        .iter()
        .zip(pin_node)
        .any(|(&p, &n)| dead[n as usize] && design.pin_cells[p.0 as usize].is_none());
    let lumped = dead_port || dead_cap >= total * 0.32;
    (lumped, dead_cap)
}

/// Escapes the characters OpenSTA's SPEF parser treats specially: `$` (an
/// unescaped `$` truncates the net name) and `/` (the `*DIVIDER`). Bus brackets
/// stay raw — `name[N]` is valid SPEF bus-bit syntax that resolves to the
/// literal DEF net name, matching what OpenROAD's `write_spef` emits for plain
/// `identifier[N]` nets. `parse_spef`'s `unescape_spef_name` strips every
/// backslash, so `\$`/`\/` round-trip through our own reader as well.
fn escape_spef_name(name: &str) -> String {
    name.replace('$', "\\$").replace('/', "\\/")
}

/// OpenROAD writes pin/via and zero-length links as 1e-6 kohm stubs; pins that
/// share one RC-tree node are split onto such a stub so every pin keeps its own
/// SPEF node name. The stub is electrically transparent at STA precision.
const SPEF_STUB_RES_KOHM: f32 = 1e-6;

fn spef_pin_dir(dir: PinDir) -> &'static str {
    match dir {
        PinDir::Input => "I",
        PinDir::Output => "O",
        PinDir::Inout | PinDir::Internal => "IO",
    }
}

/// Serializes the placement RC trees to IEEE 1481 SPEF — literal names (no
/// NAME_MAP), `*CAP` per tree node, `*RES` per tree edge, `*CONN` listing every
/// pin, `*R_UNIT 1 KOHM` so resistances round-trip bit-exact through
/// `parse_spef` (an OHM unit would cost ~1e-7 ns of Elmore per edge through the
/// f32 rescale). Capacitance stays wire-only (`PIN_CAP NONE`): `NetRcTree` keeps
/// sink pin caps out of `wire_cap_pf`, so re-reading through `spef_parasitics`
/// reproduces the same trees.
pub fn write_spef(design: &DesignDb, lib: &LibDb, parasitics: &NetParasitics) -> Result<String> {
    let mut out = String::new();
    // Header mirrors what OpenROAD's `write_spef` emits: OpenSTA's SPEF parser
    // requires the full keyword block (`*DIVIDER`, `*BUS_DELIMITER`, `*L_UNIT`,
    // ...) and rejects a bare `*DELIMITER` header.
    out.push_str("*SPEF \"ieee 1481-1999\"\n");
    out.push_str(&format!("*DESIGN \"{}\"\n", design.design_name));
    out.push_str("*DATE \"00:00:00 Thu 01 01, 1970\"\n");
    out.push_str(concat!(
        "*VENDOR \"grus-eda\"\n*PROGRAM \"grus-eda\"\n*VERSION \"",
        env!("CARGO_PKG_VERSION"),
        "\"\n"
    ));
    out.push_str("*DESIGN_FLOW \"NAME_SCOPE LOCAL\" \"PIN_CAP NONE\"\n");
    out.push_str("*DIVIDER /\n*DELIMITER :\n*BUS_DELIMITER []\n");
    out.push_str("*T_UNIT 1 NS\n*C_UNIT 1 PF\n*R_UNIT 1 KOHM\n*L_UNIT 1 HENRY\n\n");
    out.push_str("*PORTS\n");
    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        let name = escape_spef_name(&unescape_spef_name(
            design.strings.resolve(design.pin_names[p]),
        ));
        out.push_str(&format!("{name} {}\n", spef_pin_dir(design.pin_dirs[p])));
    }
    out.push('\n');
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let Some(tree) = parasitics.net(net) else {
            continue;
        };
        let net_name = escape_spef_name(&unescape_spef_name(
            design.strings.resolve(design.net_names[n]),
        ));
        let pins = design.net_pins(net);

        // Tree node -> SPEF node name. A node carrying several pins keeps the
        // first pin's name; the rest become stub nodes below.
        let nnodes = tree.parent.len();
        let mut node_pins: Vec<Vec<usize>> = vec![Vec::new(); nnodes];
        for (i, &node) in tree.pin_node.iter().enumerate() {
            node_pins[node as usize].push(i);
        }
        let mut node_names: Vec<String> = Vec::with_capacity(nnodes);
        // (stub name, primary node index) pairs for pins sharing a tree node.
        let mut stubs: Vec<(String, usize)> = Vec::new();
        for (v, pins_at_node) in node_pins.iter().enumerate() {
            if let Some(&primary) = pins_at_node.first() {
                node_names.push(escape_spef_name(&spef_pin_node_name(design, pins[primary])));
                for &extra in &pins_at_node[1..] {
                    stubs.push((
                        escape_spef_name(&spef_pin_node_name(design, pins[extra])),
                        v,
                    ));
                }
            } else {
                node_names.push(format!("{net_name}:{v}"));
            }
        }

        out.push_str(&format!("*D_NET {net_name} {}\n", tree.total_wire_cap_pf));
        out.push_str("*CONN\n");
        for &pin in pins {
            let p = pin.0 as usize;
            let pin_name = escape_spef_name(&unescape_spef_name(
                design.strings.resolve(design.pin_names[p]),
            ));
            let dir = spef_pin_dir(design.pin_dirs[p]);
            match design.pin_cells[p] {
                Some(cell) => {
                    let cell_name = escape_spef_name(&unescape_spef_name(
                        design.strings.resolve(design.cell_names[cell.0 as usize]),
                    ));
                    let cell_type = design.cell_lib_ids[cell.0 as usize]
                        .map(|lcid| {
                            escape_spef_name(&unescape_spef_name(
                                lib.strings.resolve(lib.cells[lcid.0 as usize].name),
                            ))
                        })
                        .unwrap_or_else(|| cell_name.clone());
                    out.push_str(&format!("*I {cell_name}:{pin_name} {dir} *D {cell_type}\n"));
                }
                None => out.push_str(&format!("*P {pin_name} {dir}\n")),
            }
        }
        out.push_str("*CAP\n");
        let mut serial = 0u32;
        for (v, name) in node_names.iter().enumerate() {
            serial += 1;
            out.push_str(&format!("{serial} {name} {}\n", tree.wire_cap_pf[v]));
        }
        for (name, _) in &stubs {
            serial += 1;
            out.push_str(&format!("{serial} {name} 0\n"));
        }
        out.push_str("*RES\n");
        serial = 0;
        for v in 1..nnodes {
            // Self-parented dead-component roots emit no resistor — no edge
            // joins an island to the driver's component.
            if tree.parent[v] as usize == v {
                continue;
            }
            serial += 1;
            out.push_str(&format!(
                "{serial} {} {} {}\n",
                node_names[tree.parent[v] as usize], node_names[v], tree.edge_r_kohm[v]
            ));
        }
        for &(node, r) in &tree.self_loops {
            serial += 1;
            let name = &node_names[node as usize];
            out.push_str(&format!("{serial} {name} {name} {r}\n"));
        }
        for (name, primary) in &stubs {
            serial += 1;
            out.push_str(&format!(
                "{serial} {name} {} {}\n",
                node_names[*primary], SPEF_STUB_RES_KOHM
            ));
        }
        out.push_str("*END\n\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::congestion::rsmt::{EXACT_MAX_DEGREE, build_precise_rsmt_points};
    use crate::interner::StringInterner;
    use crate::readers::{load_liberty, load_spef, load_tech_db, parse_def, parse_spef};
    use crate::test_support::{design_dir, sky130_hd};
    use std::collections::BTreeMap;

    struct Lengths {
        table: BTreeMap<usize, (usize, usize, f64)>,
        mismatches: Vec<String>,
        /// Nets of up to `EXACT_MAX_DEGREE` points where our tree is longer than OpenROAD's.
        longer: Vec<String>,
    }

    /// Tree length of every net against OpenROAD's SPEF (`sum of *CAP / c0`), per net degree.
    /// The table maps degree to (nets, matching nets, worst excess in um).
    fn compare_lengths(design_name: &str) -> Lengths {
        let lib_dir = sky130_hd();
        let dir = design_dir(design_name);
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(
            &dir.join(format!("{design_name}.placed.def")),
            &tech,
            Some(&lib),
        )
        .expect("DEF");
        let spef = load_spef(&dir.join(format!("{design_name}.routed.spef"))).expect("SPEF");
        let c0 = WireRc::from_layer(&tech, "met2")
            .expect("met2 RC")
            .c_pf_per_um as f64;
        let dbu = tech.dbu_per_micron as f64;

        let mut table: BTreeMap<usize, (usize, usize, f64)> = BTreeMap::new();
        let (mut mismatches, mut longer) = (Vec::new(), Vec::new());
        for n in 0..design.num_nets() {
            let net = NetId(n as u32);
            let pins = design.net_pins(net);
            if design.net_driver(net).is_none() || pins.len() < 2 {
                continue;
            }
            let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
            let Some(spef_net) = spef.nets.get(&name) else {
                continue;
            };
            let pts: Vec<Pt> = pins.iter().map(|&p| design.pin_pos_dbu(p, &tech)).collect();
            let ours: f64 = build_precise_rsmt_points(&pts)
                .iter()
                .map(|&(a, b)| (a.0.abs_diff(b.0) + a.1.abs_diff(b.1)) as f64)
                .sum::<f64>()
                / dbu;
            let reference = spef_net.caps.iter().map(|(_, c)| *c as f64).sum::<f64>() / c0;
            // The SPEF prints 6 significant digits of capacitance: allow 1e-4 relative or 1 DBU.
            let matches = (ours - reference).abs() <= (1e-4 * reference).max(1.0 / dbu);
            let entry = table.entry(pins.len()).or_insert((0, 0, 0.0));
            entry.0 += 1;
            if matches {
                entry.1 += 1;
            } else {
                entry.2 = entry.2.max(ours - reference);
                if pins.len() <= EXACT_MAX_DEGREE {
                    if ours > reference {
                        longer.push(format!(
                            "{name} (degree {}): ours {ours:.4} um, SPEF {reference:.4} um",
                            pins.len()
                        ));
                    }
                    mismatches.push(format!(
                        "{name} (degree {}): ours {ours:.4} um, SPEF {reference:.4} um",
                        pins.len()
                    ));
                }
            }
        }
        Lengths {
            table,
            mismatches,
            longer,
        }
    }

    fn print_table(design_name: &str, table: &BTreeMap<usize, (usize, usize, f64)>) {
        println!("{design_name}: degree | nets | matching | worst excess over SPEF (um)");
        for (degree, (nets, matching, excess)) in table {
            println!("{degree:>7} | {nets:>5} | {matching:>8} | {excess:.4}");
        }
    }

    /// Checks what is known about OpenROAD's trees (docs/results.md, E3.4): two- and three-point nets match
    /// exactly, and the optimal tree is never longer than OpenROAD's (OpenROAD does not use the shortest tree).
    fn check_known_properties(design_name: &str) {
        let lengths = compare_lengths(design_name);
        print_table(design_name, &lengths.table);
        println!(
            "{} nets of up to {EXACT_MAX_DEGREE} points differ from OpenROAD's length",
            lengths.mismatches.len()
        );
        for degree in [2, 3] {
            let (nets, matching, _) = lengths.table[&degree];
            assert_eq!(
                nets, matching,
                "{design_name}: {degree}-point nets must match OpenROAD exactly"
            );
        }
        assert!(
            lengths.longer.is_empty(),
            "{design_name}: tree longer than OpenROAD's: {:?}",
            lengths.longer
        );
    }

    /// Edge-length multiset of our PD tree vs OpenROAD's placement-estimate
    /// SPEF resistors (wire resistors only; the 1e-3 kohm pin/via and
    /// zero-length links are filtered on both sides).
    #[test]
    fn test_pd_edge_multiset_vs_placement_spef() {
        for design_name in ["gcd", "picorv32", "aes", "ibex", "jpeg"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let spef = load_spef(&dir.join(format!("{design_name}.routed.spef"))).expect("SPEF");
            let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");
            let dbu = tech.dbu_per_micron as f64;
            let (mut nets, mut mismatched, mut cap_mismatches) = (0usize, 0usize, 0usize);
            let (mut examples, mut cap_examples) = (Vec::new(), Vec::new());
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let pins = design.net_pins(net);
                if design.net_driver(net).is_none() || pins.len() < 2 {
                    continue;
                }
                let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
                let Some(spef_net) = spef.nets.get(&name) else {
                    continue;
                };
                nets += 1;
                let pts: Vec<Pt> = pins.iter().map(|&p| design.pin_pos_dbu(p, &tech)).collect();
                let mut ours: Vec<f64> = crate::congestion::pdr::build_pd_points(&pts)
                    .iter()
                    .map(|&(a, b)| (a.0.abs_diff(b.0) + a.1.abs_diff(b.1)) as f64 / dbu)
                    .filter(|&l| l > 0.0)
                    .collect();
                ours.sort_by(|a, b| a.partial_cmp(b).unwrap());
                // Pin/via and zero-length links are exactly 1e-6 kohm in the
                // SPEF; the shortest real wire (1 DBU) is ~2.6e-6 kohm, so an
                // exact-match filter cannot drop wire.
                let mut reference: Vec<f64> = spef_net
                    .resistors
                    .iter()
                    .map(|(_, _, r)| *r as f64)
                    .filter(|&r| (r - 1e-6).abs() > 1e-9)
                    .map(|r| r / rc.r_kohm_per_um as f64)
                    .collect();
                reference.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let same = ours.len() == reference.len()
                    && ours
                        .iter()
                        .zip(&reference)
                        .all(|(&a, &b)| (a - b).abs() <= (1e-3 * b).max(0.01));
                if !same {
                    mismatched += 1;
                    if examples.len() < 5 {
                        examples.push(format!(
                            "{name} deg{} ours_edges={} ref_edges={}",
                            pins.len(),
                            ours.len(),
                            reference.len()
                        ));
                    }
                }

                // Node-cap multiset: each tree node's wire capacitance is
                // c0/2 * sum of incident edge lengths on both sides. A few
                // nets differ by one node where coincident points are merged
                // by coordinate (`intern` keys on Pt); total wire capacitance
                // is unaffected since it depends only on total length, and a
                // merge of zero-edge-adjacent nodes is electrically exact.
                // Diagnostic only: timing parity is asserted by the
                // placement-RC OpenSTA tests, not here.
                if let Some(tree) = build_net_tree(
                    &design,
                    &tech,
                    rc,
                    net,
                    &crate::congestion::pdr::build_pd_points,
                ) {
                    let mut ours_caps: Vec<f64> =
                        tree.wire_cap_pf.iter().map(|&c| c as f64).collect();
                    ours_caps.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let mut ref_caps: Vec<f64> =
                        spef_net.caps.iter().map(|(_, c)| *c as f64).collect();
                    ref_caps.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let caps_same = ours_caps.len() == ref_caps.len()
                        && ours_caps
                            .iter()
                            .zip(&ref_caps)
                            .all(|(&a, &b)| (a - b).abs() <= (1e-3 * b).max(1e-6));
                    if !caps_same {
                        cap_mismatches += 1;
                        if cap_examples.len() < 5 {
                            cap_examples.push(format!(
                                "{name} deg{} ours_nodes={} ref_nodes={}",
                                pins.len(),
                                ours_caps.len(),
                                ref_caps.len()
                            ));
                        }
                    }
                }
            }
            println!(
                "{design_name}: {nets} nets, {mismatched} edge-multiset mismatches, {cap_mismatches} cap-multiset mismatches"
            );
            for e in examples {
                println!("  {e}");
            }
            for e in cap_examples {
                println!("  cap {e}");
            }
            // Node-cap multiset may differ by one node on nets with coincident
            // points (merged by coordinate in `intern`); the counts below are
            // the measured 2026-11-18 values and may only shrink.
            let expected_cap_mismatches = match design_name {
                "ibex" => 8,
                "jpeg" => 1,
                _ => 0,
            };
            assert_eq!(
                cap_mismatches, expected_cap_mismatches,
                "{design_name}: coincident-merge node count changed; re-measure before updating"
            );
            assert_eq!(
                mismatched, 0,
                "{design_name}: PD tree edge multiset must match OpenROAD's placement SPEF"
            );
        }
    }

    /// PD-built RC trees vs the same nets read back from OpenROAD's placement
    /// SPEF: per-pin Elmore delays must agree both for the bare wire and with a
    /// uniform 1 fF load at every pin (which exercises the pin->node mapping).
    /// This is the evidence that `placement_rc` measures the same engine
    /// residual as `placed_spef`, not the estimator.
    #[test]
    fn test_pd_vs_spef_parasitics_agree() {
        for (design_name, expected_nets) in [
            ("gcd", 268usize),
            ("picorv32", 6743),
            ("aes", 9038),
            ("ibex", 12127),
            ("jpeg", 46567),
        ] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let spef = load_spef(&dir.join(format!("{design_name}.routed.spef"))).expect("SPEF");
            let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");
            let (mut nets, mut worst_wire, mut worst_loaded) = (0usize, 0.0f64, 0.0f64);
            let (mut worst_wire_net, mut worst_loaded_net) = (String::new(), String::new());
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let pins = design.net_pins(net);
                if design.net_driver(net).is_none() || pins.len() < 2 {
                    continue;
                }
                let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
                // SPEF legitimately omits some nets (e.g. the ideal-clock port
                // net); the asserted count below pins the covered set.
                let Some(spef_net) = spef.nets.get(&name) else {
                    continue;
                };
                nets += 1;
                let pd = build_net_tree(
                    &design,
                    &tech,
                    rc,
                    net,
                    &crate::congestion::pdr::build_pd_points,
                )
                .expect("PD tree");
                let sf = spef_net_tree(&design, net, &name, spef_net).expect("SPEF tree");
                for caps in [vec![0.0f32; pins.len()], vec![1.0f32; pins.len()]] {
                    let (pd_d, sf_d) = (pd.elmore_ns(&caps), sf.elmore_ns(&caps));
                    assert_eq!(
                        pd_d.len(),
                        pins.len(),
                        "{design_name}/{name}: PD Elmore pin count"
                    );
                    assert_eq!(
                        sf_d.len(),
                        pins.len(),
                        "{design_name}/{name}: SPEF Elmore pin count"
                    );
                    for (&a, &b) in pd_d.iter().zip(sf_d.iter()) {
                        let d = (a as f64 - b as f64).abs();
                        if caps[0] == 0.0 && d > worst_wire {
                            worst_wire = d;
                            worst_wire_net = name.clone();
                        } else if caps[0] != 0.0 && d > worst_loaded {
                            worst_loaded = d;
                            worst_loaded_net = name.clone();
                        }
                    }
                }
            }
            assert_eq!(nets, expected_nets, "{design_name}: compared-net count");
            println!(
                "{design_name}: {nets} nets, worst Elmore |diff| wire {worst_wire:.6} ns ({worst_wire_net}), loaded {worst_loaded:.6} ns ({worst_loaded_net})"
            );
            // Measured 2026-11-18 (printed per design above): bare-wire worst
            // 3e-6 ns (jpeg `ena`), loaded worst 3.8e-3 ns (jpeg `ena`; SPEF pin
            // nodes hang off 1e-6 kohm links and coincident-point merges shift
            // where the uniform load lands). Bounds: wire 1e-5 (~3x headroom),
            // loaded 5e-3 = the strict timing floor — per-pin Elmore agrees
            // within it even under a uniform load.
            assert!(
                worst_wire <= 1e-5,
                "{design_name}: PD and SPEF wire parasitics diverge by {worst_wire} ns"
            );
            assert!(
                worst_loaded <= 5e-3,
                "{design_name}: PD and SPEF loaded parasitics diverge by {worst_loaded} ns"
            );
        }
    }

    /// Per-net PD tree length vs OpenROAD's placement-estimate SPEF
    /// (`*.routed.spef` is the `-placement -spef_file` output for these designs).
    #[test]
    fn test_pd_length_vs_placement_spef() {
        for design_name in ["gcd", "picorv32", "aes", "ibex", "jpeg"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let spef = load_spef(&dir.join(format!("{design_name}.routed.spef"))).expect("SPEF");
            let c0 = WireRc::from_layer(&tech, "met2")
                .expect("met2 RC")
                .c_pf_per_um as f64;
            let dbu = tech.dbu_per_micron as f64;
            let (mut nets, mut match_n, mut worst) = (0usize, 0usize, 0.0f64);
            let mut worst_net = String::new();
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let pins = design.net_pins(net);
                if design.net_driver(net).is_none() || pins.len() < 2 {
                    continue;
                }
                let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
                let Some(spef_net) = spef.nets.get(&name) else {
                    continue;
                };
                let pts: Vec<Pt> = pins.iter().map(|&p| design.pin_pos_dbu(p, &tech)).collect();
                let ours: f64 = crate::congestion::pdr::build_pd_points(&pts)
                    .iter()
                    .map(|&(a, b)| (a.0.abs_diff(b.0) + a.1.abs_diff(b.1)) as f64)
                    .sum::<f64>()
                    / dbu;
                let reference = spef_net.caps.iter().map(|(_, c)| *c as f64).sum::<f64>() / c0;
                nets += 1;
                let diff = (ours - reference).abs();
                let tol = (1e-4 * reference).max(1.0 / dbu);
                if diff <= tol {
                    match_n += 1;
                }
                if diff > worst {
                    worst = diff;
                    worst_net =
                        format!("{name} deg{} ours {ours:.2} ref {reference:.2}", pins.len());
                }
            }
            println!(
                "{design_name}: {nets} nets, {match_n} length-match, worst |diff| {worst:.3} um ({worst_net})"
            );
            assert_eq!(
                match_n, nets,
                "{design_name}: every PD tree length must match OpenROAD's SPEF"
            );
        }
    }

    /// Counts nets whose PD tree fails OpenROAD's `checkTree` (crossing
    /// bounding boxes), which is where the reference falls back to FLUTE.
    #[test]
    fn test_pd_checktree_failures() {
        for design_name in ["gcd", "picorv32", "aes", "ibex", "jpeg"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let (mut nets, mut failed) = (0usize, 0usize);
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                if design.net_driver(net).is_none() || design.net_pins(net).len() < 2 {
                    continue;
                }
                nets += 1;
                let pts: Vec<Pt> = design
                    .net_pins(net)
                    .iter()
                    .map(|&p| design.pin_pos_dbu(p, &tech))
                    .collect();
                if !crate::congestion::pdr::build_pd_tree_for_points(&pts).check_passed {
                    failed += 1;
                }
            }
            println!(
                "{design_name}: {nets} nets, {failed} PD trees failed checkTree (FLUTE fallback in OpenROAD)"
            );
            assert_eq!(
                failed, 0,
                "{design_name}: PD tree failed checkTree — OpenROAD would use FLUTE here"
            );
        }
    }

    #[test]
    fn test_steiner_length_vs_openroad_spef_gcd() {
        check_known_properties("gcd");
    }

    #[test]
    fn test_steiner_length_vs_openroad_spef_picorv32() {
        check_known_properties("picorv32");
    }

    /// N4: `write_spef` output must re-read through `parse_spef`/`spef_parasitics`
    /// into electrically identical trees — per-pin Elmore delays agree for the bare
    /// wire and under a uniform 1 fF load (which exercises the pin->node mapping).
    #[test]
    fn test_write_spef_round_trip_parasitics() {
        for design_name in ["gcd", "aes"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");
            let parasitics = estimate_placement_parasitics(&design, &tech, rc);
            let text = write_spef(&design, &lib, &parasitics).expect("write_spef");
            let spef = parse_spef(&text).expect("parse_spef");
            let reparsed = spef_parasitics(&design, &spef).expect("spef_parasitics");

            let (mut nets, mut worst) = (0usize, 0.0f64);
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let (a, b) = (parasitics.net(net), reparsed.net(net));
                assert_eq!(
                    a.is_some(),
                    b.is_some(),
                    "{design_name} net {}: tree presence differs after SPEF round-trip",
                    design.strings.resolve(design.net_names[n])
                );
                let (Some(a), Some(b)) = (a, b) else { continue };
                nets += 1;
                let pins = design.net_pins(net);
                for caps in [vec![0.0f32; pins.len()], vec![1.0f32; pins.len()]] {
                    let (da, db) = (a.elmore_ns(&caps), b.elmore_ns(&caps));
                    assert_eq!(da.len(), db.len(), "{design_name} net {n}: pin count");
                    for (&x, &y) in da.iter().zip(&db) {
                        worst = worst.max((x as f64 - y as f64).abs());
                    }
                }
            }
            println!("{design_name}: {nets} nets round-tripped, worst Elmore diff {worst:.3e} ns");
            assert!(
                worst < 1e-9,
                "{design_name}: SPEF round-trip changed Elmore delays by {worst} ns"
            );
        }
    }

    /// N4 end to end: an emitted SPEF re-read through `--spef` must reproduce the
    /// placement-RC STA report — same endpoints, same per-endpoint numbers.
    #[test]
    fn test_write_spef_round_trip_sta() {
        let lib_dir = sky130_hd();
        let dir = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("gcd.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = crate::sta::load_sdc(&dir.join("gcd.sdc")).expect("SDC");
        let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");
        let placement = estimate_placement_parasitics(&design, &tech, rc);
        let text = write_spef(&design, &lib, &placement).expect("write_spef");
        let spef = parse_spef(&text).expect("parse_spef");
        let reparsed = spef_parasitics(&design, &spef).expect("spef_parasitics");

        let a = crate::sta::run_sta(&design, &lib, &sdc, Some(&placement)).expect("placement STA");
        let b = crate::sta::run_sta(&design, &lib, &sdc, Some(&reparsed)).expect("SPEF STA");
        assert_eq!(a.endpoints.len(), b.endpoints.len());
        assert_eq!(a.wns_max_ns, b.wns_max_ns);
        assert_eq!(a.tns_max_ns, b.tns_max_ns);
        assert_eq!(a.wns_min_ns, b.wns_min_ns);
        assert_eq!(a.tns_min_ns, b.tns_min_ns);
        assert_eq!(a.setup_violations, b.setup_violations);
        assert_eq!(a.hold_violations, b.hold_violations);
        for (ea, eb) in a.endpoints.iter().zip(&b.endpoints) {
            assert_eq!(ea.pin, eb.pin);
            for (x, y) in [
                (ea.max_arr_ns, eb.max_arr_ns),
                (ea.max_req_ns, eb.max_req_ns),
                (ea.max_slack_ns, eb.max_slack_ns),
                (ea.max_slew_ns, eb.max_slew_ns),
                (ea.min_arr_ns, eb.min_arr_ns),
                (ea.min_req_ns, eb.min_req_ns),
                (ea.min_slack_ns, eb.min_slack_ns),
                (ea.min_slew_ns, eb.min_slew_ns),
            ] {
                assert_eq!(x.is_some(), y.is_some(), "{}: corner coverage", ea.pin);
                if let (Some(x), Some(y)) = (x, y) {
                    assert!(
                        (x - y).abs() < 1e-6,
                        "{}: {x} vs {y} after SPEF round-trip",
                        ea.pin
                    );
                }
            }
        }
    }

    // ---- E4.4 guide-parasitics fixtures ----

    use crate::congestion::guide::{parse_guides, resolve_guide_nets};
    use crate::design_db::PlacementStatus;
    use crate::interner::{CellId, LibPinId, MacroId, MacroPinId};
    use crate::tech_db::{MacroClass, MacroDef, MacroPin, RoutingLayer};

    /// Three routing layers on a 100-DBU GCell grid (dbu 1000): li1 is
    /// vertical, met1 horizontal, met2 vertical. Via resistance above
    /// li1 is 9.3 ohm (mcon), above met1 4.5 ohm (via1). Unit RC uses
    /// round numbers so hand-computed expectations stay readable.
    fn guide_tech() -> TechDb {
        let mut t = TechDb {
            dbu_per_micron: 1000,
            ..Default::default()
        };
        for (name, dir, res) in [
            ("li1", LayerDir::Vertical, 1.0f32),
            ("met1", LayerDir::Horizontal, 0.5),
            ("met2", LayerDir::Vertical, 0.25),
        ] {
            let sym = t.strings.intern(name);
            t.routing_layers.push(RoutingLayer {
                name: sym,
                dir,
                pitch_dbu: 100,
                width_dbu: 100,
                offset_dbu: 0,
                res_per_sq: res,
                cap_per_sq_um: 0.001,
                edge_cap_per_um: 5e-5,
                spacing_dbu: 0,
                spacing_table: Vec::new(),
            });
        }
        t.via_res_ohm_above = vec![9.3, 4.5, 0.0];
        let mac_sym = t.strings.intern("cell");
        t.macros.push(MacroDef {
            name: mac_sym,
            class: MacroClass::Core,
            width_dbu: 60,
            height_dbu: 40,
            pin_offset: 0,
            pin_count: 2,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        });
        // Pin A is an li1 box (10,10,20,20), pin B (30,10,40,20), in
        // macro coords; `pt` is each box's lower-left corner.
        for (pname, xl) in [("A", 10), ("B", 30)] {
            let sym = t.strings.intern(pname);
            t.macro_pins.push(MacroPin {
                name: sym,
                dir: PinDir::Input,
                is_power: false,
                offset_x_dbu: xl + 5,
                offset_y_dbu: 15,
                offset_x_frac: false,
                offset_y_frac: false,
                layer: Some(LayerId(0)),
                rects: vec![(LayerId(0), RectDbu::new(xl, 10, xl + 10, 20))],
            });
        }
        t.rebuild_maps();
        t
    }

    /// Five cells of `guide_tech`'s macro on a 400x300 die (4x3 tiles of
    /// 100 DBU): c0@(0,0), c1@(200,0), c2@(0,200), c3@(100,100),
    /// c4@(300,200), each with pins A (PinId 2c) and B (PinId 2c+1).
    /// Nets: n0 = c0:A -> c1:B (straight run), n1 = c0:B -> c2:B
    /// (vertical run), n2 = c3:A -> c3:B (same GCell), n3 = c4:A (one
    /// pin), n4 = c1:A -> c4:B.
    fn guide_design() -> (DesignDb, GCellGrid) {
        let mut d = DesignDb {
            die_area: RectDbu::new(0, 0, 400, 300),
            ..Default::default()
        };
        let net_syms: Vec<_> = ["n0", "n1", "n2", "n3", "n4"]
            .iter()
            .map(|s| d.strings.intern(s))
            .collect();
        d.net_names = net_syms;
        d.net_pin_offsets = vec![0, 2, 4, 6, 7, 9];
        // pin ids: A of cell c = 2c, B = 2c+1.
        d.net_pin_ids = vec![
            PinId(0),
            PinId(3), // n0: c0:A -> c1:B
            PinId(1),
            PinId(5), // n1: c0:B -> c2:B
            PinId(6),
            PinId(7), // n2: c3:A -> c3:B
            PinId(8), // n3: c4:A alone
            PinId(2),
            PinId(9), // n4: c1:A -> c4:B
        ];
        for (i, (cx, cy)) in [(0, 0), (200, 0), (0, 200), (100, 100), (300, 200)]
            .iter()
            .enumerate()
        {
            let sym = d.strings.intern(&format!("c{i}"));
            d.cell_names.push(sym);
            d.cell_macros.push(MacroId(0));
            d.cell_lib_ids.push(None);
            d.cell_status.push(PlacementStatus::Placed);
            d.cell_orient.push(Orient::N);
            d.cell_x_dbu.push(*cx);
            d.cell_y_dbu.push(*cy);
            d.cell_pin_offsets.push((2 * i) as u32);
            d.cell_pin_ids.push(PinId((2 * i) as u32));
            d.cell_pin_ids.push(PinId((2 * i + 1) as u32));
        }
        d.cell_pin_offsets.push(10);
        for i in 0..10 {
            let pname = if i % 2 == 0 { "A" } else { "B" };
            let sym = d.strings.intern(pname);
            d.pin_names.push(sym);
            d.pin_dirs.push(PinDir::Input);
            d.pin_lib_pins.push(None::<LibPinId>);
            d.pin_cells.push(Some(CellId((i / 2) as u32)));
            d.pin_macro_pins.push(Some(MacroPinId((i % 2) as u32)));
            d.pin_offset_x_dbu.push(if i % 2 == 0 { 15 } else { 35 });
            d.pin_offset_y_dbu.push(15);
            d.pin_io_layer.push(None);
            d.pin_io_rects.push(Vec::new());
        }
        // net -> pins: n0 [0,3], n1 [1,5], n2 [6,7], n3 [8], n4 [2,9].
        let net_of = [0usize, 1, 4, 0, 99, 1, 2, 2, 3, 4];
        d.pin_nets = net_of
            .iter()
            .map(|&n| (n != 99).then_some(NetId(n as u32)))
            .collect();
        // drivers: first pin of each net.
        d.pin_is_driver = vec![true, true, true, false, false, false, true, false, true, false];
        d.io_pins = Vec::new();
        let grid = GCellGrid {
            origin_x_dbu: 0,
            origin_y_dbu: 0,
            gcell_w_dbu: 100,
            gcell_h_dbu: 100,
            nx: 4,
            ny: 3,
            h_cap: vec![0.0; 9],
            h_raw_cap: vec![0.0; 9],
            h_demand: vec![0.0; 9],
            v_cap: vec![0.0; 8],
            v_raw_cap: vec![0.0; 8],
            v_demand: vec![0.0; 8],
            layer_raw_edge_tracks: Vec::new(),
            layer_orig_edge_tracks: Vec::new(),
            layer_derated_edge_tracks: Vec::new(),
        };
        (d, grid)
    }

    fn guide_box(layer: u8, xl: i32, yl: i32, xh: i32, yh: i32) -> GuideBox {
        GuideBox {
            layer: LayerId(layer),
            rect: RectDbu::new(xl, yl, xh, yh),
        }
    }

    fn sorted_edge_r(tree: &NetRcTree) -> Vec<f32> {
        let mut rs: Vec<f32> = tree.edge_r_kohm[1..].to_vec();
        rs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        rs
    }

    /// A straight 3-cell strip on met1 between two li1 stubs: pins attach
    /// through the cut via (9.3 ohm) plus a met1 pin wire from `pt` to the
    /// tile-center grid point.
    #[test]
    fn test_guide_straight_strip_rc() {
        let (design, grid) = guide_design();
        let tech = guide_tech();
        let boxes = vec![(
            NetId(0),
            vec![
                guide_box(0, 0, 0, 100, 100),   // li1 stub at (50,50)
                guide_box(1, 0, 0, 300, 100),   // met1 wire (50,50)-(250,50)
                guide_box(0, 200, 0, 300, 100), // li1 stub at (250,50)
            ],
        )];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &boxes);
        let tree = p.net(NetId(0)).expect("n0 tree");
        // 5 route nodes + 2 pin leaves; a stub pairs into the wire as a via.
        assert_eq!(tree.parent.len(), 7);
        assert_eq!(tree.pin_node.len(), 2);
        // met1: r = 0.5/0.1/1000 = 0.005 kohm/um; li1 stub->met1 via 9.3 ohm.
        // Pin wires ride conn+1 = met1: c0:A pt (10,10) is 0.08 um from
        // (50,50); c1:B pt (230,10) is 0.06 um from (250,50).
        let expect = [0.0005f32, 0.0005, 0.0093, 0.0093, 0.0096, 0.0097];
        for (r, e) in sorted_edge_r(tree).iter().zip(expect) {
            assert!((r - e).abs() < 1e-6, "edge R {r} != {e}");
        }
        // c/um = 0.001*0.1 + 2*5e-5 = 2e-4 pF/um: met1 wire 2*0.1 um and
        // pin wires 0.08+0.06 um -> 4e-5 + 2.8e-5 = 6.8e-5 pF total.
        assert!(
            (tree.total_wire_cap_pf - 6.8e-5).abs() < 1e-9,
            "total cap {}",
            tree.total_wire_cap_pf
        );
    }

    /// A met2 vertical run between met1 stubs: the layer-pair links are
    /// via1 resistors (4.5 ohm), and a trailing li1 stub that pairs with
    /// nothing stays a dead self-loop island — no edge joins it to the
    /// driver tree; it surfaces as a `n n 0.0093` self-loop resistor.
    #[test]
    fn test_guide_via_transition() {
        let (design, grid) = guide_design();
        let tech = guide_tech();
        let boxes = vec![(
            NetId(1),
            vec![
                guide_box(0, 0, 0, 100, 100),   // li1 stub (50,50)
                guide_box(1, 0, 0, 100, 100),   // met1 stub (50,50)
                guide_box(2, 0, 0, 100, 300),   // met2 wire (50,50)-(50,250)
                guide_box(1, 0, 200, 100, 300), // met1 stub (50,250)
                guide_box(0, 0, 200, 100, 300), // li1 stub (50,250) -> dead island
            ],
        )];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &boxes);
        let tree = p.net(NetId(1)).expect("n1 tree");
        // 7 route nodes (li1(50,50), m1(50,50), m2(50,50), m2(50,150),
        // m2(50,250), m1(50,250), li1(50,250)) + 2 pin leaves; li1(50,250)
        // is a self-parented dead root carrying a 0.0093 self-loop.
        assert_eq!(tree.parent.len(), 9);
        assert_eq!(tree.dead.iter().filter(|&&d| d).count(), 1);
        assert_eq!(tree.self_loops.len(), 1);
        assert!((tree.self_loops[0].1 - 0.0093).abs() < 1e-6);
        let li1_island = tree.self_loops[0].0 as usize;
        assert!(tree.dead[li1_island]);
        assert_eq!(tree.parent[li1_island] as usize, li1_island);
        let rs = sorted_edge_r(tree);
        // met2: r = 0.25/0.1/1000 = 0.0025 kohm/um; 2 x 0.1 um links.
        // Vias: li1->m1 at (50,50) 9.3 ohm; m1->m2 twice at 4.5 ohm.
        // Pin wires on met1: c0:B pt (30,10) -> 0.06 um; c2:B pt (30,210)
        // -> 0.06 um, each plus the 9.3 ohm cut. The dead root's 0.0 edge
        // sorts first — it is the sentinel, not a real resistor.
        let expect = [
            0.0f32, 0.00025, 0.00025, 0.0045, 0.0045, 0.0093, 0.0096, 0.0096,
        ];
        assert_eq!(rs.len(), expect.len());
        for (r, e) in rs.iter().zip(expect) {
            assert!((r - e).abs() < 1e-6, "edge R {r} != {e} (all: {rs:?})");
        }
        // met2 wire 0.2 um + met1 pin wires 0.12 um -> 4e-5 + 2.4e-5.
        assert!(
            (tree.total_wire_cap_pf - 6.4e-5).abs() < 1e-9,
            "total cap {}",
            tree.total_wire_cap_pf
        );
    }

    /// Dead-island semantics end to end on a small fixture: a sink pin
    /// attached to an unpaired-stub island keeps its caps in
    /// `total_wire_cap_pf` but gets zero wire delay, and `write_spef`
    /// emits `n n r` self-loops with no resistor joining the island to
    /// the driver component — OpenROAD's `-global_routing` shape.
    #[test]
    fn test_guide_island_dead_semantics() {
        let (design, grid) = guide_design();
        let tech = guide_tech();
        // n0 = c0:A -> c1:B. The met1 strip covers cells (0,0)-(1,0) only;
        // the sink cell (2,0) carries a lone li1 stub -> dead island that
        // c1:B's pin resistor lands on. The met1 stub at (50,50) stays
        // unpaired (no met2 coverage) -> self-loop on a live node.
        let boxes = vec![(
            NetId(0),
            vec![
                guide_box(0, 0, 0, 100, 100),   // li1 stub (50,50)
                guide_box(1, 0, 0, 100, 100),   // met1 stub (50,50) -> self-loop
                guide_box(1, 0, 0, 200, 100),   // met1 wire (50,50)-(150,50)
                guide_box(0, 200, 0, 300, 100), // li1 stub (250,50) -> island
            ],
        )];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &boxes);
        let tree = p.net(NetId(0)).expect("n0 tree");
        // Nodes: li1(50,50), m1(50,50), m1(150,50), li1(250,50) + 2 leaves.
        assert_eq!(tree.parent.len(), 6);
        // The island chain li1(250,50) -> sink leaf is dead; pin 1 (c1:B)
        // gets no wire delay while the driver pin keeps a real one.
        assert!(!tree.dead[tree.pin_node[0] as usize]);
        assert!(tree.dead[tree.pin_node[1] as usize]);
        let delays = tree.elmore_ns(&[0.0, 0.0]);
        assert_eq!(delays[1], 0.0, "island pin must see no wire delay");
        // Both stub self-loops survive: met1 (0.0045) on the live node,
        // li1 (0.0093) on the dead one.
        let mut loop_rs: Vec<f32> = tree.self_loops.iter().map(|&(_, r)| r).collect();
        loop_rs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(loop_rs.len(), 2);
        assert!((loop_rs[0] - 0.0045).abs() < 1e-6);
        assert!((loop_rs[1] - 0.0093).abs() < 1e-6);
        // met1 wire 0.1 um (2e-5) + pin wires 0.08 (m1) + 0.06 (li1) um
        // -> 2e-5 + 1.6e-5 + 1.2e-5; the island pin wire still counts.
        assert!(
            (tree.total_wire_cap_pf - 4.8e-5).abs() < 1e-9,
            "total cap {}",
            tree.total_wire_cap_pf
        );

        // The emitted SPEF must not bridge the island: every resistor is
        // either fully inside the dead component or fully outside; the two
        // self-loops appear verbatim. Round-trip re-read keeps the dead pin.
        let text = write_spef(&design, &LibDb::default(), &p).expect("write_spef");
        let spef = parse_spef(&text).expect("parse_spef");
        let net = &spef.nets["n0"];
        // The dead component is exactly {dead root, sink pin leaf} here.
        let mut dead_root = tree.pin_node[1] as usize;
        while tree.parent[dead_root] as usize != dead_root {
            dead_root = tree.parent[dead_root] as usize;
        }
        let island_name = format!("n0:{dead_root}");
        let on_island = |name: &str| name == island_name || name == "c1:B";
        for (a, b, _) in &net.resistors {
            assert_eq!(
                on_island(a),
                on_island(b),
                "resistor {a} {b} bridges the dead component"
            );
        }
        let mut self_rs: Vec<f32> = net
            .resistors
            .iter()
            .filter(|(a, b, _)| a == b)
            .map(|(_, _, r)| *r)
            .collect();
        self_rs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(self_rs.len(), 2);
        assert!((self_rs[0] - 0.0045).abs() < 1e-6, "{self_rs:?}");
        assert!((self_rs[1] - 0.0093).abs() < 1e-6, "{self_rs:?}");
        let reparsed = spef_parasitics(&design, &spef).expect("spef_parasitics");
        let rt = reparsed.net(NetId(0)).expect("round-trip n0 tree");
        assert!(rt.dead[rt.pin_node[1] as usize]);
        assert_eq!(rt.elmore_ns(&[0.0, 0.0])[1], 0.0);
        assert_eq!(rt.total_wire_cap_pf, tree.total_wire_cap_pf);
    }

    /// The reader accepts a SPEF whose sink pin sits on a component the
    /// driver cannot reach (OpenROAD writes island pins into `*CONN`
    /// anyway): the pin maps dead instead of bailing.
    #[test]
    fn test_spef_dead_component_pin() {
        let (design, _grid) = guide_design();
        // n0 = c0:A -> c1:B; the sink lands on a dangling stub node.
        let text = concat!(
            "*SPEF \"ieee 1481-1999\"\n*DESIGN \"guide_design\"\n",
            "*DATE \"00:00:00 Thu 01 01, 1970\"\n*VENDOR \"t\"\n*PROGRAM \"t\"\n*VERSION \"0\"\n",
            "*DESIGN_FLOW \"PIN_CAP NONE\"\n*DIVIDER /\n*DELIMITER :\n*BUS_DELIMITER []\n",
            "*T_UNIT 1 NS\n*C_UNIT 1 PF\n*R_UNIT 1 KOHM\n*L_UNIT 1 HENRY\n\n",
            "*D_NET n0 0.0001\n*CONN\n*I c0:A O *D cell\n*I c1:B I *D cell\n",
            "*CAP\n1 c0:A 0.00005\n2 n0:1 0.00005\n3 n0:2 0\n4 c1:B 0\n",
            "*RES\n1 c0:A n0:1 0.01\n2 n0:2 n0:2 0.0093\n3 n0:2 c1:B 0.001\n*END\n",
        );
        let spef = parse_spef(text).expect("parse_spef");
        let p = spef_parasitics(&design, &spef).expect("dead pin must not bail");
        let tree = p.net(NetId(0)).expect("n0 tree");
        assert!(tree.dead[tree.pin_node[1] as usize]);
        assert_eq!(tree.elmore_ns(&[0.0, 0.0])[1], 0.0);
        assert_eq!(tree.self_loops.len(), 1);
        assert!((tree.total_wire_cap_pf - 0.0001).abs() < 1e-9);
    }

    /// OpenROAD's own `gcd.guide.spef` carries dead islands (`_057_`'s
    /// island pin): `spef_parasitics` reads it now instead of bailing.
    #[test]
    fn test_spef_parasitics_accepts_openroad_guide_spef() {
        let lib_dir = sky130_hd();
        let dir = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("gcd.placed.def"), &tech, Some(&lib)).expect("DEF");
        let spef = load_spef(&dir.join("gcd.guide.spef")).expect("guide SPEF");
        let p = spef_parasitics(&design, &spef).expect("guide SPEF with islands must read");
        let mut dead_nets = 0;
        for n in 0..design.num_nets() {
            if let Some(t) = p.net(NetId(n as u32)) {
                dead_nets += t.dead.iter().any(|&d| d) as usize;
            }
        }
        // Measured 2026-11-22: 152 of 269 guide nets carry >= 1 island node
        // (self-loops on live nodes account for the rest of the 325 loops).
        assert_eq!(dead_nets, 152, "dead-island net census changed");
    }

    /// Both pins in one GCell: stub boxes only, no route — the tree must
    /// still connect driver and sink (through the met1 stub node).
    #[test]
    fn test_guide_pin_pair_one_gcell() {
        let (design, grid) = guide_design();
        let tech = guide_tech();
        let boxes = vec![(
            NetId(2),
            vec![
                guide_box(0, 100, 100, 200, 200), // li1 stub (150,150)
                guide_box(1, 100, 100, 200, 200), // met1 stub (150,150)
            ],
        )];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &boxes);
        let tree = p.net(NetId(2)).expect("n2 tree");
        // 2 route nodes + 2 pin leaves; distinct pin leaves even though
        // both attach to the same met1 node.
        assert_eq!(tree.parent.len(), 4);
        assert_ne!(tree.pin_node[0], tree.pin_node[1]);
        // c3:A pt (110,110) -> 0.08 um; c3:B pt (130,110) -> 0.06 um.
        let expect = [0.0093f32, 0.0096, 0.0097];
        for (r, e) in sorted_edge_r(tree).iter().zip(expect) {
            assert!((r - e).abs() < 1e-6, "edge R {r} != {e}");
        }
        assert!(
            (tree.total_wire_cap_pf - 2.8e-5).abs() < 1e-9,
            "total cap {}",
            tree.total_wire_cap_pf
        );
    }

    /// Usable-net filter: < 2 pins, no boxes, or boxes that emit no route
    /// nodes (a 2D blob) all yield `None`.
    #[test]
    fn test_guide_skips() {
        let (design, grid) = guide_design();
        let tech = guide_tech();
        let boxes = vec![
            (NetId(3), vec![guide_box(1, 300, 200, 400, 300)]),
            (NetId(4), vec![]),
        ];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &boxes);
        assert!(p.net(NetId(3)).is_none(), "1-pin net must be skipped");
        assert!(p.net(NetId(4)).is_none(), "no boxes must be skipped");
        let blob = vec![(NetId(4), vec![guide_box(1, 0, 0, 200, 200)])];
        let p = estimate_guide_parasitics(&design, &tech, &grid, &blob);
        assert!(
            p.net(NetId(4)).is_none(),
            "a box covering >1 cell in both dims emits no route nodes"
        );
    }

    /// `resolve_guide_nets` maps raw DEF spellings (including escapes)
    /// and bare names; an unknown guide net is a hard error.
    #[test]
    fn test_resolve_guide_nets() {
        let (mut design, _) = guide_design();
        let esc = design.strings.intern("net\\[0\\]");
        // n1 is spelled `net\[0\]` in the DEF; the rest keep their names.
        design.net_def_spellings = vec![
            design.net_names[0],
            esc,
            design.net_names[2],
            design.net_names[3],
            design.net_names[4],
        ];
        let tech = guide_tech();
        let parsed = parse_guides("n0\n(\n0 0 100 100 met1\n)\n", &tech).unwrap();
        let resolved = resolve_guide_nets(&design, &parsed).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, NetId(0));
        let parsed = parse_guides("net\\[0\\]\n(\n0 0 100 100 met1\n)\n", &tech).unwrap();
        // `net\[0\]` is n1's DEF spelling in the fixture.
        let resolved = resolve_guide_nets(&design, &parsed).unwrap();
        assert_eq!(resolved[0].0, NetId(1));
        let parsed = parse_guides("ghost\n(\n0 0 100 100 met1\n)\n", &tech).unwrap();
        assert!(
            resolve_guide_nets(&design, &parsed).is_err(),
            "unknown guide net must error, not skip"
        );
    }

    /// Guide-parasitics SPEF round-trip: `write_spef` on the guide model
    /// must re-read through `parse_spef`/`spef_parasitics` into the same
    /// per-net totals and pin coverage — the `sta --spef` consumer path.
    #[test]
    fn test_guide_parasitics_spef_round_trip() {
        let lib_dir = sky130_hd();
        let dir = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(
            &dir.join("gcd.placed.def"),
            &tech,
            Some(&lib),
        )
        .expect("DEF");
        let grid = GCellGrid::build(&design, &tech, None, None);
        let parsed =
            parse_guides(&std::fs::read_to_string(dir.join("gcd.guide")).unwrap(), &tech).unwrap();
        let net_boxes = resolve_guide_nets(&design, &parsed).unwrap();
        let parasitics = estimate_guide_parasitics(&design, &tech, &grid, &net_boxes);
        let text = write_spef(&design, &lib, &parasitics).expect("write_spef");
        let spef = parse_spef(&text).expect("parse_spef");
        let reparsed = spef_parasitics(&design, &spef).expect("spef_parasitics");

        let (mut nets, mut worst_cap, mut worst_ns) = (0usize, 0.0f64, 0.0f64);
        for n in 0..design.num_nets() {
            let net = NetId(n as u32);
            let (a, b) = (parasitics.net(net), reparsed.net(net));
            assert_eq!(
                a.is_some(),
                b.is_some(),
                "net {}: tree presence differs after SPEF round-trip",
                design.strings.resolve(design.net_names[n])
            );
            let (Some(a), Some(b)) = (a, b) else { continue };
            nets += 1;
            worst_cap = worst_cap.max((a.total_wire_cap_pf - b.total_wire_cap_pf).abs() as f64);
            let pins = design.net_pins(net);
            let caps = vec![1.0f32; pins.len()];
            for (&x, &y) in a.elmore_ns(&caps).iter().zip(&b.elmore_ns(&caps)) {
                worst_ns = worst_ns.max((x as f64 - y as f64).abs());
            }
        }
        println!(
            "gcd guide SPEF round-trip: {nets} nets, worst cap diff {worst_cap:.3e} pF, \
             worst Elmore diff {worst_ns:.3e} ns"
        );
        assert!(worst_cap < 1e-6, "cap drifted by {worst_cap} pF");
        assert!(worst_ns < 1e-9, "Elmore drifted by {worst_ns} ns");
    }

    /// e4-topology census gate on our own emitted guides (route ->
    /// `build_guides` -> `estimate_guide_parasitics`, no golden files):
    /// no `*P` port pin may land on a dead island — a dead port forces
    /// OpenSTA's lumped fallback (gcd `resp_val` was the +250 ps e2e
    /// extreme before the rung/bend coverage) — and no dead component
    /// may chain past its self-parented root: every remaining island is
    /// a one-node unpaired stub, the same class OpenROAD's reference
    /// guide carries.
    #[test]
    fn test_guide_parasitics_no_dead_ports_or_multi_node_islands() {
        for design_name in ["gcd", "picorv32"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &crate::test_support::require(dir.join(format!("{design_name}.placed.def"))),
                &tech,
                None,
            )
            .expect("DEF");
            let routed = crate::congestion::route_design(&design, &tech, None, None);
            let guides = crate::congestion::guide::build_guides(&design, &tech, &routed);
            let net_boxes: Vec<(NetId, Vec<GuideBox>)> =
                guides.iter().map(|g| (g.net, g.boxes.clone())).collect();
            let p = estimate_guide_parasitics(&design, &tech, &routed.grid, &net_boxes);

            let (mut dead_ports, mut dead_chained) = (0usize, 0usize);
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let Some(t) = p.net(net) else { continue };
                for (i, &pin) in design.net_pins(net).iter().enumerate() {
                    if design.pin_cells[pin.0 as usize].is_none()
                        && t.dead[t.pin_node[i] as usize]
                    {
                        dead_ports += 1;
                    }
                }
                dead_chained += (0..t.dead.len())
                    .filter(|&i| {
                        t.dead[i]
                            && t.parent[i] as usize != i
                            && t.dead[t.parent[i] as usize]
                    })
                    .count();
            }
            assert_eq!(
                dead_ports, 0,
                "{design_name}: port pins left on dead guide islands"
            );
            assert_eq!(
                dead_chained, 0,
                "{design_name}: multi-node dead islands remain in emitted guide"
            );
        }
    }

    /// Direction (a) gate: our guide-RC model on OpenROAD's committed
    /// guide must reproduce `estimate_parasitics -global_routing`'s
    /// per-net `*D_NET` totals within `max(5%, 1 fF)`. Names normalize by
    /// stripping backslashes — the caps file keeps the raw DEF spelling
    /// and both files escape bus brackets differently.
    fn guide_caps_match(design_name: &str) {
        let lib_dir = sky130_hd();
        let dir = design_dir(design_name);
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(
            &crate::test_support::require(dir.join(format!("{design_name}.placed.def"))),
            &tech,
            Some(&lib),
        )
        .expect("DEF");
        let grid = GCellGrid::build(&design, &tech, None, None);
        let guide_text = std::fs::read_to_string(crate::test_support::require(
            dir.join(format!("{design_name}.guide")),
        ))
        .unwrap();
        let parsed = parse_guides(&guide_text, &tech).unwrap();
        let net_boxes = resolve_guide_nets(&design, &parsed).unwrap();
        let parasitics = estimate_guide_parasitics(&design, &tech, &grid, &net_boxes);
        // our name -> cap, names unescaped like the caps-file join key.
        let mut ours: BTreeMap<String, f32> = BTreeMap::new();
        for n in 0..design.num_nets() {
            let net = NetId(n as u32);
            if let Some(t) = parasitics.net(net) {
                let name = crate::congestion::guide::guide_net_name(&design, net)
                    .replace('\\', "");
                ours.insert(name, t.total_wire_cap_pf);
            }
        }
        let caps_text = std::fs::read_to_string(crate::test_support::require(
            dir.join(format!("{design_name}.guide.caps.txt")),
        ))
        .unwrap();
        let mut refs: BTreeMap<String, f64> = BTreeMap::new();
        for line in caps_text.lines() {
            let (name, cap) = line.split_once('\t').expect("caps line");
            refs.insert(name.replace('\\', ""), cap.parse().unwrap());
        }

        let (mut compared, mut inside, mut missing) = (0usize, 0usize, 0usize);
        let (mut sum_ours, mut sum_ref) = (0.0f64, 0.0f64);
        let mut rels: Vec<f64> = Vec::new();
        let mut worst: Vec<(String, f64, f64, f64)> = Vec::new();
        for (name, &ref_cap) in &refs {
            compared += 1;
            let Some(&our_cap) = ours.get(name) else {
                missing += 1;
                worst.push((name.clone(), f64::INFINITY, ref_cap, 0.0));
                continue;
            };
            sum_ours += our_cap as f64;
            sum_ref += ref_cap;
            let diff = (our_cap as f64 - ref_cap).abs();
            let rel = diff / ref_cap;
            rels.push(rel);
            if diff <= (0.05 * ref_cap).max(1e-3) {
                inside += 1;
            }
            worst.push((name.clone(), rel, ref_cap, our_cap as f64));
        }
        rels.sort_by(|a, b| a.partial_cmp(b).unwrap());
        worst.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let n = rels.len();
        let (med, p90) = (rels[n / 2], rels[(0.9 * (n - 1) as f64) as usize]);
        println!(
            "{design_name}: {compared} reference nets, {missing} missing on our side, \
             {inside}/{compared} within max(5%, 1 fF)\n  sum ours {sum_ours:.6} pF, \
             sum ref {sum_ref:.6} pF, rel diff median {med:.2e} p90 {p90:.2e} max {:.2e}",
            rels[n - 1]
        );
        println!("  worst 5:");
        for (name, rel, rc, oc) in worst.iter().take(5) {
            println!("    {name}: ours {oc:.6} ref {rc:.6} rel {rel:.2e}");
        }
        assert_eq!(missing, 0, "{design_name}: reference nets with no tree");
        assert_eq!(
            inside, compared,
            "{design_name}: {} nets outside max(5%, 1 fF)",
            compared - inside
        );
    }

    #[test]
    fn test_gcd_guide_parasitics_match_openroad() {
        guide_caps_match("gcd");
    }

    #[test]
    fn test_picorv32_guide_parasitics_match_openroad() {
        guide_caps_match("picorv32");
    }

    /// The builder is deterministic: two runs produce bit-identical
    /// per-net totals (ordering/HashMap nondeterminism guard).
    #[test]
    fn test_guide_parasitics_deterministic() {
        let lib_dir = sky130_hd();
        let dir = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("gcd.placed.def"), &tech, Some(&lib)).expect("DEF");
        let grid = GCellGrid::build(&design, &tech, None, None);
        let parsed =
            parse_guides(&std::fs::read_to_string(dir.join("gcd.guide")).unwrap(), &tech).unwrap();
        let net_boxes = resolve_guide_nets(&design, &parsed).unwrap();
        let a = estimate_guide_parasitics(&design, &tech, &grid, &net_boxes);
        let b = estimate_guide_parasitics(&design, &tech, &grid, &net_boxes);
        for n in 0..design.num_nets() {
            let net = NetId(n as u32);
            assert_eq!(
                a.net(net).map(|t| t.total_wire_cap_pf),
                b.net(net).map(|t| t.total_wire_cap_pf),
                "net {n} differs between runs"
            );
        }
    }
}
