//! E6 DRT substrate gate: the merged LEF∪DEF track grid, the
//! candidate access-point census, the guide-bounded track graph, and
//! the via candidate table + layer-transition edge census over the
//! committed placed DEFs of gcd and picorv32.
//!
//! What this proves:
//! - Every routing layer carries nonempty track lists on both axes and
//!   no preferred-axis `TrackGap` — the grid the maze router needs.
//! - Every signal-net member pin (instance pins and io `BTERM`s) has at
//!   least one access point; `off_track`/`no_shapes` findings are empty
//!   on both designs and would name their pins if not.
//! - Every adjacent routing-layer pair has ≥1 via candidate and a named
//!   default, joined to the committed `<d>.drt.reference.json`
//!   `vias_per_cut_layer` census.
//! - Via edges exist only at real shared track crossings; met2–met3
//!   yields exactly 0 on both designs — the named edge-domain tech gap.
//! - Output is deterministic (double-run asserts on nonempty data).
//! - Synthetic fixtures prove the gate can fail: an off-track pin, a
//!   shrunk guide region, a dropped coverage box, and a gap pair are
//!   all named findings.
//!
//! Tests fail loudly on missing inputs — no silent skips.

use grus_core::congestion::guide::{GuideBox, guide_net_name, parse_guides};
use grus_core::design_db::DesignDb;
use grus_core::drt::access::{AccessReport, access_points};
use grus_core::drt::graph::{GuideJoin, build_route_graphs, join_guides};
use grus_core::drt::tracks::{Axis, LayerTracks, layer_tracks, track_gaps};
use grus_core::drt::via::{ViaPair, ViaTable, via_table};
use grus_core::interner::{LayerId, NetId, PinId, StringInterner};
use grus_core::readers::{load_tech_db, parse_def, parse_def_bytes, parse_lef_bytes};
use grus_core::tech_db::{LayerDir, TechDb};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Inputs (same convention as tests/drc.rs)
// ---------------------------------------------------------------------------

fn pdk_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PDK_ROOT") {
        return PathBuf::from(root);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".ciel"),
        None => panic!("neither PDK_ROOT nor HOME is set; cannot locate the PDK (set PDK_ROOT)"),
    }
}

fn sky130_hd() -> PathBuf {
    pdk_root().join("sky130A/libs.ref/sky130_fd_sc_hd")
}

fn design_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/designs")
        .join(name)
}

fn require(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if !path.exists() {
        panic!("required test input missing: {}", path.display());
    }
    path.to_path_buf()
}

fn load(name: &str) -> (TechDb, DesignDb, Vec<LayerTracks>, AccessReport) {
    let lib_dir = sky130_hd();
    let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
    let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
    let def = require(design_dir(name).join(format!("{name}.placed.def")));

    let mut interner = StringInterner::new();
    let tech = load_tech_db(&tlef, &lef, &mut interner)
        .expect("load_tech_db")
        .0;
    let design = parse_def(&def, &tech, None).expect("parse_def");
    let tracks = layer_tracks(&design, &tech);
    let report = access_points(&design, &tech, &tracks);
    (tech, design, tracks, report)
}

/// `cell/pin` for instance pins, the port name for io `BTERM`s — the
/// name a finding prints.
fn pin_label(db: &DesignDb, pin: PinId) -> String {
    let i = pin.0 as usize;
    let pname = db.strings.resolve(db.pin_names[i]);
    match db.pin_cells[i] {
        Some(c) => format!(
            "{}/{pname}",
            db.strings.resolve(db.cell_names[c.0 as usize])
        ),
        None => pname.to_string(),
    }
}

/// Reads the committed `<d>.guide` reference (OpenROAD
/// `global_route -guide_file` output), parses it, and joins the
/// blocks to design nets. A missing or unparseable file is a loud
/// failure — never a skip.
fn load_guide(
    db: &DesignDb,
    tech: &TechDb,
    name: &str,
) -> (Vec<(String, Vec<GuideBox>)>, GuideJoin) {
    let path = require(design_dir(name).join(format!("{name}.guide")));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let parsed = parse_guides(&text, tech)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    let join = join_guides(db, tech, &parsed).expect("guide join failed");
    (parsed, join)
}

fn net_name(db: &DesignDb, net: NetId) -> String {
    db.strings.resolve(db.net_names[net.0 as usize]).to_string()
}

fn print_findings(db: &DesignDb, report: &AccessReport) {
    for f in &report.off_track {
        let net = f
            .net
            .map(|n| db.strings.resolve(db.net_names[n.0 as usize]))
            .unwrap_or("<none>");
        println!(
            "  off_track {} net={net} rects={:?}",
            pin_label(db, f.pin),
            f.rects
        );
    }
    for &p in &report.no_shapes {
        println!("  no_shapes {}", pin_label(db, p));
    }
}

// ---------------------------------------------------------------------------
// Track census
// ---------------------------------------------------------------------------

/// Per-layer, per-axis track counts on the merged grid; the preferred
/// axis is covered on every routing layer, and the sky130 techlef's
/// LEF-side lists are empty — the whole grid is DEF-sourced.
#[test]
fn test_track_census() {
    for name in ["gcd", "picorv32"] {
        let (tech, _, tracks, _) = load(name);
        println!("== {name} track census (layer dir tracks_x tracks_y) ==");
        for (i, layer) in tech.routing_layers.iter().enumerate() {
            println!(
                "  {:5} {:?} {:6} {:6}",
                tech.strings.resolve(layer.name),
                layer.dir,
                tracks[i].tracks_x.len(),
                tracks[i].tracks_y.len(),
            );
            // Both placed DEFs declare both axes on all six routing layers.
            assert!(
                !tracks[i].tracks_x.is_empty() && !tracks[i].tracks_y.is_empty(),
                "{name}: {} has an empty track axis",
                tech.strings.resolve(layer.name)
            );
        }
        let gaps = track_gaps(&tech, &tracks);
        for g in &gaps {
            println!(
                "  gap: {} axis {:?}",
                tech.strings
                    .resolve(tech.routing_layers[g.layer.0 as usize].name),
                g.axis
            );
        }
        assert!(gaps.is_empty(), "{name}: preferred-axis track gaps");

        // sky130 honesty: the techlef carries zero `TRACKS` records, so
        // every count above is DEF-sourced — the LEF lists are empty.
        let lef_total: usize = tech
            .routing_layers
            .iter()
            .map(|l| l.tracks_x.len() + l.tracks_y.len())
            .sum();
        println!("{name}: LEF-source track coords total = {lef_total} (grid is DEF-sourced)");
    }
}

// ---------------------------------------------------------------------------
// Access census
// ---------------------------------------------------------------------------

