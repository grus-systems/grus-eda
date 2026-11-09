use crate::congestion::rsmt::{Pt, build_rsmt_points};
use crate::design_db::DesignDb;
use crate::interner::{NetId, PinId};
use crate::tech_db::TechDb;
use anyhow::{Result, bail};
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
/// precedes its children and both Elmore passes are plain loops.
#[derive(Clone, Debug)]
pub struct NetRcTree {
    parent: Vec<u32>,
    edge_r_kohm: Vec<f32>,
    wire_cap_pf: Vec<f32>,
    /// Tree node of each pin of the net, in `DesignDb::net_pins` order.
    pin_node: Vec<u32>,
    pub total_wire_cap_pf: f32,
}

impl NetRcTree {
    /// Elmore delay (ns) from the driver to each pin of the net, in `net_pins` order.
    /// `pin_caps_pf[i]` is the pin capacitance of pin `i` for the transition being analysed.
    pub fn elmore_ns(&self, pin_caps_pf: &[f32]) -> Vec<f32> {
        let n = self.parent.len();
        let mut downstream = self.wire_cap_pf.clone();
        for (pin, &node) in self.pin_node.iter().enumerate() {
            downstream[node as usize] += pin_caps_pf[pin];
        }
        for v in (1..n).rev() {
            let p = self.parent[v] as usize;
            downstream[p] += downstream[v];
        }
        let mut tau = vec![0.0f32; n];
        for v in 1..n {
            tau[v] = tau[self.parent[v] as usize] + self.edge_r_kohm[v] * downstream[v];
        }
        self.pin_node.iter().map(|&node| tau[node as usize]).collect()
    }
}

/// Placement-based parasitic estimate: one RC tree per net, `None` for nets without a driver
/// or with a single pin.
#[derive(Clone, Debug)]
pub struct PlacementParasitics {
    nets: Vec<Option<NetRcTree>>,
}

impl PlacementParasitics {
    pub fn net(&self, net: NetId) -> Option<&NetRcTree> {
        self.nets[net.0 as usize].as_ref()
    }
}

pub fn estimate_placement_parasitics(design: &DesignDb, tech: &TechDb, rc: WireRc) -> PlacementParasitics {
    let nets = (0..design.num_nets())
        .map(|n| build_net_tree(design, tech, rc, NetId(n as u32)))
        .collect();
    PlacementParasitics { nets }
}

fn build_net_tree(design: &DesignDb, tech: &TechDb, rc: WireRc, net: NetId) -> Option<NetRcTree> {
    design.net_driver(net)?;
    let pins: &[PinId] = design.net_pins(net);
    if pins.len() < 2 {
        return None;
    }
    let dbu_per_um = tech.dbu_per_micron as f32;
    let pin_pts: Vec<Pt> = pins
        .iter()
        .map(|&p| {
            let (x, y) = design.pin_pos_dbu(p, tech);
            (x, y)
        })
        .collect();

    let mut points: Vec<Pt> = Vec::new();
    let mut index: HashMap<Pt, usize> = HashMap::new();
    let mut intern = |p: Pt, points: &mut Vec<Pt>| {
        *index.entry(p).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    let pin_point: Vec<usize> = pin_pts.iter().map(|&p| intern(p, &mut points)).collect();
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (a, b) in build_rsmt_points(&pin_pts) {
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
            let length_um = (points[u].0.abs_diff(points[v].0) + points[u].1.abs_diff(points[v].1)) as f32 / dbu_per_um;
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
    Some(NetRcTree { parent, edge_r_kohm: edge_r, wire_cap_pf: wire_cap, pin_node, total_wire_cap_pf: total_cap })
}
