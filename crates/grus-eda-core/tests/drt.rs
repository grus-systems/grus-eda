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

use grus_eda_core::congestion::guide::{GuideBox, guide_net_name, parse_guides};
use grus_eda_core::design_db::DesignDb;
use grus_eda_core::drt::access::{AccessReport, access_points, pin_shapes};
use grus_eda_core::drt::graph::{
    EscapeKind, GraphOpts, GuideJoin, GuideUnion, HullMask, build_route_graphs, cell_hull_rect,
    guide_union_connected, join_guides, pin_rects,
};
use grus_eda_core::drt::tracks::{Axis, LayerTracks, layer_tracks, track_gaps};
use grus_eda_core::drt::via::{ViaPair, ViaTable, via_table};
use grus_eda_core::interner::{LayerId, NetId, PinId, StringInterner};
use grus_eda_core::readers::{load_tech_db, parse_def, parse_def_bytes, parse_lef_bytes};
use grus_eda_core::tech_db::{LayerDir, RectDbu, TechDb};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `build_route_graphs`'s options (`spare_above: 0`, pin-halo escape
/// on) — `route_nets` rebuilds per-net graphs internally, so callers
/// spell out the same `GraphOpts` the graph build used.
const GRAPHS_OPTS: GraphOpts = GraphOpts {
    spare_above: 0,
    escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
};
/// `load_routed_inputs`'s options (`spare_above: 1` — the OBS-sealed
/// pin-pocket escape).
const LOADED_OPTS: GraphOpts = GraphOpts {
    spare_above: 1,
    escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
};

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
            .map(|n| db.net_pins(grus_eda_core::interner::NetId(n as u32)).len())
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
            for &p in db.net_pins(grus_eda_core::interner::NetId(n as u32)) {
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
        let grid = |r: &grus_eda_core::tech_db::RectDbu, lt: &LayerTracks| -> usize {
            let nx = lt.tracks_x.partition_point(|&t| t <= r.xh)
                - lt.tracks_x.partition_point(|&t| t < r.xl);
            let ny = lt.tracks_y.partition_point(|&t| t <= r.yh)
                - lt.tracks_y.partition_point(|&t| t < r.yl);
            nx * ny
        };
        let union =
            |a: grus_eda_core::tech_db::RectDbu, b: grus_eda_core::tech_db::RectDbu| {
                grus_eda_core::tech_db::RectDbu::new(
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
                let mut span: Option<grus_eda_core::tech_db::RectDbu> = None;
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
            // Escape pairs: the halo ⊆ the pin shape-bbox's covered
            // range inflated `halo_lines` index steps per axis, and a
            // corridor stays within `corridor_lines` steps of the halo
            // (its far end is a pre-existing pair already bounded
            // above) — so halo+corridor ⊆ the range inflated by both.
            let esc = grus_eda_core::drt::graph::EscapeCfg::DEFAULT;
            let h = (esc.halo_lines + esc.corridor_lines).max(0) as usize;
            for rec in &graphs.escapes {
                if rec.net != *net || rec.pairs_added == 0 {
                    continue;
                }
                let shapes = pin_shapes(&db, &tech, rec.pin);
                let Some(bbox) = shapes.iter().map(|s| s.1).reduce(union) else {
                    continue;
                };
                let lo = shapes.iter().map(|s| s.0.0).min().unwrap_or(0);
                let hi = shapes
                    .iter()
                    .map(|s| s.0.0)
                    .max()
                    .unwrap_or(0)
                    .max(transit_l.unwrap_or(0));
                for l in lo.min(hi)..=hi {
                    let lt = &tracks[l as usize];
                    let xa = lt
                        .tracks_x
                        .partition_point(|&t| t < bbox.xl)
                        .saturating_sub(h);
                    let xb = (lt.tracks_x.partition_point(|&t| t <= bbox.xh) + h)
                        .min(lt.tracks_x.len());
                    let ya = lt
                        .tracks_y
                        .partition_point(|&t| t < bbox.yl)
                        .saturating_sub(h);
                    let yb = (lt.tracks_y.partition_point(|&t| t <= bbox.yh) + h)
                        .min(lt.tracks_y.len());
                    *bounds.entry(l).or_default() += (xb - xa) * (yb - ya);
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
/// `region_escape` — the pre-halo escape set, recorded even when the
/// pin-halo escape later re-attached the pin. `escapes` parallels
/// `region_escape` exactly: refused escapes (`CorridorBound`,
/// `NoAnchor`, `HaloEmpty`) added no pairs and still hold an empty `pin_nodes`
/// list, while a pin with attached nodes must carry a `Halo` or
/// `CorridorMasked` record. Escape names print per design; the gcd
/// set is pinned.
#[test]
fn test_pin_attachment() {
    for name in ["gcd", "picorv32"] {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let graphs = build_route_graphs(&db, &tech, &tracks, &join);
        let escapes: std::collections::BTreeSet<PinId> =
            graphs.region_escape.iter().map(|e| e.pin).collect();
        let records: std::collections::BTreeMap<PinId, &grus_eda_core::drt::graph::EscapeRecord> =
            graphs.escapes.iter().map(|e| (e.pin, e)).collect();
        assert_eq!(
            records.len(),
            escapes.len(),
            "{name}: escapes does not parallel region_escape"
        );
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for e in &graphs.escapes {
            let tok = e.token();
            *kinds
                .entry(tok.split('(').next().unwrap().to_string())
                .or_default() += 1;
        }
        println!("{name}: escape kinds {kinds:?}");

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
                match (escapes.contains(&pin), records.get(&pin)) {
                    (true, Some(rec)) => {
                        match rec.kind {
                            // Refused escapes added nothing — the pin
                            // stays unattached.
                            EscapeKind::CorridorBound
                            | EscapeKind::NoAnchor
                            | EscapeKind::HaloEmpty => {
                                assert!(
                                    ids.is_empty(),
                                    "{name}: refused escape {} has nodes",
                                    pin_label(&db, pin)
                                );
                            }
                            // A landed halo/corridor puts its pairs in
                            // region; attach is non-structural (a pin
                            // rect covering no track crossing on its
                            // own layers stays empty).
                            EscapeKind::Halo | EscapeKind::CorridorMasked => {}
                        }
                        if !ids.is_empty() {
                            assert!(
                                matches!(
                                    rec.kind,
                                    EscapeKind::Halo | EscapeKind::CorridorMasked
                                ),
                                "{name}: re-attached escape {} records {:?}",
                                pin_label(&db, pin),
                                rec.kind
                            );
                        }
                    }
                    (false, None) => {
                        assert!(
                            !ids.is_empty(),
                            "{name}: pin {} of net {} attached nothing and is not named",
                            pin_label(&db, pin),
                            net_name(&db, g.net)
                        );
                    }
                    _ => panic!(
                        "{name}: pin {} region_escape/escapes mismatch",
                        pin_label(&db, pin)
                    ),
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
        assert_eq!(g1.escapes, g2.escapes);
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
    // The halo reaches index-adjacent pairs of the io box's region —
    // a span-1 merge, no corridor.
    assert_eq!(graphs.escapes.len(), 1);
    let rec = graphs.escapes[0];
    assert_eq!(rec.kind, EscapeKind::Halo);
    assert_eq!(rec.corridor_span, 0);
    assert_eq!(rec.token(), "halo");

    // Fail-proof: both boxes under the grid — every member pin escapes
    // and (n1, met1) is a named zero-coverage pair, then restoring the
    // full guide returns green.
    let empty = "n1\n(\n500 200 1500 800 met1\n500 200 1500 800 met1\n)\n";
    let graphs = build_route_graphs(&db, &tech, &tracks, &guide(empty));
    assert_eq!(graphs.total_nodes, 0);
    assert_eq!(graphs.region_escape.len(), 2, "{:?}", graphs.region_escape);
    // With no region pairs to anchor to, every escape is refused as
    // `NoAnchor` — nothing enters the graph.
    assert_eq!(graphs.escapes.len(), 2);
    assert!(
        graphs.escapes.iter().all(|e| e.kind == EscapeKind::NoAnchor
            && e.pairs_added == 0
            && e.corridor_span == 0
            && e.token() == "no_anchor"),
        "{:?}",
        graphs.escapes
    );
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
            .map(|g| {
                format!(
                    "({},{})",
                    routing_name(&tech, g.lower),
                    routing_name(&tech, g.upper)
                )
            })
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
            vec![180_427, 11_148, 0, 47, 0]
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
    let node_xy = |g: &grus_eda_core::drt::graph::NetGraph, id: u32| {
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
        vec![grus_eda_core::drt::via::ViaGap {
            lower: LayerId(0),
            upper: LayerId(1)
        }]
    );
    assert!(table.pairs[0].default.is_none());
}

// ---------------------------------------------------------------------------
// Maze router
// ---------------------------------------------------------------------------

use grus_eda_core::drt::maze::{
    FailKind, IterStat, MazeConfig, OccMode, RouteDb, RouteProgress, overlap_census, route_digest,
    route_nets, route_nets_with_progress,
};
use grus_eda_core::drt::probe::{ProbeNet, ProbePin, ProbeReport, RootCause, probe_unrouted};
use grus_eda_core::drt::graph::RouteGraphs;

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
    // escapes on the next layer inside the same guide XY) — plus the
    // bounded pin-halo escape.
    let graphs = grus_eda_core::drt::graph::build_route_graphs_ext(
        &db,
        &tech,
        &tracks,
        &join,
        grus_eda_core::drt::graph::GraphOpts {
            spare_above: 1,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
        },
        0,
    );
    let vias = via_table(&tech, &db).expect("via_table");
    (tech, db, tracks, join, graphs, vias)
}

/// One `(fixture, config)` pair's shared route: the immutable input
/// bundle plus its `RouteDb`, resolved once per test process. Tests
/// that only read the route borrow `&'static RoutedLeg`; tests that
/// mutate emit geometry clone `leg.db` (the `&'static` borrow can
/// never yield `&mut`). `route_ms` is measured inside the
/// initializer — it is the single real execution's time, whatever
/// `Instant` would read in the resolving thread.
struct RoutedLeg {
    name: &'static str,
    tech: TechDb,
    db: DesignDb,
    tracks: Vec<LayerTracks>,
    join: GuideJoin,
    vias: ViaTable,
    cfg: MazeConfig,
    route: RouteDb,
    route_ms: u128,
}

static GCD_LEG: std::sync::OnceLock<RoutedLeg> = std::sync::OnceLock::new();
static GCD_OCC_OFF_LEG: std::sync::OnceLock<RoutedLeg> = std::sync::OnceLock::new();
static PICORV32_LEG: std::sync::OnceLock<RoutedLeg> = std::sync::OnceLock::new();

/// Resolves `cell` once per process. The init deliberately skips
/// `build_route_graphs_ext`: `route_nets` rebuilds per-net graphs
/// internally (the `opts` argument only documents the build options)
/// and no shared consumer needs the `RouteGraphs` — the two that do
/// (`test_mem_census_sanity`, `test_net_graph_rebuild_identity`)
/// keep `load_routed_inputs` / explicit builds.
fn shared_leg(
    name: &'static str,
    occ: OccMode,
    cell: &'static std::sync::OnceLock<RoutedLeg>,
) -> &'static RoutedLeg {
    cell.get_or_init(|| {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let vias = via_table(&tech, &db).expect("via_table");
        let cfg = MazeConfig {
            occ,
            ..MazeConfig::default()
        };
        let t0 = std::time::Instant::now();
        let route = route_nets(&db, &tech, &tracks, &join, LOADED_OPTS, &vias, &cfg);
        RoutedLeg {
            name,
            tech,
            db,
            tracks,
            join,
            vias,
            cfg,
            route,
            route_ms: t0.elapsed().as_millis(),
        }
    })
}

/// The shared legs are read across test threads — a `Cell`/`RefCell`
/// regression inside any field fails here naming the type.
const _: fn() = || {
    fn need<T: Sync + Send>() {}
    need::<RoutedLeg>();
};

fn gcd_leg() -> &'static RoutedLeg {
    shared_leg("gcd", OccMode::Enabled, &GCD_LEG)
}

fn gcd_occ_off_leg() -> &'static RoutedLeg {
    shared_leg("gcd", OccMode::Disabled, &GCD_OCC_OFF_LEG)
}

fn leg_for(name: &str) -> &'static RoutedLeg {
    match name {
        "gcd" => gcd_leg(),
        "picorv32" => shared_leg("picorv32", OccMode::Enabled, &PICORV32_LEG),
        _ => panic!("no shared leg for {name}"),
    }
}

/// One fresh `route_nets` execution on a shared leg's inputs — the
/// independent second run every determinism check compares against
/// (`leg.route` vs `leg.route` proves nothing).
fn route_fresh(leg: &'static RoutedLeg) -> RouteDb {
    route_nets(
        &leg.db,
        &leg.tech,
        &leg.tracks,
        &leg.join,
        LOADED_OPTS,
        &leg.vias,
        &leg.cfg,
    )
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

/// The `geom_obs_sealed` arm's injected fixture (`probe.md` §4, arm
/// C): `CELLD` is `CELLC` grown to 0.4×0.6 so the pin rect keeps one
/// covered met1 line (y=1000 at the (1400,800) placement) while
/// macro OBS walls off the hull interior above and below it — the
/// pin's strict set is empty (escaped), every `project_attach`
/// candidate's stub and every access-via plate hits a fixed `Obs`
/// mark inside the host hull (host-open confinement admits the
/// candidates to the wall; `Obs` then blocks them). `CELLE` is the
/// identical outline without the OBS — the baseline arm.
const SEAL_LEF: &str = r#"MACRO CELLD
  CLASS CORE ;
  SIZE 0.4 BY 0.6 ;
  PIN C
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.0 0.2 0.4 0.4 ;
    END
  END C
  OBS
    LAYER met1 ;
      RECT 0.0 0.0 0.4 0.2 ;
      RECT 0.0 0.4 0.4 0.6 ;
  END
END CELLD
MACRO CELLE
  CLASS CORE ;
  SIZE 0.4 BY 0.6 ;
  PIN C
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.0 0.2 0.4 0.4 ;
    END
  END C
END CELLE
"#;

/// `maze_tech` plus the arm-C seal macros.
fn maze_tech_seal() -> TechDb {
    let mut interner = StringInterner::new();
    let mut tech = TechDb::new();
    parse_lef_bytes(MAZE_LEF.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
    parse_lef_bytes(SEAL_LEF.as_bytes(), &mut tech, &mut interner).expect("seal LEF parses");
    tech.strings = interner.clone();
    tech.rebuild_maps();
    tech
}

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
    let vias = via_table(&tech, &db).unwrap();
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 1);
    let n1 = &r.nets[0];
    assert_eq!(
        n1.wires,
        vec![grus_eda_core::drt::maze::WireSeg {
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
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    let n2 = &r.nets[0];
    assert_eq!(n2.connected.len(), 2);
    assert_eq!(
        n2.vias,
        vec![grus_eda_core::drt::maze::ViaPlace {
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
    let vias_blk = via_table(&tech, &db_blk).unwrap();
    let r = route_nets(
        &db_blk,
        &tech,
        &tracks_blk,
        &join,
        GRAPHS_OPTS,
        &vias_blk,
        &cfg,
    );
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
    let inflated = grus_eda_core::tech_db::RectDbu::new(1500 - 210, 900 - 210, 2500 + 210, 1100 + 210);
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
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.nets.is_empty(), "{:?}", r.nets);
    assert_eq!(r.unrouted.len(), 2, "{:?}", r.unrouted);
    let labels = |u: &grus_eda_core::drt::maze::UnroutedNet| -> Vec<(String, FailKind)> {
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

/// The progress hook sees one `RouteProgress` per attempted net in
/// `bad` order, `done` walks `1..=queued` per iteration, and an
/// iteration's last event carries the `IterStat` failure count.
/// Hooked and unhooked runs return identical `RouteDb`s — the sink
/// cannot perturb routing (it holds a `Copy` snapshot, no state
/// references).
#[test]
fn test_route_nets_progress_hook() {
    let tech = maze_tech();
    let cfg = MazeConfig::default();
    let db = maze_db(&tech, false);
    let tracks = layer_tracks(&db, &tech);
    // n1 routes straight, n2 routes through a via, n3's u2/C pin is
    // `Unattachable` — iteration 1 attempts 3 nets, iterations 2..
    // retry n3 alone until the hard cap.
    let join = maze_guide(
        &db,
        &tech,
        "n1\n(\n0 0 5000 2000 met1\n)\n\
         n2\n(\n0 0 5000 5000 met1\n0 0 5000 5000 met2\n)\n\
         n3\n(\n0 0 5000 5000 met1\n)\n",
    );
    let vias = via_table(&tech, &db).unwrap();

    let mut events = Vec::new();
    let (_census, hooked) = route_nets_with_progress(
        &db,
        &tech,
        &tracks,
        &join,
        GRAPHS_OPTS,
        &vias,
        &cfg,
        Some(&mut |p: RouteProgress| events.push(p)),
    );

    // One event per attempt, across all iterations.
    let total: u32 = hooked.iter_stats.iter().map(|s| s.attempted).sum();
    assert_eq!(events.len() as u32, total);
    assert!(
        hooked.iterations > 1,
        "fixture must exercise rip-up iterations"
    );

    let mut cursor = 0usize;
    for (it, stat) in hooked.iter_stats.iter().enumerate() {
        let group = &events[cursor..cursor + stat.attempted as usize];
        for (k, ev) in group.iter().enumerate() {
            assert_eq!(ev.iteration, it as u32 + 1);
            assert_eq!(ev.done, k as u32 + 1);
            assert_eq!(ev.queued, stat.attempted);
        }
        let last = group.last().unwrap();
        assert_eq!(last.done, last.queued);
        assert_eq!(last.unrouted, stat.attempted - stat.routed);
        cursor += stat.attempted as usize;
    }

    let plain = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert_eq!(hooked, plain);
}

/// `MAZE_LEF` plus a foreign-hull wall: `w0`'s `SIZE 1.1×5.0` hull
/// sits at x∈[1400,2500] across the whole grid height, so every
/// met1 segment from the guide region to u0's neighborhood crosses
/// a foreign hull and dies at `seg_allowed` (the `relaxed_vias`
/// confinement check) — u0's met2 transit island stays unreachable
/// and the pin is `NoPath` before the escape. met2 is unmasked, so
/// the escape halo bridges the wall there and the merged graph
/// routes.
const ESC_LEF_WALL: &str = "MACRO WALL\n  CLASS CORE ;\n  SIZE 1.1 BY 5.0 ;\nEND WALL\n";

const ESC_DEF_TAIL: &str = r#"PINS 1 ;
- io1 + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 1000 ) N ;
END PINS
NETS 1 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

/// Arm F's occupancy band: io4's met2 port rect lands at absolute
/// `x∈[1900,2100] × y∈[-2100,5100]`, rasterizing `Pin(n2)` on the
/// met2 `x = 2000` line over every y-track — every met2 edge on the
/// `ix = 2` escape column or crossing the band hits the mark, while
/// `ix = 1`/`ix = 3` stay clear (edge-to-track ≥ 800 > w/2 + s ≈ 210).
/// A foreign `Pin` is the one light-weight mark that contrasts:
/// `hit_verdict` hard-blocks `Obs` and foreign `ObsPin` before the
/// `OccMode::Disabled` check, so OBS rects and instance-pin marks
/// block in both modes.
const ESC_DEF_TAIL_OCC: &str = r#"PINS 2 ;
- io1 + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 1000 ) N ;
- io4 + NET n2 + DIRECTION INPUT + USE SIGNAL + LAYER met2 ( -100 -3600 ) ( 100 3600 ) + PLACED ( 2000 1500 ) N ;
END PINS
NETS 2 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
- n2 ( PIN io4 ) + USE SIGNAL ;
END NETS
END DESIGN
"#;

/// The bounded pin-halo escape (`docs/e6-drt.md` §5): u0/A's met1
/// rect sits at crossing (1000,1000) but the guide covers only the
/// io side, and the `w0` wall walls off every met1 stub the
/// route-time fallbacks could project — `NoPath` before the escape,
/// routed once the halo merges. `region_escape` names u0/A either
/// way (the flag is the pre-halo statement); `escapes` records what
/// the mechanism did. Arms: off / merged halo / `CorridorBound`
/// under a zero bound / `CorridorMasked` when the corridor dies at
/// the wall. The corridor-landing arm runs on the open fixture,
/// where a clean L-path records `halo(span=3)`. The last arm is the
/// occupancy contrast: a foreign io-port `Pin` band covering only
/// escaped pairs fails `Enabled` and routes `Disabled` — escaped
/// metal is occupancy-checked like any other.
#[test]
fn test_maze_guide_escape() {
    let mut interner = StringInterner::new();
    let mut tech = TechDb::new();
    let lef = MAZE_LEF.replace("END LIBRARY", &format!("{ESC_LEF_WALL}END LIBRARY"));
    parse_lef_bytes(lef.as_bytes(), &mut tech, &mut interner).expect("fixture LEF parses");
    tech.strings = interner.clone();
    tech.rebuild_maps();
    let cfg = MazeConfig::default();
    let comps = "COMPONENTS 2 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- w0 WALL + PLACED ( 1400 0 ) N ;\nEND COMPONENTS\n";
    let db = maze_db_custom(&tech, comps, ESC_DEF_TAIL);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    let build = |join: &GuideJoin, escape| {
        grus_eda_core::drt::graph::build_route_graphs_ext(
            &db,
            &tech,
            &tracks,
            join,
            grus_eda_core::drt::graph::GraphOpts {
                spare_above: 0,
                escape,
            },
            0,
        )
    };
    let u0a = |graphs: &RouteGraphs| -> PinId {
        graphs.nets[0]
            .pin_nodes
            .iter()
            .find(|&&(p, _)| pin_label(&db, p) == "u0/A")
            .map(|&(p, _)| p)
            .unwrap()
    };
    let pin_ids = |graphs: &RouteGraphs, pin: PinId| -> usize {
        graphs.nets[0]
            .pin_nodes
            .iter()
            .find(|&&(p, _)| p == pin)
            .map(|(_, ids)| ids.len())
            .unwrap()
    };
    let failed = |r: &grus_eda_core::drt::maze::RouteDb, pin: PinId| -> Vec<FailKind> {
        r.unrouted
            .iter()
            .flat_map(|u| u.failed.iter())
            .filter(|&&(p, _)| p == pin)
            .map(|&(_, k)| k)
            .collect()
    };

    // Arm A — mechanism off: bit-identical to the pre-escape build.
    // The strict-escape pin holds no nodes; the wall kills every
    // fallback stub, so the net fails `NoPath` (the pin still
    // attaches to its met2 transit island).
    let join = maze_guide(&db, &tech, "n1\n(\n3000 0 5000 2000 met1\n)\n");
    let graphs = build(&join, None);
    let pin = u0a(&graphs);
    assert!(graphs.escapes.is_empty());
    assert_eq!(graphs.region_escape.len(), 1);
    assert_eq!(graphs.region_escape[0].pin, pin);
    assert_eq!(pin_ids(&graphs, pin), 0, "escape pin must hold no nodes");
    let r = route_nets(
        &db,
        &tech,
        &tracks,
        &join,
        GraphOpts {
            spare_above: 0,
            escape: None,
        },
        &vias,
        &cfg,
    );
    assert_eq!(failed(&r, pin), vec![FailKind::NoPath], "{:?}", r.unrouted);

    // Arm B — mechanism on: the halo merges (met1 pairs adjacent to
    // the guide region, met2 pairs adjacent to the transit island),
    // u0/A attaches at its own crossing, and the met2 halo bridges
    // the wall — the net routes. `region_escape` still names u0/A.
    let graphs = build(&join, Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT));
    assert_eq!(graphs.region_escape.len(), 1, "{:?}", graphs.region_escape);
    assert_eq!(graphs.escapes.len(), 1);
    let rec = graphs.escapes[0];
    assert_eq!(rec.pin, pin);
    assert_eq!(rec.kind, EscapeKind::Halo, "{:?}", rec);
    assert_eq!(rec.corridor_span, 0, "{:?}", rec);
    assert_eq!(rec.token(), "halo");
    assert!(rec.pairs_added > 0);
    assert!(pin_ids(&graphs, pin) > 0, "halo pairs inside the pin rect");
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 1);
    assert_eq!(r.nets[0].connected.len(), 2);

    // Arm C — refusal is honest: under a zero corridor bound the
    // same escape set records `CorridorBound`, no pairs are added,
    // and the pin stays unreachable.
    let join = maze_guide(&db, &tech, "n1\n(\n4000 0 5000 2000 met1\n)\n");
    let graphs = build(
        &join,
        Some(grus_eda_core::drt::graph::EscapeCfg {
            halo_lines: 0,
            corridor_lines: 0,
        }),
    );
    assert_eq!(graphs.escapes.len(), 1);
    let rec = graphs.escapes[0];
    assert_eq!(rec.kind, EscapeKind::CorridorBound, "{:?}", rec);
    assert_eq!(rec.pairs_added, 0);
    assert_eq!(rec.corridor_span, 3);
    assert_eq!(rec.token(), "corridor_bound(span=3)");
    assert_eq!(pin_ids(&graphs, pin), 0);
    let r = route_nets(
        &db,
        &tech,
        &tracks,
        &join,
        GraphOpts {
            spare_above: 0,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg {
                halo_lines: 0,
                corridor_lines: 0,
            }),
        },
        &vias,
        &cfg,
    );
    assert_eq!(failed(&r, pin), vec![FailKind::NoPath], "{:?}", r.unrouted);

    // Arm D — the corridor dies at the mask: halo_lines=0 puts the
    // pin's own crossing in the halo, the bound admits the 3-step
    // corridor, but the L-path crosses the wall — `CorridorMasked`
    // keeps the legal halo pair (u0/A attaches to it) while the
    // net still fails `NoPath`.
    let graphs = build(
        &join,
        Some(grus_eda_core::drt::graph::EscapeCfg {
            halo_lines: 0,
            corridor_lines: 8,
        }),
    );
    assert_eq!(graphs.escapes.len(), 1);
    let rec = graphs.escapes[0];
    assert_eq!(rec.kind, EscapeKind::CorridorMasked, "{:?}", rec);
    assert_eq!(rec.corridor_span, 3, "{:?}", rec);
    assert_eq!(rec.token(), "corridor_masked");
    assert_eq!(rec.pairs_added, 1, "{:?}", rec);
    assert!(pin_ids(&graphs, pin) > 0);
    let r = route_nets(
        &db,
        &tech,
        &tracks,
        &join,
        GraphOpts {
            spare_above: 0,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg {
                halo_lines: 0,
                corridor_lines: 8,
            }),
        },
        &vias,
        &cfg,
    );
    assert_eq!(failed(&r, pin), vec![FailKind::NoPath], "{:?}", r.unrouted);

    // Corridor landing, open fixture: no wall, halo_lines=0 —
    // met1's halo is just the pin crossing, the guide starts three
    // index steps away, and the L-corridor lands on every pair. The
    // record names `halo(span=3)`; here the route-time fallbacks
    // would have rescued the pin anyway (no wall), so the arm pins
    // corridor geometry and the token, not necessity.
    let db_open = maze_db(&tech, false);
    let tracks_open = layer_tracks(&db_open, &tech);
    let vias_open = via_table(&tech, &db_open).unwrap();
    let join = maze_guide(&db_open, &tech, "n1\n(\n4000 0 5000 2000 met1\n)\n");
    let graphs = grus_eda_core::drt::graph::build_route_graphs_ext(
        &db_open,
        &tech,
        &tracks_open,
        &join,
        grus_eda_core::drt::graph::GraphOpts {
            spare_above: 0,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg {
                halo_lines: 0,
                corridor_lines: 8,
            }),
        },
        0,
    );
    assert_eq!(graphs.escapes.len(), 1);
    let rec = graphs.escapes[0];
    assert_eq!(rec.kind, EscapeKind::Halo, "{:?}", rec);
    assert_eq!(rec.corridor_span, 3, "{:?}", rec);
    assert_eq!(rec.token(), "halo(span=3)");
    // The union-count pin: met1 halo {(1,1)} + corridor path
    // {(2,1),(3,1)} = 3 distinct pairs; met2's halo {(1,1)} is
    // already `region` (the transit island) and adds nothing — the
    // retired `halo_total + extra.len()` arithmetic reported 4.
    assert_eq!(rec.pairs_added, 3, "{:?}", rec);
    let r = route_nets(
        &db_open,
        &tech,
        &tracks_open,
        &join,
        GraphOpts {
            spare_above: 0,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg {
                halo_lines: 0,
                corridor_lines: 8,
            }),
        },
        &vias_open,
        &cfg,
    );
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);

    // Arm F — the occupancy contrast (`docs/e6-drt.md` §5): every
    // met2 pair io4's `Pin` band can hit is an escaped pair
    // (non-escape met2 `region` is exactly the (1,1) transit
    // island; the guide covers no met2 at all), so the block is
    // attributable to escape metal by construction, not by
    // argument. `Enabled` fails `NoPath` — soft-priced crossings
    // escalate until the hard iteration blocks them — while
    // `Disabled` routes the identical graph. The `escapes` record
    // is `Halo` either way: the graph build is occupancy-
    // independent, the check is route-time.
    let db_occ = maze_db_custom(&tech, comps, ESC_DEF_TAIL_OCC);
    let tracks_occ = layer_tracks(&db_occ, &tech);
    let vias_occ = via_table(&tech, &db_occ).unwrap();
    let join_occ = maze_guide(&db_occ, &tech, "n1\n(\n3000 0 5000 2000 met1\n)\n");
    let graphs_occ = grus_eda_core::drt::graph::build_route_graphs_ext(
        &db_occ,
        &tech,
        &tracks_occ,
        &join_occ,
        grus_eda_core::drt::graph::GraphOpts {
            spare_above: 0,
            escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
        },
        0,
    );
    assert_eq!(graphs_occ.escapes.len(), 1);
    assert_eq!(
        graphs_occ.escapes[0].kind,
        EscapeKind::Halo,
        "{:?}",
        graphs_occ.escapes
    );
    let pin_occ = graphs_occ.nets[0]
        .pin_nodes
        .iter()
        .find(|&&(p, _)| pin_label(&db_occ, p) == "u0/A")
        .map(|&(p, _)| p)
        .unwrap();
    let r = route_nets(
        &db_occ,
        &tech,
        &tracks_occ,
        &join_occ,
        GRAPHS_OPTS,
        &vias_occ,
        &cfg,
    );
    assert_eq!(
        failed(&r, pin_occ),
        vec![FailKind::NoPath],
        "{:?}",
        r.unrouted
    );
    let r = route_nets(
        &db_occ,
        &tech,
        &tracks_occ,
        &join_occ,
        GRAPHS_OPTS,
        &vias_occ,
        &MazeConfig {
            occ: OccMode::Disabled,
            ..cfg
        },
    );
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 1);
    assert_eq!(r.nets[0].connected.len(), 2);
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
    let band = grus_eda_core::tech_db::RectDbu::new(1690, 690, 2310, 1310);
    let n2_guide = "n2\n(\n0 0 5000 5000 met1\n0 0 5000 5000 met2\n)\n";

    // Arm A — open guide: the router must find a clean detour (an
    // off-axis jog to y=0/2000 or a met2 leg), never the silent
    // overlap.
    let join = maze_guide(
        &db,
        &tech,
        &format!("n1\n(\n0 0 5000 2000 met1\n0 0 5000 5000 met2\n)\n{n2_guide}"),
    );
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);
    assert_eq!(r.nets.len(), 2);
    let n1 = r
        .nets
        .iter()
        .find(|n| net_name(&db, n.net) == "n1")
        .unwrap();
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
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
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

