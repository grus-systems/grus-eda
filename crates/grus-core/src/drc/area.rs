//! The area family: 6 minimum-area rules over merged plane polygons
//! (`drc::union::components` + `union_area`) and 2 minimum-hole-area
//! rules over `enclosed_voids`. Bounds are the routing-layer `AREA`
//! / `MINENCLOSEDAREA` values retained in `RoutingLayer`; `rule_name`
//! strings are verbatim `magic_rules_checked.names` templates.
//!
//! A component is *evaluated* only when it holds at least one drawn
//! member (`SignalWire`/`SpecialWire`/`Via`/`IoPort`): `MacroPin`-only
//! islands are pin abstracts that understate the real GDS, so
//! flagging them can only over-report — same class as the width
//! family's finding filter.

use crate::drc::union;
use crate::drc::{DrcDesign, DrcLayer, LayerShapes, ShapeSrc};
use crate::interner::SymbolId;
use crate::tech_db::{RectDbu, TechDb};

/// Deck-pinned fallback for `MINENCLOSEDAREA` — the `rule_name`
/// literals themselves carry "0.14um^2", so a LEF that omits the
/// statement still checks 0.14 µm², not a guessed value.
const HOLE_FALLBACK_DBU2: i64 = 140_000;

/// One minimum-area or minimum-hole-area rule.
pub struct AreaRule {
    /// First `(id)` token inside `rule_name`'s parens — join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template.
    pub rule_name: &'static str,
    /// LEF routing-layer name.
    pub layer: &'static str,
}

/// The 6 minimum-area rules, `lef-rules.md` table order.
pub const AREA_RULES: &[AreaRule] = &[
    AreaRule {
        rule: "li.6",
        rule_name: "Local interconnect minimum area < %a (li.6)",
        layer: "li1",
    },
    AreaRule {
        rule: "met1.6",
        rule_name: "Metal1 minimum area < %a (met1.6)",
        layer: "met1",
    },
    AreaRule {
        rule: "met2.6",
        rule_name: "Metal2 minimum area < %a (met2.6)",
        layer: "met2",
    },
    AreaRule {
        rule: "met3.6",
        rule_name: "Metal3 minimum area < %a (met3.6)",
        layer: "met3",
    },
    AreaRule {
        rule: "met4.4a",
        rule_name: "Metal4 minimum area < %a (met4.4a)",
        layer: "met4",
    },
    AreaRule {
        rule: "met5.4",
        rule_name: "Metal5 minimum area < %a (met5.4)",
        layer: "met5",
    },
];

/// The 2 minimum-hole-area rules.
pub const HOLE_RULES: &[AreaRule] = &[
    AreaRule {
        rule: "met1.7",
        rule_name: "Min area of metal1 holes > 0.14um^2 (met1.7)",
        layer: "met1",
    },
    AreaRule {
        rule: "met2.7",
        rule_name: "Min area of metal2 holes > 0.14um^2 (met2.7)",
        layer: "met2",
    },
];

/// A sub-bound merged polygon (min-area) or enclosed void (holes).
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// `(id)` inside the rule_name parens — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template.
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// Component bbox (min-area) or void bbox (holes).
    pub region: RectDbu,
    pub area_dbu2: i64,
    pub bound_dbu2: i64,
    /// First net-tagged member's net, when any — diagnostics.
    pub net: Option<SymbolId>,
}

/// Per-rule outcome for the zero-violation census. `candidates` is
/// evaluated components for min-area, detected enclosed voids for
/// holes.
#[derive(Debug, PartialEq)]
pub struct RuleReport {
    pub rule: &'static str,
    /// `None` when the rule's layer is absent from the loaded LEF.
    pub layer: Option<DrcLayer>,
    /// The area bound applied; 0 when `AREA` is undeclared (min-area)
    /// — holes fall back to the deck-pinned 140000, never 0.
    pub bound_dbu2: i64,
    pub candidates: usize,
    pub violations: Vec<Violation>,
}