/// Every member pin of every signal net has ≥1 access point on at least
/// one layer — checked per element (per pin), not only via `off_track`
/// emptiness. Per-layer counts are the real-coverage evidence.
#[test]
fn test_access_census() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, report) = load(name);

        println!("== {name} access census ==");
        let member_pins: usize = (0..db.num_nets())
            .map(|n| db.net_pins(grus_core::interner::NetId(n as u32)).len())
            .sum();
        println!(
            "{name}: member_pins={member_pins} points={} off_track={} no_shapes={}",
            report.points.len(),
            report.off_track.len(),
            report.no_shapes.len()
        );
        print_findings(&db, &report);
        assert!(
            report.off_track.is_empty() && report.no_shapes.is_empty(),
            "{name}: access findings (see printed names)"
        );

        // §7.5 per-element check: every iterated member pin produced at
        // least one point.
        let mut covered: std::collections::BTreeSet<PinId> = std::collections::BTreeSet::new();
        for ap in &report.points {
            covered.insert(ap.pin);
        }
        for n in 0..db.num_nets() {
            for &p in db.net_pins(grus_core::interner::NetId(n as u32)) {
                assert!(
                    covered.contains(&p),
                    "{name}: member pin {} has no access point",
                    pin_label(&db, p)
                );
            }
        }

        println!("{name}: per-layer access points");
        let li1 = tech.layer_by_name[&tech.strings.get("li1").unwrap()].0 as usize;
        let met1 = tech.layer_by_name[&tech.strings.get("met1").unwrap()].0 as usize;
        for (i, layer) in tech.routing_layers.iter().enumerate() {
            println!(
                "  {:5} {:6}",
                tech.strings.resolve(layer.name),
                report.per_layer[i]
            );
        }
        assert!(!report.points.is_empty());
        // Pin-bearing layers: instance pins live on li1 (+ some met1),
        // io ports on met2+.
        assert!(report.per_layer[li1] > 0, "{name}: li1 access points");
        assert!(report.per_layer[met1] > 0, "{name}: met1 access points");

        // Determinism with nonzero evidence (§7.7): a second run on the
        // same grid emits the identical point list.
        let r2 = access_points(&db, &tech, &tracks);
        assert_eq!(report.points, r2.points, "{name}: nondeterministic APs");
    }
}

// ---------------------------------------------------------------------------
// Off-track fixture
// ---------------------------------------------------------------------------

/// The macro pin rect `[100,300]×[100,300]` sits strictly between the
/// 1000-DBU-period met1 tracks — every axis misses. The io port covers
/// the (1000, 1000) crossing, so the report is a named finding on one
/// pin, not "everything failed".
const OFFTRACK_LEF: &str = r#"
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
TRACKS X 0.0 DO 3 STEP 1.0 LAYER met1 ;
TRACKS Y 0.0 DO 3 STEP 1.0 LAYER met1 ;
MACRO CELL
  CLASS CORE ;
  SIZE 1.0 BY 1.0 ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.1 0.1 0.3 0.3 ;
    END
  END A
END CELL
END LIBRARY
"#;

const OFFTRACK_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN d ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 4000 4000 ) ;
ROW ROW_0 unithd 0 0 N DO 4 BY 1 STEP 460 2720 ;
TRACKS X 0 DO 5 STEP 1000 LAYER met1 ;
TRACKS Y 0 DO 5 STEP 1000 LAYER met1 ;
COMPONENTS 1 ;
- u0 CELL + PLACED ( 0 0 ) N ;
END COMPONENTS
PINS 1 ;
- io + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 1000 1000 ) N ;
END PINS
NETS 1 ;
- n1 ( PIN io ) ( u0 A ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

#[test]
fn test_access_off_track_fixture() {
    let mut interner = StringInterner::new();
    let mut tech = TechDb::new();
    parse_lef_bytes(OFFTRACK_LEF.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
    tech.strings = interner.clone();
    tech.rebuild_maps();
    let db = parse_def_bytes(OFFTRACK_DEF.as_bytes(), &tech, None).expect("fixture DEF parses");

    let tracks = layer_tracks(&db, &tech);
    let report = access_points(&db, &tech, &tracks);

    // u0/A rect [100,300]×[100,300] misses every met1 track on both
    // axes — exactly one named off-track finding.
    assert_eq!(report.off_track.len(), 1, "{:?}", report.off_track);
    let f = &report.off_track[0];
    let label = pin_label(&db, f.pin);
    println!("off_track finding: {label} rects={:?}", f.rects);
    assert_eq!(label, "u0/A");
    assert!(report.no_shapes.is_empty(), "{:?}", report.no_shapes);

    // The io port covers the (1000,1000) track crossing — its points
    // keep `points` nonempty so the finding is a named pin, not a void.
    assert!(!report.points.is_empty());
    assert!(report.points.iter().all(|ap| ap.pin != f.pin));
}

// ---------------------------------------------------------------------------
// Fail-proof (AGENTS §7.7)
// ---------------------------------------------------------------------------

/// Green → red → green: clearing every layer's preferred-axis list must
/// turn the census red, and rebuilding the grid restores green.
#[test]
fn test_access_failproof() {
    let (tech, db, tracks, report) = load("gcd");
    assert!(
        report.off_track.is_empty(),
        "gcd control must start clean: {:?}",
        report.off_track
    );

    let mut degraded = tracks.clone();
    for (i, layer) in tech.routing_layers.iter().enumerate() {
        match layer.dir {
            LayerDir::Horizontal => degraded[i].tracks_y.clear(),
            LayerDir::Vertical => degraded[i].tracks_x.clear(),
        }
    }
    let red = access_points(&db, &tech, &degraded);
    println!(
        "gcd with preferred axes dropped: off_track={} (first few: {})",
        red.off_track.len(),
        red.off_track
            .iter()
            .take(5)
            .map(|f| pin_label(&db, f.pin))
            .collect::<Vec<_>>()
            .join(", ")
    );
    assert!(
        !red.off_track.is_empty(),
        "dropping preferred-axis tracks must turn the census red"
    );

    let restored = access_points(&db, &tech, &layer_tracks(&db, &tech));
    assert!(
        restored.off_track.is_empty(),
        "rebuilt grid must restore green"
    );

    // The axis enum is also exercised: gaps report the preferred axis.
    let gaps = track_gaps(&tech, &degraded);
    assert_eq!(gaps.len(), tech.routing_layers.len());
    assert!(
        gaps.iter()
            .all(|g| match tech.routing_layers[g.layer.0 as usize].dir {
                LayerDir::Horizontal => g.axis == Axis::Y,
                LayerDir::Vertical => g.axis == Axis::X,
            })
    );
}

// ---------------------------------------------------------------------------
// Guide join
// ---------------------------------------------------------------------------

/// Every guide block resolves to a design net in file order (an extra
/// name would bail inside `resolve_guide_nets`); the absent `net_names`
/// nets classify into `missing_signal` (a gap — empty on both designs)
/// and `absent_expected` (<2-pin or non-signal, named not silent).
#[test]
fn test_guide_join() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, _, _) = load(name);
        let (parsed, join) = load_guide(&db, &tech, name);

        assert_eq!(
            join.nets.len(),
            parsed.len(),
            "{name}: unresolved guide blocks"
        );
        for ((net, _), (pname, _)) in join.nets.iter().zip(&parsed) {
            assert_eq!(guide_net_name(&db, *net), *pname, "{name}: order drift");
        }

        let missing: Vec<String> = join
            .missing_signal
            .iter()
            .map(|&n| net_name(&db, n))
            .collect();
        let mut absent: Vec<String> = join
            .absent_expected
            .iter()
            .map(|&n| net_name(&db, n))
            .collect();
        absent.sort();
        println!(
            "{name}: guide_blocks={} joined={} missing_signal={:?} absent_expected={} {:?}",
            parsed.len(),
            join.nets.len(),
            missing,
            absent.len(),
            absent
        );
        assert!(
            join.missing_signal.is_empty(),
            "{name}: missing_signal {missing:?}"
        );
        if name == "gcd" {
            // VGND/VPWR live in SPECIALNETS (`pdn_nets`), not `NETS`
            // — every `net_names` net has a guide block on gcd.
            assert!(absent.is_empty(), "{name}: absent_expected");
        }
    }
}

