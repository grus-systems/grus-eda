//! The clock-tree constructor internals, as designed in
//! `kurena/tasks/e5-cts-tree/design/algorithm.md`:
//!
//! - **Topology**: count-equalized median bisection, bottom-up. Each
//!   level partitions its load pins into `ceil(n / max_loads)` spatially
//!   coherent groups, so every sink passes through exactly `tree_depth`
//!   buffers — depth balance by construction, load balance to ±1 pin.
//!   A PD-tree/RSMT backbone was the alternative; it optimizes
//!   wirelength, while equal-depth grouping is what bounds skew.
//! - **Fanout via primitive ops, not `insert_buffer_on_net`**: the
//!   convenience method moves *all* sinks of a net into one new net — a
//!   k-way group split needs `insert_cell` + `create_net` +
//!   `connect_pin` directly (same tested primitives).
//! - **Pin-and-place legalization**: pre-existing `Placed` `Core`/
//!   `CoreTie` cells are temporarily marked `Fixed`, Abacus drops only
//!   the inserted buffers into free sub-row gaps, then `Placed` is
//!   restored — the emitted DEF's non-clock component and net lines
//!   stay byte-identical.

use super::{CtsConfig, CtsReport};
use crate::design_db::{DesignDb, PlacementStatus};
use crate::interner::{CellId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::placement::legalize_placement;
use crate::sta::parasitics::WireRc;
use crate::sta::sdc::Sdc;
use crate::tech_db::TechDb;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashSet};

/// One design clock anchored on the net its source port sits on.
#[derive(Debug)]
pub(super) struct ClockRoot {
    /// `Clock::name` ("core_clk").
    pub(super) clock_name: String,
    /// Net the source port's pin sits on.
    pub(super) root_net: NetId,
    /// `net_driver(root_net)` — a port pin on committed designs, a cell
    /// pin for an internal/generated clock (accepted as-is).
    pub(super) driver_pin: PinId,
}

/// Resolves every `create_clock` with a source port to a single-driver
/// root net — the same resolution `sta/analysis.rs` performs when
/// seeding the propagated-clock walk. Two clock rows naming the same
/// port deduplicate to one tree. Bails loudly on a missing port, an
/// unconnected port pin, a driverless net, and a multi-driver net —
/// the same refusal `insert_buffer_on_net` makes.
pub(super) fn clock_roots(db: &DesignDb, sdc: &Sdc) -> Result<Vec<ClockRoot>> {
    let mut roots = Vec::new();
    let mut seen: HashSet<NetId> = HashSet::new();
    for clock in &sdc.clocks {
        let Some(port) = clock.port.as_deref() else {
            continue; // virtual I/O clock: no tree to build
        };
        let port_pin = db
            .io_pins
            .iter()
            .copied()
            .find(|&p| db.strings.resolve(db.pin_names[p.0 as usize]) == port)
            .with_context(|| {
                format!(
                    "cts: clock `{}` source `{port}` is not an io port",
                    clock.name
                )
            })?;
        let root_net = db.pin_nets[port_pin.0 as usize].with_context(|| {
            format!(
                "cts: clock `{}` port `{port}` is not connected to a net",
                clock.name
            )
        })?;
        let net_name = db.strings.resolve(db.net_names[root_net.0 as usize]);
        let driver = db.net_driver(root_net).with_context(|| {
            format!(
                "cts: clock `{}` root net `{net_name}` has no driver",
                clock.name
            )
        })?;
        if db
            .net_pins(root_net)
            .iter()
            .filter(|&&p| db.pin_is_driver[p.0 as usize])
            .count()
            > 1
        {
            bail!(
                "cts: clock `{}` root net `{net_name}` has multiple drivers",
                clock.name
            );
        }
        if seen.insert(root_net) {
            roots.push(ClockRoot {
                clock_name: clock.name.clone(),
                root_net,
                driver_pin: driver,
            });
        }
    }
    if roots.is_empty() {
        bail!("cts: no design clock resolved to a port-driven net");
    }
    Ok(roots)
}