/// Sources that count as drawn metal for the evaluation gate —
/// `MacroPin` makes a component *real* (it merges, it fills a hole's
/// solid set) but cannot be the reason one is checked. `drt::maze`'s
/// min-area patch reuses the same gate for its component model.
pub(crate) fn is_drawn(src: ShapeSrc) -> bool {
    matches!(
        src,
        ShapeSrc::SignalWire | ShapeSrc::SpecialWire | ShapeSrc::Via | ShapeSrc::IoPort
    )
}

fn bbox_of(ls: &LayerShapes, members: &[u32]) -> RectDbu {
    let mut b = ls.rects[members[0] as usize].rect;
    for &i in &members[1..] {
        let r = ls.rects[i as usize].rect;
        b = RectDbu::new(
            b.xl.min(r.xl),
            b.yl.min(r.yl),
            b.xh.max(r.xh),
            b.yh.max(r.yh),
        );
    }
    b
}

/// Component evaluation gate + shared pieces (member rects, drawn
/// flag, first net). `None` components fail the gate.
struct EvalComp {
    members: Vec<u32>,
    net: Option<SymbolId>,
}

fn drawn_components(ls: &LayerShapes) -> Vec<EvalComp> {
    union::components(ls)
        .members
        .into_iter()
        .filter_map(|members| {
            let drawn = members
                .iter()
                .any(|&i| is_drawn(ls.rects[i as usize].src));
            drawn.then(|| EvalComp {
                net: members
                    .iter()
                    .find_map(|&i| ls.rects[i as usize].net),
                members,
            })
        })
        .collect()
}

/// Every resolvable `AREA_RULES` rule over `design`. A rule whose
/// layer is absent or whose `AREA` is undeclared (`area_dbu2 <= 0`)
/// contributes nothing — the `bound == 0` skip convention. Comparison
/// is strict `<` on exact integer areas.
pub fn check_area_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport> {
    AREA_RULES
        .iter()
        .map(|rule| {
            let lid = tech
                .strings
                .get(rule.layer)
                .and_then(|s| tech.layer_by_name.get(&s))
                .copied();
            let mut report = RuleReport {
                rule: rule.rule,
                layer: lid.map(DrcLayer::Routing),
                bound_dbu2: 0,
                candidates: 0,
                violations: Vec::new(),
            };
            let Some(lid) = lid else {
                return report;
            };
            let bound = tech.routing_layers[lid.0 as usize].area_dbu2;
            report.bound_dbu2 = bound;
            if bound <= 0 {
                return report;
            }
            let ls = &design.routing[lid.0 as usize];
            for comp in drawn_components(ls) {
                report.candidates += 1;
                let rects: Vec<RectDbu> = comp
                    .members
                    .iter()
                    .map(|&i| ls.rects[i as usize].rect)
                    .collect();
                let area = union::union_area(&rects);
                if area < bound {
                    report.violations.push(Violation {
                        rule: rule.rule,
                        rule_name: rule.rule_name,
                        layer: DrcLayer::Routing(lid),
                        region: bbox_of(ls, &comp.members),
                        area_dbu2: area,
                        bound_dbu2: bound,
                        net: comp.net,
                    });
                }
            }
            report
        })
        .collect()
}

/// Flat violation list across all 6 min-area rules.
pub fn check_area(design: &DrcDesign, tech: &TechDb) -> Vec<Violation> {
    check_area_report(design, tech)
        .into_iter()
        .flat_map(|r| r.violations)
        .collect()
}