/// Recorded gcd route digest (`docs/results.md` `drt-rayon` entry):
/// a `route_digest` drift means the routed geometry, the
/// connectivity record, or the iteration stats changed — the pin for
/// any router/occupancy data-structure rewrite. Re-pinned from
/// `0x8a4768ab15bd4bd5` at drt-rayon: soft-iteration searches now
/// see the iteration-start snapshot rather than serial-interleave
/// occupancy, so committed routes legitimately differ — identical
/// across thread counts by the ordered-commit contract
/// (`test_rayon_route_determinism`).
const GCD_ROUTE_DIGEST: u64 = 0xef2b3ff70271a2e9;

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
/// pinned there. Run: `cargo test --release -p grus-eda-core --test drt
/// test_maze_census_picorv32 -- --ignored --nocapture`.
#[test]
#[ignore]
fn test_maze_census_picorv32() {
    maze_census("picorv32");
}

fn maze_census(name: &str) {
    {
        let leg = leg_for(name);
        let (db, join, r, ms) = (&leg.db, &leg.join, &leg.route, leg.route_ms);

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
            route_digest(r),
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
                .map(|&(p, k)| format!("{}:{k:?}", pin_label(db, p)))
                .collect();
            println!("  unrouted {} pins={kinds:?}", net_name(db, u.net));
        }

        // Occupancy data-structure pin: the route output must be
        // byte-identical across the `BTreeMap` → `Vec<Line>` swap.
        if leg.name == "gcd" {
            assert_eq!(
                route_digest(r),
                GCD_ROUTE_DIGEST,
                "gcd route digest drifted"
            );
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
                    unclassified.push(net_name(db, u.net));
                }
            }
            println!("  {} met3_bridge={bridged} met2_connected={pocket}", net_name(db, u.net));
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