// ---------------------------------------------------------------------------
// Graph census
// ---------------------------------------------------------------------------

/// Per-guided-net region/node/edge census: nonzero totals, no
/// zero-coverage (net,layer) pairs, nodes sorted and deduped, and the
/// union bound `nodes <= Σ_box |covered ix|·|covered iy|` — the
/// O(boxes × tracks) sizing, never O(die).
#[test]
fn test_graph_census() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let graphs = build_route_graphs(&db, &tech, &tracks, &join);

        assert!(
            graphs.total_nodes > 0 && graphs.total_edges > 0,
            "{name}: empty graph"
        );
        let zc: Vec<String> = graphs
            .zero_coverage
            .iter()
            .map(|z| format!("({},{})", net_name(&db, z.net), z.layer.0))
            .collect();
        assert!(
            graphs.zero_coverage.is_empty(),
            "{name}: zero_coverage {zc:?}"
        );

        for (g, (_, boxes)) in graphs.nets.iter().zip(&join.nets) {
            assert!(
                g.nodes.windows(2).all(|w| w[0] < w[1]),
                "{name}: nodes unsorted or duplicated"
            );
            let bound: usize = boxes
                .iter()
                .map(|b| {
                    let lt = &tracks[b.layer.0 as usize];
                    let nx = lt.tracks_x.partition_point(|&t| t <= b.rect.xh)
                        - lt.tracks_x.partition_point(|&t| t < b.rect.xl);
                    let ny = lt.tracks_y.partition_point(|&t| t <= b.rect.yh)
                        - lt.tracks_y.partition_point(|&t| t < b.rect.yl);
                    nx * ny
                })
                .sum();
            assert!(
                g.nodes.len() <= bound,
                "{name}: net {} nodes {} > union bound {bound}",
                net_name(&db, g.net),
                g.nodes.len()
            );
        }

        println!(
            "{name}: guide_nets={} missing_signal={} absent_expected={} zero_cov={} nodes={} edges={} escapes={}",
            join.nets.len(),
            join.missing_signal.len(),
            join.absent_expected.len(),
            graphs.zero_coverage.len(),
            graphs.total_nodes,
            graphs.total_edges,
            graphs.region_escape.len()
        );
    }
}

/// The attachment partition, checked per element (§7.5): every member
/// pin of every guided net has ≥1 attached node or appears in
/// `region_escape` — and every escape appears in `pin_nodes` with an
/// empty list. Escape names print per design; the gcd set is pinned.
#[test]
fn test_pin_attachment() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let graphs = build_route_graphs(&db, &tech, &tracks, &join);
        let escapes: std::collections::BTreeSet<PinId> =
            graphs.region_escape.iter().map(|e| e.pin).collect();

        let mut members_total = 0usize;
        for g in &graphs.nets {
            let members = db.net_pins(g.net);
            members_total += members.len();
            assert_eq!(
                g.pin_nodes.len(),
                members.len(),
                "{name}: pin_nodes does not cover every member pin"
            );
            for (i, &(pin, ref ids)) in g.pin_nodes.iter().enumerate() {
                assert_eq!(pin, members[i], "{name}: pin order");
                assert!(ids.windows(2).all(|w| w[0] < w[1]));
                if escapes.contains(&pin) {
                    assert!(ids.is_empty(), "{name}: escape pin has nodes");
                } else {
                    assert!(
                        !ids.is_empty(),
                        "{name}: pin {} of net {} attached nothing and is not named",
                        pin_label(&db, pin),
                        net_name(&db, g.net)
                    );
                }
            }
        }

        let labels: Vec<String> = graphs
            .region_escape
            .iter()
            .map(|e| format!("{} (net {})", pin_label(&db, e.pin), net_name(&db, e.net)))
            .collect();
        let io_escapes = graphs
            .region_escape
            .iter()
            .filter(|e| db.pin_cells[e.pin.0 as usize].is_none())
            .count();
        println!(
            "{name}: member_pins={members_total} region_escape={} (io={io_escapes} inst={}) {labels:?}",
            labels.len(),
            labels.len() - io_escapes
        );
        let mut names: Vec<String> = graphs
            .region_escape
            .iter()
            .map(|e| pin_label(&db, e.pin))
            .collect();
        names.sort();
        if name == "gcd" {
            // 3 instance pins of the guide-stops-at-GCell-boundary
            // class plus 14 io ports whose ±140 met2 square holds an
            // x-track but no y-track — no in-region crossing exists
            // inside the pin shape. Recorded in docs/results.md.
            assert_eq!(
                names,
                [
                    "_301_/B1",
                    "_307_/B1",
                    "_313_/X",
                    "req_msg[0]",
                    "req_msg[16]",
                    "req_msg[17]",
                    "req_msg[18]",
                    "req_msg[1]",
                    "req_msg[25]",
                    "req_msg[26]",
                    "req_msg[2]",
                    "req_msg[3]",
                    "req_msg[9]",
                    "resp_msg[0]",
                    "resp_msg[10]",
                    "resp_msg[1]",
                    "resp_msg[9]",
                ]
            );
        } else {
            // Picorv32: same two classes — 355 instance pins at
            // guide-region boundaries and 237 io ports whose ±140
            // met2 square holds no in-region track crossing. Pin the
            // split so silent drift in either class trips the gate.
            assert_eq!(names.len(), 592, "{name}: region_escape drift");
            assert_eq!(io_escapes, 237, "{name}: io escape drift");
        }
    }
}

/// Determinism with nonzero evidence (§7.7): two runs produce
/// identical nodes, edges, and pin attachments on both designs.
#[test]
fn test_graph_determinism() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let g1 = build_route_graphs(&db, &tech, &tracks, &join);
        let g2 = build_route_graphs(&db, &tech, &tracks, &join);
        assert!(
            g1.total_nodes > 0 && g1.total_edges > 0,
            "{name}: empty graph proves nothing"
        );
        assert_eq!(g1.nets, g2.nets, "{name}: nondeterministic graphs");
        assert_eq!(g1.zero_coverage, g2.zero_coverage);
        assert_eq!(g1.region_escape, g2.region_escape);
        assert_eq!(g1.total_nodes, g2.total_nodes);
        assert_eq!(g1.total_edges, g2.total_edges);
        assert_eq!(g1.total_via_edges, g2.total_via_edges);
        assert_eq!(g1.via_edges_per_pair, g2.via_edges_per_pair);
    }
}

// ---------------------------------------------------------------------------
// Region-escape fixture
// ---------------------------------------------------------------------------