/// Every resolvable `HOLE_RULES` rule. Per drawn component: `solid` =
/// every member rect overlapping the component bbox (any component,
/// any net — a foreign wire inside a donut occupies void space),
/// `enclosed_voids` finds candidates, and a void whose free rects
/// closed-intersect (touch counts) any `MacroObs` rect on the layer
/// is discarded — real content hides behind the obs abstract, and
/// keeping it can only over-report.
pub fn check_hole_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport> {
    HOLE_RULES
        .iter()
        .map(|rule| {
            let lid = tech
                .strings
                .get(rule.layer)
                .and_then(|s| tech.layer_by_name.get(&s))
                .copied();
            let mut report = RuleReport {
                rule: rule.rule,
                layer: lid.map(DrcLayer::Routing),
                bound_dbu2: 0,
                candidates: 0,
                violations: Vec::new(),
            };
            let Some(lid) = lid else {
                return report;
            };
            let rl = &tech.routing_layers[lid.0 as usize];
            let bound = if rl.min_enclosed_area_dbu2 > 0 {
                rl.min_enclosed_area_dbu2
            } else {
                HOLE_FALLBACK_DBU2
            };
            report.bound_dbu2 = bound;
            let ls = &design.routing[lid.0 as usize];
            for comp in drawn_components(ls) {
                let window = bbox_of(ls, &comp.members);
                // `iter_overlapping` is strict, but every candidate
                // piece sits strictly inside the window except obs
                // rects — fetched the same way; a non-degenerate obs
                // touching a void edge still intrudes past the edge.
                let solid: Vec<RectDbu> = ls
                    .iter_overlapping(&window)
                    .filter(|s| union::is_member(s.src))
                    .map(|s| s.rect)
                    .collect();
                let obs: Vec<RectDbu> = ls
                    .iter_overlapping(&window)
                    .filter(|s| s.src == ShapeSrc::MacroObs)
                    .map(|s| s.rect)
                    .collect();
                for hole in union::enclosed_voids(&solid, window) {
                    let obs_hit = hole.rects.iter().any(|f| {
                        obs.iter().any(|o| {
                            f.xl <= o.xh && f.xh >= o.xl && f.yl <= o.yh && f.yh >= o.yl
                        })
                    });
                    if obs_hit {
                        continue;
                    }
                    report.candidates += 1;
                    if hole.area_dbu2 < bound {
                        report.violations.push(Violation {
                            rule: rule.rule,
                            rule_name: rule.rule_name,
                            layer: DrcLayer::Routing(lid),
                            region: hole.bbox,
                            area_dbu2: hole.area_dbu2,
                            bound_dbu2: bound,
                            net: comp.net,
                        });
                    }
                }
            }
            report
        })
        .collect()
}