// ---------------------------------------------------------------------------
// Root-cause probe (kurena/tasks/drt-unrouted-analysis)
// ---------------------------------------------------------------------------

/// The per-pin decomposition table `docs/results.md` transcribes,
/// plus the anchor block: the met3-bridge union split and the
/// `region_escape ∩ u.failed` per-class split.
fn probe_report_lines(name: &str, rep: &ProbeReport, db: &DesignDb) {
    let pins: usize = rep.nets.iter().map(|n| n.pins.len()).sum();
    println!("== {name} unrouted probe ==");
    println!(
        "{name}: unrouted_nets={} unrouted_pins={pins} unguided_io_only={} probe_ms={}",
        rep.nets.len(),
        rep.unguided.len(),
        rep.elapsed.as_millis(),
    );
    let class_counts = rep.class_counts();
    let net_counts = rep.net_counts();
    for (k, v) in &class_counts {
        println!("{name}: {k}={v} nets={}", net_counts[k]);
    }
    for pn in &rep.nets {
        let kinds: Vec<String> = pn
            .pins
            .iter()
            .map(|p| format!("{}:{}", pin_label(db, p.pin), p.cause.name()))
            .collect();
        println!("  {} pins={kinds:?}", net_name(db, pn.net));
    }
    // Anchor: bridged-union nets split into `geom_met3_bridge` vs
    // probe-rescued (every pin landing non-`geom_met3_bridge` is a
    // rescue by definition — the bridge was not the blocker).
    let bridged: Vec<&ProbeNet> =
        rep.nets.iter().filter(|n| n.union == GuideUnion::Bridged).collect();
    let m3b: usize = bridged
        .iter()
        .map(|n| n.pins.iter().filter(|p| p.cause == RootCause::GeomMet3Bridge).count())
        .sum();
    let rescued: usize = bridged.iter().map(|n| n.pins.len()).sum::<usize>() - m3b;
    println!(
        "{name}: bridged_union_nets={} geom_met3_bridge_pins={m3b} rescued_pins={rescued}",
        bridged.len()
    );
    // Anchor: `region_escape ∩ u.failed` split by class — the flag is
    // provenance, not a class.
    let mut escape_split: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for pn in &rep.nets {
        for pp in pn.pins.iter().filter(|p| p.region_escape) {
            escape_split
                .entry(pp.cause.name())
                .or_default()
                .push(format!("{}/{}", net_name(db, pn.net), pin_label(db, pp.pin)));
        }
    }
    for (k, v) in &escape_split {
        println!("{name}: region_escape {k}={} {v:?}", v.len());
    }
}

