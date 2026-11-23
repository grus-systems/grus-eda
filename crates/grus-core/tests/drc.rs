//! E7 DRC gate: the geometry substrate + width rule family on the
//! committed routed DEFs of gcd and picorv32, checked against the Magic
//! `drc(full)` goldens (`<design>.drc.reference.json`).
//!
//! What this proves:
//! - `NETS` routed geometry parses: `net_geoms` is populated parallel to
//!   `net_names`, and every routed via in both designs resolves.
//! - Assembly is deterministic and emits all six shape sources.
//! - `check_width` reports zero violations for the 13 implemented rules
//!   on both designs — and Magic itself reports no `magic_per_rule`
//!   entry for any of them (same verdict, independent checker).
//! - A one-character mutation (`NEW met1 480` → `NEW met1 100` on one
//!   stripe) produces exactly one `met1.1` violation, so the zero is not
//!   a vacuous pass.
//!
//! Tests fail loudly on missing inputs — no silent skips.

use grus_core::drc::geom::assemble;
use grus_core::drc::spacing::{SPACING_RULES, check_spacing, check_spacing_report};
use grus_core::drc::width::{Violation, WIDTH_RULES, check_width};
use grus_core::drc::{DrcDesign, DrcLayer, ShapeSrc};
use grus_core::design_db::DesignDb;
use grus_core::interner::StringInterner;
use grus_core::readers::{load_tech_db, parse_def, parse_def_bytes};
use grus_core::tech_db::TechDb;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Inputs
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

// ---------------------------------------------------------------------------
// Loaded design
// ---------------------------------------------------------------------------

struct Loaded {
    tech: TechDb,
    design: DesignDb,
    drc: DrcDesign,
    golden: serde_json::Value,
}

fn load(name: &str) -> Loaded {
    let lib_dir = sky130_hd();
    let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
    let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
    let def = require(design_dir(name).join(format!("{name}.routed.def")));
    let golden_path = require(design_dir(name).join(format!("{name}.drc.reference.json")));

    let mut interner = StringInterner::new();
    let tech = load_tech_db(&tlef, &lef, &mut interner)
        .expect("load_tech_db")
        .0;
    let design = parse_def(&def, &tech, None).expect("parse_def");
    let drc = assemble(&design, &tech).expect("assemble");
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&golden_path).expect("read golden"),
    )
    .expect("golden JSON parses");
    Loaded {
        tech,
        design,
        drc,
        golden,
    }
}

/// Per-layer shape census, printed for `docs/results.md`.
fn print_census(name: &str, tech: &TechDb, drc: &DrcDesign) {
    println!("== {name} layer census ==");
    for (i, ls) in drc.routing.iter().enumerate() {
        let lname = tech.strings.resolve(tech.routing_layers[i].name);
        let counts = src_counts(&ls.rects.iter().map(|s| s.src).collect::<Vec<_>>());
        println!("  {lname:6} shapes={:6} {counts}", ls.rects.len());
    }
    for (i, ls) in drc.cut.iter().enumerate() {
        let cname = tech.strings.resolve(tech.cut_layers[i].name);
        println!("  {cname:6} shapes={:6} (cut)", ls.rects.len());
    }
}

