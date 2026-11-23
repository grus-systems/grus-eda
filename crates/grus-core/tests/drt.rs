//! E6 DRT substrate gate: the merged LEF∪DEF track grid and the
//! candidate access-point census over the committed placed DEFs of gcd
//! and picorv32.
//!
//! What this proves:
//! - Every routing layer carries nonempty track lists on both axes and
//!   no preferred-axis `TrackGap` — the grid the maze router needs.
//! - Every signal-net member pin (instance pins and io `BTERM`s) has at
//!   least one access point; `off_track`/`no_shapes` findings are empty
//!   on both designs and would name their pins if not.
//! - Output is deterministic (double-run assert on a nonempty `points`).
//! - A synthetic off-track pin is a named finding, and dropping the
//!   preferred-axis tracks turns the census red — the gate can fail.
//!
//! Tests fail loudly on missing inputs — no silent skips.

use grus_core::congestion::guide::{
    GuideBox, guide_net_name, parse_guides,
};
use grus_core::design_db::DesignDb;
use grus_core::drt::access::{AccessReport, access_points};
use grus_core::drt::graph::{
    GuideJoin, build_route_graphs, join_guides,
};
use grus_core::drt::tracks::{Axis, LayerTracks, layer_tracks, track_gaps};
use grus_core::interner::{NetId, PinId, StringInterner};
use grus_core::readers::{load_tech_db, parse_def, parse_def_bytes, parse_lef_bytes};
use grus_core::tech_db::{LayerDir, TechDb};
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
                tech.strings.resolve(tech.routing_layers[g.layer.0 as usize].name),
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
    parse_lef_bytes(OFFTRACK_LEF.as_bytes(), &mut tech, &mut interner)
        .expect("fixture LEF parses");
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
    assert!(restored.off_track.is_empty(), "rebuilt grid must restore green");

    // The axis enum is also exercised: gaps report the preferred axis.
    let gaps = track_gaps(&tech, &degraded);
    assert_eq!(gaps.len(), tech.routing_layers.len());
    assert!(gaps.iter().all(|g| match tech.routing_layers[g.layer.0 as usize].dir {
        LayerDir::Horizontal => g.axis == Axis::Y,
        LayerDir::Vertical => g.axis == Axis::X,
    }));
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
    parse_lef_bytes(REGION_LEF.as_bytes(), &mut tech, &mut interner)
        .expect("fixture LEF parses");
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
    assert!(graphs.region_escape.is_empty(), "{:?}", graphs.region_escape);
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
    assert!(graphs.zero_coverage.is_empty(), "{:?}", graphs.zero_coverage);
    assert_eq!(
        graphs.region_escape.len(),
        1,
        "{:?}",
        graphs.region_escape
    );
    let e = &graphs.region_escape[0];
    assert_eq!(pin_label(&db, e.pin), "u0/A");
    assert_eq!(net_name(&db, e.net), "n1");

    // Fail-proof: both boxes under the grid — every member pin escapes
    // and (n1, met1) is a named zero-coverage pair, then restoring the
    // full guide returns green.
    let empty = "n1\n(\n500 200 1500 800 met1\n500 200 1500 800 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(empty));
    assert_eq!(graphs.total_nodes, 0);
    assert_eq!(
        graphs.region_escape.len(),
        2,
        "{:?}",
        graphs.region_escape
    );
    assert_eq!(graphs.zero_coverage.len(), 1);
    let z = &graphs.zero_coverage[0];
    assert_eq!(net_name(&db, z.net), "n1");
    assert_eq!(z.layer.0, 0);

    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(full));
    assert!(graphs.region_escape.is_empty() && graphs.zero_coverage.is_empty());
}