/// The invariant bundle every probe leg asserts: nets/pins mirror
/// `r.unrouted` positionally, every pin lands in exactly one class,
/// `geom_met3_bridge` nets stay inside the bridged-union set, and the
/// residual buckets stay empty (they are assertable because
/// `class_counts` always carries the keys).
fn assert_probe_coverage(rep: &ProbeReport, db: &DesignDb, r: &RouteDb) {
    assert_eq!(rep.nets.len(), r.unrouted.len(), "probe nets != unrouted");
    for (pn, u) in rep.nets.iter().zip(&r.unrouted) {
        assert_eq!(pn.net, u.net, "probe net order diverged from unrouted");
        assert_eq!(pn.pins.len(), u.failed.len());
        for (pp, &(pin, kind)) in pn.pins.iter().zip(&u.failed) {
            assert_eq!(pp.pin, pin);
            assert_eq!(pp.real_kind, kind);
        }
    }
    let class_counts = rep.class_counts();
    let failed_total: usize = r.unrouted.iter().map(|u| u.failed.len()).sum();
    assert_eq!(
        class_counts.values().sum::<usize>(),
        failed_total,
        "root-cause classes must sum to the unrouted pin census"
    );
    assert_eq!(
        class_counts["residual_missing_pin"], 0,
        "{:?}",
        rep.nets
            .iter()
            .filter(|n| n
                .pins
                .iter()
                .any(|p| p.cause == RootCause::ResidualMissingPin))
            .map(|n| net_name(db, n.net))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        class_counts["residual_attach_inconsistent"], 0,
        "{:?}",
        rep.nets
            .iter()
            .filter(|n| n
                .pins
                .iter()
                .any(|p| p.cause == RootCause::ResidualAttachInconsistent))
            .map(|n| net_name(db, n.net))
            .collect::<Vec<_>>()
    );
    // met3-bridge anchor: `geom_met3_bridge`-carrying nets ⊆ the
    // bridged-union set, and a bridged net's pins are met3_bridge or
    // rescued (negotiation/residual) — never another `geom` class.
    let bridged: BTreeSet<NetId> = rep
        .nets
        .iter()
        .filter(|n| n.union == GuideUnion::Bridged)
        .map(|n| n.net)
        .collect();
    let m3b_nets: BTreeSet<NetId> = rep
        .nets
        .iter()
        .filter(|n| n.pins.iter().any(|p| p.cause == RootCause::GeomMet3Bridge))
        .map(|n| n.net)
        .collect();
    assert!(
        m3b_nets.is_subset(&bridged),
        "geom_met3_bridge nets outside the bridged-union set: {:?}",
        m3b_nets.difference(&bridged).collect::<Vec<_>>()
    );
    for pn in rep.nets.iter().filter(|n| n.union == GuideUnion::Bridged) {
        for pp in &pn.pins {
            assert!(
                pp.cause == RootCause::GeomMet3Bridge || !pp.cause.is_geometric(),
                "{}: {} landed {} on a bridged-union net",
                net_name(db, pn.net),
                pin_label(db, pp.pin),
                pp.cause.name()
            );
        }
    }
}

