use crate::congestion::rsmt::{Pt, build_precise_rsmt_points};
use super::dmp::PiModel;
use crate::design_db::DesignDb;
use crate::readers::spef::{Spef, SpefNet, unescape_spef_name};
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
        for v in (1..n).rev() {
            let (p, r) = (self.parent[v] as usize, self.edge_r_kohm[v] as f64);
            let (a1, a2, a3) = (y1[v], y2[v], y3[v]);
            y2[p] += a2 - r * a1 * a1;
            y3[p] += a3 - 2.0 * r * a1 * a2 + r * r * a1 * a1 * a1;
            y1[p] += a1;
        }
        PiModel::from_moments(y1[0], y2[0], y3[0])
    }

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

/// One RC tree per net (from placement or from a SPEF), `None` for nets without a driver,
/// with a single pin, or absent from the SPEF.
#[derive(Clone, Debug)]
pub struct NetParasitics {
    nets: Vec<Option<NetRcTree>>,
}

impl NetParasitics {
    pub fn net(&self, net: NetId) -> Option<&NetRcTree> {
        self.nets[net.0 as usize].as_ref()
    }
}

pub fn estimate_placement_parasitics(design: &DesignDb, tech: &TechDb, rc: WireRc) -> NetParasitics {
    let nets = (0..design.num_nets())
        .map(|n| build_net_tree(design, tech, rc, NetId(n as u32)))
        .collect();
    NetParasitics { nets }
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
    for (a, b) in build_precise_rsmt_points(&pin_pts) {
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

/// Builds the RC trees from an extracted SPEF. SPEF capacitance is wire-only (`PIN_CAP NONE`), so
/// sink pin capacitance is added later, exactly as for the placement estimate. Resistor loops are
/// broken by a BFS spanning tree from the driver; self-loops and parallel resistors are dropped.
pub fn spef_parasitics(design: &DesignDb, spef: &Spef) -> Result<NetParasitics> {
    let mut nets = Vec::with_capacity(design.num_nets());
    for n in 0..design.num_nets() {
        let net = NetId(n as u32);
        let name = unescape_spef_name(design.strings.resolve(design.net_names[n]));
        let usable = design.net_driver(net).is_some() && design.net_pins(net).len() >= 2;
        let tree = match (usable, spef.nets.get(&name)) {
            (true, Some(spef_net)) => Some(spef_net_tree(design, net, &name, spef_net)?),
            _ => None,
        };
        nets.push(tree);
    }
    Ok(NetParasitics { nets })
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

fn spef_net_tree(design: &DesignDb, net: NetId, net_name: &str, spef_net: &SpefNet) -> Result<NetRcTree> {
    let pins = design.net_pins(net);
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut intern = |name: &str| -> usize {
        let next = index.len();
        *index.entry(name.to_string()).or_insert(next)
    };
    let resistor_ends: Vec<(usize, usize, f32)> =
        spef_net.resistors.iter().map(|(a, b, r)| (intern(a), intern(b), *r)).collect();
    let cap_nodes: Vec<(usize, f32)> = spef_net.caps.iter().map(|(n, c)| (intern(n), *c)).collect();
    let node_count = index.len();
    let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); node_count];
    for &(a, b, r) in &resistor_ends {
        if a != b {
            adjacency[a].push((b, r));
            adjacency[b].push((a, r));
        }
    }
    let mut node_cap = vec![0.0f32; node_count];
    for &(i, cap) in &cap_nodes {
        node_cap[i] += cap;
    }
    let pin_names: Vec<String> = pins.iter().map(|&p| spef_pin_node_name(design, p)).collect();
    let mut pin_point = Vec::with_capacity(pins.len());
    for name in &pin_names {
        let Some(&i) = index.get(name) else {
            bail!("SPEF net {net_name} has no node for pin {name}");
        };
        pin_point.push(i);
    }

    let root = pin_point[0];
    let mut node_of = vec![u32::MAX; adjacency.len()];
    let mut order = vec![root];
    node_of[root] = 0;
    let mut parent = vec![0u32];
    let mut edge_r = vec![0.0f32];
    let mut wire_cap = vec![node_cap[root]];
    let mut head = 0;
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
        }
    }
    let mut pin_node = Vec::with_capacity(pins.len());
    for (&p, name) in pin_point.iter().zip(&pin_names) {
        if node_of[p] == u32::MAX {
            bail!("SPEF net {net_name}: pin {name} is not connected to the driver by resistors");
        }
        pin_node.push(node_of[p]);
    }
    let total = wire_cap.iter().sum();
    Ok(NetRcTree { parent, edge_r_kohm: edge_r, wire_cap_pf: wire_cap, pin_node, total_wire_cap_pf: total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::congestion::rsmt::{EXACT_MAX_DEGREE, build_precise_rsmt_points};
    use crate::interner::StringInterner;
    use crate::readers::{load_liberty, load_spef, load_tech_db, parse_def};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    struct Lengths {
        table: BTreeMap<usize, (usize, usize, f64)>,
        mismatches: Vec<String>,
        /// Nets of up to `EXACT_MAX_DEGREE` points where our tree is longer than OpenROAD's.
        longer: Vec<String>,
    }

    /// Tree length of every net against OpenROAD's SPEF (`sum of *CAP / c0`), per net degree.
    /// The table maps degree to (nets, matching nets, worst excess in um).
    fn compare_lengths(design_name: &str) -> Lengths {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pdk = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".ciel"));
        let lib_dir = pdk.join("sky130A/libs.ref/sky130_fd_sc_hd");
        let dir = repo.join("tests/designs").join(design_name);
        let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF");
        let design = parse_def(&dir.join(format!("{design_name}.placed.def")), &tech, Some(&lib)).expect("DEF");
        let spef = load_spef(&dir.join(format!("{design_name}.routed.spef"))).expect("SPEF");
        let c0 = WireRc::from_layer(&tech, "met2").expect("met2 RC").c_pf_per_um as f64;
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
            let Some(spef_net) = spef.nets.get(&name) else { continue };
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
                        longer.push(format!("{name} (degree {}): ours {ours:.4} um, SPEF {reference:.4} um", pins.len()));
                    }
                    mismatches.push(format!("{name} (degree {}): ours {ours:.4} um, SPEF {reference:.4} um", pins.len()));
                }
            }
        }
        Lengths { table, mismatches, longer }
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
        println!("{} nets of up to {EXACT_MAX_DEGREE} points differ from OpenROAD's length", lengths.mismatches.len());
        for degree in [2, 3] {
            let (nets, matching, _) = lengths.table[&degree];
            assert_eq!(nets, matching, "{design_name}: {degree}-point nets must match OpenROAD exactly");
        }
        assert!(lengths.longer.is_empty(), "{design_name}: tree longer than OpenROAD's: {:?}", lengths.longer);
    }

    #[test]
    fn test_steiner_length_vs_openroad_spef_gcd() {
        check_known_properties("gcd");
    }

    #[test]
    fn test_steiner_length_vs_openroad_spef_picorv32() {
        check_known_properties("picorv32");
    }
}
