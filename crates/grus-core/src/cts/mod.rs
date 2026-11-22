//! Clock-tree synthesis (E5 ticket 3): build a depth-balanced buffer
//! tree on every design clock's root net, legalize the inserted cells,
//! and report the census in the same field names TritonCTS emits into
//! `<d>.cts.reference.json`.
//!
//! [`run_cts`] resolves clock roots from the parsed [`Sdc`] clock model
//! (no SDC re-parse), decomposes each root net's sinks by
//! count-equalized median bisection into fanout groups of at most
//! [`CtsConfig::max_loads`], and builds the tree bottom-up out of the
//! `DesignDb` mutation primitives (`insert_cell` + `create_net` +
//! `connect_pin` — `insert_buffer_on_net`'s all-sinks move cannot
//! express a k-way fanout). Buffers seed at their group's centroid and
//! legalize with every pre-existing cell pinned, so non-clock DEF
//! content stays byte-identical.
//!
//! Naming follows TritonCTS (`clkbuf_<n>_<root>`, `clknet_<n>_<root>`);
//! a name collision on re-run fails loudly in `insert_cell`/`create_net`
//! — the honest failure for running CTS on an already-buffered design.

mod tree;

pub use tree::run_cts;

use serde::Serialize;
use std::collections::BTreeMap;

/// Constructor parameters mirroring the TritonCTS command-line knobs.
pub struct CtsConfig {
    /// Tree buffer macros, strongest → weakest
    /// (`sky130_fd_sc_hd__clkbuf_8`, `clkbuf_4`, `clkbuf_2` — the
    /// `-buf_list` set). The first entry is also the capacity fallback.
    pub buf_list: Vec<String>,
    /// Macro driving the top level (`sky130_fd_sc_hd__clkbuf_16` —
    /// `-root_buf`).
    pub root_buf: String,
    /// Soft cap on loads per buffer output pin; groups split above it.
    /// `8` sits inside TritonCTS's ~7–9 loads/buffer on both committed
    /// designs (35/5 ≈ 7, 1597/197 ≈ 8.1).
    pub max_loads: usize,
    /// Routing layer whose RC supplies the wire-cap estimate in buffer
    /// sizing — `met2`, the STA placement-RC layer.
    pub wire_layer: String,
}

impl Default for CtsConfig {
    fn default() -> Self {
        Self {
            buf_list: vec![
                "sky130_fd_sc_hd__clkbuf_8".to_string(),
                "sky130_fd_sc_hd__clkbuf_4".to_string(),
                "sky130_fd_sc_hd__clkbuf_2".to_string(),
            ],
            root_buf: "sky130_fd_sc_hd__clkbuf_16".to_string(),
            max_loads: 8,
            wire_layer: "met2".to_string(),
        }
    }
}

/// Census of one [`run_cts`] call — the criterion-4 honesty-gate
/// fields, named to mirror `<d>.cts.reference.json` wherever a direct
/// analogue exists (`kurena/tasks/e5-cts-tree/design/report.md`).
/// STA-side fields of the reference (`skew_*_ns`, `latency_*_ns`) and
/// `cts_wire_unit` are deliberately absent: they are outputs of the
/// *reference* DEF and arrive with the e2e comparison ticket.
#[derive(Debug, Serialize)]
pub struct CtsReport {
    pub design: String,
    /// Design clock names, comma-joined (one per resolved root net).
    pub clock: String,
    pub cts_root_buf: String,
    /// `buf_list` in config order (strongest → weakest).
    pub cts_buf_list: Vec<String>,
    /// Cells CTS added — tree buffers only (no dummy loads this stage).
    pub inserted_instances: usize,
    /// Inserted cells censused by macro name.
    pub tree_buffers: BTreeMap<String, usize>,
    /// Always empty: `repair_clock_nets` dummy loads are deferred work.
    pub dummy_loads: BTreeMap<String, usize>,
    /// `clknet_*` nets created — equals the `tree_buffers` total since
    /// each tree buffer creates exactly one net (the reference's
    /// `clock_subnets` likewise counts only net-driving buffers;
    /// its `inserted_instances` is larger because dummy loads are
    /// cells without nets).
    pub clock_subnets: usize,
    /// Non-driver pins on the clock root nets — the tree's consumers.
    pub sinks: usize,
    /// Buffer count on the longest root→sink path (the reference's
    /// `CTS-0102` "Path depth" convention).
    pub tree_depth: usize,
    /// Mean Manhattan distance from each sink pin to its driving leaf
    /// buffer's output pin — the same quantity class as the
    /// reference's `CTS-0101` "Average sink wire length".
    pub avg_sink_wirelength_um: f64,
    /// Σ HPWL over the tree's nets (root net + every `clknet_*`) — the
    /// same set the reference census HPWL'd ("every net attached to an
    /// inserted instance").
    pub clock_net_wirelength_um: f64,
    pub clock_net_wirelength_basis: String,
    /// `num_cells` after insertion.
    pub def_components: usize,
    /// `num_nets` after mutation.
    pub def_signal_nets: usize,
}