fn probe_find<'a>(
    rep: &'a ProbeReport,
    db: &DesignDb,
    net: &str,
    pin: &str,
) -> &'a ProbePin {
    rep.nets
        .iter()
        .find(|n| net_name(db, n.net) == net)
        .and_then(|n| n.pins.iter().find(|p| pin_label(db, p.pin) == pin))
        .unwrap_or_else(|| panic!("probe pin {net}/{pin} missing"))
}

/// gcd's final unrouted population decomposes exactly once per pin;
/// the counts transcribe into `docs/results.md`.
#[test]
fn test_unrouted_probe_gcd() {
    let leg = gcd_leg();
    let (db, tech, tracks, join, vias, r) = (
        &leg.db,
        &leg.tech,
        &leg.tracks,
        &leg.join,
        &leg.vias,
        &leg.route,
    );
    let rep = probe_unrouted(db, tech, tracks, join, LOADED_OPTS, vias, r, 0);
    probe_report_lines("gcd", &rep, db);
    assert_probe_coverage(&rep, db, r);
    // The measured decomposition — `docs/results.md` transcribes the
    // same counts: 80 unrouted nets / 93 failed pins, all but four
    // negotiation (the `stub_outside_cover`-backed attach stubs and
    // the `Net`/`Pin`-evidenced wave deaths); no guide-absent io-only
    // catch-alls.
    assert_eq!(rep.nets.len(), 80);
    assert_eq!(rep.nets.iter().map(|n| n.pins.len()).sum::<usize>(), 93);
    assert_eq!(rep.unguided.len(), 0, "{:?}", rep.unguided);
    // `unguided` joins the golden `ours_router.unguided` by name
    // (gcd's golden list is empty).
    let golden_unguided: BTreeSet<String> =
        j_arr(&load_ours_ref("gcd")["ours_router"]["unguided"], "ours_router.unguided")
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    assert_eq!(
        rep.unguided.iter().map(|&n| net_name(db, n)).collect::<BTreeSet<String>>(),
        golden_unguided
    );
    let class_counts = rep.class_counts();
    assert_eq!(class_counts["neg_attach_stub"], 54);
    assert_eq!(class_counts["neg_wave"], 35);
    assert_eq!(class_counts["geom_pocket"], 4);
    // Output-unchanged by construction, pinned anyway — the probe ran
    // against `&RouteDb` and cannot move the digest.
    assert_eq!(route_digest(&leg.route), GCD_ROUTE_DIGEST);
}

/// The probe is a pure function of the same inputs — two runs on the
/// shared leg must land every pin in the same class.
#[test]
fn test_unrouted_probe_deterministic_gcd() {
    let leg = gcd_leg();
    let run = || {
        probe_unrouted(
            &leg.db,
            &leg.tech,
            &leg.tracks,
            &leg.join,
            LOADED_OPTS,
            &leg.vias,
            &leg.route,
            0,
        )
    };
    let (a, b) = (run(), run());
    // `nets`/`unguided` compare by `PartialEq` — `elapsed` differs
    // between runs and is excluded by construction (it is wall time).
    assert_eq!(a.nets, b.nets, "probe decomposition must be deterministic");
    assert_eq!(a.unguided, b.unguided);
}

/// Arm B (`probe.md` §4): the `test_maze_pin_collision` fixture's
/// hard arm — io2's met1 port band is the only `Pin` mark on n1's
/// wave path. Dropping it must route n1, and the `(met1, Pin)` wave
/// postmortem lands u0/A in `neg_wave`.
#[test]
fn test_unrouted_probe_pin_collision() {
    let tech = maze_tech();
    let cfg = MazeConfig::default();
    let comps = "COMPONENTS 2 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\nEND COMPONENTS\n";
    let db = maze_db_custom(&tech, comps, MAZE_PIN_TAIL);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    let n2_guide = "n2\n(\n0 0 5000 5000 met1\n0 0 5000 5000 met2\n)\n";
    let join = maze_guide(
        &db,
        &tech,
        &format!("n1\n(\n0 800 5000 1200 met1\n)\n{n2_guide}"),
    );
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert_eq!(r.unrouted.len(), 1, "{:?}", r.unrouted);
    let rep = probe_unrouted(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &r, 0);
    probe_report_lines("pin_collision", &rep, &db);
    assert_probe_coverage(&rep, &db, &r);
    let p = probe_find(&rep, &db, "n1", "u0/A");
    assert_eq!(p.real_kind, FailKind::NoPath);
    assert_eq!(p.cause, RootCause::NegWave);
    assert_eq!(p.wave_evidence, Some((0, 3)), "{:?}", p);
    assert!(
        rep.nets.iter().all(|n| n.probe_routed),
        "the probe must route n1 once the foreign Pin band is gone"
    );
}

/// Arm A: case 4 of `test_maze_cases` — n1's disjoint guide union
/// lands u0/A in `geom_disjoint`; u2/C's track-less rect lands in
/// `geom_off_track` with `region_escape` set.
#[test]
fn test_unrouted_probe_geom_disjoint() {
    let tech = maze_tech();
    let cfg = MazeConfig::default();
    let db = maze_db(&tech, false);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    let join = maze_guide(
        &db,
        &tech,
        "n1\n(\n0 0 2000 2000 met1\n3500 500 4500 1500 met1\n)\nn3\n(\n0 0 5000 5000 met1\n)\n",
    );
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert_eq!(r.unrouted.len(), 2, "{:?}", r.unrouted);
    let rep = probe_unrouted(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &r, 0);
    probe_report_lines("geom_disjoint", &rep, &db);
    assert_probe_coverage(&rep, &db, &r);
    let p = probe_find(&rep, &db, "n1", "u0/A");
    assert_eq!(p.real_kind, FailKind::NoPath);
    assert_eq!(p.cause, RootCause::GeomDisjoint);
    let p = probe_find(&rep, &db, "n3", "u2/C");
    assert_eq!(p.real_kind, FailKind::Unattachable);
    assert_eq!(p.cause, RootCause::GeomOffTrack);
    assert!(p.region_escape, "the off-track pin must escape the region");
}