/// Splits `loads` into `ceil(n / max_loads)` groups whose sizes differ
/// by at most one pin, each spatially coherent: recursively bisect the
/// set on the longer bbox axis (ties broken by the other axis, then
/// `PinId` — fully deterministic), giving each sub-problem a count of
/// groups proportional to its share of the loads.
fn partition(loads: &[(PinId, (i32, i32))], max_loads: usize) -> Vec<Vec<(PinId, (i32, i32))>> {
    let n = loads.len();
    if n == 0 {
        return Vec::new();
    }
    let g = n.div_ceil(max_loads);
    if g <= 1 {
        return vec![loads.to_vec()];
    }
    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &(_, (x, y)) in loads {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let mut sorted = loads.to_vec();
    // Split the bbox's longer axis; ties on the other axis, then PinId.
    if max_x - min_x >= max_y - min_y {
        sorted.sort_by_key(|&(p, (x, y))| (x, y, p.0));
    } else {
        sorted.sort_by_key(|&(p, (x, y))| (y, x, p.0));
    }
    let g_lo = g / 2;
    // Proportional split: the left sub-problem takes `g_lo` of the `g`
    // groups, so it receives `floor(n * g_lo / g)` loads — that is what
    // keeps every leaf group within one pin of `n / g`.
    let n_lo = n * g_lo / g;
    let mut out = partition(&sorted[..n_lo], max_loads);
    out.extend(partition(&sorted[n_lo..], max_loads));
    out
}

/// Picks the tree buffer for one group: the first macro in
/// `cfg.buf_list` (ordered strongest → weakest) whose output-pin
/// Liberty `max_cap` covers the estimated load — sink pin `cap_nom`s
/// plus `c_pf_per_um` of the sizing layer over the group's bbox half
/// perimeter. Falls back to the strongest entry when nothing covers
/// the load or no `max_cap` is declared; an unmapped sink contributes
/// 0 capacitance (still counted in `sinks`).
fn pick_buffer(
    group: &[(PinId, (i32, i32))],
    db: &DesignDb,
    tech: &TechDb,
    lib: &LibDb,
    buf_list: &[String],
    c_pf_per_um: f32,
) -> String {
    let mut c_est = 0.0f32;
    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &(p, (x, y)) in group {
        if let Some(lp) = db.pin_lib_pins[p.0 as usize] {
            c_est += lib.pins[lp.0 as usize].cap_nom;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let half_perim_um =
        ((max_x - min_x) + (max_y - min_y)) as f32 / tech.dbu_per_micron.max(1) as f32;
    c_est += c_pf_per_um * half_perim_um;
    for name in buf_list {
        let max_cap = lib
            .strings
            .get(name)
            .and_then(|s| lib.cell_by_name.get(&s))
            .and_then(|&lcid| {
                lib.cell_pins(&lib.cells[lcid.0 as usize])
                    .iter()
                    .find(|p| p.dir == PinDir::Output)
                    .and_then(|p| p.max_cap)
            });
        if max_cap.is_some_and(|mc| mc >= c_est) {
            return name.clone();
        }
    }
    buf_list[0].clone()
}

/// The buffer cell's single non-power `Input` and `Output` pins —
/// `(in, out)`. The `is_power` guard is load-bearing: `clkbuf_*` power
/// pins are `DIRECTION INOUT` in LEF and would pass a direction-only
/// filter. `run_cts` validates the exactly-one-of-each shape for every
/// configured macro before mutating, so `expect` cannot fire here.
fn buffer_signal_pins(db: &DesignDb, cell: CellId, tech: &TechDb) -> (PinId, PinId) {
    let (mut input, mut output) = (None, None);
    for &p in db.cell_pins(cell) {
        let mp = db.pin_macro_pins[p.0 as usize].expect("inserted pin has a MacroPin");
        let mpin = &tech.macro_pins[mp.0 as usize];
        if mpin.is_power {
            continue;
        }
        match mpin.dir {
            PinDir::Input => input = Some(p),
            PinDir::Output => output = Some(p),
            _ => {}
        }
    }
    (
        input.expect("buffer macro has no signal input pin"),
        output.expect("buffer macro has no signal output pin"),
    )
}

/// Legalizes only the inserted cells: every pre-existing movable
/// (`Placed`/`Unplaced`) `Core`/`CoreTie` cell is temporarily `Fixed`
/// so Abacus treats it as a row obstacle and drops the new buffers
/// into free sub-row gaps near their seeds. Original statuses are
/// restored afterwards — even on failure — keeping every pre-existing
/// component line byte-identical in the emitted DEF.
/// `legalize_placement` self-verifies its output, but under the pinned
/// statuses; `run_cts` re-verifies after the restore.
fn legalize_inserted_only(
    db: &mut DesignDb,
    tech: &TechDb,
    inserted: &HashSet<CellId>,
) -> Result<()> {
    let mut pinned = Vec::new();
    for c in 0..db.num_cells() {
        let status = db.cell_status[c];
        if inserted.contains(&CellId(c as u32))
            || !matches!(status, PlacementStatus::Placed | PlacementStatus::Unplaced)
        {
            continue;
        }
        // Pin regardless of macro class: `Fixed`/`Cover` are the only
        // statuses Abacus buckets as row obstacles, so a `Placed`
        // welltap or spacer left as-is would be neither movable nor
        // obstacle — a cell a buffer could silently overlap.
        db.cell_status[c] = PlacementStatus::Fixed;
        pinned.push((CellId(c as u32), status));
    }
    let dbu = db.dbu_per_micron.max(1) as f32;
    let gp_x: Vec<f32> = db.cell_x_dbu.iter().map(|&x| x as f32 / dbu).collect();
    let gp_y: Vec<f32> = db.cell_y_dbu.iter().map(|&y| y as f32 / dbu).collect();
    let result = legalize_placement(db, tech, &gp_x, &gp_y);
    for &(c, status) in &pinned {
        db.cell_status[c.0 as usize] = status;
    }
    result.map(|_| ())
}

/// Per-net HPWL from member-pin positions — the same arithmetic as
/// `compute_hpwl_dbu`, restricted to one net.
pub(super) fn net_hpwl_dbu(db: &DesignDb, tech: &TechDb, net: NetId) -> i64 {
    let pins = db.net_pins(net);
    if pins.len() <= 1 {
        return 0;
    }
    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &p in pins {
        let (px, py) = db.pin_pos_dbu(p, tech);
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
    }
    (max_x - min_x) as i64 + (max_y - min_y) as i64
}

/// Builds a depth-balanced buffer tree on every design clock's root
/// net and legalizes the inserted cells only. See [`crate::cts`] for
/// the construction contract.
pub fn run_cts(
    db: &mut DesignDb,
    tech: &TechDb,
    lib: &LibDb,
    sdc: &Sdc,
    cfg: &CtsConfig,
) -> Result<CtsReport> {
    if cfg.buf_list.is_empty() {
        bail!("cts: buffer list is empty");
    }
    if cfg.max_loads < 2 {
        bail!("cts: max_loads must be >= 2 (1 yields one group per load — the tree never terminates)");
    }
    // Validate every configured macro before mutating: it must exist in
    // the tech DB and have exactly one non-power input and output pin.
    let mut macro_ids = Vec::new();
    for name in cfg.buf_list.iter().chain(std::iter::once(&cfg.root_buf)) {
        let sym = tech
            .strings
            .get(name)
            .with_context(|| format!("cts: buffer macro `{name}` not in the tech DB"))?;
        let &macro_id = tech
            .macro_by_name
            .get(&sym)
            .with_context(|| format!("cts: buffer macro `{name}` not in the tech DB"))?;
        let mac = &tech.macros[macro_id.0 as usize];
        let ins = tech
            .macro_pins(mac)
            .iter()
            .filter(|m| !m.is_power && m.dir == PinDir::Input)
            .count();
        let outs = tech
            .macro_pins(mac)
            .iter()
            .filter(|m| !m.is_power && m.dir == PinDir::Output)
            .count();
        if ins != 1 || outs != 1 {
            bail!(
                "cts: buffer macro `{name}` has {ins} signal inputs and {outs} signal outputs; need exactly one of each"
            );
        }
        macro_ids.push(macro_id);
    }
    let root_macro = *macro_ids.last().unwrap();
    let tree_macros = &macro_ids[..macro_ids.len() - 1];
    let wire_c = WireRc::from_layer(tech, &cfg.wire_layer)
        .with_context(|| format!("cts: sizing wire layer `{}`", cfg.wire_layer))?
        .c_pf_per_um;

    let roots = clock_roots(db, sdc)?;

    let mut tree_buffers: BTreeMap<String, usize> = BTreeMap::new();
    let mut clock_names = Vec::new();
    let mut inserted: HashSet<CellId> = HashSet::new();
    let mut tree_nets: Vec<NetId> = Vec::new();
    // (original sink pin, the leaf buffer output pin driving it) — the
    // post-legalization `avg_sink_wirelength_um` pairs.
    let mut sink_drivers: Vec<(PinId, PinId)> = Vec::new();
    let mut tree_depth = 0usize;
    let mut sinks_total = 0usize;

    for root in &roots {
        // Snapshot the sinks before any mutation: never iterate a CSR
        // segment while mutating it.
        let sinks: Vec<PinId> = db
            .net_pins(root.root_net)
            .iter()
            .copied()
            .filter(|&p| p != root.driver_pin)
            .collect();
        clock_names.push(root.clock_name.clone());
        if sinks.is_empty() {
            continue; // empty tree: the clock is reported but builds nothing
        }
        sinks_total += sinks.len();
        tree_nets.push(root.root_net);
        let root_name = db
            .strings
            .resolve(db.net_names[root.root_net.0 as usize])
            .to_string();
        // Existing cells never move under pin-and-place legalization,
        // so these positions stay valid through construction.
        let mut loads: Vec<(PinId, (i32, i32))> = sinks
            .iter()
            .map(|&p| (p, db.pin_pos_dbu(p, tech)))
            .collect();
        let mut ctr = 0usize;
        let mut level = 0usize;
        loop {
            let groups = partition(&loads, cfg.max_loads);
            let is_top = groups.len() == 1;
            let mut buf_ins = Vec::with_capacity(groups.len());
            for g in &groups {
                let macro_name = if is_top {
                    cfg.root_buf.clone()
                } else {
                    pick_buffer(g, db, tech, lib, &cfg.buf_list, wire_c)
                };
                let macro_id = if is_top {
                    root_macro
                } else {
                    tree_macros[cfg
                        .buf_list
                        .iter()
                        .position(|n| n == &macro_name)
                        .expect("pick_buffer returns a buf_list entry")]
                };
                let cell_name = format!("clkbuf_{ctr}_{root_name}");
                let net_name = format!("clknet_{ctr}_{root_name}");
                let cell = db
                    .insert_cell(&cell_name, macro_id, tech, Some(lib))
                    .with_context(|| format!("cts: insert {cell_name}"))?;
                let net = db
                    .create_net(&net_name)
                    .with_context(|| format!("cts: create {net_name}"))?;
                let (in_pin, out_pin) = buffer_signal_pins(db, cell, tech);
                db.connect_pin(out_pin, net); // driver -> index 0
                for &(p, _) in g {
                    db.connect_pin(p, net); // sink -> segment end
                    if level == 0 {
                        sink_drivers.push((p, out_pin));
                    }
                }
                let cx =
                    (g.iter().map(|&(_, (x, _))| x as i64).sum::<i64>() / g.len() as i64) as i32;
                let cy =
                    (g.iter().map(|&(_, (_, y))| y as i64).sum::<i64>() / g.len() as i64) as i32;
                db.cell_x_dbu[cell.0 as usize] = cx;
                db.cell_y_dbu[cell.0 as usize] = cy;
                *tree_buffers.entry(macro_name).or_default() += 1;
                inserted.insert(cell);
                tree_nets.push(net);
                buf_ins.push(in_pin);
                ctr += 1;
            }
            if buf_ins.len() == 1 {
                // The tree root buffer: the root net ends as
                // `{driver, root_buf.in}` — the TritonCTS shape.
                db.connect_pin(buf_ins[0], root.root_net);
                break;
            }
            loads = buf_ins
                .iter()
                .map(|&p| (p, db.pin_pos_dbu(p, tech)))
                .collect();
            level += 1;
        }
        tree_depth = tree_depth.max(level + 1);
    }

    legalize_inserted_only(db, tech, &inserted)
        .context("cts: legalization of inserted clock buffers failed")?;
    // legalize_placement self-verified while the pre-existing cells
    // were still pinned `Fixed`; re-verify the restored final state.
    crate::placement::verify_legality(db, tech)
        .context("cts: post-restore legality check failed")?;

    let dbu = db.dbu_per_micron.max(1) as f64;
    let avg_sink_wirelength_um = if sink_drivers.is_empty() {
        0.0
    } else {
        let sum: i64 = sink_drivers
            .iter()
            .map(|&(sink, drv)| {
                let (sx, sy) = db.pin_pos_dbu(sink, tech);
                let (dx, dy) = db.pin_pos_dbu(drv, tech);
                (sx - dx).abs() as i64 + (sy - dy).abs() as i64
            })
            .sum();
        sum as f64 / sink_drivers.len() as f64 / dbu
    };
    let clock_net_wirelength_dbu: i64 = tree_nets.iter().map(|&n| net_hpwl_dbu(db, tech, n)).sum();

    Ok(CtsReport {
        design: db.design_name.clone(),
        clock: clock_names.join(","),
        cts_root_buf: cfg.root_buf.clone(),
        cts_buf_list: cfg.buf_list.clone(),
        inserted_instances: inserted.len(),
        clock_subnets: tree_buffers.values().sum(),
        tree_buffers,
        dummy_loads: BTreeMap::new(),
        sinks: sinks_total,
        tree_depth,
        avg_sink_wirelength_um,
        clock_net_wirelength_um: clock_net_wirelength_dbu as f64 / dbu,
        clock_net_wirelength_basis: "hpwl".to_string(),
        def_components: db.num_cells(),
        def_signal_nets: db.num_nets(),
    })
}