/// A met1 5×5 grid (tracks at 0..4000 step 1000); `u0/A` is a macro
/// pin rect `[900,1100]²` centered on crossing (1000,1000), io port a
/// `[2900,3100]²` rect on crossing (3000,3000). Net `n1` joins them.
const REGION_LEF: &str = r#"
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
MACRO CELL
  CLASS CORE ;
  SIZE 2.0 BY 2.0 ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.9 0.9 1.1 1.1 ;
    END
  END A
END CELL
END LIBRARY
"#;

const REGION_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN d ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 4000 4000 ) ;
ROW ROW_0 unithd 0 0 N DO 8 BY 1 STEP 460 2720 ;
TRACKS X 0 DO 5 STEP 1000 LAYER met1 ;
TRACKS Y 0 DO 5 STEP 1000 LAYER met1 ;
COMPONENTS 1 ;
- u0 CELL + PLACED ( 0 0 ) N ;
END COMPONENTS
PINS 1 ;
- io + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 3000 3000 ) N ;
END PINS
NETS 1 ;
- n1 ( PIN io ) ( u0 A ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

/// Green → red → green: the full guide attaches both pins; shrinking
/// u0/A's box below its pin rect's y-tracks escapes exactly `u0/A`;
/// shrinking both boxes under the grid escapes every member pin and
/// names `(n1, met1)` a zero-coverage pair; restoring goes green.
#[test]
fn test_region_escape_fixture() {
    let mut interner = StringInterner::new();
    let mut tech = TechDb::new();
    parse_lef_bytes(REGION_LEF.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
    tech.strings = interner.clone();
    tech.rebuild_maps();
    let db = parse_def_bytes(REGION_DEF.as_bytes(), &tech, None).expect("fixture DEF parses");
    let tracks = layer_tracks(&db, &tech);

    let guide = |text: &str| {
        let parsed = parse_guides(text, &tech).expect("fixture guide parses");
        join_guides(&db, &tech, &parsed).expect("fixture join")
    };

    // Control: each pin's crossing sits inside its own guide box.
    let full = "n1\n(\n0 0 2000 2000 met1\n2500 2500 3500 3500 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(full));
    assert_eq!(graphs.nets.len(), 1);
    assert!(graphs.total_nodes > 0 && graphs.total_edges > 0);
    assert!(
        graphs.region_escape.is_empty(),
        "{:?}",
        graphs.region_escape
    );
    assert!(graphs.zero_coverage.is_empty());
    assert!(
        graphs.nets[0]
            .pin_nodes
            .iter()
            .all(|(_, ids)| !ids.is_empty())
    );

    // The shrunk box covers x-track 1000 but no y-track inside u0/A's
    // rect — exactly u0/A escapes; io stays attached; met1 still has
    // covered crossings so the census stays clean of zero_coverage.
    let shrunk = "n1\n(\n500 200 1500 800 met1\n2500 2500 3500 3500 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(shrunk));
    assert!(
        graphs.zero_coverage.is_empty(),
        "{:?}",
        graphs.zero_coverage
    );
    assert_eq!(graphs.region_escape.len(), 1, "{:?}", graphs.region_escape);
    let e = &graphs.region_escape[0];
    assert_eq!(pin_label(&db, e.pin), "u0/A");
    assert_eq!(net_name(&db, e.net), "n1");

    // Fail-proof: both boxes under the grid — every member pin escapes
    // and (n1, met1) is a named zero-coverage pair, then restoring the
    // full guide returns green.
    let empty = "n1\n(\n500 200 1500 800 met1\n500 200 1500 800 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(empty));
    assert_eq!(graphs.total_nodes, 0);
    assert_eq!(graphs.region_escape.len(), 2, "{:?}", graphs.region_escape);
    assert_eq!(graphs.zero_coverage.len(), 1);
    let z = &graphs.zero_coverage[0];
    assert_eq!(net_name(&db, z.net), "n1");
    assert_eq!(z.layer.0, 0);

    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(full));
    assert!(graphs.region_escape.is_empty() && graphs.zero_coverage.is_empty());
}

// ---------------------------------------------------------------------------
// Via candidates + layer-transition edges
// ---------------------------------------------------------------------------

/// `variants.baseline.vias_per_cut_layer` (+ `via_names_meaning`) of
/// the committed `<d>.drt.reference.json` — `require`'d so a missing
/// file is a loud failure.
fn load_drt_ref(name: &str) -> serde_json::Value {
    let path = require(design_dir(name).join(format!("{name}.drt.reference.json")));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

/// The routed-DEF via census grouped by cut layer: `vias_per_cut_layer`
/// (name → instance count) bucketed by `via_names_meaning`'s cut name —
/// per cut layer `(majority name, total instances, used names)`.
/// `def_viarule:`-prefixed DEF names carry no `via_names_meaning`
/// entry and land under `<unknown>`, matching no real cut layer.
fn ref_via_census(r: &serde_json::Value) -> BTreeMap<String, (String, u64, Vec<String>)> {
    let census = r["variants"]["baseline"]["vias_per_cut_layer"]
        .as_object()
        .expect("vias_per_cut_layer missing");
    let meaning = r["variants"]["baseline"]["via_names_meaning"]
        .as_object()
        .expect("via_names_meaning missing");
    let mut by_cut: BTreeMap<String, Vec<(String, u64)>> = BTreeMap::new();
    for (via, count) in census {
        let cut = meaning
            .get(via)
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>");
        by_cut
            .entry(cut.to_string())
            .or_default()
            .push((via.clone(), count.as_u64().unwrap()));
    }
    by_cut
        .into_iter()
        .map(|(cut, mut used)| {
            used.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            let names: Vec<String> = used.iter().map(|(n, _)| n.clone()).collect();
            let total: u64 = used.iter().map(|(_, c)| c).sum();
            (cut, (used[0].0.clone(), total, names))
        })
        .collect()
}

/// The pair's policy pick, resolved to its interned name. `db.strings`
/// — not `tech.strings` — resolves every candidate name: it clones the
/// tech interner at parse start and additionally carries the DEF `VIAS`
/// names.
fn default_name<'a>(db: &'a DesignDb, table: &'a ViaTable, p: &ViaPair) -> &'a str {
    let c = &table.candidates[p.candidates[p.default.unwrap() as usize] as usize];
    db.strings.resolve(c.name)
}

fn routing_name(tech: &TechDb, l: LayerId) -> &str {
    tech.strings.resolve(tech.routing_layers[l.0 as usize].name)
}

/// The candidate table on both designs: every adjacent pair has ≥1
/// candidate (`gaps`/`unmatched` empty), counts pinned `{5,6,6,6,6}`
/// (five LEF defs per pair plus the four DEF `VIAS` generated records
/// on pairs 1–4 — `def_vias` retention exercised end to end), and the
/// per-pair default joins to the reference `vias_per_cut_layer`
/// census — matched on via/via2/via3, a name-only divergence on mcon
/// (`L1M1_PR` ≡ `L1M1_PR_MR` rect-for-rect, asserted per element), and
/// via4 unobserved.
#[test]
fn test_via_candidate_table() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, _, _) = load(name);
        let table = via_table(&tech, &db).expect("via_table");
        let ref_census = ref_via_census(&load_drt_ref(name));

        let gaps: Vec<String> = table
            .gaps
            .iter()
            .map(|g| format!("({},{})", routing_name(&tech, g.lower), routing_name(&tech, g.upper)))
            .collect();
        assert!(table.gaps.is_empty(), "{name}: via gaps {gaps:?}");
        let unmatched: Vec<String> = table
            .unmatched
            .iter()
            .map(|u| db.strings.resolve(u.name).to_string())
            .collect();
        assert!(
            table.unmatched.is_empty(),
            "{name}: unmatched via defs {unmatched:?}"
        );

        let counts: Vec<usize> = table.pairs.iter().map(|p| p.candidates.len()).collect();
        assert_eq!(counts, [5, 6, 6, 6, 6], "{name}: candidate counts");
        assert_eq!(table.candidates.len(), 29, "{name}: 25 LEF + 4 DEF vias");
        for p in &table.pairs {
            assert!(p.cut.is_some(), "{name}: pair {p:?} has no cut layer");
        }

        let defaults: Vec<&str> = table
            .pairs
            .iter()
            .map(|p| default_name(&db, &table, p))
            .collect();
        assert_eq!(
            defaults,
            ["L1M1_PR", "M1M2_PR", "M2M3_PR", "M3M4_PR", "M4M5_PR"],
            "{name}: default picks"
        );

        println!("== {name} via candidate table ==");
        for p in &table.pairs {
            let cut_name = tech.strings.resolve(p.cut.unwrap());
            let (majority, total, used) = ref_census
                .get(cut_name)
                .map(|(m, t, u)| (m.as_str(), *t, u.clone()))
                .unwrap_or(("<unobserved>", 0, Vec::new()));
            let our = default_name(&db, &table, p).to_string();
            println!(
                "  {}–{} cut={cut_name} candidates={} default={our} ref_majority={majority} ref_total={total} ref_used={used:?}",
                routing_name(&tech, p.lower),
                routing_name(&tech, p.upper),
                p.candidates.len(),
            );
            match cut_name {
                "via" | "via2" | "via3" => {
                    assert_eq!(
                        our, majority,
                        "{name}: default diverges from reference on {cut_name}"
                    );
                }
                "mcon" => {
                    assert_eq!(majority, "L1M1_PR_MR", "{name}: mcon majority drift");
                    assert_eq!(our, "L1M1_PR");
                    // Name-only divergence: the two defs must stay
                    // rect-for-rect identical — a future deck change
                    // that differentiates them fails loudly here.
                    let rects_of = |n: &str| {
                        table
                            .candidates
                            .iter()
                            .find(|c| db.strings.resolve(c.name) == n)
                            .unwrap()
                            .rects
                            .clone()
                    };
                    assert_eq!(rects_of("L1M1_PR"), rects_of("L1M1_PR_MR"));
                }
                "via4" => {
                    // Zero M4M5 instances in either routed DEF — a
                    // policy pick with no reference evidence.
                    assert!(
                        !ref_census.contains_key("via4"),
                        "{name}: via4 unexpectedly observed"
                    );
                }
                other => panic!("{name}: unexpected cut layer {other}"),
            }
        }
    }
}

