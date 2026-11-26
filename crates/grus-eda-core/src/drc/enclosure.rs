//! The enclosure rule family: 9 `lef_expressible` overlap rules
//! checking that a cut rect is enclosed by the owning net's metal on
//! the rule's routing layer.
//!
//! Bounds are the LEF cut-layer `ENCLOSURE {BELOW|ABOVE} o1 o2` pairs
//! retained as `CutLayer::{enc_below_dbu, enc_above_dbu}` — read
//! symmetrically as `min(o1,o2)` required on both axes plus `max(o1,o2)`
//! on at least one axis, which is what the deck's `surround`
//! statements encode and what OpenDB's generated-via expansion
//! produces. `rule_name` strings are verbatim
//! `magic_rules_checked.names` templates — the width/spacing join
//! convention (first `(id)` token inside the parens).
//!
//! Measurement is per side: split the cut's perpendicular span into
//! elementary sub-intervals at the candidate metal rects' edges; in a
//! sub-interval the union intervals are constant, so the side's
//! overhang — cut edge to the union boundary, negative when the cut
//! edge protrudes past all same-net metal — is exact. The four side
//! overhangs reduce to `enc_x = min(left, right)` and
//! `enc_y = min(bottom, top)`, then the rule's clause evaluates.
//!
//! Checked sources: only `ShapeSrc::Via` cut rects generate findings —
//! the only source that produces cut-layer shapes. Metal candidates
//! are every *same-net* rect on the routing layer regardless of source
//! (`Via` metals, wires, `IoPort`, `MacroPin` — all real metal);
//! `MacroObs` is a blockage abstract, not metal, so it never encloses.
//! A cut with no same-net metal reaching its edge gets a negative
//! overhang (`absence_illegal` semantics).

use crate::drc::{DrcDesign, DrcLayer, LayerShapes, ShapeSrc};
use crate::interner::{LayerId, SymbolId};
use crate::tech_db::{RectDbu, TechDb};
use std::collections::BTreeMap;

/// Which `CutLayer` enclosure slot supplies the bound pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `ENCLOSURE BELOW` — the routing layer below the cut.
    Below,
    /// `ENCLOSURE ABOVE` — the routing layer above the cut.
    Above,
}

/// Which clause(s) of the overhang pair the deck name checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum EncMode {
    /// `enc >= min(pair)` required on both axes.
    Both,
    /// `enc >= max(pair)` required on at least one axis.
    One,
    /// Both clauses of the composite `(id_a - id_b)` names: the
    /// both-directions clause at `min(pair)` first, then the
    /// one-direction clause at `max(pair)`.
    Full,
}

/// One LEF-expressible enclosure rule.
pub struct EnclosureRule {
    /// First `(id)` token inside `rule_name`'s parens — join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template, `%d` intact.
    pub rule_name: &'static str,
    /// LEF `TYPE CUT` layer name.
    pub cut: &'static str,
    /// LEF routing-layer name — the metal plane measured.
    pub metal: &'static str,
    /// Which `CutLayer` enclosure slot supplies the bound.
    pub side: Side,
    /// Which clause(s) of the overhang pair the name checks.
    pub mode: EncMode,
}

