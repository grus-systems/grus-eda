//! The width rule family: 13 `lef_expressible` width rules checked
//! per assembled `ShapeRect`.
//!
//! Bounds come from LEF — routing rules read the layer's `WIDTH`, via
//! rules read the cut layer's `WIDTH`; nothing here is a Magic techfile
//! constant. `rule_name` strings are verbatim `magic_rules_checked.names`
//! templates from the committed goldens, so test results join
//! `magic_per_rule` by the `(id)` inside the parens.
//!
//! Deliberate simplification vs Magic: Magic measures width on the
//! merged per-layer polygon; `check_width` measures each `ShapeRect`
//! (`min(w, h) < bound`) and then applies merge-relief — a sub-bound
//! wire rect whose narrow boundary pair dissolves into the same-layer
//! union (covering neighbours extend past it on the narrow axis) is not
//! reported, mirroring what Magic's merged polygon does to `RECT` stubs
//! and jog caps. Per-rect checking is only sound for shapes that are
//! what they look like standalone:
//!
//! - `SignalWire`/`SpecialWire`/`IoPort`: router-drawn metal; min-width
//!   is a hard floor on the drawn shape, so per-rect is exact.
//! - `Via` cut rects: the actual cut geometry — checked on cut rules.
//!
//! Excluded (checked but never flagged): `Via` *metal* rects merge with
//! the wire they sit on, so a sub-bound tab is a false positive Magic's
//! merged polygon absorbs; `MacroPin`/`MacroObs` are LEF abstract rects,
//! not the fabricated GDS Magic measures — pin stubs and OBS markers are
//! legitimately narrower than routing min-width. Dropping them is a
//! recorded scope decision (`docs/e7-drc.md`), not silent filtering.

use crate::drc::{DrcDesign, DrcLayer, ShapeRect, ShapeSrc};
use crate::interner::SymbolId;
use crate::tech_db::{RectDbu, TechDb};

/// One LEF-expressible width rule. `layer` is a LEF routing-layer name
/// unless `cut` is set, then a `TYPE CUT` name.
pub struct WidthRule {
    /// The `(id)` inside `rule_name`'s parens — the `magic_per_rule`
    /// join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template, `%d` intact.
    pub rule_name: &'static str,
    pub layer: &'static str,
    pub cut: bool,
}

/// The 13 width rules the goal lists, in `width.md` table order.
pub const WIDTH_RULES: &[WidthRule] = &[
    WidthRule {
        rule: "li.c1",
        rule_name: "Core local interconnect width < %d (li.c1)",
        layer: "li1",
        cut: false,
    },
    WidthRule {
        rule: "li.1",
        rule_name: "Local interconnect width < %d (li.1)",
        layer: "li1",
        cut: false,
    },
    WidthRule {
        rule: "li.7",
        rule_name: "Local interconnect width < %d (li.7)",
        layer: "li1",
        cut: false,
    },
    WidthRule {
        rule: "mcon.1",
        rule_name: "mcon.width < %d (mcon.1)",
        layer: "mcon",
        cut: true,
    },
    WidthRule {
        rule: "met1.1",
        rule_name: "Metal1 width < %d (met1.1)",
        layer: "met1",
        cut: false,
    },
    WidthRule {
        rule: "met2.1",
        rule_name: "Metal2 width < %d (met2.1)",
        layer: "met2",
        cut: false,
    },
    WidthRule {
        rule: "met3.1",
        rule_name: "Metal3 width < %d (met3.1)",
        layer: "met3",
        cut: false,
    },
    WidthRule {
        rule: "met4.1",
        rule_name: "Metal4 width < %d (met4.1)",
        layer: "met4",
        cut: false,
    },
    WidthRule {
        rule: "met5.1",
        rule_name: "Metal5 width < %d (met5.1)",
        layer: "met5",
        cut: false,
    },
    WidthRule {
        rule: "via.1a",
        rule_name: "Via1 width < %d (via.1a + 2 * via.4a)",
        layer: "via",
        cut: true,
    },
    WidthRule {
        rule: "via2.1a",
        rule_name: "via2 width < %d (via2.1a + 2 * via2.4)",
        layer: "via2",
        cut: true,
    },
    WidthRule {
        rule: "via3.1",
        rule_name: "via3 width < %d (via3.1 + 2 * via3.4)",
        layer: "via3",
        cut: true,
    },
    WidthRule {
        rule: "via4.1",
        rule_name: "via4 width < %d (via4.1 + 2 * via4.4)",
        layer: "via4",
        cut: true,
    },
];