fn src_counts(srcs: &[ShapeSrc]) -> String {
    let mut parts = Vec::new();
    for src in [
        ShapeSrc::SignalWire,
        ShapeSrc::SpecialWire,
        ShapeSrc::Via,
        ShapeSrc::MacroPin,
        ShapeSrc::MacroObs,
        ShapeSrc::IoPort,
    ] {
        let n = srcs.iter().filter(|&&s| s == src).count();
        if n > 0 {
            parts.push(format!("{src:?}={n}"));
        }
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Geometry retention + determinism
// ---------------------------------------------------------------------------

/// `net_geoms` is populated parallel to `net_names`, with real geometry:
/// both designs carry signal wire rects and resolved routed vias.
#[test]
fn test_nets_geometry_retained() {
    for name in ["gcd", "picorv32"] {
        let l = load(name);
        assert_eq!(
            l.design.net_geoms.len(),
            l.design.num_nets(),
            "{name}: net_geoms must parallel net_names"
        );
        let wires: usize = l
            .design
            .net_geoms
            .iter()
            .map(|g| g.wire_rects.len())
            .sum();
        let vias: usize = l.design.net_geoms.iter().map(|g| g.vias.len()).sum();
        println!("{name}: nets={} wire_rects={wires} routed_vias={vias}", l.design.num_nets());
        assert!(wires > 0, "{name}: routed signal wires retained");
        assert!(vias > 0, "{name}: routed vias resolved");
        // Every geometry layer index is a real routing layer.
        for g in &l.design.net_geoms {
            for (lid, r) in &g.wire_rects {
                assert!((lid.0 as usize) < l.tech.routing_layers.len());
                assert!(r.xl < r.xh && r.yl < r.yh);
            }
        }
        print_census(name, &l.tech, &l.drc);
    }
}

/// Reparsing the same DEF yields identical `net_geoms`; reassembling the
/// same DB yields an identical `DrcDesign`.
#[test]
fn test_drc_parse_and_assemble_deterministic() {
    for name in ["gcd", "picorv32"] {
        let lib_dir = sky130_hd();
        let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
        let def = require(design_dir(name).join(format!("{name}.routed.def")));
        let bytes = std::fs::read(&def).unwrap();
        let mut interner = StringInterner::new();
        let tech = load_tech_db(&tlef, &lef, &mut interner)
            .expect("load_tech_db")
            .0;
        let d1 = parse_def_bytes(&bytes, &tech, None).unwrap();
        let d2 = parse_def_bytes(&bytes, &tech, None).unwrap();
        assert_eq!(d1.net_geoms, d2.net_geoms, "{name}: reparse diverges");
        assert_eq!(
            assemble(&d1, &tech).unwrap(),
            assemble(&d2, &tech).unwrap(),
            "{name}: assembly is nondeterministic"
        );
    }
}

// ---------------------------------------------------------------------------
// Width gate vs Magic golden
// ---------------------------------------------------------------------------

/// Zero width violations on both committed routed DEFs — the same verdict
/// Magic's `drc(full)` reached (`magic_per_rule` lists no width rule).
#[test]
fn test_width_zero_violations() {
    for name in ["gcd", "picorv32"] {
        let l = load(name);
        let v = check_width(&l.drc, &l.tech);
        for viol in &v {
            println!("{name}: {viol:?}");
        }
        assert!(v.is_empty(), "{name}: {} width violations", v.len());
    }
}

/// `(id)` tokens inside a `magic_per_rule` key's parens — the keys
/// carry resolved values ("< 0.14um"), so the join splits the paren
/// text into tokens.
fn paren_tokens(key: &str) -> Vec<&str> {
    let paren = &key[key.rfind('(').unwrap()..];
    paren
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .collect()
}

/// The join convention: every implemented `rule_name` is a verbatim
/// `magic_rules_checked.names` template, and no `magic_per_rule` key
/// carries any implemented rule's `(id)`.
#[test]
fn test_width_rules_join_golden() {
    for name in ["gcd", "picorv32"] {
        let l = load(name);
        let names: Vec<&str> = l.golden["magic_rules_checked"]["names"]
            .as_array()
            .expect("magic_rules_checked.names")
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        for r in WIDTH_RULES {
            assert!(
                names.contains(&r.rule_name),
                "{name}: golden names lack `{}`",
                r.rule_name
            );
            assert_eq!(
                l.golden["magic_rules_checked"]["classification"][r.rule_name],
                "lef_expressible",
                "{name}: `{}` is not lef_expressible in the golden taxonomy",
                r.rule_name
            );
        }
        let per_rule = l.golden["magic_per_rule"]
            .as_object()
            .expect("magic_per_rule");
        for key in per_rule.keys() {
            let tokens = paren_tokens(key);
            for r in WIDTH_RULES {
                assert!(
                    !tokens.contains(&r.rule),
                    "{name}: magic_per_rule reports `{}` via `{key}`",
                    r.rule
                );
            }
        }
    }
}

/// Mutation gate: narrowing one `NEW met1 480` stripe to 100 DBU must
/// produce exactly one `met1.1` violation (measured 100, bound 140).
#[test]
fn test_width_mutation_met1() {
    let lib_dir = sky130_hd();
    let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
    let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
    let def = require(design_dir("gcd").join("gcd.routed.def"));
    let mut interner = StringInterner::new();
    let tech = load_tech_db(&tlef, &lef, &mut interner)
        .expect("load_tech_db")
        .0;

    let text = std::fs::read_to_string(&def).unwrap();
    let mutated = text.replacen("NEW met1 480", "NEW met1 100", 1);
    assert_ne!(text, mutated, "fixture must contain `NEW met1 480`");
    let db = parse_def_bytes(mutated.as_bytes(), &tech, None).unwrap();
    let drc = assemble(&db, &tech).unwrap();
    let all = check_width(&drc, &tech);
    let v: Vec<&Violation> = all.iter().filter(|v| v.rule == "met1.1").collect();
    assert_eq!(v.len(), 1, "exactly one met1.1 violation, got {v:?}");
    assert_eq!(all.len(), 1, "no other rule may fire, got {all:?}");
    assert_eq!(v[0].measured_dbu, 100);
    assert_eq!(v[0].bound_dbu, 140);
    assert_eq!(
        v[0].net.map(|n| db.strings.resolve(n).to_string()),
        Some("VGND".to_string()),
        "the violated stripe is the first NEW met1 of VGND"
    );
    let met1 = tech.layer_by_name[&tech.strings.get("met1").unwrap()];
    assert_eq!(v[0].layer, DrcLayer::Routing(met1));
}

// ---------------------------------------------------------------------------
// Spacing gate vs Magic golden
// ---------------------------------------------------------------------------

fn layer_name(tech: &TechDb, layer: DrcLayer) -> String {
    match layer {
        DrcLayer::Routing(l) => tech
            .strings
            .resolve(tech.routing_layers[l.0 as usize].name)
            .to_string(),
        DrcLayer::Cut(c) => tech
            .strings
            .resolve(tech.cut_layers[c as usize].name)
            .to_string(),
    }
}

/// Zero spacing violations on both committed routed DEFs — the same
/// verdict Magic's `drc(full)` reached (`magic_per_rule` lists no
/// spacing rule). The census names rules a design does not exercise
/// (zero candidates is not coverage).
#[test]
fn test_spacing_zero_violations() {
    for name in ["gcd", "picorv32"] {
        let l = load(name);
        let reports = check_spacing_report(&l.drc, &l.tech);
        println!("== {name} spacing census ==");
        for r in &reports {
            let lname = r
                .layer
                .map(|dl| layer_name(&l.tech, dl))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "  {:8} layer={lname:5} bound={:5} candidates={:6} violations={}",
                r.rule,
                r.bound_dbu,
                r.candidates,
                r.violations.len()
            );
        }
        for r in &reports {
            for v in &r.violations {
                println!("{name}: {v:?}");
            }
        }
        assert!(
            reports.iter().all(|r| r.violations.is_empty()),
            "{name}: {} spacing violations",
            reports.iter().map(|r| r.violations.len()).sum::<usize>()
        );
        // Deterministic double-run: islands, candidate-pair census and
        // pair emission are input-order stable — compare the report
        // (which carries nonzero `candidates`), not just the empty
        // violation vec.
        let (ra, rb) = (
            check_spacing_report(&l.drc, &l.tech),
            check_spacing_report(&l.drc, &l.tech),
        );
        assert_eq!(ra, rb, "{name}: spacing check is nondeterministic");
        assert!(
            ra.iter().map(|r| r.candidates).sum::<usize>() > 0,
            "{name}: zero candidate pairs — the pipeline ran on nothing"
        );
    }
}

/// Every implemented spacing `rule_name` is a verbatim
/// `magic_rules_checked.names` template classified `lef_expressible`,
/// and no `magic_per_rule` key carries any implemented `(id)`.
#[test]
fn test_spacing_rules_join_golden() {
    for name in ["gcd", "picorv32"] {
        let l = load(name);
        let names: Vec<&str> = l.golden["magic_rules_checked"]["names"]
            .as_array()
            .expect("magic_rules_checked.names")
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        for r in SPACING_RULES {
            assert!(
                names.contains(&r.rule_name),
                "{name}: golden names lack `{}`",
                r.rule_name
            );
            assert_eq!(
                l.golden["magic_rules_checked"]["classification"][r.rule_name],
                "lef_expressible",
                "{name}: `{}` is not lef_expressible in the golden taxonomy",
                r.rule_name
            );
        }
        let per_rule = l.golden["magic_per_rule"]
            .as_object()
            .expect("magic_per_rule");
        for key in per_rule.keys() {
            let tokens = paren_tokens(key);
            for r in SPACING_RULES {
                assert!(
                    !tokens.contains(&r.rule),
                    "{name}: magic_per_rule reports `{}` via `{key}`",
                    r.rule
                );
            }
        }
    }
}

/// Mutation gate, golden-driven: replay `sanity_control.mutation` —
/// the committed control appended `NEW met2 ( 10520 48450 ) ( 10520
/// 48950 )` to net `_001_` at a 0.03 µm gap from `_000_`'s wire and
/// Magic flagged `Metal2 spacing < 0.14um (met2.2)`. Our checker must
/// flag exactly `met2.2` (our count unit is rect pairs, not merged
/// regions).
#[test]
fn test_spacing_mutation_met2_probe() {
    let lib_dir = sky130_hd();
    let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
    let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
    let def = require(design_dir("gcd").join("gcd.routed.def"));
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(require(design_dir("gcd").join("gcd.drc.reference.json")))
            .expect("read golden"),
    )
    .expect("golden JSON parses");
    let mut interner = StringInterner::new();
    let tech = load_tech_db(&tlef, &lef, &mut interner)
        .expect("load_tech_db")
        .0;

    let m = &golden["sanity_control"]["mutation"];
    let net_b = m["net_b"].as_str().unwrap();
    let layer = m["layer"].as_str().unwrap();
    let seg: Vec<i32> = m["segment_dbu"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let half = m["layer_width_dbu"].as_i64().unwrap() as i32 / 2;
    let probe_rect = grus_core::tech_db::RectDbu::new(
        seg[0].min(seg[2]) - half,
        seg[1].min(seg[3]) - half,
        seg[0].max(seg[2]) + half,
        seg[1].max(seg[3]) + half,
    );

    let text = std::fs::read_to_string(&def).unwrap();
    // The control's magic_per_rule verdict names only met2.2 as new.
    assert_eq!(
        golden["sanity_control"]["new_rules_vs_clean"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        golden["sanity_control"]["new_rules_vs_clean"][0]
            .as_str()
            .unwrap()
            .contains("(met2.2)")
    );

    let anchor = format!("- {net_b} ");
    let net_at = text
        .find(&anchor)
        .unwrap_or_else(|| panic!("{anchor} must anchor a NETS record"));
    let end_at = text[net_at..]
        .find(';')
        .map(|i| net_at + i)
        .expect("net record terminates with `;`");
    let stmt = format!("      NEW {layer} ( {} {} ) ( {} {} )\n", seg[0], seg[1], seg[2], seg[3]);
    let mutated = format!("{}{}{}", &text[..end_at], stmt, &text[end_at..]);
    assert_ne!(text, mutated, "mutation must change the DEF text");

    let db = parse_def_bytes(mutated.as_bytes(), &tech, None).unwrap();
    let drc = assemble(&db, &tech).unwrap();
    let all = check_spacing(&drc, &tech);
    assert!(!all.is_empty(), "the probe must produce violations");
    for v in &all {
        println!("{v:?}");
    }
    assert!(
        all.iter().all(|v| v.rule == "met2.2"),
        "only met2.2 may fire, got {} other",
        all.iter().filter(|v| v.rule != "met2.2").count()
    );
    assert!(
        all.iter().any(|v| v.a == probe_rect || v.b == probe_rect),
        "a violation must contain the probe rect {probe_rect:?}, got {all:?}"
    );

    // Control: the unmutated DEF is clean.
    let db = parse_def_bytes(text.as_bytes(), &tech, None).unwrap();
    let drc = assemble(&db, &tech).unwrap();
    assert!(check_spacing(&drc, &tech).is_empty());
}