/// The 9 enclosure rules, `lef-rules.md` table order. `met1.4`/`met1.5`
/// are two deck names over the same mcon ABOVE pair. Not implemented —
/// no deck statement exists: mcon BELOW (LEF `0 0`) and via4 BELOW →
/// met4 (`via4.4`, folded into the `via4.1` width composite the width
/// family already checks).
pub const ENCLOSURE_RULES: &[EnclosureRule] = &[
    EnclosureRule {
        rule: "met1.4",
        rule_name: "Metal1 overlap of local interconnect contact < %d (met1.4)",
        cut: "mcon",
        metal: "met1",
        side: Side::Above,
        mode: EncMode::Both,
    },
    EnclosureRule {
        rule: "met1.5",
        rule_name: "Metal1 overlap of local interconnect contact < %d in one direction (met1.5)",
        cut: "mcon",
        metal: "met1",
        side: Side::Above,
        mode: EncMode::One,
    },
    EnclosureRule {
        rule: "via.5a",
        rule_name: "Metal1 overlap of Via1 < %d in one direction (via.5a - via.4a)",
        cut: "via",
        metal: "met1",
        side: Side::Below,
        mode: EncMode::Full,
    },
    EnclosureRule {
        rule: "met2.5",
        rule_name: "Metal2 overlap of Via1 < %d in one direction (met2.5 - met2.4)",
        cut: "via",
        metal: "met2",
        side: Side::Above,
        mode: EncMode::Full,
    },
    EnclosureRule {
        rule: "via2.4a",
        rule_name: "Metal2 overlap of via2 < %d in one direction (via2.4a - via2.4)",
        cut: "via2",
        metal: "met2",
        side: Side::Below,
        mode: EncMode::Full,
    },
    EnclosureRule {
        rule: "met3.4",
        rule_name: "Metal3 overlap of via2 < %d (met3.4)",
        cut: "via2",
        metal: "met3",
        side: Side::Above,
        mode: EncMode::Both,
    },
    EnclosureRule {
        rule: "via3.5",
        rule_name: "Metal3 overlap of via3 in one direction < %d (via3.5 - via3.4)",
        cut: "via3",
        metal: "met3",
        side: Side::Below,
        mode: EncMode::Full,
    },
    EnclosureRule {
        rule: "met4.3",
        rule_name: "Metal4 overlap of via3 < %d (met4.3 - via3.4)",
        cut: "via3",
        metal: "met4",
        side: Side::Above,
        mode: EncMode::Both,
    },
    EnclosureRule {
        rule: "met5.3",
        rule_name: "Metal5 overlap of via4 < %d (met5.3 - via4.4)",
        cut: "via4",
        metal: "met5",
        side: Side::Above,
        mode: EncMode::Both,
    },
];

/// A cut whose same-net metal under-encloses one side — the offending
/// cut rect, not a merged region.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// `(id)` inside the rule_name parens — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template — `%d` intact.
    pub rule_name: &'static str,
    /// The metal routing layer measured.
    pub layer: DrcLayer,
    /// The offending cut rect.
    pub cut: RectDbu,
    /// `min(left, right)` / `min(bottom, top)` overhang measured on the
    /// owning net's metal union; negative = local protrusion.
    pub enc_x_dbu: i32,
    pub enc_y_dbu: i32,
    /// Bound of the failing clause.
    pub bound_dbu: i32,
    /// Which clause failed — `Both` or `One` (`Full` maps to one of them).
    pub failed: EncMode,
    pub net: Option<SymbolId>,
    /// Owning via-instance id (`ShapeRect::group`).
    pub group: u32,
}

/// Per-rule outcome for the zero-violation census — same shape as
/// `spacing::RuleReport`; `candidates` = cut rects evaluated.
#[derive(Debug, PartialEq)]
pub struct RuleReport {
    pub rule: &'static str,
    /// The metal routing layer; `None` when the rule's cut or metal
    /// name is absent from the loaded LEF.
    pub layer: Option<DrcLayer>,
    /// The clause bound this rule applies (`min` for `Both`, `max`
    /// otherwise); 0 when the enclosure slot is undeclared.
    pub bound_dbu: i32,
    /// Cut rects evaluated.
    pub candidates: usize,
    pub violations: Vec<Violation>,
}