/// Flat violation list across both hole rules.
pub fn check_holes(design: &DrcDesign, tech: &TechDb) -> Vec<Violation> {
    check_hole_report(design, tech)
        .into_iter()
        .flat_map(|r| r.violations)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc::ShapeRect;
    use crate::interner::StringInterner;
    use crate::readers::lef::parse_lef_bytes;

    /// met1/met2 routing with the sky130 `AREA`/`MINENCLOSEDAREA`
    /// values; `li1` declares no `AREA` → `area_dbu2 == 0` skip.
    const A_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
LAYER li1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.23 ;
  WIDTH 0.17 ;
END li1
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
  AREA 0.083 ;
  MINENCLOSEDAREA 0.14 ;
END met1
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
  AREA 0.0676 ;
  MINENCLOSEDAREA 0.14 ;
END met2
END LIBRARY
"#;

    /// routing = [li1, met1, met2].
    fn a_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        parse_lef_bytes(A_LEF.as_bytes(), &mut db, &mut interner).expect("LEF must parse");
        db.strings = interner;
        for net in ["n1", "n2"] {
            db.strings.intern(net);
        }
        db.rebuild_maps();
        db
    }

    fn sh(rect: RectDbu, src: ShapeSrc) -> ShapeRect {
        ShapeRect {
            rect,
            src,
            net: None,
            group: 0,
        }
    }

    /// Layer order in A_LEF: li1=0, met1=1, met2=2.
    fn design(layers: [Vec<ShapeRect>; 3]) -> DrcDesign {
        DrcDesign {
            routing: layers.into_iter().map(LayerShapes::from_rects).collect(),
            cut: vec![],
        }
    }

    /// Rule ids / verbatim names / layers — the golden join key, pinned.
    #[test]
    fn test_area_rules_metadata() {
        let expect: &[(&str, &str, &str)] = &[
            ("li.6", "Local interconnect minimum area < %a (li.6)", "li1"),
            ("met1.6", "Metal1 minimum area < %a (met1.6)", "met1"),
            ("met2.6", "Metal2 minimum area < %a (met2.6)", "met2"),
            ("met3.6", "Metal3 minimum area < %a (met3.6)", "met3"),
            ("met4.4a", "Metal4 minimum area < %a (met4.4a)", "met4"),
            ("met5.4", "Metal5 minimum area < %a (met5.4)", "met5"),
        ];
        assert_eq!(AREA_RULES.len(), expect.len());
        for (got, &(rule, name, layer)) in AREA_RULES.iter().zip(expect) {
            assert_eq!(got.rule, rule);
            assert_eq!(got.rule_name, name);
            assert_eq!(got.layer, layer);
        }
        let expect_holes: &[(&str, &str)] = &[
            ("met1.7", "Min area of metal1 holes > 0.14um^2 (met1.7)"),
            ("met2.7", "Min area of metal2 holes > 0.14um^2 (met2.7)"),
        ];
        assert_eq!(HOLE_RULES.len(), expect_holes.len());
        for (got, &(rule, name)) in HOLE_RULES.iter().zip(expect_holes) {
            assert_eq!(got.rule, rule);
            assert_eq!(got.rule_name, name);
        }
    }

    /// Island area `bound` passes, `bound − 1` flags `met1.6`
    /// (bound 83000 on A_LEF).
    #[test]
    fn test_min_area_boundary() {
        let tech = a_tech();
        // 83000 = 200*415 — exact-bound rect.
        let at = design([
            vec![],
            vec![sh(RectDbu::new(0, 0, 200, 415), ShapeSrc::SignalWire)],
            vec![],
        ]);
        assert!(check_area(&at, &tech).is_empty());

        // 82999 = 82999*1 — degenerate but exact-integer.
        let under = design([
            vec![],
            vec![sh(RectDbu::new(0, 0, 82999, 1), ShapeSrc::SignalWire)],
            vec![],
        ]);
        let v = check_area(&under, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "met1.6");
        assert_eq!(v[0].area_dbu2, 82999);
        assert_eq!(v[0].bound_dbu2, 83000);
    }

    /// A `MacroPin`-only island is never evaluated; adding one drawn
    /// member puts it under the bound check — the pin metal counts in
    /// its area.
    #[test]
    fn test_drawn_member_gate() {
        let tech = a_tech();
        let pin_only = design([
            vec![],
            vec![sh(RectDbu::new(0, 0, 10, 10), ShapeSrc::MacroPin)],
            vec![],
        ]);
        let report = check_area_report(&pin_only, &tech);
        let m1 = report.iter().find(|r| r.rule == "met1.6").unwrap();
        assert_eq!(m1.candidates, 0);
        assert!(m1.violations.is_empty());

        let mixed = design([
            vec![],
            vec![
                sh(RectDbu::new(0, 0, 10, 10), ShapeSrc::MacroPin),
                sh(RectDbu::new(10, 0, 20, 10), ShapeSrc::SignalWire),
            ],
            vec![],
        ]);
        let m1 = check_area_report(&mixed, &tech)
            .into_iter()
            .find(|r| r.rule == "met1.6")
            .unwrap();
        assert_eq!(m1.candidates, 1);
        assert_eq!(m1.violations.len(), 1);
        assert_eq!(m1.violations[0].area_dbu2, 200);
    }

    /// Two sub-bound different-net rects that touch merge into one
    /// component above the bound — the plane model, not node labels.
    #[test]
    fn test_net_agnostic_merge() {
        let tech = a_tech();
        let mut a = sh(RectDbu::new(0, 0, 200, 300), ShapeSrc::SignalWire);
        a.net = Some(tech.strings.get("n1").unwrap());
        let mut b = sh(RectDbu::new(200, 0, 400, 300), ShapeSrc::SignalWire);
        b.net = Some(tech.strings.get("n2").unwrap());
        let d = design([vec![], vec![a, b], vec![]]);
        // 400*300 = 120000 ≥ 83000 merged; split it would be 60000 each.
        assert!(check_area(&d, &tech).is_empty());
    }

    /// `li.6` has no `AREA` in A_LEF → `bound_dbu2 == 0`, never
    /// evaluated even with li1 members.
    #[test]
    fn test_absent_bound_skips() {
        let tech = a_tech();
        let d = design(
            [vec![sh(RectDbu::new(0, 0, 10, 10), ShapeSrc::SignalWire)],
             vec![], vec![]],
        );
        let report = check_area_report(&d, &tech);
        let li = report.iter().find(|r| r.rule == "li.6").unwrap();
        assert_eq!(li.bound_dbu2, 0);
        assert_eq!(li.candidates, 0);
        assert!(li.violations.is_empty());
    }

    /// met1 ring with a `100×100` void → `met1.7` (140000 bound);
    /// `200×700` void = exact bound → clean; same on `met2.7`.
    #[test]
    fn test_hole_bound() {
        let tech = a_tech();
        let ring = |inner: i32| {
            let (o, i) = (inner + 200, inner);
            [
                sh(RectDbu::new(0, 0, o, 100), ShapeSrc::SignalWire),
                sh(RectDbu::new(0, 100 + i, o, 200 + i), ShapeSrc::SignalWire),
                sh(RectDbu::new(0, 100, 100, 100 + i), ShapeSrc::SignalWire),
                sh(RectDbu::new(100 + i, 100, o, 100 + i), ShapeSrc::SignalWire),
            ]
        };
        // inner 100 → void 100×100 = 10000 < 140000.
        let d = design([vec![], ring(100).to_vec(), vec![]]);
        let v = check_holes(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "met1.7");
        assert_eq!(v[0].area_dbu2, 10_000);
        assert_eq!(v[0].region, RectDbu::new(100, 100, 200, 200));

        // inner 700 × 200 → 140000 = bound → clean. Build a 700×200
        // void: ring 200 wall, void 700 wide × 200 tall.
        let void_w = 700i32;
        let ring700 = [
            sh(RectDbu::new(0, 0, 100 + void_w + 100, 100), ShapeSrc::SignalWire),
            sh(RectDbu::new(0, 300, 100 + void_w + 100, 400), ShapeSrc::SignalWire),
            sh(RectDbu::new(0, 100, 100, 300), ShapeSrc::SignalWire),
            sh(RectDbu::new(100 + void_w, 100, 200 + void_w, 300), ShapeSrc::SignalWire),
        ];
        let d = design([vec![], ring700.to_vec(), vec![]]);
        assert!(check_holes(&d, &tech).is_empty(), "{:?}", check_holes(&d, &tech));

        // met2 ring with a 100×100 void → met2.7.
        let d = design([vec![], vec![], ring(100).to_vec()]);
        let v = check_holes(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "met2.7");
    }

    /// OBS discard: a MacroObs rect touching the void drops it; obs
    /// fully outside leaves the flag.
    #[test]
    fn test_hole_obs_discard() {
        let tech = a_tech();
        let ring = vec![
            sh(RectDbu::new(0, 0, 400, 100), ShapeSrc::SignalWire),
            sh(RectDbu::new(0, 300, 400, 400), ShapeSrc::SignalWire),
            sh(RectDbu::new(0, 100, 100, 300), ShapeSrc::SignalWire),
            sh(RectDbu::new(300, 100, 400, 300), ShapeSrc::SignalWire),
        ];
        let flags = |extra: Vec<ShapeRect>| {
            let mut l = ring.clone();
            l.extend(extra);
            check_holes(&design([vec![], l, vec![]]), &tech)
        };
        // Baseline: 200×200 void flags.
        assert_eq!(flags(vec![]).len(), 1);
        // Obs rect touching the void's edge → discarded.
        let v = flags(vec![sh(
            RectDbu::new(200, 100, 260, 200),
            ShapeSrc::MacroObs,
        )]);
        assert!(v.is_empty(), "{v:?}");
        // Obs fully outside the void → still flags.
        let v = flags(vec![sh(
            RectDbu::new(1000, 1000, 1100, 1200),
            ShapeSrc::MacroObs,
        )]);
        assert_eq!(v.len(), 1);
    }
}
