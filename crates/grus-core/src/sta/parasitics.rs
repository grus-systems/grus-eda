use super::dmp::PiModel;
use crate::congestion::rsmt::Pt;
use crate::design_db::DesignDb;
use crate::interner::{NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::readers::spef::{Spef, SpefNet, unescape_spef_name};
use crate::tech_db::TechDb;
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
        self.pin_node
            .iter()
            .map(|&node| tau[node as usize])
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
        parent,
        edge_r_kohm: edge_r,
        wire_cap_pf: wire_cap,
        pin_node,
        total_wire_cap_pf: total_cap,
    })
}

/// Builds the RC trees from an extracted SPEF. SPEF capacitance is wire-only (`PIN_CAP NONE`), so
/// sink pin capacitance is added later, exactly as for the placement estimate. Resistor loops are
/// broken by a BFS spanning tree from the driver; self-loops and parallel resistors are dropped.
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
    Ok(NetRcTree {
        parent,
        edge_r_kohm: edge_r,
        wire_cap_pf: wire_cap,
        pin_node,
        total_wire_cap_pf: total,
    })
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
            serial += 1;
            out.push_str(&format!(
                "{serial} {} {} {}\n",
                node_names[tree.parent[v] as usize], node_names[v], tree.edge_r_kohm[v]
            ));
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
        .expect("LEF");
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
            .expect("LEF");
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
            .expect("LEF");
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
            .expect("LEF");
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
            .expect("LEF");
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
            .expect("LEF");
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
        .expect("LEF");
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
                assert!(
                    (x - y).abs() < 1e-6,
                    "{}: {x} vs {y} after SPEF round-trip",
                    ea.pin
                );
            }
        }
    }
}