/// The layer-transition edge census: `total_via_edges` nonzero on both
/// designs; `via_edges_per_pair` printed and joined to the reference
/// census; met2–met3 pinned at 0 — the named edge-domain tech gap
/// (met2 coords ≡10 mod 20, met3 ≡0 mod 20, the grids never coincide);
/// every edge endpoint checked per element for adjacent layers at an
/// identical (x,y); deterministic double-run.
#[test]
fn test_via_edge_census() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let graphs = build_route_graphs(&db, &tech, &tracks, &join);
        let table = via_table(&tech, &db).expect("via_table");
        let ref_census = ref_via_census(&load_drt_ref(name));

        assert!(
            graphs.total_via_edges > 0,
            "{name}: zero via edges — the census is vacuous"
        );
        assert_eq!(
            graphs.via_edges_per_pair.len(),
            tech.routing_layers.len() - 1
        );
        // met2–met3 is pinned at 0 — the named edge-domain tech gap
        // (met2 coords ≡10 mod 20, met3 ≡0 mod 20, the grids never
        // coincide; docs/e6-drt.md §6). met4–met5's shared crossings
        // (1 gcd / 36 picorv32 pre-coverage) admit no guide-covered
        // node pair on either design. The full row is pinned so
        // silent drift trips the gate.
        let expected: Vec<usize> = if name == "gcd" {
            vec![13_292, 375, 0, 2, 0]
        } else {
            vec![395_479, 13_961, 0, 47, 0]
        };
        assert_eq!(
            graphs.via_edges_per_pair, expected,
            "{name}: via-edge census drift"
        );
        assert_eq!(graphs.total_via_edges, expected.iter().sum::<usize>());

        // §7.5 per-element check: every via edge connects adjacent
        // layers at the same (x,y).
        for g in &graphs.nets {
            for e in &g.via_edges {
                let lo = g.nodes[e.lower as usize];
                let hi = g.nodes[e.upper as usize];
                assert!(e.lower < e.upper, "{name}: via edge order");
                assert_eq!(lo.layer.0 + 1, hi.layer.0, "{name}: non-adjacent via");
                let lt = &tracks[lo.layer.0 as usize];
                let ut = &tracks[hi.layer.0 as usize];
                assert_eq!(lt.tracks_x[lo.ix as usize], ut.tracks_x[hi.ix as usize]);
                assert_eq!(lt.tracks_y[lo.iy as usize], ut.tracks_y[hi.iy as usize]);
            }
        }

        assert!(table.gaps.is_empty(), "{name}: via gaps");
        assert!(
            graphs.via_edges_per_pair[0] > 0
                && graphs.via_edges_per_pair[1] > 0
                && graphs.via_edges_per_pair[3] > 0,
            "{name}: pairs the reference transitions have no edges"
        );

        println!("== {name} via edge census ==");
        for (k, p) in table.pairs.iter().enumerate() {
            let cut_name = tech.strings.resolve(p.cut.unwrap());
            let (majority, used) = ref_census
                .get(cut_name)
                .map(|(m, _, u)| (m.as_str(), u.clone()))
                .unwrap_or(("<unobserved>", Vec::new()));
            let our = p
                .default
                .map(|_| default_name(&db, &table, p))
                .unwrap_or("<none>");
            println!(
                "  {}–{} cut={cut_name} candidates={} via_edges={} default={our} ref_majority={majority} ref_used={used:?}",
                routing_name(&tech, p.lower),
                routing_name(&tech, p.upper),
                p.candidates.len(),
                graphs.via_edges_per_pair[k],
            );
        }
        println!("{name}: total_via_edges={}", graphs.total_via_edges);

        // Determinism: a second run yields identical per-net via edges
        // and pair counts.
        let g2 = build_route_graphs(&db, &tech, &tracks, &join);
        for (a, b) in graphs.nets.iter().zip(&g2.nets) {
            assert_eq!(a.via_edges, b.via_edges, "{name}: nondeterministic via edges");
        }
        assert_eq!(graphs.via_edges_per_pair, g2.via_edges_per_pair);
    }
}

// ---------------------------------------------------------------------------
// Via-edge fixture
// ---------------------------------------------------------------------------