/// `(enc_left, enc_right)`-style overhang pair along one axis.
/// `proj` maps a rect to `(axis_lo, axis_hi, perp_lo, perp_hi)`.
///
/// Per elementary band of the cut's perpendicular span the covering
/// union is constant, so the overhang is exact: `lo` reads the union
/// interval covering the cut's low edge (negative when uncovered —
/// nearest interval inside/right of the edge, `lo - hi` when the band
/// sees no metal at all); `hi` symmetric.
fn measure_axis(
    cut: &RectDbu,
    cands: &[RectDbu],
    proj: impl Fn(&RectDbu) -> (i32, i32, i32, i32),
) -> (i32, i32) {
    let (cl, ch, pl, ph) = proj(cut);
    let mut edges = vec![pl, ph];
    for r in cands {
        let (_, _, rpl, rph) = proj(r);
        if rpl < ph && rph > pl {
            edges.push(rpl.clamp(pl, ph));
            edges.push(rph.clamp(pl, ph));
        }
    }
    edges.sort_unstable();
    edges.dedup();

    // Empty bands yield `cl - ch` through the `else` arms below, so the
    // min-fold starts at MAX — seeding it with the fallback would make
    // the fallback always win.
    let (mut enc_lo, mut enc_hi) = (i32::MAX, i32::MAX);
    for w in edges.windows(2) {
        let (b0, b1) = (w[0], w[1]);
        // Merge the axis-intervals of rects covering this band into the
        // metal union (closed intervals — touching merges).
        let mut ivs: Vec<(i32, i32)> = cands
            .iter()
            .map(&proj)
            .filter(|&(_, _, rpl, rph)| rpl < b1 && rph > b0)
            .map(|(a0, a1, _, _)| (a0, a1))
            .collect();
        ivs.sort_unstable();
        let mut merged: Vec<(i32, i32)> = Vec::with_capacity(ivs.len());
        for (a0, a1) in ivs {
            match merged.last_mut() {
                Some(m) if a0 <= m.1 => m.1 = m.1.max(a1),
                _ => merged.push((a0, a1)),
            }
        }
        // lo side: interval covering `cl` gives `cl - iv.lo`; else the
        // first interval at/right of `cl` gives a negative value; else
        // `cl - ch`.
        let lo = if let Some(&(m0, _)) = merged.iter().find(|&&(m0, m1)| m0 <= cl && m1 > cl) {
            cl - m0
        } else if let Some(&(m0, _)) = merged.iter().find(|&&(m0, _)| m0 >= cl) {
            cl - m0
        } else {
            cl - ch
        };
        // hi side: covering `ch` gives `iv.hi - ch`; else the last
        // interval at/left of `ch` (negative); else `cl - ch`.
        let hi = if let Some(&(_, m1)) = merged.iter().rev().find(|&&(m0, m1)| m0 < ch && m1 >= ch)
        {
            m1 - ch
        } else if let Some(&(_, m1)) = merged.iter().rev().find(|&&(_, m1)| m1 <= ch) {
            m1 - ch
        } else {
            cl - ch
        };
        enc_lo = enc_lo.min(lo);
        enc_hi = enc_hi.min(hi);
    }
    if enc_lo == i32::MAX {
        // Degenerate zero-span cut (pl == ph): no bands to measure.
        // Return the no-metal fallback — `absence_illegal`, not a
        // silent pass.
        return (cl - ch, cl - ch);
    }
    (enc_lo, enc_hi)
}

/// `(enc_x, enc_y)` — `min(left, right)` and `min(bottom, top)` side
/// overhangs of `cut` on the metal union `cands` forms.
fn measure_cut(cut: &RectDbu, cands: &[RectDbu]) -> (i32, i32) {
    let (el, er) = measure_axis(cut, cands, |r| (r.xl, r.xh, r.yl, r.yh));
    let (eb, et) = measure_axis(cut, cands, |r| (r.yl, r.yh, r.xl, r.xh));
    (el.min(er), eb.min(et))
}

/// A `(cut layer, metal layer, side)` measurement pass shared by rules
/// over the same pair — `met1.4`/`met1.5` evaluate one mcon→met1 pass.
struct Pass {
    cut_idx: u32,
    metal: LayerId,
    bmin: i32,
    bmax: i32,
    /// `ENCLOSURE_RULES` indices sharing this pass.
    rules: Vec<usize>,
}