/// A sub-bound shape on a checked layer — the offending rect, not a
/// merged Magic region.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// `(id)` inside the rule_name parens — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template — `%d` intact.
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// The offending shape.
    pub rect: RectDbu,
    /// `min(rect.w, rect.h)`.
    pub measured_dbu: i32,
    pub bound_dbu: i32,
    pub net: Option<SymbolId>,
}

/// Which axis of a sub-bound rect is narrow — the direction its boundary
/// pair must be dissolved in.
enum Axis {
    X,
    Y,
}

/// Merge-relief, one direction. `r` is narrow on `axis`; its narrow
/// boundary pair dissolves into the merged polygon iff drawn metal on
/// the layer both (a) covers `r`'s entire perpendicular span — the union
/// of member rects' projections clipped to `r`'s span — and (b) chains
/// `r`'s narrow extent outward far enough that the union itself spans
/// `bound` — a shorter reach still leaves a sub-bound corridor. Members
/// join the chain when they overlap `r`'s perpendicular span (merge
/// with `r` where they share it) and touch the growing narrow extent
/// (abutment merges like overlap).
/// Runs only on the rare sub-bound wire path, so the O(n·k) walk is fine.
fn chain_outreaches(
    rects: &[ShapeRect],
    r: &RectDbu,
    skip: &ShapeRect,
    axis: Axis,
    bound: i32,
) -> bool {
    let relief_src = |o: &ShapeRect| {
        !std::ptr::eq(o, skip)
            && matches!(
                o.src,
                ShapeSrc::SignalWire
                    | ShapeSrc::SpecialWire
                    | ShapeSrc::Via
                    | ShapeSrc::IoPort
                    // Pin abstracts understate the real GDS metal Magic
                    // sees, so they relieve but never generate findings.
                    | ShapeSrc::MacroPin
            )
    };
    let proj = |q: &RectDbu| -> ((i32, i32), (i32, i32)) {
        match axis {
            Axis::X => ((q.xl, q.xh), (q.yl, q.yh)),
            Axis::Y => ((q.yl, q.yh), (q.xl, q.xh)),
        }
    };
    let ((mut lo, mut hi), (perp_lo, perp_hi)) = proj(r);
    let (r_lo, r_hi) = (lo, hi);
    // Perp projections of every member, clipped to r's span.
    let mut covered: Vec<(i32, i32)> = Vec::new();
    loop {
        let mut grew = false;
        for o in rects.iter().filter(|o| relief_src(o)) {
            let ((olo, ohi), (opl, oph)) = proj(&o.rect);
            if olo <= hi && ohi >= lo && opl <= perp_hi && oph >= perp_lo {
                covered.push((opl.max(perp_lo), oph.min(perp_hi)));
                if olo < lo {
                    lo = olo;
                    grew = true;
                }
                if ohi > hi {
                    hi = ohi;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    // The chained union must reach `bound` on the narrow axis measured
    // back from one of r's edges — anything less is still a corridor
    // Magic would flag (e.g. a 25-wide stub beside a 30-wide tab under
    // a 140 bound merges into a 55-wide shape, not a clean dissolve).
    if r_hi - lo < bound && hi - r_lo < bound {
        return false;
    }
    // The perpendicular span must be fully covered — an uncovered slice
    // leaves a sub-bound corridor.
    covered.sort_unstable();
    let mut reach = perp_lo;
    for &(cl, ch) in &covered {
        if cl > reach {
            return false;
        }
        reach = reach.max(ch);
        if reach >= perp_hi {
            return true;
        }
    }
    reach >= perp_hi
}

/// Runs every resolvable `WIDTH_RULES` rule over `design`. A rule whose
/// layer is absent from the loaded LEF, or whose bound is 0 (LEF omitted
/// `WIDTH`), contributes nothing. Strict `<`: a shape exactly at the
/// bound passes, matching Magic's `width < %d` reading.
pub fn check_width(design: &DrcDesign, tech: &TechDb) -> Vec<Violation> {
    let mut out = Vec::new();
    for rule in WIDTH_RULES {
        let (layer, bound, rects): (DrcLayer, i32, &[ShapeRect]) = if rule.cut {
            let Some(&idx) = tech
                .strings
                .get(rule.layer)
                .and_then(|s| tech.cut_layer_by_name.get(&s))
            else {
                continue;
            };
            (
                DrcLayer::Cut(idx),
                tech.cut_layers[idx as usize].width_dbu,
                design.cut[idx as usize].rects.as_slice(),
            )
        } else {
            let Some(&lid) = tech
                .strings
                .get(rule.layer)
                .and_then(|s| tech.layer_by_name.get(&s))
            else {
                continue;
            };
            (
                DrcLayer::Routing(lid),
                tech.routing_layers[lid.0 as usize].width_dbu,
                design.routing[lid.0 as usize].rects.as_slice(),
            )
        };
        if bound == 0 {
            continue;
        }
        let ls_rects = rects;
        for s in rects {
            // Per-rect width is only sound on shapes that are standalone:
            // drawn wires/ports, and via cut rects. Via metals merge with
            // their wire; macro rects are LEF abstracts, not GDS.
            let checked = if rule.cut {
                s.src == ShapeSrc::Via
            } else {
                matches!(
                    s.src,
                    ShapeSrc::SignalWire | ShapeSrc::SpecialWire | ShapeSrc::IoPort
                )
            };
            if !checked {
                continue;
            }
            let measured = s.rect.width().min(s.rect.height());
            if measured < bound {
                // Merge-relief: Magic measures the merged polygon. A wire
                // segment whose *short* axis is the segment's length (a
                // 45-DBU jog ending on a via) is absorbed into the body it
                // touches; a wire whose *cross-section* is sub-bound (a
                // narrowed stripe) is not. Per direction: an x-narrow rect
                // is relieved iff same-layer drawn metal covers its whole
                // y-span, touches its x-extent, and extends past it in x —
                // so the narrow boundary pair disappears into the union.
                if matches!(s.src, ShapeSrc::SignalWire | ShapeSrc::SpecialWire) {
                    let r = s.rect;
                    // x-narrow: relieved iff the union of drawn metal that
                    // vertically contains r and touches r's x-extent chains
                    // out to `bound` in x — the narrow boundary pair
                    // dissolves into a union that itself meets the bound,
                    // and since the union covers r's whole y-span the
                    // other pair dissolves too. Symmetric for y. Abutment
                    // counts as merge (`<=`/`>=`), matching Magic. A rect
                    // with no sub-bound axis is never relieved — it cannot
                    // reach here under the strict `<` check.
                    let mut relieved = false;
                    if r.width() < bound {
                        relieved = chain_outreaches(ls_rects, &r, s, Axis::X, bound);
                    }
                    if !relieved && r.height() < bound {
                        relieved = chain_outreaches(ls_rects, &r, s, Axis::Y, bound);
                    }
                    if relieved {
                        continue;
                    }
                }
                out.push(Violation {
                    rule: rule.rule,
                    rule_name: rule.rule_name,
                    layer,
                    rect: s.rect,
                    measured_dbu: measured,
                    bound_dbu: bound,
                    net: s.net,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc::{DrcDesign, LayerShapes, ShapeSrc};
    use crate::interner::{LayerId, StringInterner};
    use crate::readers::lef::parse_lef_bytes;

    /// met1 WIDTH 0.14 → bound 140; cut layer `via` WIDTH 0.15 → 150.
    const W_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
END met1
LAYER via
  TYPE CUT ;
  WIDTH 0.15 ;
END via
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
END LIBRARY
"#;

    fn w_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        parse_lef_bytes(W_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("LEF must parse");
        tech_db.strings = interner;
        tech_db.rebuild_maps();
        tech_db
    }

    fn design_with(m1_rects: &[RectDbu], cut_rects: &[RectDbu]) -> DrcDesign {
        let shapes = |rs: &[RectDbu], src: ShapeSrc| {
            LayerShapes::from_rects(
                rs.iter()
                    .map(|&r| ShapeRect {
                        rect: r,
                        src,
                        net: None,
                    })
                    .collect(),
            )
        };
        DrcDesign {
            routing: vec![
                shapes(m1_rects, ShapeSrc::SignalWire),
                LayerShapes::new(),
            ],
            cut: vec![shapes(cut_rects, ShapeSrc::Via)],
        }
    }

    /// Rule ids and verbatim `rule_name` templates are the golden join
    /// key — pin the full table.
    #[test]
    fn test_width_rules_metadata() {
        assert_eq!(WIDTH_RULES.len(), 13);
        let expect = [
            ("li.c1", "Core local interconnect width < %d (li.c1)", "li1", false),
            ("li.1", "Local interconnect width < %d (li.1)", "li1", false),
            ("li.7", "Local interconnect width < %d (li.7)", "li1", false),
            ("mcon.1", "mcon.width < %d (mcon.1)", "mcon", true),
            ("met1.1", "Metal1 width < %d (met1.1)", "met1", false),
            ("met2.1", "Metal2 width < %d (met2.1)", "met2", false),
            ("met3.1", "Metal3 width < %d (met3.1)", "met3", false),
            ("met4.1", "Metal4 width < %d (met4.1)", "met4", false),
            ("met5.1", "Metal5 width < %d (met5.1)", "met5", false),
            ("via.1a", "Via1 width < %d (via.1a + 2 * via.4a)", "via", true),
            ("via2.1a", "via2 width < %d (via2.1a + 2 * via2.4)", "via2", true),
            ("via3.1", "via3 width < %d (via3.1 + 2 * via3.4)", "via3", true),
            ("via4.1", "via4 width < %d (via4.1 + 2 * via4.4)", "via4", true),
        ];
        for (got, (rule, name, layer, cut)) in WIDTH_RULES.iter().zip(expect) {
            assert_eq!(got.rule, rule);
            assert_eq!(got.rule_name, name);
            assert_eq!(got.layer, layer);
            assert_eq!(got.cut, cut);
        }
    }

    /// Bound comes from LEF `WIDTH` (met1 → 140), strict `<`: a shape
    /// exactly at the bound passes, one DBU below fails.
    #[test]
    fn test_check_width_bound_parity_and_strictness() {
        let tech = w_tech();
        let m1 = tech.layer_by_name[&tech.strings.get("met1").unwrap()];
        assert_eq!(m1, LayerId(0));
        let at = RectDbu::new(0, 0, 140, 1000); // min edge 140 == bound
        // Violators sit clear of `at` so merge-relief cannot cover them.
        let below = RectDbu::new(5000, 0, 5139, 1000);
        let tall_narrow = RectDbu::new(5000, 2000, 5139, 7000); // min(w,h) picks w
        let d = design_with(&[at, below, tall_narrow], &[]);
        let v = check_width(&d, &tech);
        assert_eq!(v.len(), 2, "exact-bound rect must pass");
        assert_eq!(
            v[0],
            Violation {
                rule: "met1.1",
                rule_name: "Metal1 width < %d (met1.1)",
                layer: DrcLayer::Routing(m1),
                rect: below,
                measured_dbu: 139,
                bound_dbu: 140,
                net: None,
            }
        );
        assert_eq!(v[1].rect, tall_narrow);
    }

    /// Merge-relief dissolves a sub-bound rect only when the chained
    /// union itself spans the bound on the narrow axis: a 25-wide stub
    /// beside a 30-wide tab is a 55-wide corridor Magic still flags,
    /// while the same stub beside a 150-wide body is absorbed.
    #[test]
    fn test_merge_relief_requires_bound_span() {
        let tech = w_tech();
        let stub = RectDbu::new(0, 0, 25, 50); // met1 bound 140
        // Covers stub's whole y-span and abuts it, but reaches only 55.
        let tab = RectDbu::new(25, -10, 55, 60);
        let v = check_width(&design_with(&[stub, tab], &[]), &tech);
        assert_eq!(v.len(), 2, "a 55-wide union is still sub-bound");
        // A 150-wide body under the stub dissolves it.
        let body = RectDbu::new(25, -50, 175, 100);
        let v = check_width(&design_with(&[stub, body], &[]), &tech);
        assert!(v.is_empty(), "a bound-spanning union absorbs the stub");
    }

    /// Cut-layer rules read the cut layer's `WIDTH` (`via` → 150) and
    /// index `design.cut`, not `routing`.
    #[test]
    fn test_check_width_cut_layer() {
        let tech = w_tech();
        let cut = RectDbu::new(0, 0, 149, 149);
        let d = design_with(&[], &[cut]);
        let v = check_width(&d, &tech);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "via.1a");
        assert_eq!(v[0].layer, DrcLayer::Cut(0));
        assert_eq!(v[0].bound_dbu, 150);
    }

    /// Layers the LEF lacks (`li1`, `mcon`, `met3+`, `via2+`) contribute
    /// nothing — no panic on absent layers or empty `LayerShapes`.
    #[test]
    fn test_check_width_skips_missing_layers() {
        let tech = w_tech();
        let d = design_with(&[RectDbu::new(0, 0, 1, 1)], &[]);
        let v = check_width(&d, &tech);
        // Only the tiny met1 rect can fail; li/mcon/met3+/via2+ layers
        // are absent from W_LEF and must be skipped, not panic.
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "met1.1");
    }
}