/// Arm C — inject `CELLD`'s OBS seal over u2/C's only attach window,
/// confirm the `geom_obs_sealed` classification, and revert to the
/// `CELLE` baseline which routes clean. The exact class is asserted
/// on the rescan evidence (host-open confinement admits one
/// candidate; its stub dies on the `Obs` mark).
#[test]
fn test_unrouted_probe_geom_obs_sealed() {
    let tech = maze_tech_seal();
    let cfg = MazeConfig::default();
    let comps = |mac: &str| {
        format!(
            "COMPONENTS 3 ;\n- u0 CELL + PLACED ( 900 900 ) N ;\n- u1 CELLB + PLACED ( 3900 2900 ) N ;\n- u2 {mac} + PLACED ( 1400 800 ) N ;\nEND COMPONENTS\n"
        )
    };
    let guide = "n3\n(\n0 0 5000 5000 met1\n)\n";

    // Baseline — the same pin geometry with no OBS routes.
    let db = maze_db_custom(&tech, &comps("CELLE"), MAZE_DEF_TAIL);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    let join = maze_guide(&db, &tech, guide);
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert!(r.unrouted.is_empty(), "{:?}", r.unrouted);

    // Injected — the OBS seal makes u2/C `Unattachable`, and the
    // probe's rescan lands it in `geom_obs_sealed` (covered line,
    // every admitted candidate's stub refused by `Obs`).
    let db = maze_db_custom(&tech, &comps("CELLD"), MAZE_DEF_TAIL);
    let tracks = layer_tracks(&db, &tech);
    let vias = via_table(&tech, &db).unwrap();
    let join = maze_guide(&db, &tech, guide);
    let r = route_nets(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &cfg);
    assert_eq!(r.unrouted.len(), 1, "{:?}", r.unrouted);
    let rep = probe_unrouted(&db, &tech, &tracks, &join, GRAPHS_OPTS, &vias, &r, 0);
    probe_report_lines("geom_obs_sealed", &rep, &db);
    assert_probe_coverage(&rep, &db, &r);
    let p = probe_find(&rep, &db, "n3", "u2/C");
    assert_eq!(p.real_kind, FailKind::Unattachable);
    assert_eq!(p.cause, RootCause::GeomObsSealed);
    assert!(p.region_escape, "the sealed pin's strict set must be empty");
}

/// Ignored (runtime): the picorv32 probe re-attempts the route's
/// 1,731 final-unrouted nets — measured ~57 min total on this host
/// (route leg ~53 min + probe ~3.7 min). Run: `cargo test --release
/// -p grus-eda-core --test drt test_unrouted_probe_picorv32 --
/// --ignored --nocapture`.
///
/// The live census is 1,731 nets / 3,083 pins — the earlier 2,016-net
/// pin is the stale v0.12.9 census; `drt-guide-escape` + `drt-rayon`
/// moved the routed output with this leg deferred (970 rescued / 685
/// newly unrouted vs the committed golden — `docs/results.md` names
/// the delta).
#[test]
#[ignore]
fn test_unrouted_probe_picorv32() {
    let leg = leg_for("picorv32");
    let rep = probe_unrouted(
        &leg.db,
        &leg.tech,
        &leg.tracks,
        &leg.join,
        LOADED_OPTS,
        &leg.vias,
        &leg.route,
        0,
    );
    probe_report_lines("picorv32", &rep, &leg.db);
    assert_probe_coverage(&rep, &leg.db, &leg.route);
    assert_eq!(rep.nets.len(), 1731);
    assert_eq!(rep.nets.iter().map(|n| n.pins.len()).sum::<usize>(), 3083);
    // The met3-bridge anchor pinned as the union census (142 bridged
    // unions over the live unrouted set) separately from the class
    // count (32 nets carry `geom_met3_bridge` pins after the
    // probe-rescue split — 546 pins on bridged nets rescued to
    // negotiation). The spec's "43" was the union census of the
    // 108-net v0.12.4 era; lineage in `docs/results.md`.
    assert_eq!(
        rep.nets.iter().filter(|n| n.union == GuideUnion::Bridged).count(),
        142
    );
    assert_eq!(rep.net_counts()["geom_met3_bridge"], 32);
    // The measured decomposition — every named class pinned, zeros
    // included (a named class at 0 is evidence).
    let class_counts = rep.class_counts();
    assert_eq!(class_counts["neg_attach_stub"], 1230);
    assert_eq!(class_counts["neg_wave"], 1206);
    assert_eq!(class_counts["geom_met3_bridge"], 466);
    assert_eq!(class_counts["geom_pocket"], 73);
    assert_eq!(class_counts["neg_wave_obs_frontier"], 59);
    assert_eq!(class_counts["neg_wave_unattributed"], 49);
    assert_eq!(class_counts["neg_attach_port"], 0);
    assert_eq!(class_counts["geom_disjoint"], 0);
    assert_eq!(class_counts["geom_no_in_region"], 0);
    assert_eq!(class_counts["geom_obs_sealed"], 0);
    assert_eq!(class_counts["geom_off_track"], 0);
    // The guide-absent io-port population (67), joined by name to
    // the golden `ours_router.unguided` list.
    let golden_unguided: BTreeSet<String> =
        j_arr(&load_ours_ref("picorv32")["ours_router"]["unguided"], "ours_router.unguided")
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    assert_eq!(
        rep.unguided.iter().map(|&n| net_name(&leg.db, n)).collect::<BTreeSet<String>>(),
        golden_unguided
    );
    assert_eq!(rep.unguided.len(), 67, "{:?}", rep.unguided);
}

