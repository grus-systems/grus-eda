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
use grus_core::drt::graph::{
    GuideJoin, HullMask, build_route_graphs, cell_hull_rect, join_guides, pin_rects,
};
use grus_core::drt::tracks::{Axis, LayerTracks, layer_tracks, track_gaps};
use grus_core::drt::via::{ViaPair, ViaTable, via_table};
use grus_core::interner::{LayerId, NetId, PinId, StringInterner};
use grus_core::readers::{load_tech_db, parse_def, parse_def_bytes, parse_lef_bytes};
use grus_core::tech_db::{LayerDir, TechDb};
use std::collections::{BTreeMap, BTreeSet};
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

        let hull = HullMask::new(&db, &tech);
        let transit_l = (0..tech.routing_layers.len())
            .find(|&l| !hull.masked(l as u8))
            .map(|l| l as u8);
        let grid = |r: &grus_core::tech_db::RectDbu, lt: &LayerTracks| -> usize {
            let nx = lt.tracks_x.partition_point(|&t| t <= r.xh)
                - lt.tracks_x.partition_point(|&t| t < r.xl);
            let ny = lt.tracks_y.partition_point(|&t| t <= r.yh)
                - lt.tracks_y.partition_point(|&t| t < r.yl);
            nx * ny
        };
        let union =
            |a: grus_core::tech_db::RectDbu, b: grus_core::tech_db::RectDbu| {
                grus_core::tech_db::RectDbu::new(
                    a.xl.min(b.xl),
                    a.yl.min(b.yl),
                    a.xh.max(b.xh),
                    a.yh.max(b.yh),
                )
            };
        for (g, (net, boxes)) in graphs.nets.iter().zip(&join.nets) {
            assert!(
                g.nodes.windows(2).all(|w| w[0] < w[1]),
                "{name}: nodes unsorted or duplicated"
            );
            // Per-layer union bound: guide boxes contribute their
            // covered crossings (the mask only removes). On the
            // lowest unmasked layer `transit_extend` adds each member
            // host hull's track grid plus an index-adjacent
            // L-corridor to the nearest box pair — the corridor lies
            // inside the index bbox spanning the hull and the net's
            // box union on that layer, so per host hull the grid of
            // that bbox bounds the extension.
            let mut bounds: BTreeMap<u8, usize> = BTreeMap::new();
            for b in boxes {
                *bounds.entry(b.layer.0).or_default() +=
                    grid(&b.rect, &tracks[b.layer.0 as usize]);
            }
            let cover = pin_rects(&db, &tech, *net);
            if let Some(l) = transit_l
                && !cover.is_empty()
            {
                let mut span: Option<grus_core::tech_db::RectDbu> = None;
                for b in boxes.iter().filter(|b| b.layer.0 == l) {
                    span = Some(match span {
                        None => b.rect,
                        Some(r) => union(r, b.rect),
                    });
                }
                for &c in cover.keys() {
                    let h = cell_hull_rect(&db, &tech, c);
                    // Hull grid alone when no box covers the layer
                    // (`base.is_empty()` skips only the corridor).
                    let ext = span.map_or(h, |s| union(h, s));
                    *bounds.entry(l).or_default() += grid(&ext, &tracks[l as usize]);
                }
            }
            let mut per_layer: BTreeMap<u8, usize> = BTreeMap::new();
            for n in &g.nodes {
                *per_layer.entry(n.layer.0).or_default() += 1;
            }
            for (l, cnt) in &per_layer {
                let bound = bounds.get(l).copied().unwrap_or(0);
                assert!(
                    *cnt <= bound,
                    "{name}: net {} layer {l} nodes {cnt} > union bound {bound}",
                    net_name(&db, g.net),
                );
            }
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
        // The hull mask's node filter removes shared crossings inside
        // foreign instance hulls on masked layers (li1/met1) — the
        // census drop vs the pre-mask pins is the mask working, not
        // lost connectivity (masked nodes are unreachable by
        // definition).
        let expected: Vec<usize> = if name == "gcd" {
            vec![6_636, 322, 0, 2, 0]
        } else {
            vec![180_346, 11_128, 0, 47, 0]
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

    // Coverage gate: dropping the met1 box leaves the transit-layer
    // grid (guide + `transit_extend`'s host-hull met2 fill) but no
    // lower node → 0 edges; restoring returns 1. (met2 coverage is
    // partially hull-derived for a net with member pins, so the
    // met2-drop arm no longer isolates the guide-coverage rule.)
    let no_met1 = "n1\n(\n0 0 2000 2000 met2\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(no_met1));
    assert_eq!(graphs.total_via_edges, 0, "met1 box dropped must go red");
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
    FailKind, IterStat, MazeConfig, OccMode, overlap_census, route_digest, route_nets,
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
/// y=1000 between x 1500..2500. Each macro's `SIZE` tightly covers
/// its own pin/OBS rect so instance hulls stay inside the pin
/// neighborhoods — a die-spanning hull would mask every met1 node
/// and edge as subcell-internal geometry.
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
  SIZE 0.4 BY 0.4 ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.0 0.0 0.2 0.2 ;
    END
  END A
END CELL
MACRO CELLB
  CLASS CORE ;
  SIZE 0.4 BY 0.4 ;
  PIN B
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met2 ;
        RECT 0.0 0.0 0.2 0.2 ;
    END
  END B
END CELLB
MACRO CELLC
  CLASS CORE ;
  SIZE 0.4 BY 0.4 ;
  PIN C
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.0 0.0 0.2 0.2 ;
    END
  END C
END CELLC
MACRO BLK
  CLASS CORE ;
  SIZE 1.2 BY 0.4 ;
  OBS
    LAYER met1 ;
      RECT 0.0 0.0 1.0 0.2 ;
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

/// The pin-collision fixture's tail: io2's met1 port rect sits
/// mid-row between io1 and u0/A, so n1's direct y=1000 path crosses
/// a foreign `Pin` mark band. io2 is placed at x=2000 — its mark
/// interval [1900,2100] then sits on u0/A's very first attach arc
/// (1000,1000)→(2000,1000), which is what lets the hard-mode
/// postmortem attribute `(met1, Pin)` in the named failure (an
/// x=2500 port leaves every attach-node arc free and `blockers`
/// comes out empty).
const MAZE_PIN_TAIL: &str = r#"PINS 2 ;
- io1 + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 1000 ) N ;
- io2 + NET n2 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 2000 1000 ) N ;
END PINS
NETS 2 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
- n2 ( PIN io2 ) ( u1 B ) + USE SIGNAL ;
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
    // The placements put each macro-local rect at the absolute
    // geometry the case asserts (pin crossings, the OBS band) while
    // the tight outlines keep every instance hull inside its own pin
    // neighborhood — the hull mask (`HullMask`) treats a hull
    // covering the routing area as subcell-internal geometry.
    let comps = if with_blk {
        "COMPONENTS 4 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\n- u2 CELLC + PLACED ( 1400 1400 ) N ;\n- blk BLK + PLACED ( 1500 900 ) N ;\nEND COMPONENTS\n"
    } else {
        "COMPONENTS 3 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\n- u2 CELLC + PLACED ( 1400 1400 ) N ;\nEND COMPONENTS\n"
    };
    maze_db_custom(tech, comps, MAZE_DEF_TAIL)
}

