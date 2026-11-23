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

use grus_core::design_db::DesignDb;
use grus_core::drt::access::{AccessReport, access_points};
use grus_core::drt::tracks::{Axis, LayerTracks, layer_tracks, track_gaps};
use grus_core::interner::{PinId, StringInterner};
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