/// Occupancy fail-proof (AGENTS §7.7): identical inputs, `Enabled` vs
/// `Disabled` — the outcomes must measurably differ somewhere (route
/// digest or post-hoc collision census), else occupancy constrains
/// nothing and the gate must go red.
#[test]
fn test_maze_occupancy() {
    let on = gcd_leg();
    let off = gcd_occ_off_leg();
    let on_census = overlap_census(&on.route.nets, &on.tech, &on.tracks, &on.vias).len();
    let off_census = overlap_census(&off.route.nets, &off.tech, &off.tracks, &off.vias).len();
    println!(
        "occupancy fail-proof: enabled digest={:#x} census={on_census} | disabled digest={:#x} census={off_census}",
        route_digest(&on.route),
        route_digest(&off.route),
    );
    assert!(
        route_digest(&on.route) != route_digest(&off.route) || on_census != off_census,
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
        let leg = leg_for(name);
        let r1 = &leg.route;
        let r2 = route_fresh(leg);
        assert!(
            !r1.nets.is_empty(),
            "{name}: empty route set proves nothing"
        );
        assert_ne!(route_digest(r1), 0, "{name}: zero digest");
        assert_eq!(r1, &r2, "{name}: nondeterministic routes");
        assert_eq!(route_digest(r1), route_digest(&r2));
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
    b.pin_collisions.push(grus_eda_core::drt::maze::Collision {
        a: NetId(1),
        b: NetId(2),
        layer: LayerId(0),
        line: 1000,
        lo: 1900,
        hi: 2100,
    });
    assert_ne!(route_digest(&a), route_digest(&b));
}

/// `GRUS_DRT_MEM` census fail-proof (`drt-scale-memory` commit 1):
/// the capacity walk must read nonzero, internally consistent bytes
/// on the structures it reports — a census returning 0 makes the
/// whole measurement worthless. Asserts the floor (`Σ len × size_of`,
/// capacity ≥ len), never a literal: sizes legitimately move with
/// later changes.
#[test]
fn test_mem_census_sanity() {
    let (tech, db, tracks, _join, graphs, vias) = load_routed_inputs("gcd");
    let graphs_b = grus_eda_core::drt::graph::route_graphs_bytes(&graphs);
    let topos_b =
        grus_eda_core::drt::maze::topos_footprint_bytes(&db, &tech, &tracks, &graphs, &vias);
    println!("gcd mem census: route_graphs={graphs_b} topos={topos_b}");
    let min_graphs: usize = graphs
        .nets
        .iter()
        .map(|g| {
            use grus_eda_core::drt::graph::{RouteEdge, RouteNode, ViaEdge};
            g.nodes.len() * std::mem::size_of::<RouteNode>()
                + g.edges.len() * std::mem::size_of::<RouteEdge>()
                + g.via_edges.len() * std::mem::size_of::<ViaEdge>()
        })
        .sum();
    assert!(graphs_b > 0, "route_graphs_bytes read zero");
    assert!(
        graphs_b >= min_graphs,
        "route_graphs_bytes {graphs_b} under the len floor {min_graphs}"
    );
    assert!(topos_b > 0, "topos_footprint_bytes read zero");
}

// ---------------------------------------------------------------------------
// Rayon determinism (drt-rayon)
//
// The drt parallel sections run inside a per-call thread pool whose
// size is an explicit `threads` input (`drt::pool`): the route legs
// read `MazeConfig::threads`, and `build_route_graphs_ext` /
// `apply_routes` / `write_routed_def` take a `threads` parameter —
// `0` is rayon's default. Build and emit are pinned byte-identical
// across thread counts; the route loop's contract is determinism
// under a *fixed* thread count plus a collision-free end state —
// serial identity across counts is a named non-goal (same-iteration
// searches run against the iteration-start snapshot, so the
// committed stream is a function of pool size).
// ---------------------------------------------------------------------------

/// Graph build folds per-net partials in `join.nets` order — the
/// emitted `RouteGraphs` must be byte-identical between a
/// single-threaded and a multi-threaded pool. The Debug-format
/// comparison covers every field including order.
#[test]
fn test_rayon_graphs_serial_identity() {
    let (tech, db, tracks, _) = load("gcd");
    let (_, join) = load_guide(&db, &tech, "gcd");
    let graphs = |threads: usize| {
        format!(
            "{:?}",
            grus_eda_core::drt::graph::build_route_graphs_ext(
                &db,
                &tech,
                &tracks,
                &join,
                grus_eda_core::drt::graph::GraphOpts {
                    spare_above: 1,
                    escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
                },
                threads,
            )
        )
    };
    assert_eq!(graphs(1), graphs(8), "graph build diverged under 8 threads");
}

/// The streamed attach pass's contract (`docs/e6-drt.md` §7
/// "Resident footprint"): `build_net_graph` per net is a pure
/// function of the shared inputs — occupancy never enters the build —
/// so the per-attempt rebuild is bit-identical to the bulk
/// `build_route_graphs_ext` output. Asserts `PartialEq` per net plus
/// the streamed `RouteGraphs` census (every field but `nets`, which
/// stays empty by construction), under both pool sizes the rebuild
/// can run inside.
#[test]
fn test_net_graph_rebuild_identity() {
    let (tech, db, tracks, _) = load("gcd");
    let (_, join) = load_guide(&db, &tech, "gcd");
    let vias = via_table(&tech, &db).unwrap();
    let opts = grus_eda_core::drt::graph::GraphOpts {
        spare_above: 1,
        escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
    };
    let bulk =
        grus_eda_core::drt::graph::build_route_graphs_ext(&db, &tech, &tracks, &join, opts, 0);
    assert_eq!(bulk.nets.len(), join.nets.len());
    let hull = HullMask::new(&db, &tech);
    for (i, &(net, ref boxes)) in join.nets.iter().enumerate() {
        let mut scratch = Vec::new();
        let nb = grus_eda_core::drt::graph::build_net_graph(
            &db, &tech, &tracks, &hull, net, boxes, opts, &mut scratch,
        );
        assert_eq!(nb.graph, bulk.nets[i], "rebuilt graph diverged at index {i}");
    }
    let mut streamed = Vec::new();
    for threads in [1, 8] {
        let (census, _route_db) = route_nets_with_progress(
            &db,
            &tech,
            &tracks,
            &join,
            opts,
            &vias,
            &MazeConfig {
                threads,
                ..MazeConfig::default()
            },
            None,
        );
        streamed.push(format!("{census:?}"));
    }
    assert_eq!(
        streamed[0], streamed[1],
        "streamed census diverged under 8 threads"
    );
    let bulk_census = format!("{:?}", {
        let mut b = bulk.clone();
        b.nets.clear();
        b
    });
    assert_eq!(streamed[0], bulk_census, "streamed census != bulk census");
}

/// The route loop's named invariant: snapshot-search / ordered-commit
/// makes the committed stream a pure function of the inputs, so the
/// output is deterministic at *any* thread count — threads=1,
/// threads=8, and a second 8-thread run must all agree on the
/// `route_digest` serialization (geometry, connectivity, iteration
/// stats), with a collision-free end state and an identical unrouted
/// census.
#[test]
fn test_rayon_route_determinism() {
    let leg = gcd_leg();
    let route = |threads: usize| {
        route_nets(
            &leg.db,
            &leg.tech,
            &leg.tracks,
            &leg.join,
            LOADED_OPTS,
            &leg.vias,
            &MazeConfig {
                threads,
                ..leg.cfg
            },
        )
    };
    let r1 = route(1);
    let r2 = route(8);
    let r3 = route(8);
    assert_ne!(route_digest(&r1), 0, "zero digest proves nothing");
    for (i, r) in [&r1, &r2, &r3].iter().enumerate() {
        assert!(
            r.collisions.is_empty(),
            "arm {i}: routed end state has {} residual collisions",
            r.collisions.len()
        );
    }
    assert_eq!(
        route_digest(&r1),
        route_digest(&r2),
        "threads=1 vs threads=8 route output diverged"
    );
    assert_eq!(
        route_digest(&r2),
        route_digest(&r3),
        "two 8-thread runs diverged"
    );
    assert_eq!(
        r1.unrouted, r2.unrouted,
        "unrouted census differs across thread counts"
    );
    println!(
        "gcd rayon route: digest={:#x} routed={} unrouted={} collisions={} iterations={}",
        route_digest(&r1),
        r1.nets.len(),
        r1.unrouted.len(),
        r1.collisions.len(),
        r1.iterations
    );
}

// ---------------------------------------------------------------------------
// Routed DEF emission + self-DRC gate
// ---------------------------------------------------------------------------

use grus_eda_core::drc::ShapeSrc;
use grus_eda_core::drc::geom::assemble;
use grus_eda_core::drc::report::{Family, SubsetReport, run_all};
use grus_eda_core::drt::emit::{EmitReport, apply_routes, preemit_check, write_routed_def};

/// Clone `leg.db`, apply the shared `leg.route` into it — the emit
/// half of the gate. Route re-execution is never part of emit; the
/// clone keeps `apply_routes`' `net_geoms` writes off shared state.
fn emit_leg(leg: &'static RoutedLeg) -> (DesignDb, EmitReport) {
    let mut db = leg.db.clone();
    let guided: Vec<NetId> = leg.join.nets.iter().map(|&(n, _)| n).collect();
    let report =
        apply_routes(&mut db, &leg.tech, &guided, &leg.route, &leg.vias, 0).expect("apply_routes");
    (db, report)
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

/// Emit → write → re-parse → `run_all` on the shared leg — the
/// shared body of the gcd/picorv32 gate tests. Returns the leg's
/// `RouteDb` so a caller can assert on the min-area patch census.
fn emit_gate(leg: &'static RoutedLeg) -> &'static RouteDb {
    let name = leg.name;
    let tech = &leg.tech;
    let (db, report) = emit_leg(leg);
    let route_db = &leg.route;
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
    print_emit_census(name, tech, &db, &report);

    let out = tmp_def(&format!("{name}-gate"));
    write_routed_def(&db, tech, &placed_def(name), &out, 0).expect("write routed DEF");
    let (db2, drc_report) = self_drc(&out, tech);
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
    let drc2 = assemble(&db2, tech).expect("assemble");
    assert_eq!(drc_report, run_all(&drc2, tech, &db2.strings));

    // Byte stability on the design under test itself: the re-parsed
    // geometry re-emits to the same bytes (`out` is its own template,
    // so this exercises the NETS-splice fixed point).
    let out2 = tmp_def(&format!("{name}-gate2"));
    write_routed_def(&db2, tech, &out, &out2, 0).expect("re-emit");
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
    let leg = gcd_leg();
    let tech = &leg.tech;
    let (db, report) = emit_leg(leg);
    assert_eq!(report.routed, leg.route.nets.len());
    // The unrouted set is pinned by the e2e golden — a drift names the
    // differing nets in the failure rather than re-baselining a count.
    let golden_unrouted: BTreeSet<String> = j_arr(
        &load_ours_ref("gcd")["ours_router"]["unrouted"],
        "ours_router.unrouted",
    )
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
    write_routed_def(&db, tech, &placed_def("gcd"), &out1, 0).expect("write out1");
    let db2 = parse_def(&out1, tech, None).expect("re-parse out1");
    assert_eq!(
        db2.net_geoms, db.net_geoms,
        "re-parsed net_geoms differ from applied geometry"
    );
    write_routed_def(&db2, tech, &out1, &out2, 0).expect("write out2");
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
    let route_db = emit_gate(gcd_leg());
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
    emit_gate(leg_for("picorv32"));
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
    let leg = gcd_leg();
    let tech = &leg.tech;
    let (mut db, _rep) = emit_leg(leg);
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
    write_routed_def(&db, tech, &placed_def("gcd"), &out, 0).expect("write");
    let (_db2, drc_report) = self_drc(&out, tech);
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
    let off_leg = gcd_occ_off_leg();
    let tech = &off_leg.tech;
    let (db, _rep) = emit_leg(off_leg);
    assert!(!off_leg.route.nets.is_empty());
    let out = tmp_def("gcd-fp-occ");
    write_routed_def(&db, tech, &placed_def("gcd"), &out, 0).expect("write");
    let (_db2, drc_report) = self_drc(&out, tech);
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
    let leg = gcd_leg();
    let tech = &leg.tech;
    let (db1, rep1) = emit_leg(leg);
    // Pass 2 routes fresh on the shared inputs — comparing
    // `leg.route` to itself would be reflexive.
    let r2 = route_fresh(leg);
    let mut db2 = leg.db.clone();
    let guided: Vec<NetId> = leg.join.nets.iter().map(|&(n, _)| n).collect();
    let rep2 = apply_routes(&mut db2, tech, &guided, &r2, &leg.vias, 0).expect("apply_routes");
    assert!(!leg.route.nets.is_empty());
    assert_eq!(leg.route, r2, "nondeterministic routes");
    assert_eq!(rep1.wire_length_dbu, rep2.wire_length_dbu);
    assert_eq!(rep1.vias_per_cut, rep2.vias_per_cut);
    assert_eq!(db1.net_geoms, db2.net_geoms);

    let out1 = tmp_def("gcd-det1");
    let out2 = tmp_def("gcd-det2");
    let placed = placed_def("gcd");
    write_routed_def(&db1, tech, &placed, &out1, 0).expect("write 1");
    write_routed_def(&db2, tech, &placed, &out2, 0).expect("write 2");
    assert_eq!(
        std::fs::read(&out1).unwrap(),
        std::fs::read(&out2).unwrap(),
        "two emit passes produced different bytes"
    );
}

/// Emit is byte-identical across thread counts (drt-rayon): the
/// `NetGeom` partials and the `NETS` section strings fold in input
/// order, so `net_geoms`, the `EmitReport` counters, and the emitted
/// DEF bytes must match between a single-threaded and an 8-thread
/// pool on the same `RouteDb`.
#[test]
fn test_rayon_emit_serial_identity() {
    // The leg resolves before any forced-thread arm; the shared
    // `leg.route` feeds both apply arms.
    let leg = gcd_leg();
    let tech = &leg.tech;
    let mut db1 = leg.db.clone();
    let mut db2 = leg.db.clone();
    assert!(!leg.route.nets.is_empty());
    let guided: Vec<NetId> = leg.join.nets.iter().map(|&(n, _)| n).collect();

    let rep1 =
        apply_routes(&mut db1, tech, &guided, &leg.route, &leg.vias, 1).expect("apply_routes serial");
    let rep8 =
        apply_routes(&mut db2, tech, &guided, &leg.route, &leg.vias, 8).expect("apply_routes par8");
    assert_eq!(rep1.wire_length_dbu, rep8.wire_length_dbu);
    assert_eq!(rep1.vias_per_cut, rep8.vias_per_cut);
    assert_eq!(rep1.unrouted, rep8.unrouted);
    assert_eq!(
        db1.net_geoms, db2.net_geoms,
        "net_geoms diverged under 8 threads"
    );

    let placed = placed_def("gcd");
    let out1 = tmp_def("gcd-rayon-e1");
    let out8 = tmp_def("gcd-rayon-e8");
    write_routed_def(&db1, tech, &placed, &out1, 1).expect("write serial");
    write_routed_def(&db2, tech, &placed, &out8, 8).expect("write par8");
    assert_eq!(
        std::fs::read(&out1).unwrap(),
        std::fs::read(&out8).unwrap(),
        "emitted DEF bytes diverged under 8 threads"
    );
}

/// The pre-emit check on clean gcd: zero findings over nonzero
/// coverage, deterministic, `IoPort` rects present in the assembled
/// set — and the load-bearing equivalence: the in-memory report equals
/// the write → re-parse → `run_all` report verbatim, which is what
/// "the checked set is the emitted set" means (AGENTS §7.2 — tested,
/// not asserted in prose).
#[test]
fn test_preemit_gcd() {
    let leg = gcd_leg();
    let tech = &leg.tech;
    let (db, _rep) = emit_leg(leg);
    let pre = preemit_check(&db, tech).expect("preemit_check");
    assert_eq!(
        findings_total(&pre),
        0,
        "clean gcd carries pre-emit findings: {}",
        pre.rules
            .iter()
            .filter(|o| !o.findings.is_empty())
            .map(|o| format!("{}:{}", o.rule, o.findings.len()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    for family in [
        Family::Width,
        Family::Spacing,
        Family::Enclosure,
        Family::Area,
    ] {
        let candidates: usize = pre
            .rules
            .iter()
            .filter(|o| o.family == family)
            .map(|o| o.candidates)
            .sum();
        assert!(candidates > 0, "gcd: {family:?} family checked nothing");
    }
    assert_eq!(pre, preemit_check(&db, tech).expect("preemit_check 2"));

    let io_ports = assemble(&db, tech)
        .expect("assemble")
        .routing
        .iter()
        .flat_map(|ls| ls.rects.iter())
        .filter(|s| s.src == ShapeSrc::IoPort)
        .count();
    assert!(io_ports > 0, "gcd emit carries no IoPort shapes");

    let out = tmp_def("gcd-preemit");
    write_routed_def(&db, tech, &placed_def("gcd"), &out, 0).expect("write");
    let (_db2, drc_report) = self_drc(&out, tech);
    assert_eq!(
        pre, drc_report,
        "in-memory report diverges from the emit→parse report — a writer bug"
    );
}

/// Fail-proof arm for the pre-emit check: a `SignalWire` injected at a
/// 1-DBU gap to a foreign io-port rect must surface as a
/// `Family::Spacing` finding that names the port's rect and net —
/// `IoPort` is a checkable endpoint in the emitted set, not just
/// present geometry.
#[test]
fn test_preemit_foreign_pin() {
    let leg = gcd_leg();
    let tech = &leg.tech;
    let (mut db, _rep) = emit_leg(leg);
    let d = assemble(&db, tech).expect("assemble");
    let (lid, pin_rect, port_net) = d
        .routing
        .iter()
        .enumerate()
        .flat_map(|(l, ls)| ls.rects.iter().map(move |s| (l, s)))
        .find_map(|(l, s)| {
            (s.src == ShapeSrc::IoPort && s.net.is_some())
                .then(|| (LayerId(l as u8), s.rect, s.net.unwrap()))
        })
        .expect("gcd emit carries net-tagged io-port shapes");
    let bound = tech.routing_layers[lid.0 as usize].min_spacing_dbu();
    assert!(bound > 1, "io-port layer has no spacing bound to violate");
    let b = (0..db.num_nets())
        .find(|&n| db.net_names[n] != port_net)
        .expect("a foreign net exists");
    let w = RectDbu::new(
        pin_rect.xh + 1,
        pin_rect.yl,
        pin_rect.xh + 1 + bound,
        pin_rect.yh,
    );
    println!(
        "injected: net {} wire {w:?} on layer {lid:?} at gap 1 < bound {bound} of port rect {pin_rect:?}",
        net_name(&db, NetId(b as u32)),
    );
    db.net_geoms[b].wire_rects.push((lid, w));

    let pre = preemit_check(&db, tech).expect("preemit_check");
    print_gate("gcd(preemit-injected)", &pre);
    assert!(
        findings_total(&pre) >= 1,
        "io-port-adjacent inject produced no finding — the check is vacuous"
    );
    let port_name = db.strings.resolve(port_net).to_string();
    let hit = pre
        .rules
        .iter()
        .filter(|o| o.family == Family::Spacing)
        .flat_map(|o| o.findings.iter())
        .find(|f| f.bboxes.contains(&pin_rect) && f.nets.iter().any(|n| n == &port_name));
    assert!(
        hit.is_some(),
        "no spacing finding names the io-port rect {pin_rect:?} and net {port_name}"
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
    let vias = via_table(&tech, &db).unwrap();
    let route_db = route_nets(
        &db,
        &tech,
        &tracks,
        &join,
        GRAPHS_OPTS,
        &vias,
        &MazeConfig::default(),
    );
    assert_eq!(route_db.unrouted.len(), 1, "{:?}", route_db.unrouted);
    let guided: Vec<NetId> = join.nets.iter().map(|&(n, _)| n).collect();
    let report = apply_routes(&mut db, &tech, &guided, &route_db, &vias, 0).expect("apply");
    assert_eq!(report.unrouted, vec!["n3".to_string()]);

    // The fixture's own text is the splice template.
    let template = tmp_def("fixture-template");
    std::fs::write(&template, &def_text).unwrap();
    let out = tmp_def("fixture-unrouted");
    write_routed_def(&db, &tech, &template, &out, 0).expect("write");
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
const PINNED_LEF_RESIDUALS: &[(&str, &str, i64)] =
    &[("picorv32", "li.3", 19), ("picorv32", "met1.2", 146)];

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

fn j_obj<'a>(
    v: &'a serde_json::Value,
    ctx: &str,
) -> &'a serde_json::Map<String, serde_json::Value> {
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
        let rid = magic_rule_id(tmpl)
            .unwrap_or_else(|| panic!("{name}: lef_expressible template has no rule id: {tmpl}"));
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
    let leg = gcd_leg();
    let (db, _rep) = emit_leg(leg);
    let out = tmp_def("gcd-ours-det");
    write_routed_def(&db, &leg.tech, &placed_def("gcd"), &out, 0).expect("write routed DEF");
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
    let leg = gcd_leg();
    let (db, _rep) = emit_leg(leg);
    let vias = &leg.vias;
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