/// Two-layer stack with `mv12` last so truncating at `"VIA mv12"`
/// yields the no-via variant. met1 tracks land on {0,1000,2000}, met2
/// on {0,1500} — (0,0) is the only shared crossing.
const VIA_FIXTURE_LEF: &str = r#"
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
  WIDTH 0.15 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
MACRO CELL
  CLASS CORE ;
  SIZE 2.0 BY 2.0 ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.9 0.9 1.1 1.1 ;
    END
  END A
END CELL
VIA mv12
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END mv12
"#;

const VIA_FIXTURE_DEF_HEAD: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN d ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 4000 4000 ) ;
ROW ROW_0 unithd 0 0 N DO 8 BY 1 STEP 460 2720 ;
TRACKS X 0 DO 3 STEP 1000 LAYER met1 ;
TRACKS Y 0 DO 3 STEP 1000 LAYER met1 ;
TRACKS X 0 DO 2 STEP 1500 LAYER met2 ;
TRACKS Y 0 DO 2 STEP 1500 LAYER met2 ;
"#;

/// `+ VIARULE` generated record — exercises `def_vias` retention
/// end-to-end through `parse_def_bytes`.
const VIA_FIXTURE_VIAS: &str = r#"VIAS 1 ;
- gen + VIARULE r + CUTSIZE 100 100 + LAYERS met1 via12 met2 + CUTSPACING 0 0 + ENCLOSURE 10 10 10 10 ;
END VIAS
"#;

const VIA_FIXTURE_DEF_TAIL: &str = r#"COMPONENTS 1 ;
- u0 CELL + PLACED ( 0 0 ) N ;
END COMPONENTS
PINS 1 ;
- io + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 0 0 ) N ;
END PINS
NETS 1 ;
- n1 ( PIN io ) ( u0 A ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

/// Shared-crossing via edges on a synthetic grid: exactly one edge at
/// (0,0); off-crossing nodes touch none; dropping the met2 guide box
/// turns the count red; and a LEF with no via def makes (met1, met2)
/// a named `ViaGap`.
#[test]
fn test_via_edge_fixture() {
    let lef = |with_via: bool| {
        let text = if with_via {
            VIA_FIXTURE_LEF
        } else {
            &VIA_FIXTURE_LEF[..VIA_FIXTURE_LEF.find("VIA mv12").unwrap()]
        };
        let mut interner = StringInterner::new();
        let mut tech = TechDb::new();
        parse_lef_bytes(text.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
        tech.strings = interner.clone();
        tech.rebuild_maps();
        tech
    };
    let def_text = |with_vias: bool| {
        format!(
            "{}{}{}",
            VIA_FIXTURE_DEF_HEAD,
            if with_vias { VIA_FIXTURE_VIAS } else { "" },
            VIA_FIXTURE_DEF_TAIL
        )
    };

    let tech = lef(true);
    let db = parse_def_bytes(def_text(true).as_bytes(), &tech, None).expect("fixture DEF parses");
    assert_eq!(db.def_vias.len(), 1, "DEF VIAS record not retained");

    let tracks = layer_tracks(&db, &tech);
    let guide = |text: &str| {
        let parsed = parse_guides(text, &tech).expect("fixture guide parses");
        join_guides(&db, &tech, &parsed).expect("fixture join")
    };
    let node_xy = |g: &grus_core::drt::graph::NetGraph, id: u32| {
        let n = g.nodes[id as usize];
        let lt = &tracks[n.layer.0 as usize];
        (lt.tracks_x[n.ix as usize], lt.tracks_y[n.iy as usize])
    };

    // Full guide: exactly one via edge, between the (0,0) nodes.
    let full = "n1\n(\n0 0 2000 2000 met1\n0 0 2000 2000 met2\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(full));
    assert_eq!(graphs.via_edges_per_pair, vec![1]);
    assert_eq!(graphs.total_via_edges, 1);
    let g = &graphs.nets[0];
    assert_eq!(g.nodes.len(), 9 + 4, "9 met1 + 4 met2 crossings");
    let e = g.via_edges[0];
    assert_eq!(node_xy(g, e.lower), (0, 0));
    assert_eq!(node_xy(g, e.upper), (0, 0));

    // Off-crossing nodes touch no via edge: (met1,1000,1000) is a met1
    // crossing off every met2 grid line; (met2,1500,1500) vice versa.
    for &id in &[4usize, 12usize] {
        assert_eq!(node_xy(g, id as u32), if id == 4 { (1000, 1000) } else { (1500, 1500) });
        assert!(
            g.via_edges
                .iter()
                .all(|e| e.lower != id as u32 && e.upper != id as u32),
            "off-crossing node {id} touches a via edge"
        );
    }

    // Coverage gate: dropping the met2 box leaves the crossing on the
    // grid but no upper node → 0 edges; restoring returns 1.
    let no_met2 = "n1\n(\n0 0 2000 2000 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(no_met2));
    assert_eq!(graphs.total_via_edges, 0, "met2 box dropped must go red");
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(full));
    assert_eq!(graphs.total_via_edges, 1, "restored guide must go green");

    // Table side on the fixture: `mv12` (LEF) + `gen` (DEF VIAS) both
    // match (met1, met2); dropping `mv12` leaves `gen` the lone
    // candidate; no defs at all makes the pair a named ViaGap.
    let table = via_table(&tech, &db).unwrap();
    assert_eq!(table.pairs[0].candidates.len(), 2);
    let names: Vec<&str> = table.pairs[0]
        .candidates
        .iter()
        .map(|&i| db.strings.resolve(table.candidates[i as usize].name))
        .collect();
    assert_eq!(names, ["mv12", "gen"]);

    let tech_novia = lef(false);
    let db_bare = parse_def_bytes(def_text(false).as_bytes(), &tech_novia, None)
        .expect("fixture DEF parses");
    let table = via_table(&tech_novia, &db_bare).unwrap();
    assert_eq!(
        table.gaps,
        vec![grus_core::drt::via::ViaGap {
            lower: LayerId(0),
            upper: LayerId(1)
        }]
    );
    assert!(table.pairs[0].default.is_none());
}

// ---------------------------------------------------------------------------
// Maze router
// ---------------------------------------------------------------------------

use grus_core::drt::maze::{
    FailKind, MazeConfig, OccMode, overlap_census, route_digest, route_nets,
};
use grus_core::drt::graph::RouteGraphs;

/// `load` + `load_guide` + `build_route_graphs` + `via_table` — the
/// full router input bundle for one placed design.
fn load_routed_inputs(
    name: &str,
) -> (
    TechDb,
    DesignDb,
    Vec<LayerTracks>,
    GuideJoin,
    RouteGraphs,
    ViaTable,
) {
    let (tech, db, tracks, _) = load(name);
    let (_, join) = load_guide(&db, &tech, name);
    // The maze gets one spare layer above each net's topmost guided
    // layer — OBS-sealed pin pockets need it (OpenROAD's routed DEF
    // escapes on the next layer inside the same guide XY).
    let graphs = grus_core::drt::graph::build_route_graphs_ext(&db, &tech, &tracks, &join, 1);
    let vias = via_table(&tech, &db).expect("via_table");
    (tech, db, tracks, join, graphs, vias)
}

/// The maze fixture: met1 `HORIZONTAL` + met2 `VERTICAL` on one 6×6
/// shared-crossing grid, `mv12` between them. `u0/A` is a met1 pin
/// rect on crossing (1000,1000), `u1/B` a met2 pin on (4000,3000),
/// `u2/C` a met1 rect holding no track at all (the `Unattachable`
/// class), and `blk`'s macro `OBS` covers the direct met1 line at
/// y=1000 between x 1500..2500.
const MAZE_LEF: &str = r#"
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
  SPACING 0.14 ;
END met1
LAYER via12
  TYPE CUT ;
  WIDTH 0.15 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
  SPACING 0.14 ;
END met2
MACRO CELL
  CLASS CORE ;
  SIZE 5.0 BY 5.0 ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.9 0.9 1.1 1.1 ;
    END
  END A
END CELL
MACRO CELLB
  CLASS CORE ;
  SIZE 5.0 BY 5.0 ;
  PIN B
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met2 ;
        RECT 3.9 2.9 4.1 3.1 ;
    END
  END B
END CELLB
MACRO CELLC
  CLASS CORE ;
  SIZE 5.0 BY 5.0 ;
  PIN C
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 1.4 1.4 1.6 1.6 ;
    END
  END C
END CELLC
MACRO BLK
  CLASS CORE ;
  SIZE 5.0 BY 5.0 ;
  OBS
    LAYER met1 ;
      RECT 1.5 0.9 2.5 1.1 ;
  END
END BLK
VIA mv12
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END mv12
END LIBRARY
"#;

const MAZE_DEF_HEAD: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN d ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 5000 5000 ) ;
ROW ROW_0 unithd 0 0 N DO 10 BY 1 STEP 460 2720 ;
TRACKS X 0 DO 6 STEP 1000 LAYER met1 ;
TRACKS Y 0 DO 6 STEP 1000 LAYER met1 ;
TRACKS X 0 DO 6 STEP 1000 LAYER met2 ;
TRACKS Y 0 DO 6 STEP 1000 LAYER met2 ;
"#;