fn maze_db_custom(tech: &TechDb, comps: &str, tail: &str) -> DesignDb {
    let text = format!("{MAZE_DEF_HEAD}{comps}{tail}");
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

/// The foreign-pin collision contract (`docs/e6-drt.md` §7): a wire
/// crossing a foreign io-port `Pin` mark band is soft-priced and
/// *recorded* in soft iterations (rip-up re-queues the wire side
/// only) and hard-blocked on the last iteration — never committed
/// silently. io2's met1 port sits mid-row between io1 and u0/A.
/// Arm A's open guide must detour; arm B's met1-row-only guide has
/// no legal path and must fail named with `(met1, Pin)` blockers.
/// Proven red on the unfixed verdict: arm B committed the overlap
/// (`pin_collisions = 1`) and routed instead of failing.
#[test]
fn test_maze_pin_collision() {
    let tech = maze_tech();
    let cfg = MazeConfig::default();
    let comps = "COMPONENTS 2 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\nEND COMPONENTS\n";
    let db = maze_db_custom(&tech, comps, MAZE_PIN_TAIL);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    // io2's met1 port rect [1900,2100]×[900,1100] inflated by
    // w/2 + s = 210 — the foreign band no committed n1 wire may enter.
    let band = grus_core::tech_db::RectDbu::new(1690, 690, 2310, 1310);
    let n2_guide = "n2\n(\n0 0 5000 5000 met1\n0 0 5000 5000 met2\n)\n";

    // Arm A — open guide: the router must find a clean detour (an
    // off-axis jog to y=0/2000 or a met2 leg), never the silent
    // overlap.
    let join = maze_guide(
        &db,
        &tech,
        &format!("n1\n(\n0 0 5000 2000 met1\n0 0 5000 5000 met2\n)\n{n2_guide}"),
    );
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 2);
    let n1 = r.nets.iter().find(|n| net_name(&db, n.net) == "n1").unwrap();
    assert_eq!(n1.connected.len(), 2);
    assert!(
        r.pin_collisions.is_empty(),
        "committed pin overlap must not survive: {:?}",
        r.pin_collisions
    );
    for w in &n1.wires {
        let hit = w.x1 < band.xh && w.x2 > band.xl && w.y1 < band.yh && w.y2 > band.yl;
        assert!(!hit, "wire {w:?} enters the foreign port spacing band");
    }

    // Arm B — the only wave path crosses the port band: soft
    // iterations commit and record the overlap (re-queuing n1); the
    // hard iteration blocks it and the net must fail named — a
    // `NoPath` unrouted entry attributing `(met1, Pin)`, never a
    // silent overlap and never a hang (the cap bounds the loop).
    let join = maze_guide(
        &db,
        &tech,
        &format!("n1\n(\n0 800 5000 1200 met1\n)\n{n2_guide}"),
    );
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let r = route_nets(&db, &tech, &tracks, &graphs, &vias, &cfg);
    assert_eq!(r.unrouted.len(), 1, "{:?}", r.unrouted);
    let u = &r.unrouted[0];
    assert_eq!(net_name(&db, u.net), "n1");
    assert!(
        u.failed.iter().all(|&(_, k)| k == FailKind::NoPath),
        "{:?}",
        u.failed
    );
    assert!(
        u.blockers.contains(&(0, 3)),
        "expected (met1, Pin) blocker attribution, got {:?}",
        u.blockers
    );
    assert!(
        r.iter_stats[..r.iter_stats.len() - 1]
            .iter()
            .any(|s| s.prov_collisions > 0),
        "the wire×pin record must fire before the hard iteration: {:?}",
        r.iter_stats
    );
    assert!(
        r.pin_collisions.is_empty(),
        "the overlap must not survive to emit: {:?}",
        r.pin_collisions
    );
    assert!(
        r.nets.iter().any(|n| net_name(&db, n.net) == "n2"),
        "n2 must still route"
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
    maze_census("gcd");
}

/// Ignored (runtime): the picorv32 route is real work for a sequential
/// single-threaded router — a release run measures ~135 min wall
/// (recorded in docs/results.md), well over the test-suite budget. The
/// census runs identically under `--ignored`; the measured counts are
/// pinned there. Run: `cargo test --release -p grus-core --test drt
/// test_maze_census_picorv32 -- --ignored --nocapture`.
#[test]
#[ignore]
fn test_maze_census_picorv32() {
    maze_census("picorv32");
}

fn maze_census(name: &str) {
    {
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
        println!(
            "{name}: iter_progress={} iter_prov={}",
            r.iter_stats
                .iter()
                .map(|s| s.attempted.to_string())
                .collect::<Vec<_>>()
                .join(","),
            r.iter_stats
                .iter()
                .map(|s| s.prov_collisions.to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        println!(
            "{name}: iter_progress_monotonic={}",
            r.iter_stats
                .windows(2)
                .all(|w| w[0].attempted >= w[1].attempted)
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

        // Convergence record (`IterStat` semantics, `docs/e6-drt.md`
        // §7): one entry per iteration; entry 0 attempts every guided
        // net; `bad[i+1] = failed ∪ both parties of every provisional
        // collision`, and a collision's foreign party may be a kept
        // net outside `bad[i]`, so `attempted` can rise — observed
        // non-increasing on the recorded designs, reported above, not
        // asserted. The last entry always has `prov_collisions == 0`
        // (hard mode reports no provisionals; early convergence has
        // none) and its failure count is the unrouted census.
        assert_eq!(
            r.iter_stats.len(),
            r.iterations as usize,
            "{name}: iter_stats len != iterations"
        );
        if !r.iter_stats.is_empty() {
            assert_eq!(
                r.iter_stats[0].attempted as usize,
                join.nets.len(),
                "{name}: iter_stats[0].attempted != guided"
            );
            let last = r.iter_stats.last().unwrap();
            assert_eq!(
                last.prov_collisions, 0,
                "{name}: last-iteration prov_collisions != 0"
            );
            assert_eq!(
                (last.attempted - last.routed) as usize,
                r.unrouted.len(),
                "{name}: last-iteration failures != unrouted"
            );
        }
        for s in &r.iter_stats {
            assert!(s.routed <= s.attempted, "{name}: routed > attempted");
        }

        // Classify `NoPath` nets: `met3_bridge` has a disconnected
        // {li1,met1,met2} box union that only the met3+ boxes rejoin
        // (the recorded met2–met3 crossing gap); `met2_connected`
        // already has a connected ≤met2 union — the failure is inside
        // the connected region (pin-pocket walls, OBS seals, or
        // occupancy in the final hard-mode pass, `docs/e6-drt.md` §7).
        // Anything else means a fully-disjoint guide set — not
        // possible on real GRT output — so it must never appear.
        let mut unclassified = Vec::new();
        let (mut n_bridge, mut n_met2) = (0usize, 0usize);
        for u in &r.unrouted {
            let (_, boxes) = join
                .nets
                .iter()
                .find(|(n, _)| *n == u.net)
                .expect("unrouted net must be guided");
            let bridged =
                !guide_union_connected(boxes, 2) && guide_union_connected(boxes, u8::MAX);
            let pocket = guide_union_connected(boxes, 2);
            if !bridged && pocket {
                n_met2 += 1;
            }
            n_bridge += bridged as usize;
            for (_, k) in &u.failed {
                if *k == FailKind::NoPath && !bridged && !pocket {
                    unclassified.push(net_name(&db, u.net));
                }
            }
            println!("  {} met3_bridge={bridged} met2_connected={pocket}", net_name(&db, u.net));
        }
        if !r.unrouted.is_empty() {
            println!(
                "{name}: unrouted class counts met3_bridge={n_bridge} met2_connected={n_met2}"
            );
        }
        assert!(
            unclassified.is_empty(),
            "{name}: NoPath nets with fully-disjoint guides: {unclassified:?}"
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
    maze_determinism("gcd");
}

/// Ignored (runtime): two full picorv32 routes, same bound as the
/// census — see `test_maze_census_picorv32` and docs/results.md.
#[test]
#[ignore]
fn test_maze_determinism_picorv32() {
    maze_determinism("picorv32");
}

fn maze_determinism(name: &str) {
    {
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

/// Digest coverage fail-proof (AGENTS §7.7): `iter_stats` is a
/// deterministic output serialized into `route_digest` — flip one
/// field and the digest must move, else the new record is invisible
/// to the determinism gate.
#[test]
fn test_route_digest_covers_iter_stats() {
    let a = RouteDb {
        iter_stats: vec![IterStat {
            attempted: 3,
            routed: 3,
            prov_collisions: 0,
        }],
        ..RouteDb::default()
    };
    let mut b = a.clone();
    b.iter_stats[0].prov_collisions = 1;
    assert_ne!(route_digest(&a), route_digest(&b));
}

#[test]
fn test_route_digest_covers_pin_collisions() {
    let a = RouteDb::default();
    let mut b = a.clone();
    b.pin_collisions.push(grus_core::drt::maze::Collision {
        a: NetId(1),
        b: NetId(2),
        layer: LayerId(0),
        line: 1000,
        lo: 1900,
        hi: 2100,
    });
    assert_ne!(route_digest(&a), route_digest(&b));
}

// ---------------------------------------------------------------------------
// Routed DEF emission + self-DRC gate
// ---------------------------------------------------------------------------

use grus_core::drc::geom::assemble;
use grus_core::drc::report::{Family, SubsetReport, run_all};
use grus_core::drt::emit::{EmitReport, apply_routes, write_routed_def};
use grus_core::drt::maze::RouteDb;

/// Route `name` and apply the result into `db.net_geoms` — the shared
/// emit leg (gcd routes in seconds; picorv32 legs live under
/// `#[ignore]`).
fn emit_leg(
    name: &str,
    cfg: &MazeConfig,
) -> (TechDb, DesignDb, RouteDb, ViaTable, EmitReport) {
    let (tech, mut db, tracks, _join, graphs, vias) = load_routed_inputs(name);
    let route_db = route_nets(&db, &tech, &tracks, &graphs, &vias, cfg);
    let report =
        apply_routes(&mut db, &tech, &graphs, &route_db, &vias).expect("apply_routes");
    (tech, db, route_db, vias, report)
}

fn tmp_def(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("grus-drt-emit-{}-{tag}.def", std::process::id()))
}

fn placed_def(name: &str) -> PathBuf {
    require(design_dir(name).join(format!("{name}.placed.def")))
}

/// The gate sequence: write → re-parse → assemble → `run_all`. The
/// re-parse (`db2`, not `db`) is deliberate — it forces the emitted
/// text through `parse_net_wire`/`resolve_def_via`, so broken grammar
/// or dropped conns surface as parse errors, not wrong geometry.
fn self_drc(out: &Path, tech: &TechDb) -> (DesignDb, SubsetReport) {
    let db2 = parse_def(out, tech, None).expect("emitted DEF re-parses");
    let drc = assemble(&db2, tech).expect("assemble");
    let report = run_all(&drc, tech, &db2.strings);
    (db2, report)
}

fn findings_total(report: &SubsetReport) -> usize {
    report.rules.iter().map(|o| o.findings.len()).sum()
}

fn print_gate(name: &str, report: &SubsetReport) {
    println!("{name} self_drc findings_total={}", findings_total(report));
    for o in &report.rules {
        if o.findings.is_empty() {
            continue;
        }
        println!(
            "  rule={} layer={} candidates={} findings={}",
            o.rule,
            o.layer.as_deref().unwrap_or("-"),
            o.candidates,
            o.findings.len()
        );
        for f in &o.findings {
            println!("    {f:?}");
        }
    }
}

/// Wire/via totals across all nets — the gate's non-vacuity evidence.
fn geom_totals(db: &DesignDb) -> (usize, usize) {
    (
        db.net_geoms.iter().map(|g| g.wire_rects.len()).sum(),
        db.net_geoms.iter().map(|g| g.vias.len()).sum(),
    )
}

/// The structural census vs `<d>.drt.reference.json` — informational
/// signed deltas against the delegated (same-guide) variant, baseline
/// as context. Asserts stay on the non-vacuity properties: emitted
/// cut names are real tech cut layers, totals are nonzero, and both
/// reference variants carry a mappable via census.
fn print_emit_census(name: &str, tech: &TechDb, db: &DesignDb, report: &EmitReport) {
    let cut_names: std::collections::BTreeSet<String> = tech
        .cut_layers
        .iter()
        .map(|c| tech.strings.resolve(c.name).to_string())
        .collect();
    let refj = load_drt_ref(name);
    let by_cut = |variant: &str| -> BTreeMap<String, u64> {
        let v = &refj["variants"][variant];
        let census = v["vias_per_cut_layer"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: variants.{variant}.vias_per_cut_layer missing"));
        let meaning = v["via_names_meaning"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: variants.{variant}.via_names_meaning missing"));
        let mut out = BTreeMap::new();
        for (via_name, count) in census {
            let cut = meaning
                .get(via_name)
                .and_then(|m| m.as_str())
                .unwrap_or("<unknown>");
            *out.entry(cut.to_string()).or_default() += count.as_u64().unwrap();
        }
        out
    };
    let delegated = by_cut("delegated");
    let baseline = by_cut("baseline");

    let um = report.wire_length_dbu as f64 / db.dbu_per_micron.max(1) as f64;
    let wl = |v: &serde_json::Value| {
        v["total_wirelength_um"]
            .as_f64()
            .unwrap_or_else(|| panic!("{name}: total_wirelength_um missing"))
    };
    let del_wl = wl(&refj["variants"]["delegated"]);
    let base_wl = wl(&refj["variants"]["baseline"]);
    println!("== {name} emit census ==");
    println!(
        "wirelength_um ours={um:.3} delegated={del_wl:.3} delta={:+.3} baseline={base_wl:.3}",
        um - del_wl
    );
    let mut cuts: std::collections::BTreeSet<String> =
        report.vias_per_cut.keys().cloned().collect();
    cuts.extend(delegated.keys().cloned());
    cuts.extend(baseline.keys().cloned());
    for cut in &cuts {
        let ours = report.vias_per_cut.get(cut).copied().unwrap_or(0) as i64;
        let del = delegated.get(cut).copied();
        let base = baseline.get(cut).copied();
        println!(
            "via.{cut:<6} ours={ours} delegated={} delta={} baseline={}",
            del.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            del.map(|v| format!("{:+}", ours - v as i64))
                .unwrap_or_else(|| "-".into()),
            base.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
        );
    }
    assert!(!report.vias_per_cut.is_empty(), "{name}: no vias emitted");
    for cut in report.vias_per_cut.keys() {
        assert!(
            cut_names.contains(cut),
            "{name}: emitted cut name {cut} is not a tech cut layer"
        );
    }
    assert!(
        cuts.iter().filter(|c| cut_names.contains(*c)).count() >= 3,
        "{name}: reference census maps to <3 real cut layers: {cuts:?}"
    );
}

/// Route → emit → write → re-parse → `run_all` on `name` — the shared
/// body of the gcd/picorv32 gate tests. Returns the `RouteDb` so a
/// caller can assert on the min-area patch census.
fn emit_gate(name: &str) -> RouteDb {
    let (tech, db, route_db, _vias, report) = emit_leg(name, &MazeConfig::default());
    let (rects, vias) = geom_totals(&db);
    println!(
        "{name}: routed={} unrouted={} wire_rects={rects} vias={vias} wirelength_dbu={} area_patches={} area_residuals={}",
        report.routed,
        report.unrouted.len(),
        report.wire_length_dbu,
        route_db.area_patches,
        route_db.area_residuals.len(),
    );
    for n in &report.unrouted {
        println!("  unrouted {n}");
    }
    for r in &route_db.area_residuals {
        println!(
            "  area_residual {} {} area={} bound={} region={:?}",
            net_name(&db, r.net),
            tech.strings
                .resolve(tech.routing_layers[r.layer.0 as usize].name),
            r.area_dbu2,
            r.bound_dbu2,
            r.region,
        );
    }
    assert!(rects > 0 && vias > 0, "{name}: empty emission — vacuous gate");
    print_emit_census(name, &tech, &db, &report);

    let out = tmp_def(&format!("{name}-gate"));
    write_routed_def(&db, &tech, &placed_def(name), &out).expect("write routed DEF");
    let (db2, drc_report) = self_drc(&out, &tech);
    print_gate(name, &drc_report);

    // Non-vacuity: every checked family actually saw candidates — a
    // gate that checks nothing proves nothing.
    for family in [
        Family::Width,
        Family::Spacing,
        Family::Enclosure,
        Family::Area,
    ] {
        let candidates: usize = drc_report
            .rules
            .iter()
            .filter(|o| o.family == family)
            .map(|o| o.candidates)
            .sum();
        assert!(candidates > 0, "{name}: {family:?} family checked nothing");
    }
    // Determinism: a second `run_all` over the same assembled design
    // repeats the report verbatim.
    let drc2 = assemble(&db2, &tech).expect("assemble");
    assert_eq!(drc_report, run_all(&drc2, &tech, &db2.strings));

    // Byte stability on the design under test itself: the re-parsed
    // geometry re-emits to the same bytes (`out` is its own template,
    // so this exercises the NETS-splice fixed point).
    let out2 = tmp_def(&format!("{name}-gate2"));
    write_routed_def(&db2, &tech, &out, &out2).expect("re-emit");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&out2).unwrap(),
        "{name}: write → parse → write is not byte-stable"
    );

    let firing: Vec<String> = drc_report
        .rules
        .iter()
        .filter(|o| !o.findings.is_empty())
        .map(|o| format!("{}:{}", o.rule, o.findings.len()))
        .collect();
    assert!(
        firing.is_empty(),
        "{name}: self-DRC findings on our emitted DEF: {firing:?}"
    );
    route_db
}

/// The byte-stable roundtrip — the acceptance's write → parse → write
/// → identical. `out2` uses `out1` as its own template, exercising the
/// fixed point rather than only the placed template.
#[test]
fn test_emit_roundtrip_gcd() {
    let (tech, db, route_db, _vias, report) = emit_leg("gcd", &MazeConfig::default());
    assert_eq!(report.routed, route_db.nets.len());
    // The unrouted set is pinned by the e2e golden — a drift names the
    // differing nets in the failure rather than re-baselining a count.
    let golden_unrouted: BTreeSet<String> =
        j_arr(&load_ours_ref("gcd")["ours_router"]["unrouted"], "ours_router.unrouted")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    assert_eq!(
        BTreeSet::from_iter(report.unrouted.iter().cloned()),
        golden_unrouted,
        "gcd: unrouted set diverges from the committed golden"
    );
    assert!(report.wire_length_dbu > 0);
    let (rects, vias) = geom_totals(&db);
    assert!(rects > 0 && vias > 0);

    let out1 = tmp_def("gcd-rt1");
    let out2 = tmp_def("gcd-rt2");
    write_routed_def(&db, &tech, &placed_def("gcd"), &out1).expect("write out1");
    let db2 = parse_def(&out1, &tech, None).expect("re-parse out1");
    assert_eq!(
        db2.net_geoms, db.net_geoms,
        "re-parsed net_geoms differ from applied geometry"
    );
    write_routed_def(&db2, &tech, &out1, &out2).expect("write out2");
    assert_eq!(
        std::fs::read(&out1).unwrap(),
        std::fs::read(&out2).unwrap(),
        "write → parse → write is not byte-stable"
    );
}

/// The acceptance gate: `run_all` reports zero findings across all 50
/// subset rules on our own emitted DEF — the same bar `tests/drc.rs`
/// puts on TritonRoute's. This is our 50-rule lef-expressible subset
/// on our DEF; the full Magic deck check belongs to `e6-drt-e2e`.
#[test]
fn test_emit_selfdrc_gcd() {
    let route_db = emit_gate("gcd");
    // gcd's gate was 0 before the patch — a residual appearing here
    // is a patch regression to investigate, not a count to re-pin.
    assert!(
        route_db.area_residuals.is_empty(),
        "gcd: min-area residuals on a clean gate: {:?}",
        route_db.area_residuals
    );
    assert!(
        route_db.notch_residuals.is_empty() && route_db.hole_residuals.is_empty(),
        "gcd: closure residuals on a clean gate: {:?} {:?}",
        route_db.notch_residuals,
        route_db.hole_residuals
    );
}

/// Ignored for wall clock (`route_nets` ≈ 135 min release) — run
/// detached with `--ignored --nocapture`; the unrouted names print
/// per net (picorv32's met3-bridge `NoPath` class).
#[test]
#[ignore]
fn test_emit_selfdrc_picorv32() {
    emit_gate("picorv32");
}

/// The fail-proof (AGENTS §7.7): the gate must go red on deliberately
/// broken input. Arm A stretches one `wire_rect` onto a foreign
/// neighbour — emit→parse→`run_all` must report the spacing violation
/// the emitted text describes. Arm B routes with `OccMode::Disabled`
/// (`test_maze_occupancy` proves thousands of track collisions) — the
/// overlapping geometry must reach the checker through our emitted
/// text.
#[test]
fn test_emit_fail_proof() {
    // Arm A — injected geometry. The subset checker models spacing as
    // a positive gap in (0, bound): `axis_gap` clamps overlaps to
    // (0,0), which reads as edge-abutting `touching_ok` — a same-layer
    // short is connectivity-check territory, not this subset's. The
    // injected defect is therefore a positive sub-bound gap, the
    // class the spacing rules measure.
    let (tech, mut db, _route_db, _vias, _rep) = emit_leg("gcd", &MazeConfig::default());
    let met1 = tech.layer_by_name[&tech.strings.get("met1").unwrap()];
    let bound = tech.routing_layers[met1.0 as usize].min_spacing_dbu();
    let mut hit = None;
    'scan: for (ai, ga) in db.net_geoms.iter().enumerate() {
        for (aj, &(lid, ra)) in ga.wire_rects.iter().enumerate() {
            if lid != met1 {
                continue;
            }
            for gb in db.net_geoms.iter().skip(ai + 1) {
                for &(blid, rb) in &gb.wire_rects {
                    if blid != lid {
                        continue;
                    }
                    if rb.yl < ra.yh && ra.yl < rb.yh && rb.xl - ra.xh >= bound {
                        hit = Some((ai, aj, rb.xl));
                        break 'scan;
                    }
                }
            }
        }
    }
    let (ai, aj, foreign_xl) =
        hit.expect("no clean foreign-net met1 pair to inject against");
    println!(
        "injected: net {} rect {:?} xh -> {} (gap 1 < bound {bound})",
        net_name(&db, NetId(ai as u32)),
        db.net_geoms[ai].wire_rects[aj],
        foreign_xl - 1
    );
    db.net_geoms[ai].wire_rects[aj].1.xh = foreign_xl - 1;
    let out = tmp_def("gcd-fp-injected");
    write_routed_def(&db, &tech, &placed_def("gcd"), &out).expect("write");
    let (_db2, drc_report) = self_drc(&out, &tech);
    print_gate("gcd(injected)", &drc_report);
    let spacing_hits: Vec<&str> = drc_report
        .rules
        .iter()
        .filter(|o| o.family == Family::Spacing && !o.findings.is_empty())
        .map(|o| o.rule)
        .collect();
    assert!(
        !spacing_hits.is_empty(),
        "injected overlap produced no spacing finding — the gate is vacuous"
    );

    // Arm B — occupancy disabled.
    let (tech, db, route_db, _vias, _rep) = emit_leg(
        "gcd",
        &MazeConfig {
            occ: OccMode::Disabled,
            ..MazeConfig::default()
        },
    );
    assert!(!route_db.nets.is_empty());
    let out = tmp_def("gcd-fp-occ");
    write_routed_def(&db, &tech, &placed_def("gcd"), &out).expect("write");
    let (_db2, drc_report) = self_drc(&out, &tech);
    print_gate("gcd(occ=disabled)", &drc_report);
    assert!(
        findings_total(&drc_report) >= 1,
        "occupancy-disabled emit must produce DRC findings — the gate is vacuous"
    );
}

/// Determinism with nonzero evidence (§7.7): two full route+emit
/// passes produce identical `RouteDb`s, identical `net_geoms`, and
/// identical emitted bytes.
#[test]
fn test_emit_determinism() {
    let cfg = MazeConfig::default();
    let (tech, db1, r1, _v1, rep1) = emit_leg("gcd", &cfg);
    let (_t, db2, r2, _v2, rep2) = emit_leg("gcd", &cfg);
    assert!(!r1.nets.is_empty());
    assert_eq!(r1, r2, "nondeterministic routes");
    assert_eq!(rep1.wire_length_dbu, rep2.wire_length_dbu);
    assert_eq!(rep1.vias_per_cut, rep2.vias_per_cut);
    assert_eq!(db1.net_geoms, db2.net_geoms);

    let out1 = tmp_def("gcd-det1");
    let out2 = tmp_def("gcd-det2");
    let placed = placed_def("gcd");
    write_routed_def(&db1, &tech, &placed, &out1).expect("write 1");
    write_routed_def(&db2, &tech, &placed, &out2).expect("write 2");
    assert_eq!(
        std::fs::read(&out1).unwrap(),
        std::fs::read(&out2).unwrap(),
        "two emit passes produced different bytes"
    );
}

/// A forced-failure net emits conns-only — the picorv32 shape without
/// picorv32's cost: n1 routes straight; n3 joins but `u2/C`'s rect
/// holds no track (`Unattachable`) → its record is `( conns ) +
/// USE SIGNAL ;` with no geometry statements.
#[test]
fn test_emit_unrouted_fixture() {
    let tech = maze_tech();
    let comps = "COMPONENTS 3 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\n- u2 CELLC + PLACED ( 1400 1400 ) N ;\nEND COMPONENTS\n";
    let def_text = format!("{MAZE_DEF_HEAD}{comps}{MAZE_DEF_TAIL}");
    let mut db = parse_def_bytes(def_text.as_bytes(), &tech, None).expect("fixture DEF parses");
    let tracks = layer_tracks(&db, &tech);
    let join = maze_guide(
        &db,
        &tech,
        "n1\n(\n0 0 5000 2000 met1\n)\nn3\n(\n0 0 5000 5000 met1\n)\n",
    );
    let graphs = build_route_graphs(&db, &tech, &tracks, &join);
    let vias = via_table(&tech, &db).unwrap();
    let route_db = route_nets(&db, &tech, &tracks, &graphs, &vias, &MazeConfig::default());
    assert_eq!(route_db.unrouted.len(), 1, "{:?}", route_db.unrouted);
    let report = apply_routes(&mut db, &tech, &graphs, &route_db, &vias).expect("apply");
    assert_eq!(report.unrouted, vec!["n3".to_string()]);

    // The fixture's own text is the splice template.
    let template = tmp_def("fixture-template");
    std::fs::write(&template, &def_text).unwrap();
    let out = tmp_def("fixture-unrouted");
    write_routed_def(&db, &tech, &template, &out).expect("write");
    let text = std::fs::read_to_string(&out).unwrap();

    let n3_start = text.find(" - n3 ").expect("n3 record missing");
    let n3_end = n3_start + text[n3_start..].find("+ USE SIGNAL ;").unwrap();
    let n3_rec = &text[n3_start..n3_end];
    assert!(n3_rec.contains("( PIN io3 )") && n3_rec.contains("( u2 C )"), "{n3_rec}");
    assert!(
        !n3_rec.contains("ROUTED") && !n3_rec.contains("NEW"),
        "unrouted net emitted geometry: {n3_rec}"
    );
    parse_def(&out, &tech, None).expect("fixture emit re-parses");
}

// ---------------------------------------------------------------------------
// e6-drt-e2e gate — the routed-DEF golden (`<d>.drt.ours.reference.json`)
// ---------------------------------------------------------------------------

/// Measured Magic findings on our DEF that the ticket pinned rather
/// than closed (`kurena/tasks/e6-drt-e2e/design/gate.md` §0), as
/// `(design, rule, count)` triples — a pin scopes to one design's
/// golden, so a picorv32-only residual never asserts on gcd's zero.
/// A pinned rule asserts `ours == pin && placed == 0` — a residual is
/// ours-only, so the placed baseline firing the same rule means the
/// pin is wrong, not inherited; unpinned rules assert strict
/// `ours == placed`. `(design, rule_id, ours)` — mechanism-named
/// ours-only `lef_expressible` residuals; empty for gcd since the r5
/// met1.2 attribution closed all 7 sites (`docs/results.md`).
/// picorv32 (v0.12.9 emit, DEF sha `505baea2f937702f`, docs/results.md
/// 2026-11-25): `li.3` = our li1 pin-access paint vs LEF-declared OBS
/// li1 inside host hulls — the masked-layer class (4 sites); `met1.2`
/// = same-net disjoint met1 edges of one net's pin-access cluster
/// < 0.14um (21 sites, all X3 — no cell-internal metal1 at any site).
const PINNED_LEF_RESIDUALS: &[(&str, &str, i64)] = &[
    ("picorv32", "li.3", 19),
    ("picorv32", "met1.2", 146),
];

/// Magic's two built-in hierarchy-boundary messages — no tech-file
/// rule id, so the deck's `classification` never covers them. Mirrors
/// `check_drt_ours.sh`'s `ALLOWED_UNCLASSIFIED` (the script itself
/// aborts on any *new* unclassified name before writing a golden).
const ALLOWED_UNCLASSIFIED: &[&str] = &[
    "Can't overlap those layers",
    "This layer can't abut or partially overlap between subcells",
];

/// Reads the committed `<d>.drt.ours.reference.json` — the
/// `check_drt_ours.sh` e2e golden. `require`'d: a missing file is a
/// loud failure, never a skip.
fn load_ours_ref(name: &str) -> serde_json::Value {
    let path = require(design_dir(name).join(format!("{name}.drt.ours.reference.json")));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

/// Reads the committed `<d>.drc.reference.json` — the Magic
/// `drc(full)` golden carrying the `classification` taxonomy this
/// gate joins `lef_expressible` keys against.
fn load_drc_ref(name: &str) -> serde_json::Value {
    let path = require(design_dir(name).join(format!("{name}.drc.reference.json")));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

/// The `(…)` rule-id tail of a rendered Magic `drc why` name — the
/// same extraction `check_drt_ours.sh`'s `rule_id` applies
/// (`\(([^()]*)\)\s*$`).
fn magic_rule_id(rendered: &str) -> Option<String> {
    let inner = rendered.trim_end().strip_suffix(')')?;
    let open = inner.rfind('(')?;
    Some(inner[open + 1..].trim().to_string())
}

fn j_i64(v: &serde_json::Value, ctx: &str) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("{ctx}: expected integer, got {v}"))
}

fn j_u64(v: &serde_json::Value, ctx: &str) -> u64 {
    v.as_u64()
        .unwrap_or_else(|| panic!("{ctx}: expected unsigned integer, got {v}"))
}

fn j_obj<'a>(v: &'a serde_json::Value, ctx: &str) -> &'a serde_json::Map<String, serde_json::Value> {
    v.as_object()
        .unwrap_or_else(|| panic!("{ctx}: expected object, got {v}"))
}

fn j_arr<'a>(v: &'a serde_json::Value, ctx: &str) -> &'a [serde_json::Value] {
    v.as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("{ctx}: expected array, got {v}"))
}

/// The `gate.md` §1 joins shared by the gcd test and the deferred
/// picorv32 leg.
fn ours_reference_gate(name: &str) {
    let golden = load_ours_ref(name);
    let drc_ref = load_drc_ref(name);
    assert_eq!(
        golden["design"].as_str(),
        Some(name),
        "{name}: golden design field mismatch"
    );

    // -- lef_expressible: completeness + the §0 pinned join ----------
    let classification = j_obj(
        &drc_ref["magic_rules_checked"]["classification"],
        "magic_rules_checked.classification",
    );
    let mut expected: BTreeSet<String> = BTreeSet::new();
    for (tmpl, class) in classification {
        if class.as_str() != Some("lef_expressible") {
            continue;
        }
        let rid = magic_rule_id(tmpl).unwrap_or_else(|| {
            panic!("{name}: lef_expressible template has no rule id: {tmpl}")
        });
        assert!(
            expected.insert(rid),
            "{name}: duplicate lef_expressible rule id in {tmpl}"
        );
    }
    let lef = j_obj(&golden["magic"]["lef_expressible"], "magic.lef_expressible");
    let got: BTreeSet<String> = lef.keys().cloned().collect();
    assert_eq!(
        got, expected,
        "{name}: magic.lef_expressible keys must be the 50-name taxonomy subset"
    );
    println!("== {name} lef_expressible census ==");
    for (rid, entry) in lef {
        let ours_n = j_i64(&entry["ours"], &format!("{rid}.ours"));
        let placed_n = j_i64(&entry["placed"], &format!("{rid}.placed"));
        let ref_n = j_i64(&entry["routed_ref"], &format!("{rid}.routed_ref"));
        match PINNED_LEF_RESIDUALS
            .iter()
            .find(|(d, r, _)| *d == name && *r == rid.as_str())
        {
            Some(&(_, _, pin)) => {
                assert_eq!(
                    ours_n, pin,
                    "{name}: pinned residual {rid} moved (ours={ours_n}, pinned={pin}) — re-pin or close it"
                );
                assert_eq!(
                    placed_n, 0,
                    "{name}: {rid} fires on the placed baseline — the pin is wrong, not inherited"
                );
            }
            None => assert_eq!(
                ours_n, placed_n,
                "{name}: lef_expressible {rid}: ours={ours_n} placed={placed_n}"
            ),
        }
        assert_eq!(
            entry["inherited"].as_bool(),
            Some(ours_n == placed_n),
            "{name}: {rid}.inherited inconsistent with its counts"
        );
        println!("  lef {rid:<22} ours={ours_n} placed={placed_n} routed_ref={ref_n}");
    }

    // -- magic_only: every row inherited (ours == placed) ------------
    for row in j_arr(&golden["magic"]["magic_only"], "magic.magic_only") {
        assert_eq!(
            row["inherited"].as_bool(),
            Some(true),
            "{name}: magic_only row diverges from the placed baseline: {row}"
        );
    }

    // -- unclassified_fired: every name inside the pinned allowlist --
    for row in j_arr(
        &golden["magic"]["unclassified_fired"],
        "magic.unclassified_fired",
    ) {
        let n = row["name"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: unclassified_fired row without name: {row}"));
        assert!(
            ALLOWED_UNCLASSIFIED.contains(&n),
            "{name}: unclassified Magic name outside the pinned allowlist: {n}"
        );
    }

    // -- sanity_control: the mutation arm stays live ------------------
    let sanity = &golden["sanity_control"];
    assert_eq!(
        sanity["violated_rules_in_checked_deck"].as_bool(),
        Some(true),
        "{name}: sanity mutation violated a rule absent from the checked deck"
    );
    assert!(
        sanity["mutation"].is_object(),
        "{name}: sanity_control.mutation missing"
    );
    let pinned_ids: BTreeSet<&str> = PINNED_LEF_RESIDUALS
        .iter()
        .filter(|(d, _, _)| *d == name)
        .map(|(_, r, _)| *r)
        .collect();
    let new_unpinned: Vec<&str> = j_arr(
        &sanity["new_rules_vs_clean"],
        "sanity_control.new_rules_vs_clean",
    )
    .iter()
    .filter_map(|v| v.as_str())
    .filter(|n| magic_rule_id(n).is_none_or(|id| !pinned_ids.contains(id.as_str())))
    .collect();
    assert!(
        !new_unpinned.is_empty(),
        "{name}: sanity mutation fired only pinned rules — the control is vacuous"
    );

    // -- ours_router census -------------------------------------------
    let ours = &golden["ours_router"];
    assert!(
        j_u64(&ours["hull_masked"], "ours_router.hull_masked") > 0,
        "{name}: hull mask never fired — vacuous gate"
    );
    let notch_fills = j_u64(&ours["notch_fills"], "ours_router.notch_fills");
    let cli: Vec<&str> = j_arr(&ours["cli_lines"], "ours_router.cli_lines")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let count_lines = |prefix: &str| {
        cli.iter()
            .filter(|l| l.trim_start().starts_with(prefix))
            .count()
    };
    assert_eq!(
        j_u64(&ours["notch_residuals"], "ours_router.notch_residuals") as usize,
        count_lines("notch_residual "),
        "{name}: notch_residuals census must equal its evidence lines"
    );
    assert_eq!(
        j_u64(&ours["hole_residuals"], "ours_router.hole_residuals") as usize,
        count_lines("hole_residual "),
        "{name}: hole_residuals census must equal its evidence lines"
    );
    let cli_unrouted: BTreeSet<&str> = cli
        .iter()
        .filter_map(|l| l.trim().strip_prefix("unrouted "))
        .collect();
    let golden_unrouted: BTreeSet<&str> = j_arr(&ours["unrouted"], "ours_router.unrouted")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let unrouted_count =
        j_u64(&ours["unrouted_count"], "ours_router.unrouted_count") as usize;
    assert_eq!(unrouted_count, golden_unrouted.len());
    assert_eq!(
        cli_unrouted, golden_unrouted,
        "{name}: ours_router.unrouted diverges from its cli_lines evidence"
    );
    // `unrouted_kinds` counts per-pin failures (`UnroutedNet.failed`):
    // every unrouted net names ≥1 failed pin.
    let kinds_sum: u64 = j_obj(&ours["unrouted_kinds"], "ours_router.unrouted_kinds")
        .values()
        .filter_map(|v| v.as_u64())
        .sum();
    if unrouted_count == 0 {
        assert_eq!(kinds_sum, 0, "{name}: unrouted_kinds on a clean run");
    } else {
        assert!(
            kinds_sum >= unrouted_count as u64,
            "{name}: unrouted_kinds sum {kinds_sum} < unrouted_count {unrouted_count}"
        );
    }
    for (k, v) in j_obj(&ours["unrouted_blockers"], "ours_router.unrouted_blockers") {
        assert!(
            v.as_u64().unwrap_or(0) > 0,
            "{name}: unrouted_blockers.{k} is not a positive count"
        );
    }
    // `stub_outside_cover` records every pin that ever had no legal
    // paint — including pins of nets that re-attached on a later
    // iteration (`_165_` on gcd) — so the join is against the
    // `Unattachable` census, not `unrouted`: every final Unattachable
    // pin has a named record.
    let stub_lines = count_lines("stub_outside_cover ");
    let unatt = j_u64(
        &ours["unrouted_kinds"]["Unattachable"],
        "ours_router.unrouted_kinds.Unattachable",
    );
    assert!(
        stub_lines as u64 >= unatt,
        "{name}: {stub_lines} stub_outside_cover lines < {unatt} Unattachable pins"
    );

    // -- openroad block ------------------------------------------------
    assert_eq!(
        golden["openroad"]["drt_report_source"].as_str(),
        Some("not_applicable_native_router"),
        "{name}: openroad.drt_report_source must stay pinned"
    );
    println!(
        "{name}: antenna_violations={} antenna_pin_violations={}",
        golden["openroad"]["antenna_violations"],
        golden["openroad"]["antenna_pin_violations"],
    );

    // -- viability floor -------------------------------------------------
    let dm = &golden["def_metrics"];
    let routed = j_u64(&ours["routed"], "ours_router.routed");
    assert_eq!(
        j_u64(&dm["signal_nets_routed"], "def_metrics.signal_nets_routed"),
        routed,
        "{name}: def_metrics.signal_nets_routed != ours_router.routed"
    );
    let in_def = j_u64(&dm["signal_nets_in_def"], "def_metrics.signal_nets_in_def");
    // `unguided` names nets the guide never covered — single-pin io
    // nets emitting conns-only entries outside the unrouted census
    // (picorv32's 67 irq/pcpi ports; the step-E verifier requires each
    // to be `( PIN <port> )`-only and guide-absent).
    let unguided = j_arr(&ours["unguided"], "ours_router.unguided");
    assert!(
        unguided.iter().all(|v| v.is_string()),
        "{name}: ours_router.unguided must be a string list"
    );
    let unguided_set: BTreeSet<&str> = unguided.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        unguided_set.is_disjoint(&golden_unrouted),
        "{name}: ours_router.unguided overlaps the unrouted census"
    );
    assert_eq!(
        in_def - routed,
        unrouted_count as u64 + unguided.len() as u64,
        "{name}: signal_nets_in_def - routed != unrouted + unguided — the mask would starve the design"
    );

    println!(
        "== {name} ours-router census: routed={routed} unrouted={unrouted_count} \
         hull_masked={} notch_fills={notch_fills} stub_lines={stub_lines} ==",
        ours["hull_masked"]
    );
    println!("{}", serde_json::to_string_pretty(&golden["delta"]).unwrap());
}

/// The r5 e2e gate: the committed `gcd.drt.ours.reference.json` joins
/// against the `drc.reference.json` taxonomy — every `lef_expressible`
/// rule either equals the placed baseline or sits named in
/// `PINNED_LEF_RESIDUALS`, and the mutation control stays live.
#[test]
fn test_e2e_ours_reference_gcd() {
    ours_reference_gate("gcd");
}

/// Byte-equality against the committed `gcd.drt.ours.routed.def` —
/// the file IS the emit golden; `generate_drt_ours.sh --verify`
/// demonstrates the same property script-side.
#[test]
fn test_e2e_ours_def_deterministic_gcd() {
    let (tech, db, _route_db, _vias, _rep) = emit_leg("gcd", &MazeConfig::default());
    let out = tmp_def("gcd-ours-det");
    write_routed_def(&db, &tech, &placed_def("gcd"), &out).expect("write routed DEF");
    let committed = require(design_dir("gcd").join("gcd.drt.ours.routed.def"));
    assert_eq!(
        std::fs::read(&committed).unwrap(),
        std::fs::read(&out).unwrap(),
        "gcd: emit diverges from the committed routed DEF — regenerate via \
         tests/designs/generate_drt_ours.sh gcd"
    );
}

/// The picorv32 e2e gate (v0.12.9, DEF sha `505baea2f937702f`):
/// `li.3`=19/`met1.2`=146 pinned ours-only residuals (see
/// `PINNED_LEF_RESIDUALS`), `unrouted`=2016, `unguided`=67 io-pin-only
/// nets. No byte-compare determinism test — the routed DEF is ≥1 MB
/// and stays gitignored; `generate_drt_ours.sh picorv32 --verify`
/// demonstrates the property script-side.
#[test]
fn test_e2e_ours_reference_picorv32() {
    ours_reference_gate("picorv32");
}

/// Every emitted `PdnVia` is the `ViaTable` pair default verbatim —
/// emit reselects nothing (the placed shape must be the via the
/// router priced).
#[test]
fn test_emit_via_default_consistency() {
    let (_tech, db, _route_db, vias, _rep) = emit_leg("gcd", &MazeConfig::default());
    let mut placed = 0usize;
    for geom in &db.net_geoms {
        for via in &geom.vias {
            placed += 1;
            let pair = &vias.pairs[via.lower.0 as usize];
            let di = pair.default.expect("via placed on a gap pair");
            let cand = &vias.candidates[pair.candidates[di as usize] as usize];
            assert_eq!(via.lower, cand.lower);
            assert_eq!(via.upper, cand.upper);
            assert_eq!(via.cut, cand.cut);
            assert_eq!(via.cut_count, cand.cut_count);
            assert_eq!(via.name, cand.name);
            assert_eq!(via.geom, cand.geom);
        }
    }
    assert!(placed > 0, "no vias emitted — vacuous");
}