/// Every resolvable `ENCLOSURE_RULES` rule over `design`, one
/// `RuleReport` per rule. A rule whose layers are absent, or whose
/// `(0,0)` enclosure slot resolves, contributes nothing — mirroring the
/// `bound == 0` skips in width/spacing. Exact-bound passes
/// (`enc == b` satisfies `>=`; Magic's `surround < %d` reads "insufficient
/// when strictly below").
pub fn check_enclosure_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport> {
    let mut reports: Vec<RuleReport> = ENCLOSURE_RULES
        .iter()
        .map(|r| RuleReport {
            rule: r.rule,
            layer: None,
            bound_dbu: 0,
            candidates: 0,
            violations: Vec::new(),
        })
        .collect();
    let mut passes: BTreeMap<(u32, u8), Pass> = BTreeMap::new();
    for (ri, rule) in ENCLOSURE_RULES.iter().enumerate() {
        let cut_idx = tech
            .strings
            .get(rule.cut)
            .and_then(|s| tech.cut_layer_by_name.get(&s))
            .copied();
        let metal = tech
            .strings
            .get(rule.metal)
            .and_then(|s| tech.layer_by_name.get(&s))
            .copied();
        let (Some(ci), Some(ml)) = (cut_idx, metal) else {
            continue;
        };
        reports[ri].layer = Some(DrcLayer::Routing(ml));
        let pair = match rule.side {
            Side::Below => tech.cut_layers[ci as usize].enc_below_dbu,
            Side::Above => tech.cut_layers[ci as usize].enc_above_dbu,
        };
        let (bmin, bmax) = (pair[0].min(pair[1]), pair[0].max(pair[1]));
        reports[ri].bound_dbu = match rule.mode {
            EncMode::Both => bmin,
            _ => bmax,
        };
        if bmax <= 0 {
            continue;
        }
        passes
            .entry((ci, ml.0))
            .or_insert(Pass {
                cut_idx: ci,
                metal: ml,
                bmin,
                bmax,
                rules: Vec::new(),
            })
            .rules
            .push(ri);
    }

    for pass in passes.values() {
        let ls_cut: &LayerShapes = &design.cut[pass.cut_idx as usize];
        let ls_metal: &LayerShapes = &design.routing[pass.metal.0 as usize];
        for s in &ls_cut.rects {
            if s.src != ShapeSrc::Via {
                continue;
            }
            // Same-net metal that can reach within `bmax` of the cut —
            // a rect ending exactly at the probe edge contributes
            // nothing, so strict overlap is right.
            let probe = RectDbu::new(
                s.rect.xl - pass.bmax,
                s.rect.yl - pass.bmax,
                s.rect.xh + pass.bmax,
                s.rect.yh + pass.bmax,
            );
            let cands: Vec<RectDbu> = ls_metal
                .iter_overlapping(&probe)
                .filter(|m| m.net == s.net)
                .map(|m| m.rect)
                .collect();
            let (enc_x, enc_y) = measure_cut(&s.rect, &cands);
            for &ri in &pass.rules {
                let rule = &ENCLOSURE_RULES[ri];
                reports[ri].candidates += 1;
                let failed = match rule.mode {
                    EncMode::Both if enc_x < pass.bmin || enc_y < pass.bmin => {
                        Some((EncMode::Both, pass.bmin))
                    }
                    EncMode::One if enc_x < pass.bmax && enc_y < pass.bmax => {
                        Some((EncMode::One, pass.bmax))
                    }
                    EncMode::Full if enc_x < pass.bmin || enc_y < pass.bmin => {
                        Some((EncMode::Both, pass.bmin))
                    }
                    EncMode::Full if enc_x < pass.bmax && enc_y < pass.bmax => {
                        Some((EncMode::One, pass.bmax))
                    }
                    _ => None,
                };
                if let Some((mode, bound)) = failed {
                    reports[ri].violations.push(Violation {
                        rule: rule.rule,
                        rule_name: rule.rule_name,
                        layer: DrcLayer::Routing(pass.metal),
                        cut: s.rect,
                        enc_x_dbu: enc_x,
                        enc_y_dbu: enc_y,
                        bound_dbu: bound,
                        failed: mode,
                        net: s.net,
                        group: s.group,
                    });
                }
            }
        }
    }
    reports
}