const MAZE_DEF_TAIL: &str = r#"PINS 3 ;
- io1 + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 1000 ) N ;
- io2 + NET n2 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 1000 3000 ) N ;
- io3 + NET n3 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 4000 ) N ;
END PINS
NETS 3 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
- n2 ( PIN io2 ) ( u1 B ) + USE SIGNAL ;
- n3 ( PIN io3 ) ( u2 C ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

fn maze_tech() -> TechDb {
    let mut interner = StringInterner::new();
    let mut tech = TechDb::new();
    parse_lef_bytes(MAZE_LEF.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
    tech.strings = interner.clone();
    tech.rebuild_maps();
    tech
}

fn maze_db(tech: &TechDb, with_blk: bool) -> DesignDb {
    let comps = if with_blk {
        "COMPONENTS 4 ;\n- u0 CELL + PLACED ( 0 0 ) N ;\n- u1 CELLB + PLACED ( 0 0 ) N ;\n- u2 CELLC + PLACED ( 0 0 ) N ;\n- blk BLK + PLACED ( 0 0 ) N ;\nEND COMPONENTS\n"
    } else {
        "COMPONENTS 3 ;\n- u0 CELL + PLACED ( 0 0 ) N ;\n- u1 CELLB + PLACED ( 0 0 ) N ;\n- u2 CELLC + PLACED ( 0 0 ) N ;\nEND COMPONENTS\n"
    };
    let text = format!("{MAZE_DEF_HEAD}{comps}{MAZE_DEF_TAIL}");
    parse_def_bytes(text.as_bytes(), tech, None).expect("fixture DEF parses")
}

fn maze_guide(db: &DesignDb, tech: &TechDb, text: &str) -> GuideJoin {
    let parsed = parse_guides(text, tech).expect("fixture guide parses");
    join_guides(db, tech, &parsed).expect("fixture join")
}

/// The four acceptance cases on the hand-built design: straight met1
/// route, forced met1→met2 via, detour around a macro `OBS` interval
/// (control vs blocked), and named unroutable pins — `NoPath` from a
/// disconnected region and `Unattachable` from a track-less rect —
/// all returning inside the iteration cap.
#[test]
fn test_maze_cases() {
    let tech = maze_tech();
    let cfg = MazeConfig::default();

    // Case 1 — straight route on one track.
    let db = maze_db(&tech, false);
    let tracks = layer_tracks(&db, &tech);
    let join = maze_guide(&db, &tech, "n1\n(\n0 0 5000 2000 met1\n)\n");
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let vias = via_table(&tech, &db).unwrap();
    let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 1);
    let n1 = &r.nets[0];
    assert_eq!(
        n1.wires,
        vec![grus_core::drt::maze::WireSeg {
            layer: LayerId(0),
            x1: 1000,
            y1: 1000,
            x2: 4000,
            y2: 1000
        }],
        "straight route must be a single merged met1 segment"
    );
    assert!(n1.vias.is_empty());
    assert_eq!(n1.connected.len(), 2);
    assert!(r.collisions.is_empty());
    let control_wires = n1.wires.len();

    // Case 2 — forced via transition: met1 io pin to a met2-only pin.
    let join = maze_guide(
        &db,
        &tech,
        "n2\n(\n0 0 5000 5000 met1\n0 0 5000 5000 met2\n)\n",
    );
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    let n2 = &r.nets[0];
    assert_eq!(n2.connected.len(), 2);
    assert_eq!(
        n2.vias,
        vec![grus_core::drt::maze::ViaPlace {
            lower: LayerId(0),
            x: 4000,
            y: 3000
        }],
        "single cheapest via at the met2 pin's shared crossing"
    );

    // Case 3 — detour around the blocked interval: same n1 geometry
    // as case 1 but the `blk` macro's OBS rect sits on the direct
    // met1 line.
    let db_blk = maze_db(&tech, true);
    let tracks_blk = layer_tracks(&db_blk, &tech);
    let join = maze_guide(&db_blk, &tech, "n1\n(\n0 0 5000 2000 met1\n)\n");
    let graphs = build_route_graphs(&db_blk, &tech, &tracks_blk, &join);
    let vias_blk = via_table(&tech, &db_blk).unwrap();
    let r = route_nets(&db_blk, &tech, &tracks_blk, &graphs, &vias_blk, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    let n1 = &r.nets[0];
    assert_eq!(n1.connected.len(), 2);
    assert!(
        n1.wires.len() > control_wires,
        "the OBS detour must jog ({control_wires} control vs {} routed)",
        n1.wires.len()
    );
    // The emitted intervals — not trust — prove the detour: no met1
    // wire may enter the OBS rect inflated by w/2 + s = 210.
    let inflated = grus_core::tech_db::RectDbu::new(1500 - 210, 900 - 210, 2500 + 210, 1100 + 210);
    for w in &n1.wires {
        let hit = w.x1 < inflated.xh && w.x2 > inflated.xl && w.y1 < inflated.yh && w.y2 > inflated.yl;
        assert!(!hit, "wire {w:?} enters the OBS spacing band");
    }

    // Case 4 — provably unroutable, both classes: n1's guide splits
    // into two components (io reachable, u0/A `NoPath`), and u2/C's
    // rect covers no track at all (`Unattachable`). Bounded by
    // `max_iters` — the harness completing is the no-hang evidence.
    let join = maze_guide(
        &db,
        &tech,
        "n1\n(\n0 0 2000 2000 met1\n3500 500 4500 1500 met1\n)\nn3\n(\n0 0 5000 5000 met1\n)\n",
    );
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
    assert!(r.nets.is_empty(), "{:?}", r.nets);
    assert_eq!(r.unrouted.len(), 2, "{:?}", r.unrouted);
    let labels = |u: &grus_core::drt::maze::UnroutedNet| -> Vec<(String, FailKind)> {
        u.failed
            .iter()
            .map(|&(p, k)| (pin_label(&db, p), k))
            .collect()
    };
    assert_eq!(net_name(&db, r.unrouted[0].net), "n1");
    assert_eq!(
        labels(&r.unrouted[0]),
        vec![("u0/A".to_string(), FailKind::NoPath)]
    );
    assert_eq!(net_name(&db, r.unrouted[1].net), "n3");
    assert_eq!(
        labels(&r.unrouted[1]),
        vec![("u2/C".to_string(), FailKind::Unattachable)]
    );
}

/// True when the net's guide boxes form a single component under
/// same-layer / adjacent-layer 2D-overlap (the planning probe's union
/// connectivity). `max_layer` bounds the box set (LayerId(2) = met2).
fn guide_union_connected(boxes: &[GuideBox], max_layer: u8) -> bool {
    let kept: Vec<usize> = (0..boxes.len())
        .filter(|&i| boxes[i].layer.0 <= max_layer)
        .collect();
    if kept.is_empty() {
        return false;
    }
    let mut parent: Vec<usize> = (0..kept.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut j = i;
        while p[j] != r {
            let n = p[j];
            p[j] = r;
            j = n;
        }
        r
    }
    for a in 0..kept.len() {
        for b in a + 1..kept.len() {
            let (ba, bb) = (&boxes[kept[a]], &boxes[kept[b]]);
            if ba.layer.0.abs_diff(bb.layer.0) > 1 {
                continue;
            }
            let r = (
                ba.rect.xl < bb.rect.xh,
                ba.rect.xh > bb.rect.xl,
                ba.rect.yl < bb.rect.yh,
                ba.rect.yh > bb.rect.yl,
            );
            if r.0 && r.1 && r.2 && r.3 {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    (0..kept.len())
        .map(|i| find(&mut parent, i))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == 1
}

/// The connectivity census: every guided signal net either routes all
/// member pins or lands in the unrouted report with every failed pin
/// named (`FailKind`). Prints the counts `docs/results.md` records —
/// guided, routed, pins connected, projection-attachment count,
/// unrouted by name, collisions, iterations, wall-clock ms.
#[test]
fn test_maze_census() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, join, graphs, vias) = load_routed_inputs(name);
        let t0 = std::time::Instant::now();
        let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &MazeConfig::default());
        let ms = t0.elapsed().as_millis();

        let pins_connected: usize = r.nets.iter().map(|n| n.connected.len()).sum();
        let projected: usize = r.nets.iter().map(|n| n.projected.len()).sum();
        println!("== {name} maze census ==");
        println!(
            "{name}: guided={} routed={} pins_connected={pins_connected} projected={projected} unrouted={} collisions={} iterations={} ms={ms} digest={:#x}",
            join.nets.len(),
            r.nets.len(),
            r.unrouted.len(),
            r.collisions.len(),
            r.iterations,
            route_digest(&r),
        );
        for u in &r.unrouted {
            let kinds: Vec<String> = u
                .failed
                .iter()
                .map(|&(p, k)| format!("{}:{k:?}", pin_label(&db, p)))
                .collect();
            println!("  unrouted {} pins={kinds:?}", net_name(&db, u.net));
        }

        // Total accounting — no silent drops.
        assert_eq!(
            r.nets.len() + r.unrouted.len(),
            join.nets.len(),
            "{name}: nets + unrouted != guided"
        );
        // Every failure is a named pin with a kind.
        for u in &r.unrouted {
            assert!(!u.failed.is_empty(), "{name}: unnamed failure");
        }
        // Nonzero evidence.
        assert!(!r.nets.is_empty(), "{name}: nothing routed");
        assert!(
            r.nets.iter().all(|n| !n.wires.is_empty()),
            "{name}: routed net with no wires"
        );

        // Classify `NoPath` nets: the met3-bridge class has a
        // disconnected {li1,met1,met2} box union that only the met3+
        // boxes rejoin (the recorded met2–met3 crossing gap).
        let mut unclassified = Vec::new();
        for u in &r.unrouted {
            let (_, boxes) = join
                .nets
                .iter()
                .find(|(n, _)| *n == u.net)
                .expect("unrouted net must be guided");
            let bridged = !guide_union_connected(boxes, 2) && guide_union_connected(boxes, u8::MAX);
            for (_, k) in &u.failed {
                if *k == FailKind::NoPath && !bridged {
                    unclassified.push(net_name(&db, u.net));
                }
            }
            println!(
                "  {} met3_bridge={bridged}",
                net_name(&db, u.net)
            );
        }
        assert!(
            unclassified.is_empty(),
            "{name}: NoPath nets outside the met3-bridge class: {unclassified:?}"
        );
    }
}

/// Occupancy fail-proof (AGENTS §7.7): identical inputs, `Enabled` vs
/// `Disabled` — the outcomes must measurably differ somewhere (route
/// digest or post-hoc collision census), else occupancy constrains
/// nothing and the gate must go red.
#[test]
fn test_maze_occupancy() {
    let (tech, db, tracks, _, graphs, vias) = load_routed_inputs("gcd");
    let on = route_nets(&db, &tech, &tracks, &graphs, &vias, &MazeConfig::default());
    let off = route_nets(
        &db,
        &tech,
        &tracks,
        &graphs,
        &vias,
        &MazeConfig {
            occ: OccMode::Disabled,
            ..MazeConfig::default()
        },
    );
    let on_census = overlap_census(&on.nets, &tech, &tracks, &vias).len();
    let off_census = overlap_census(&off.nets, &tech, &tracks, &vias).len();
    println!(
        "occupancy fail-proof: enabled digest={:#x} census={on_census} | disabled digest={:#x} census={off_census}",
        route_digest(&on),
        route_digest(&off),
    );
    assert!(
        route_digest(&on) != route_digest(&off) || on_census != off_census,
        "disabling occupancy changed nothing — occupancy constrains no route"
    );
}

/// Determinism with nonzero evidence (§7.7): two full `route_nets`
/// calls produce an identical `RouteDb` and identical digest on both
/// designs.
#[test]
fn test_maze_determinism() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _, graphs, vias) = load_routed_inputs(name);
        let cfg = MazeConfig::default();
        let r1 = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
        let r2 = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
        assert!(!r1.nets.is_empty(), "{name}: empty route set proves nothing");
        assert_ne!(route_digest(&r1), 0, "{name}: zero digest");
        assert_eq!(r1, r2, "{name}: nondeterministic routes");
        assert_eq!(route_digest(&r1), route_digest(&r2));
    }
}