#[cfg(test)]
mod tests {
    use super::tree::{clock_roots, run_cts};
    use super::*;
    use crate::design_db::{DesignDb, PlacementStatus};
    use crate::interner::{NetId, PinId, StringInterner};
    use crate::lib_db::LibDb;
    use crate::placement::verify_legality;
    use crate::readers::def::{parse_def, write_def_full};
    use crate::readers::lef::load_tech_db;
    use crate::readers::liberty::load_liberty;
    use crate::sta::analysis::run_sta_with_state;
    use crate::sta::sdc::{Clock, Sdc, load_sdc, parse_sdc};
    use crate::sta::{WireRc, estimate_placement_parasitics};
    use crate::tech_db::TechDb;
    use crate::test_support::{design_dir, require, sky130_hd};
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    struct Design {
        tech: TechDb,
        lib: LibDb,
        db: DesignDb,
    }

    fn load_design(name: &str, def_file: &str) -> Design {
        let lib_dir = sky130_hd();
        let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
        let lib_path = require(lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let def_path = require(design_dir(name).join(def_file));
        let mut interner = StringInterner::new();
        let tech = load_tech_db(&tlef, &lef, &mut interner).expect("LEF").0;
        let lib = load_liberty(&lib_path).expect("Liberty");
        let db = parse_def(&def_path, &tech, Some(&lib)).expect("DEF");
        Design { tech, lib, db }
    }

    fn net_by_name(db: &DesignDb, name: &str) -> NetId {
        db.net_names
            .iter()
            .position(|&s| db.strings.resolve(s) == name)
            .map(|i| NetId(i as u32))
            .unwrap_or_else(|| panic!("net `{name}` not in design"))
    }

    /// `inst/pin` for instance pins, `PIN/port` for primary I/O — the
    /// cross-DB member identity.
    fn member_name(db: &DesignDb, p: PinId) -> String {
        match db.pin_cells[p.0 as usize] {
            Some(c) => format!(
                "{}/{}",
                db.strings.resolve(db.cell_names[c.0 as usize]),
                db.strings.resolve(db.pin_names[p.0 as usize])
            ),
            None => format!("PIN/{}", db.strings.resolve(db.pin_names[p.0 as usize])),
        }
    }

    /// STA-style `inst/pin` name (bare port name for io pins), matching
    /// `Context::pin_name`'s formatting.
    fn sta_pin_name(db: &DesignDb, p: PinId) -> String {
        match db.pin_cells[p.0 as usize] {
            Some(_) => member_name(db, p),
            None => db.strings.resolve(db.pin_names[p.0 as usize]).to_string(),
        }
    }

    fn emit_def_string(db: &DesignDb, tech: &TechDb, tag: &str) -> String {
        let tmp =
            std::env::temp_dir().join(format!("grus_e5_cts_{tag}_{}.def", std::process::id()));
        write_def_full(db, tech, &tmp).expect("write_def_full");
        let text = std::fs::read_to_string(&tmp).expect("read emitted DEF");
        let _ = std::fs::remove_file(&tmp);
        text
    }

    /// Emitted DEF sections indexed for line-level comparison.
    struct EmittedIndex {
        pins_block: String,
        comps: BTreeMap<String, String>,
        nets: BTreeMap<String, String>,
    }

    fn index_emitted(text: &str) -> EmittedIndex {
        let mut idx = EmittedIndex {
            pins_block: String::new(),
            comps: BTreeMap::new(),
            nets: BTreeMap::new(),
        };
        let mut section = 0u8; // b'c' components, b'p' pins, b'n' nets
        for line in text.lines() {
            if line.starts_with("COMPONENTS ") {
                section = b'c';
                continue;
            }
            if line.starts_with("PINS ") {
                section = b'p';
            } else if line.starts_with("NETS ") {
                section = b'n';
                continue;
            } else if line.starts_with("END ") {
                if section == b'p' {
                    idx.pins_block.push_str(line);
                    idx.pins_block.push('\n');
                }
                section = 0;
                continue;
            }
            if section == b'p' {
                idx.pins_block.push_str(line);
                idx.pins_block.push('\n');
            }
            if let Some(rest) = line.strip_prefix("    - ") {
                let name = rest.split_whitespace().next().unwrap();
                match section {
                    b'c' => {
                        idx.comps.insert(name.to_string(), line.to_string());
                    }
                    b'n' => {
                        idx.nets.insert(name.to_string(), line.to_string());
                    }
                    _ => {}
                }
            }
        }
        idx
    }

    /// T1 — clock-root discovery on gcd: one `core_clk` root on `clk`,
    /// driven by `( PIN clk )`, 35 `_*_ CLK` sinks.
    #[test]
    fn test_clock_roots_gcd() {
        let Design { db, .. } = load_design("gcd", "gcd.placed.def");
        let sdc = load_sdc(&design_dir("gcd").join("gcd.cts.sdc")).expect("SDC");
        let roots = clock_roots(&db, &sdc).expect("clock_roots");
        assert_eq!(roots.len(), 1);
        let root = &roots[0];
        assert_eq!(root.clock_name, "core_clk");
        assert_eq!(
            db.strings.resolve(db.net_names[root.root_net.0 as usize]),
            "clk"
        );
        assert!(
            db.pin_cells[root.driver_pin.0 as usize].is_none(),
            "clk's driver is the clk I/O port"
        );
        assert_eq!(
            db.strings.resolve(db.pin_names[root.driver_pin.0 as usize]),
            "clk"
        );
        let sinks: Vec<PinId> = db
            .net_pins(root.root_net)
            .iter()
            .copied()
            .filter(|&p| p != root.driver_pin)
            .collect();
        assert_eq!(sinks.len(), 35);
        for &p in &sinks {
            assert_eq!(db.strings.resolve(db.pin_names[p.0 as usize]), "CLK");
            assert!(
                db.strings
                    .resolve(db.cell_names[db.pin_cells[p.0 as usize].unwrap().0 as usize])
                    .starts_with('_'),
                "clock sink must be a register pin"
            );
        }
    }

    /// T2 — tree structure and census on gcd.
    #[test]
    fn test_cts_tree_structure_gcd() {
        let Design { tech, lib, mut db } = load_design("gcd", "gcd.placed.def");
        let sdc = load_sdc(&design_dir("gcd").join("gcd.cts.sdc")).expect("SDC");
        let report = run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts");

        assert_eq!(report.sinks, 35);
        assert!(
            (2..=3).contains(&report.tree_depth),
            "depth {}",
            report.tree_depth
        );
        assert!(
            report.inserted_instances <= 12 && report.inserted_instances != report.sinks,
            "buffer census {} must be tree-class, not one-buffer-per-sink",
            report.inserted_instances
        );
        assert_eq!(report.clock_subnets, report.inserted_instances);
        assert_eq!(report.def_components, 384 + report.inserted_instances);
        assert_eq!(report.def_signal_nets, 269 + report.clock_subnets);
        println!(
            "CTS gcd census: {:#?}",
            serde_json::to_string_pretty(&report).unwrap()
        );

        // Root net: `clk` = { PIN clk (driver), root buffer A }.
        let clk = net_by_name(&db, "clk");
        let clk_seg = db.net_pins(clk).to_vec();
        assert_eq!(clk_seg.len(), 2, "clk members {clk_seg:?}");
        assert!(db.pin_cells[clk_seg[0].0 as usize].is_none());
        assert!(db.pin_is_driver[clk_seg[0].0 as usize]);
        let root_buf_cell = db.pin_cells[clk_seg[1].0 as usize].expect("root buf cell");
        assert_eq!(
            db.strings.resolve(db.cell_names[root_buf_cell.0 as usize]),
            format!("clkbuf_{}_clk", report.inserted_instances - 1)
        );
        assert_eq!(db.strings.resolve(db.pin_names[clk_seg[1].0 as usize]), "A");

        // Every clknet_* net: driver is an inserted buffer's X pin at
        // segment index 0 (I4); non-driver members are original CLK
        // sinks (leaf nets) or buffer A pins (upper-level nets).
        let mut leaf_sinks_seen = 0usize;
        let mut nets_with_clk_members = 0usize;
        for n in 0..db.num_nets() {
            let name = db.strings.resolve(db.net_names[n]);
            if !name.starts_with("clknet_") {
                continue;
            }
            let net = NetId(n as u32);
            let seg = db.net_pins(net).to_vec();
            let driver = db.net_driver(net).expect("tree net has a driver");
            assert_eq!(driver, seg[0], "I4 driver-first on net {name}");
            let drv_cell = db.pin_cells[driver.0 as usize].unwrap();
            assert!(
                db.strings
                    .resolve(db.cell_names[drv_cell.0 as usize])
                    .starts_with("clkbuf_"),
                "{name} driver must be an inserted buffer"
            );
            assert_eq!(db.strings.resolve(db.pin_names[driver.0 as usize]), "X");
            let mut leaf_net = true;
            for &p in &seg[1..] {
                assert!(
                    db.pin_nets[p.0 as usize] == Some(net),
                    "I3 pin_nets back-pointer"
                );
                match db.strings.resolve(db.pin_names[p.0 as usize]) {
                    "CLK" => {
                        let cell = db.pin_cells[p.0 as usize].unwrap();
                        assert!(
                            !db.strings
                                .resolve(db.cell_names[cell.0 as usize])
                                .starts_with("clkbuf_"),
                            "CLK member must be an original register pin"
                        );
                        leaf_sinks_seen += 1;
                    }
                    "A" => {
                        leaf_net = false;
                        let cell = db.pin_cells[p.0 as usize].unwrap();
                        assert!(
                            db.strings
                                .resolve(db.cell_names[cell.0 as usize])
                                .starts_with("clkbuf_"),
                            "A member must be an inserted buffer input"
                        );
                    }
                    other => panic!("unexpected member pin {other} on {name}"),
                }
            }
            if leaf_net {
                nets_with_clk_members += 1;
            }
        }
        assert_eq!(
            leaf_sinks_seen, 35,
            "every original sink landed on a leaf net"
        );
        assert!(nets_with_clk_members >= 1);

        // Depth balance: every sink traces the same buffer count back to `clk`.
        let mut depths = BTreeSet::new();
        for n in 0..db.num_nets() {
            let name = db.strings.resolve(db.net_names[n]);
            if !name.starts_with("clknet_") {
                continue;
            }
            for &p in db.net_pins(NetId(n as u32)) {
                if db.strings.resolve(db.pin_names[p.0 as usize]) != "CLK" {
                    continue;
                }
                let mut depth = 0usize;
                let mut net = db.pin_nets[p.0 as usize].unwrap();
                loop {
                    let drv = db.net_driver(net).unwrap();
                    let dcell = db.pin_cells[drv.0 as usize];
                    if dcell.is_none() {
                        break; // reached the io-port-driven root net
                    }
                    depth += 1;
                    let in_pin = db
                        .cell_pins(dcell.unwrap())
                        .iter()
                        .copied()
                        .find(|&q| {
                            db.strings.resolve(db.pin_names[q.0 as usize]) == "A"
                                && db.pin_nets[q.0 as usize].is_some()
                        })
                        .expect("buffer input is connected");
                    net = db.pin_nets[in_pin.0 as usize].unwrap();
                }
                depths.insert(depth);
            }
        }
        assert_eq!(
            depths.iter().copied().collect::<Vec<_>>(),
            vec![report.tree_depth],
            "every sink descends the same number of buffers"
        );
    }

    /// T3 — the emitted CTS DEF re-parses and legalizes; pre-existing
    /// cells keep their `+ PLACED` status and positions.
    fn cts_def_roundtrip(design_name: &str, def_file: &str, sdc_file: &str) {
        let Design { tech, lib, mut db } = load_design(design_name, def_file);
        let sdc = load_sdc(&design_dir(design_name).join(sdc_file)).expect("SDC");
        let pre_cells = db.num_cells();
        let pre_nets = db.num_nets();
        let pre_pos: HashMap<String, (i32, i32, PlacementStatus)> = (0..db.num_cells())
            .map(|c| {
                (
                    db.strings.resolve(db.cell_names[c]).to_string(),
                    (db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_status[c]),
                )
            })
            .collect();
        let report = run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts");
        verify_legality(&db, &tech).expect("verify_legality in-memory");

        let tmp = std::env::temp_dir().join(format!(
            "grus_e5_cts_roundtrip_{design_name}_{}.def",
            std::process::id()
        ));
        write_def_full(&db, &tech, &tmp).expect("write_def_full");
        let db2 = parse_def(&tmp, &tech, Some(&lib)).expect("re-parse emitted DEF");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(db2.num_cells(), pre_cells + report.inserted_instances);
        assert_eq!(db2.num_nets(), pre_nets + report.clock_subnets);
        verify_legality(&db2, &tech).expect("verify_legality on re-parse");

        for c in 0..db2.num_cells() {
            let name = db2.strings.resolve(db2.cell_names[c]);
            let Some(&(px, py, pst)) = pre_pos.get(name) else {
                continue; // inserted buffer
            };
            assert_eq!(
                db2.cell_status[c], pst,
                "{name} status: pre-existing cells keep their DEF status"
            );
            assert_eq!(
                (db2.cell_x_dbu[c], db2.cell_y_dbu[c]),
                (px, py),
                "{name} position"
            );
        }
    }

    #[test]
    fn test_cts_def_roundtrip_gcd() {
        cts_def_roundtrip("gcd", "gcd.placed.def", "gcd.cts.sdc");
    }

    #[test]
    fn test_cts_def_roundtrip_picorv32() {
        cts_def_roundtrip("picorv32", "picorv32.placed.def", "picorv32.cts.sdc");
    }

    /// T4 — byte-identity of non-clock DEF content (gcd): pin-and-place
    /// legalization must leave every pre-existing component line, every
    /// non-clock net line, and the whole PINS block untouched on the
    /// emitted text. Control-net HPWL follows.
    #[test]
    fn test_cts_untouched_def_identical() {
        let Design { tech, lib, mut db } = load_design("gcd", "gcd.placed.def");
        let sdc = load_sdc(&design_dir("gcd").join("gcd.cts.sdc")).expect("SDC");
        let before = emit_def_string(&db, &tech, "before");
        // Control net: a 3-pin signal net untouched by the clock edits.
        let ctrl = net_by_name(&db, "ctrl.state.out[1]");
        let ctrl_hpwl_before = super::tree::net_hpwl_dbu(&db, &tech, ctrl);
        let ctrl_pos_before: Vec<(i32, i32)> = db
            .net_pins(ctrl)
            .iter()
            .map(|&p| db.pin_pos_dbu(p, &tech))
            .collect();

        run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts");
        let after = emit_def_string(&db, &tech, "after");

        let a = index_emitted(&before);
        let b = index_emitted(&after);
        assert_eq!(a.pins_block, b.pins_block, "PINS section byte-identical");
        for (name, line) in &a.comps {
            assert_eq!(b.comps.get(name), Some(line), "component line: {name}");
        }
        for (name, line) in &a.nets {
            if name == "clk" {
                continue;
            }
            assert_eq!(b.nets.get(name), Some(line), "net line: {name}");
        }
        for name in b.nets.keys() {
            assert!(
                a.nets.contains_key(name) || name == "clk" || name.starts_with("clknet_"),
                "unexpected new net {name}"
            );
        }

        let ctrl_pos_after: Vec<(i32, i32)> = db
            .net_pins(ctrl)
            .iter()
            .map(|&p| db.pin_pos_dbu(p, &tech))
            .collect();
        assert_eq!(ctrl_pos_before, ctrl_pos_after);
        assert_eq!(
            ctrl_hpwl_before,
            super::tree::net_hpwl_dbu(&db, &tech, ctrl),
            "control-net HPWL unchanged"
        );
    }

    /// T5 — propagated-clock STA on the emitted CTS DEF: every original
    /// clock sink keeps a propagated arrival; gcd spread is soft-bounded
    /// (the strict `max(5 ps, 1%)` e2e comparison is ticket 4).
    fn propagated_sta_on_cts_def(design_name: &str, def_file: &str, sdc_file: &str) {
        let Design { tech, lib, mut db } = load_design(design_name, def_file);
        let sdc = load_sdc(&design_dir(design_name).join(sdc_file)).expect("SDC");
        let roots = clock_roots(&db, &sdc).expect("clock_roots");
        let sink_names: BTreeSet<String> = roots
            .iter()
            .flat_map(|r| db.net_pins(r.root_net).iter().copied())
            .filter(|&p| roots.iter().all(|r| p != r.driver_pin))
            .map(|p| sta_pin_name(&db, p))
            .collect();
        run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts");

        let tmp = std::env::temp_dir().join(format!(
            "grus_e5_cts_sta_{design_name}_{}.def",
            std::process::id()
        ));
        write_def_full(&db, &tech, &tmp).expect("write_def_full");
        let db2 = parse_def(&tmp, &tech, Some(&lib)).expect("re-parse emitted DEF");
        let _ = std::fs::remove_file(&tmp);

        let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");
        let par = estimate_placement_parasitics(&db2, &tech, rc);
        let (report, state) =
            run_sta_with_state(&db2, &lib, &sdc, Some(&par)).expect("propagated STA");
        assert!(
            sdc.propagated_clock(),
            "{sdc_file} must mark the clock propagated"
        );

        let by_name: HashMap<String, PinId> = (0..db2.num_pins())
            .map(|i| PinId(i as u32))
            .map(|p| (sta_pin_name(&db2, p), p))
            .collect();
        let (mut lat_max, mut lat_min) = (f32::NEG_INFINITY, f32::INFINITY);
        let mut reached = 0usize;
        for name in &sink_names {
            let &pin = by_name
                .get(name)
                .unwrap_or_else(|| panic!("original sink {name} missing after re-parse"));
            let arr = state[pin.0 as usize].arrival_max[crate::sta::analysis::RISE];
            assert!(
                arr > f32::NEG_INFINITY,
                "clock sink {name} has no propagated arrival — the tree dropped it"
            );
            lat_max = lat_max.max(arr);
            lat_min = lat_min.min(arr);
            reached += 1;
        }
        assert_eq!(reached, sink_names.len());
        let spread_ns = lat_max - lat_min;
        println!(
            "CTS propagated STA ({design_name}): sinks={}, latency {:.4}–{:.4} ns, spread {:.1} ps, wns_max={:.4} wns_min={:.4}",
            reached,
            lat_min,
            lat_max,
            spread_ns * 1000.0,
            report.wns_max_ns,
            report.wns_min_ns,
        );
        if design_name == "gcd" {
            // The "tens-of-ps band" is the expectation; a loose bound
            // still catches a broken tree (one-buffer-per-sink would
            // land ~ns-scale skew). Strict comparison is e5-cts-e2e.
            assert!(
                spread_ns <= 0.5,
                "gcd clock spread {spread_ns} ns out of band"
            );
        }
    }

    #[test]
    fn test_cts_propagated_sta_gcd() {
        propagated_sta_on_cts_def("gcd", "gcd.placed.def", "gcd.cts.sdc");
    }

    #[test]
    fn test_cts_propagated_sta_picorv32() {
        propagated_sta_on_cts_def("picorv32", "picorv32.placed.def", "picorv32.cts.sdc");
    }

    /// T6 — picorv32 census: the reference's 253 is the center of the
    /// band; the band asserts the depth-balanced ~8-fanout league.
    #[test]
    fn test_cts_census_picorv32() {
        let Design { tech, lib, mut db } = load_design("picorv32", "picorv32.placed.def");
        let sdc = load_sdc(&design_dir("picorv32").join("picorv32.cts.sdc")).expect("SDC");
        let report = run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts");
        assert_eq!(report.sinks, 1597);
        assert!(
            (3..=6).contains(&report.tree_depth),
            "picorv32 depth {}",
            report.tree_depth
        );
        assert!(
            (100..=450).contains(&report.inserted_instances),
            "picorv32 buffer census {} out of league",
            report.inserted_instances
        );
        println!(
            "CTS picorv32 census: {:#?}",
            serde_json::to_string_pretty(&report).unwrap()
        );
    }

    /// T7 — root dedup and error paths.
    #[test]
    fn test_cts_root_errors() {
        let Design { tech, lib, mut db } = load_design("gcd", "gcd.placed.def");

        // Two create_clock rows on the same port share one tree.
        let sdc = parse_sdc(
            "create_clock -name c1 -period 5.0 [get_ports clk]\n\
             create_clock -name c2 -period 5.0 [get_ports clk]\n",
        )
        .expect("two-clock SDC");
        let roots = clock_roots(&db, &sdc).expect("clock_roots");
        assert_eq!(roots.len(), 1, "same port dedups to one root");
        assert_eq!(
            roots[0].clock_name, "c1",
            "the first row's name is recorded"
        );
        let report =
            run_cts(&mut db, &tech, &lib, &sdc, &CtsConfig::default()).expect("run_cts dedup");
        assert_eq!(report.sinks, 35, "one tree, not two");

        // A clock source naming no io pin is a loud error.
        let mut db2 = load_design("gcd", "gcd.placed.def").db;
        let sdc = parse_sdc("create_clock -name bad -period 5.0 [get_ports nosuchport]\n")
            .expect("bad-port SDC");
        let err = clock_roots(&db2, &sdc).expect_err("missing port must error");
        assert!(
            err.to_string().contains("nosuchport"),
            "error must name the port: {err:#}"
        );
        // And through the full entry point — before any mutation.
        let pins_before = db2.num_pins();
        let err = run_cts(&mut db2, &tech, &lib, &sdc, &CtsConfig::default())
            .expect_err("run_cts must propagate the root error");
        assert!(err.to_string().contains("nosuchport"));
        assert_eq!(db2.num_pins(), pins_before, "failed run must not mutate");

        // An SDC whose only clock is virtual has no design clock to
        // build. `parse_sdc` rejects a design-clock-free file outright,
        // so the fixture is a hand-built `Sdc` — the same `Clock` row a
        // `create_clock -name vclk -period 5.0` statement produces.
        let mut sdc = Sdc::default();
        sdc.clocks.push(Clock {
            name: "vclk".to_string(),
            port: None,
            period_ns: 5.0,
            ..Default::default()
        });
        let err = clock_roots(&db2, &sdc).expect_err("no design clock must error");
        assert!(err.to_string().contains("no design clock"), "{err:#}");
    }
}