/// Flat violation list across all 9 rules — the gate's checked value.
pub fn check_enclosure(design: &DrcDesign, tech: &TechDb) -> Vec<Violation> {
    check_enclosure_report(design, tech)
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

    /// met1/met2 routing + `mcon` (`BELOW 0 0`, `ABOVE 30 60`) + `via`
    /// (`BELOW`/`ABOVE 55 85`) — stack order like the real techlef.
    const E_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
END met1
LAYER mcon
  TYPE CUT ;
  WIDTH 0.17 ;
  ENCLOSURE BELOW 0 0 ;
  ENCLOSURE ABOVE 0.03 0.06 ;
END mcon
LAYER via
  TYPE CUT ;
  WIDTH 0.15 ;
  ENCLOSURE BELOW 0.055 0.085 ;
  ENCLOSURE ABOVE 0.055 0.085 ;
END via
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
END LIBRARY
"#;

    /// routing = [met1, met2]; cut = [mcon, via]. Note mcon parses
    /// after met1 here (recorded as cut layer 0), via is cut layer 1 —
    /// `rule.cut` resolves by name, so pass assignment is unaffected.
    fn e_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        parse_lef_bytes(E_LEF.as_bytes(), &mut db, &mut interner).expect("LEF must parse");
        db.strings = interner;
        for net in ["n1", "n2"] {
            db.strings.intern(net);
        }
        db.rebuild_maps();
        db
    }

    fn design_with(m1: Vec<ShapeRect>, m2: Vec<ShapeRect>, cuts: Vec<Vec<ShapeRect>>) -> DrcDesign {
        DrcDesign {
            routing: vec![LayerShapes::from_rects(m1), LayerShapes::from_rects(m2)],
            cut: cuts.into_iter().map(LayerShapes::from_rects).collect(),
        }
    }

    fn metal(tech: &TechDb, r: RectDbu, src: ShapeSrc, net: &str) -> ShapeRect {
        ShapeRect {
            rect: r,
            src,
            net: Some(tech.strings.get(net).unwrap()),
            group: 0,
        }
    }

    fn cut(tech: &TechDb, r: RectDbu, net: &str, group: u32) -> ShapeRect {
        ShapeRect {
            rect: r,
            src: ShapeSrc::Via,
            net: Some(tech.strings.get(net).unwrap()),
            group,
        }
    }

    /// `mcon` is cut index 0, `via` cut index 1 under E_LEF.
    fn mcon_cuts(tech: &TechDb, rs: &[RectDbu]) -> Vec<ShapeRect> {
        rs.iter().map(|&r| cut(tech, r, "n1", 1)).collect()
    }
    fn via_cuts(tech: &TechDb, rs: &[RectDbu]) -> Vec<ShapeRect> {
        rs.iter().map(|&r| cut(tech, r, "n1", 1)).collect()
    }

    /// Rule ids / verbatim names / cut / metal / side / mode — the
    /// golden join key, pinned.
    #[test]
    fn test_enclosure_rules_metadata() {
        assert_eq!(ENCLOSURE_RULES.len(), 9);
        let expect: &[(&str, &str, &str, &str, Side, EncMode)] = &[
            ("met1.4", "Metal1 overlap of local interconnect contact < %d (met1.4)", "mcon", "met1", Side::Above, EncMode::Both),
            ("met1.5", "Metal1 overlap of local interconnect contact < %d in one direction (met1.5)", "mcon", "met1", Side::Above, EncMode::One),
            ("via.5a", "Metal1 overlap of Via1 < %d in one direction (via.5a - via.4a)", "via", "met1", Side::Below, EncMode::Full),
            ("met2.5", "Metal2 overlap of Via1 < %d in one direction (met2.5 - met2.4)", "via", "met2", Side::Above, EncMode::Full),
            ("via2.4a", "Metal2 overlap of via2 < %d in one direction (via2.4a - via2.4)", "via2", "met2", Side::Below, EncMode::Full),
            ("met3.4", "Metal3 overlap of via2 < %d (met3.4)", "via2", "met3", Side::Above, EncMode::Both),
            ("via3.5", "Metal3 overlap of via3 in one direction < %d (via3.5 - via3.4)", "via3", "met3", Side::Below, EncMode::Full),
            ("met4.3", "Metal4 overlap of via3 < %d (met4.3 - via3.4)", "via3", "met4", Side::Above, EncMode::Both),
            ("met5.3", "Metal5 overlap of via4 < %d (met5.3 - via4.4)", "via4", "met5", Side::Above, EncMode::Both),
        ];
        for (got, &(rule, name, cut, metal, side, mode)) in
            ENCLOSURE_RULES.iter().zip(expect)
        {
            assert_eq!(got.rule, rule);
            assert_eq!(got.rule_name, name);
            assert_eq!(got.cut, cut);
            assert_eq!(got.metal, metal);
            assert_eq!(got.side, side);
            assert_eq!(got.mode, mode);
        }
    }

    /// `met1.4` (Both, bound 30): metal covering the mcon cut by
    /// exactly 30 on the min axis (60 on the other, so `met1.5`'s
    /// one-direction bound also holds) passes; 29 on one side flags
    /// `Both` while `met1.5` stays clean.
    #[test]
    fn test_both_bound_strictness() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 170, 170); // mcon cut 170
        let clean = design_with(
            vec![metal(&tech, RectDbu::new(-30, -60, 200, 230), ShapeSrc::Via, "n1")],
            vec![],
            vec![mcon_cuts(&tech, &[c]), vec![]],
        );
        assert!(check_enclosure(&clean, &tech).is_empty());

        let short = design_with(
            vec![metal(&tech, RectDbu::new(-29, -60, 200, 230), ShapeSrc::Via, "n1")],
            vec![],
            vec![mcon_cuts(&tech, &[c]), vec![]],
        );
        let v = check_enclosure(&short, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "met1.4");
        assert_eq!(v[0].failed, EncMode::Both);
        assert_eq!(v[0].bound_dbu, 30);
        assert_eq!(v[0].enc_x_dbu, 29);
    }

    /// `met1.5` (One, bound 60): enc 30/60 passes (y >= 60 on one
    /// axis); 59/30 flags `One`.
    #[test]
    fn test_one_direction_rescue() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 170, 170);
        // x overhang 30, y overhang 60 → met1.5 clean, met1.4 clean.
        let clean = design_with(
            vec![metal(&tech, RectDbu::new(-30, -60, 200, 230), ShapeSrc::Via, "n1")],
            vec![],
            vec![mcon_cuts(&tech, &[c]), vec![]],
        );
        assert!(check_enclosure(&clean, &tech).is_empty());

        // x overhang 59, y overhang 30 → met1.4 passes (both >= 30),
        // met1.5 flags (neither >= 60).
        let d = design_with(
            vec![metal(&tech, RectDbu::new(-59, -30, 229, 200), ShapeSrc::Via, "n1")],
            vec![],
            vec![mcon_cuts(&tech, &[c]), vec![]],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "met1.5");
        assert_eq!(v[0].failed, EncMode::One);
        assert_eq!(v[0].bound_dbu, 60);
    }

    /// met2 rect that encloses `c` cleanly for `met2.5` (85 on all
    /// sides) — the `via` cut layer drives both `via.5a` (met1 side)
    /// and `met2.5` (met2 side), so fixtures isolate `via.5a` by
    /// covering the met2 side.
    fn met2_cover(tech: &TechDb, c: &RectDbu) -> ShapeRect {
        metal(
            tech,
            RectDbu::new(c.xl - 85, c.yl - 85, c.xh + 85, c.yh + 85),
            ShapeSrc::Via,
            "n1",
        )
    }

    /// `via.5a` (Full, via BELOW → met1, pair 55/85): enc 55/55 meets
    /// both-dir but neither axis reaches 85 → `One` fires; enc 40/90
    /// fails both-dir first → `Both`.
    #[test]
    fn test_full_clause_order() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 150, 150);
        let d = design_with(
            vec![metal(&tech, RectDbu::new(-55, -55, 205, 205), ShapeSrc::Via, "n1")],
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "via.5a");
        assert_eq!(v[0].failed, EncMode::One);
        assert_eq!(v[0].bound_dbu, 85);

        let d = design_with(
            vec![metal(&tech, RectDbu::new(-40, -90, 190, 240), ShapeSrc::Via, "n1")],
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "via.5a");
        assert_eq!(v[0].failed, EncMode::Both);
        assert_eq!(v[0].bound_dbu, 55);
    }

    /// Two same-net rects tiling the side slab pass; the same rects on
    /// a *different* net do not enclose — the net filter is the
    /// documented model (foreign metal would be a short anyway).
    #[test]
    fn test_union_coverage_and_net_filter() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 150, 150);
        let tiles = |net: &str| {
            vec![
                metal(&tech, RectDbu::new(-55, -55, 205, 75), ShapeSrc::SignalWire, net),
                metal(&tech, RectDbu::new(-55, 75, 205, 205), ShapeSrc::SignalWire, net),
            ]
        };
        // The pair tiles the full y-span at 55 overhang both axes —
        // both-dir met, one-dir 85 fails → `One`.
        let d = design_with(
            tiles("n1"),
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "via.5a");
        assert_eq!(v[0].failed, EncMode::One);

        // Same coverage on n2 — n1's cut sees no metal → protrudes.
        let d = design_with(
            tiles("n2"),
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].rule, "via.5a");
        assert_eq!(v[0].failed, EncMode::Both);
        assert!(v[0].enc_x_dbu < 0 && v[0].enc_y_dbu < 0);
    }

    /// A same-net wire can rescue a via metal that under-encloses —
    /// candidates are every same-net rect, not only `Via` metals.
    #[test]
    fn test_wire_provided_overhang() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 150, 150);
        let d = design_with(
            vec![
                // Via's own met1: 10 overhang (under-enclosed).
                metal(&tech, RectDbu::new(-10, -10, 160, 160), ShapeSrc::Via, "n1"),
                // Same-net wire covers the whole side slabs at 60.
                metal(&tech, RectDbu::new(-60, -60, 210, 210), ShapeSrc::SignalWire, "n1"),
            ],
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        // enc 60 both dirs, one-dir 85 unmet → only the One clause.
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].failed, EncMode::One);
        assert_eq!(v[0].enc_x_dbu, 60);
    }

    /// Multi-cut array: 3 cuts under one metal rect — interior cuts see
    /// large side overhangs (the metal covers far past their edges);
    /// only the boundary cuts' outer side is at the enclosure margin.
    /// Shrinking the metal flags every cut whose top/bottom band ends
    /// up sub-bound.
    #[test]
    fn test_multi_cut_array_boundary_cuts() {
        let tech = e_tech();
        // 3 cuts side by side, pitch 320, under one metal at 55/85
        // overhang — enc 55/85: both >= 55, max = 85 >= 85 → clean.
        let cuts: Vec<RectDbu> = (0..3)
            .map(|i| RectDbu::new(i * 320, 0, i * 320 + 150, 150))
            .collect();
        let bbox = RectDbu::new(-55, -85, 845, 235);
        let d = design_with(
            vec![metal(&tech, bbox, ShapeSrc::Via, "n1")],
            vec![met2_cover(&tech, &RectDbu::new(-55, -85, 845, 235))],
            vec![vec![], via_cuts(&tech, &cuts)],
        );
        assert!(check_enclosure(&d, &tech).is_empty());

        // Shrink to 10 y-overhang: every cut's top/bottom bands sit 10
        // from the metal edge → all three flag `Both` at 55.
        let d = design_with(
            vec![metal(&tech, RectDbu::new(-55, -10, 845, 160), ShapeSrc::Via, "n1")],
            vec![met2_cover(&tech, &RectDbu::new(-55, -85, 845, 235))],
            vec![vec![], via_cuts(&tech, &cuts)],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 3, "{v:?}");
        assert!(
            v.iter()
                .all(|v| v.rule == "via.5a" && v.failed == EncMode::Both)
        );
    }

    /// Cut edge uncovered on a side → nonpositive overhang, violation.
    #[test]
    fn test_protrusion_negative_overhang() {
        let tech = e_tech();
        let c = RectDbu::new(0, 0, 150, 150);
        // Metal flush with the cut's left edge: 0 overhang → < 55 flags.
        let d = design_with(
            vec![metal(&tech, RectDbu::new(0, -85, 235, 235), ShapeSrc::Via, "n1")],
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].failed, EncMode::Both);
        assert_eq!(v[0].enc_x_dbu, 0, "metal flush with the edge = 0 overhang");

        // Cut with no same-net metal at all → enc = -(cut span).
        let d = design_with(
            vec![],
            vec![met2_cover(&tech, &c)],
            vec![vec![], via_cuts(&tech, &[c])],
        );
        let v = check_enclosure(&d, &tech);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].enc_x_dbu, -150);
        assert_eq!(v[0].enc_y_dbu, -150);
    }

    /// Cut/metal layers the LEF lacks (via2/via3/via4/met3+ here)
    /// contribute nothing — no panic on absent layers or empty
    /// `LayerShapes`.
    #[test]
    fn test_absent_layers_and_zero_slots() {
        let tech = e_tech();
        let d = design_with(vec![], vec![], vec![vec![], vec![]]);
        let reports = check_enclosure_report(&d, &tech);
        assert_eq!(reports.len(), 9);
        // via2/via3/via4 rules and their met3+ planes are absent.
        assert_eq!(
            reports.iter().filter(|r| r.layer.is_none()).count(),
            5,
            "{reports:?}"
        );
        assert!(reports.iter().all(|r| r.violations.is_empty()));
    }

    /// The rule table's `side`/`metal` pairing must match the stack
    /// arithmetic on the real techlef: `Below` reads `cut.above`'s
    /// routing layer, `Above` the next routing layer up.
    #[test]
    fn test_side_pairing_matches_stack() {
        let lib_dir = crate::test_support::sky130_hd();
        let tlef = crate::test_support::require(
            lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
        );
        let lef = crate::test_support::require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
        let mut interner = StringInterner::new();
        let tech = crate::readers::load_tech_db(&tlef, &lef, &mut interner)
            .expect("load_tech_db")
            .0;
        for rule in ENCLOSURE_RULES {
            let ci = tech.cut_layer_by_name[&tech.strings.get(rule.cut).unwrap()];
            let below = tech.cut_layers[ci as usize].above;
            let expected = match rule.side {
                Side::Below => below,
                Side::Above => LayerId(below.0 + 1),
            };
            let metal = tech.layer_by_name[&tech.strings.get(rule.metal).unwrap()];
            assert_eq!(
                metal, expected,
                "{}: metal {} is not the {:?} plane of {}",
                rule.rule, rule.metal, rule.side, rule.cut
            );
        }
    }
}
