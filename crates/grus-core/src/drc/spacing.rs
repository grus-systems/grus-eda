//! The spacing rule family: 20 `lef_expressible` spacing rules checked
//! over same-net connectivity islands.
//!
//! Bounds come from LEF — `SPACING`/`SPACINGTABLE` retained per layer
//! (`RoutingLayer::min_spacing_dbu`/`wide_spacing`,
//! `CutLayer::spacing_dbu`); `rule_name` strings are verbatim
//! `magic_rules_checked.names` templates so results join
//! `magic_per_rule` by the `(id)` inside the parens — the same
//! convention `width` uses.
//!
//! Merge semantics (Magic's connectivity model): same-net shapes that
//! touch or overlap — corner contact counts — form one island and are
//! never flagged; all rects of one via instance (`ShapeRect::group`)
//! merge unconditionally, which is what chains islands across layers
//! (a cut array's cuts merge through the via's metals, not through
//! cut-layer proximity). A pair in different islands — different nets,
//! or same-net *separated* islands — flags iff the Euclidean gap is in
//! `(0, bound)` computed integer-exact (`gx² + gy² < bound²`; the
//! reference ran `drc euclidean on`). Distance-0 different-island
//! pairs are exempt — every deck spacing statement carries
//! `touching_ok`.
//!
//! Checked sources: routing layers check
//! `SignalWire`/`SpecialWire`/`Via`/`IoPort`, cut layers check `Via`.
//! `MacroPin`/`MacroObs` never generate findings (LEF abstracts, not
//! the fabricated GDS Magic measures) but net-tagged rects still join
//! the union-find so pin metal connects same-net islands — excluding
//! them can only under-report vs Magic.

use crate::drc::{DrcDesign, DrcLayer, LayerShapes, ShapeSrc};
use crate::interner::SymbolId;
use crate::tech_db::{RectDbu, TechDb};
use std::collections::HashMap;

/// Which check a spacing rule applies — Magic evaluates each deck
/// statement independently, so one pair can flag under both a layer's
/// `Min` and its wide-metal statements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpacingKind {
    /// Minimum same-layer spacing (LEF `SPACING` / lowest
    /// `SPACINGTABLE` row).
    Min,
    /// `MetalN > 3um spacing to unrelated mN` (`widespacing`): fires
    /// when either side's `min(w,h)` exceeds the LEF `WIDTH` threshold.
    WideUnrelated,
    /// `features attached to and within <window>um of large metalN`
    /// (`cifspacing <large_halo>`): fires when a non-wide side sits in
    /// a wide rect's island within `window` of it. `window` is the
    /// rule's bound — the names literally say "within 0.28/0.40um".
    WideAttached,
}

/// One LEF-expressible spacing rule. `layer` is a LEF routing-layer
/// name unless `cut` is set, then a `TYPE CUT` name.
pub struct SpacingRule {
    /// The `(id)` inside `rule_name`'s parens — the `magic_per_rule`
    /// join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template, `%d` intact.
    pub rule_name: &'static str,
    pub layer: &'static str,
    pub cut: bool,
    pub kind: SpacingKind,
}

/// The 20 spacing rules, `spacing.md` table order. Deck id
/// irregularities are pinned, not "fixed": the met3 pair is `3c`/`3d`,
/// met4 is `5a`/`5b`, and each `*b`/`*d` name appears twice in
/// `magic_rules_checked.names` (the deck's `allmN` + `*obsmN`
/// `widespacing` statements share one rule row here — our
/// checked-source model has no obs plane).
pub const SPACING_RULES: &[SpacingRule] = &[
    SpacingRule {
        rule: "li.3",
        rule_name: "Local interconnect spacing < %d (li.3)",
        layer: "li1",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "li.c2",
        rule_name: "Core local interconnect spacing < %d (li.c2)",
        layer: "li1",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "mcon.2",
        rule_name: "mcon.spacing < %d (mcon.2)",
        layer: "mcon",
        cut: true,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "via.2",
        rule_name: "Via1 spacing < %d (via.2 - 2 * via.4a)",
        layer: "via",
        cut: true,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "via2.2",
        rule_name: "via2 spacing < %d (via2.2 - 2 * via2.4)",
        layer: "via2",
        cut: true,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "via3.2",
        rule_name: "via3 spacing < %d (via3.2 - 2 * via3.4)",
        layer: "via3",
        cut: true,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "via4.2",
        rule_name: "via4 spacing < %d (via4.2 - 2 * via4.4)",
        layer: "via4",
        cut: true,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met1.2",
        rule_name: "Metal1 spacing < %d (met1.2)",
        layer: "met1",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met2.2",
        rule_name: "Metal2 spacing < %d (met2.2)",
        layer: "met2",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met3.2",
        rule_name: "Metal3 spacing < %d (met3.2)",
        layer: "met3",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met4.2",
        rule_name: "Metal4 spacing < %d (met4.2)",
        layer: "met4",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met5.2",
        rule_name: "Metal5 spacing < %d (met5.2)",
        layer: "met5",
        cut: false,
        kind: SpacingKind::Min,
    },
    SpacingRule {
        rule: "met1.3a",
        rule_name: "Spacing of metal1 features attached to and within 0.28um of large metal1 < %d (met1.3a)",
        layer: "met1",
        cut: false,
        kind: SpacingKind::WideAttached,
    },
    SpacingRule {
        rule: "met1.3b",
        rule_name: "Metal1 > 3um spacing to unrelated m1 < %d (met1.3b)",
        layer: "met1",
        cut: false,
        kind: SpacingKind::WideUnrelated,
    },
    SpacingRule {
        rule: "met2.3a",
        rule_name: "Spacing of metal2 features attached to and within 0.28um of large metal2 < %d (met2.3a)",
        layer: "met2",
        cut: false,
        kind: SpacingKind::WideAttached,
    },
    SpacingRule {
        rule: "met2.3b",
        rule_name: "Metal2 > 3um spacing to unrelated m2 < %d (met2.3b)",
        layer: "met2",
        cut: false,
        kind: SpacingKind::WideUnrelated,
    },
    SpacingRule {
        rule: "met3.3c",
        rule_name: "Spacing of metal3 features attached to and within 0.40um of large metal3 < %d (met3.3c)",
        layer: "met3",
        cut: false,
        kind: SpacingKind::WideAttached,
    },
    SpacingRule {
        rule: "met3.3d",
        rule_name: "Metal3 > 3um spacing to unrelated m3 < %d (met3.3d)",
        layer: "met3",
        cut: false,
        kind: SpacingKind::WideUnrelated,
    },
    SpacingRule {
        rule: "met4.5a",
        rule_name: "Spacing of metal4 features attached to and within 0.40um of large metal4 < %d (met4.5a)",
        layer: "met4",
        cut: false,
        kind: SpacingKind::WideAttached,
    },
    SpacingRule {
        rule: "met4.5b",
        rule_name: "Metal4 > 3um spacing to unrelated m4 < %d (met4.5b)",
        layer: "met4",
        cut: false,
        kind: SpacingKind::WideUnrelated,
    },
];

/// A sub-bound gap between two rects in different connectivity
/// islands — the offending pair, not a merged Magic region.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// `(id)` inside the rule_name parens — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template — `%d` intact.
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// The offending pair.
    pub a: RectDbu,
    pub b: RectDbu,
    /// Per-axis separations; the Euclidean gap is `sqrt(gx²+gy²)` and
    /// the test compares squares — no float stored.
    pub gap_x_dbu: i32,
    pub gap_y_dbu: i32,
    pub bound_dbu: i32,
    pub net_a: Option<SymbolId>,
    pub net_b: Option<SymbolId>,
}

/// Per-rule outcome, for the zero-violation census in the integration
/// gate (candidate counts name rules a design does not exercise).
pub struct RuleReport {
    pub rule: &'static str,
    /// `None` when the rule's layer is absent from the loaded LEF.
    pub layer: Option<DrcLayer>,
    /// Resolved LEF bound; 0 when the layer carries no spacing data.
    pub bound_dbu: i32,
    /// Different-island, non-touching pairs within the Euclidean bound
    /// — what the kind filter then qualifies.
    pub candidates: usize,
    pub violations: Vec<Violation>,
}

/// Union-find over the flat all-layer rect space.
struct Islands {
    parent: Vec<u32>,
    /// Flat-index base per routing layer.
    routing_base: Vec<usize>,
    /// Flat-index base per cut layer (after all routing rects).
    cut_base: Vec<usize>,
}

impl Islands {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.parent[x as usize] != x {
            let p = self.parent[x as usize];
            self.parent[x as usize] = self.parent[p as usize];
            x = self.parent[x as usize];
        }
        x
    }

    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra as usize] = rb;
        }
    }
}

/// Per-axis separation: both 0 iff the rects touch or overlap.
fn axis_gap(a: &RectDbu, b: &RectDbu) -> (i32, i32) {
    let gx = (a.xl - b.xh).max(b.xl - a.xh).max(0);
    let gy = (a.yl - b.yh).max(b.yl - a.yh).max(0);
    (gx, gy)
}

fn min_dim(r: &RectDbu) -> i32 {
    r.width().min(r.height())
}

/// Which sources generate findings on a rule's layer (macro rects and
/// uncheckable shapes still participate in the island merge).
fn checkable(src: ShapeSrc, cut: bool) -> bool {
    if cut {
        src == ShapeSrc::Via
    } else {
        matches!(
            src,
            ShapeSrc::SignalWire | ShapeSrc::SpecialWire | ShapeSrc::Via | ShapeSrc::IoPort
        )
    }
}

/// Connectivity islands: same-net same-layer touching/overlap merged
/// transitively, plus unconditional per-via-instance group merge. Merge
/// membership spans every rect in the design — checkability is a
/// finding filter, not a connectivity one.
fn build_islands(design: &DrcDesign) -> Islands {
    let mut routing_base = Vec::with_capacity(design.routing.len());
    let mut cut_base = Vec::with_capacity(design.cut.len());
    let mut n = 0;
    for ls in &design.routing {
        routing_base.push(n);
        n += ls.rects.len();
    }
    for ls in &design.cut {
        cut_base.push(n);
        n += ls.rects.len();
    }
    let mut uf = Islands {
        parent: (0..n as u32).collect(),
        routing_base,
        cut_base,
    };

    // Same-layer pass: same-net rects touching or overlapping merge.
    // reach=1 yields per-axis separations < 1, i.e. gx == 0 && gy == 0
    // — touch-or-overlap including corner contact (it only ever adds
    // connectivity).
    let bases: Vec<usize> = uf
        .routing_base
        .iter()
        .chain(uf.cut_base.iter())
        .copied()
        .collect();
    for (ls, &base) in design
        .routing
        .iter()
        .chain(design.cut.iter())
        .zip(bases.iter())
    {
        ls.for_each_pair_within(1, |j, k| {
            let (a, b) = (&ls.rects[j as usize], &ls.rects[k as usize]);
            if let (Some(na), Some(nb)) = (a.net, b.net)
                && na == nb
            {
                uf.union(base as u32 + j, base as u32 + k);
            }
        });
    }

    // Instance pass: all rects sharing one `group` id merge — the via
    // metals carry connectivity across layers.
    let mut first_seen: HashMap<u32, u32> = HashMap::new();
    let mut flat = 0u32;
    for ls in design.routing.iter().chain(design.cut.iter()) {
        for s in &ls.rects {
            if s.group != 0 {
                match first_seen.get(&s.group) {
                    Some(&f) => uf.union(f, flat),
                    None => {
                        first_seen.insert(s.group, flat);
                    }
                }
            }
            flat += 1;
        }
    }
    uf
}

/// Resolve a rule's layer, check bound, and wide threshold (the strict
/// `min(w,h) > threshold` qualifier for `WideUnrelated`, the mark
/// threshold + window base for `WideAttached`). `None` when the layer
/// is absent; `bound == 0` when the layer carries no spacing data.
/// `base` is the layer's flat-index offset in `uf`.
fn resolve_rule<'a>(
    rule: &SpacingRule,
    design: &'a DrcDesign,
    tech: &TechDb,
    uf: &Islands,
) -> Option<(DrcLayer, &'a LayerShapes, usize, i32, Option<i32>)> {
    if rule.cut {
        let &idx = tech
            .strings
            .get(rule.layer)
            .and_then(|s| tech.cut_layer_by_name.get(&s))?;
        let bound = tech.cut_layers[idx as usize].spacing_dbu;
        Some((
            DrcLayer::Cut(idx),
            &design.cut[idx as usize],
            uf.cut_base[idx as usize],
            bound,
            None,
        ))
    } else {
        let &lid = tech
            .strings
            .get(rule.layer)
            .and_then(|s| tech.layer_by_name.get(&s))?;
        let rl = &tech.routing_layers[lid.0 as usize];
        let (bound, wide) = match rule.kind {
            SpacingKind::Min => (rl.min_spacing_dbu(), None),
            _ => match rl.wide_spacing() {
                Some((thr, b)) => (b, Some(thr)),
                // No wide row — both wide statements contribute nothing;
                // the `Some` report still records the resolved layer.
                None => (0, None),
            },
        };
        Some((
            DrcLayer::Routing(lid),
            &design.routing[lid.0 as usize],
            uf.routing_base[lid.0 as usize],
            bound,
            wide,
        ))
    }
}

/// `WideAttached` marks: non-wide rects in a wide rect's island within
/// `window` (Euclidean, inclusive — "within 0.28/0.40um") of a same-
/// island member whose `min(w,h) > threshold`. Returns per-`rects`
/// bools. `uf`/`base` index the flat island space.
fn wide_marks(
    ls: &LayerShapes,
    uf: &mut Islands,
    base: usize,
    threshold: i32,
    window: i32,
) -> Vec<bool> {
    let w2 = window as i64 * window as i64;
    let mut marked = vec![false; ls.rects.len()];
    // reach = window + 1 keeps gx <= window candidates — a strict
    // superset of the Euclidean <= window test.
    ls.for_each_pair_within(window + 1, |j, k| {
        let (a, b) = (&ls.rects[j as usize], &ls.rects[k as usize]);
        let (narrow, wide_idx) = match (
            min_dim(&a.rect) <= threshold,
            min_dim(&b.rect) <= threshold,
        ) {
            (true, false) => (j, k),
            (false, true) => (k, j),
            _ => return, // both wide or both narrow — `*a` never marks
        };
        if uf.find(base as u32 + narrow) != uf.find(base as u32 + wide_idx) {
            return;
        }
        let (narrow_r, wide_r) = (
            &ls.rects[narrow as usize].rect,
            &ls.rects[wide_idx as usize].rect,
        );
        let (gx, gy) = axis_gap(narrow_r, wide_r);
        if gx as i64 * gx as i64 + gy as i64 * gy as i64 <= w2 {
            marked[narrow as usize] = true;
        }
    });
    marked
}

/// Every resolvable `SPACING_RULES` rule over `design`, one
/// `RuleReport` per rule (skipped rules report `layer`/`bound` as
/// resolved and empty candidates).
pub fn check_spacing_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport> {
    let mut uf = build_islands(design);
    let mut out = Vec::with_capacity(SPACING_RULES.len());
    for rule in SPACING_RULES {
        let mut report = RuleReport {
            rule: rule.rule,
            layer: None,
            bound_dbu: 0,
            candidates: 0,
            violations: Vec::new(),
        };
        let Some((layer, ls, base, bound, wide)) = resolve_rule(rule, design, tech, &uf) else {
            out.push(report);
            continue;
        };
        report.layer = Some(layer);
        report.bound_dbu = bound;
        if bound <= 0 {
            out.push(report);
            continue;
        }
        let b2 = bound as i64 * bound as i64;
        let marked = match (rule.kind, wide) {
            (SpacingKind::WideAttached, Some(thr)) => {
                Some(wide_marks(ls, &mut uf, base, thr, bound))
            }
            _ => None,
        };
        // The reach=bound sweep yields gx < bound && gy < bound — a
        // strict superset of the Euclidean candidates
        // (euclid >= max(gx, gy)); the squared test filters the corner
        // cases.
        ls.for_each_pair_within(bound, |j, k| {
            let (a, b) = (&ls.rects[j as usize], &ls.rects[k as usize]);
            if !checkable(a.src, rule.cut) || !checkable(b.src, rule.cut) {
                return;
            }
            if uf.find(base as u32 + j) == uf.find(base as u32 + k) {
                return;
            }
            let (gx, gy) = axis_gap(&a.rect, &b.rect);
            if gx == 0 && gy == 0 {
                return; // touching_ok
            }
            if gx as i64 * gx as i64 + gy as i64 * gy as i64 >= b2 {
                return;
            }
            report.candidates += 1;
            let qualifies = match (rule.kind, wide) {
                (SpacingKind::Min, _) => true,
                (SpacingKind::WideUnrelated, Some(thr)) => {
                    min_dim(&a.rect) > thr || min_dim(&b.rect) > thr
                }
                (SpacingKind::WideAttached, Some(thr)) => {
                    (marked.as_ref().unwrap()[j as usize]
                        || marked.as_ref().unwrap()[k as usize])
                        && min_dim(&a.rect) <= thr
                        && min_dim(&b.rect) <= thr
                }
                _ => false,
            };
            if qualifies {
                report.violations.push(Violation {
                    rule: rule.rule,
                    rule_name: rule.rule_name,
                    layer,
                    a: a.rect,
                    b: b.rect,
                    gap_x_dbu: gx,
                    gap_y_dbu: gy,
                    bound_dbu: bound,
                    net_a: a.net,
                    net_b: b.net,
                });
            }
        });
        out.push(report);
    }
    out
}

/// Flat violation list across all 20 rules — the gate's checked value.
pub fn check_spacing(design: &DrcDesign, tech: &TechDb) -> Vec<Violation> {
    check_spacing_report(design, tech)
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

    /// met1 + met2 with `SPACINGTABLE` 140 / 280@3000, cut `via` with
    /// `SPACING 0.17` — stack order met1/via/met2 like the real LEF.
    const S_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
  SPACINGTABLE
     PARALLELRUNLENGTH 0
     WIDTH 0 0.14
     WIDTH 3 0.28 ;
END met1
LAYER via
  TYPE CUT ;
  WIDTH 0.15 ;
  SPACING 0.17 ;
END via
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
  SPACINGTABLE
     PARALLELRUNLENGTH 0
     WIDTH 0 0.14
     WIDTH 3 0.28 ;
END met2
END LIBRARY
"#;

    fn s_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        parse_lef_bytes(S_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("LEF must parse");
        tech_db.strings = interner;
        for net in ["n1", "n2"] {
            tech_db.strings.intern(net);
        }
        tech_db.rebuild_maps();
        tech_db
    }

    /// routing = [met1, met2]; cut = [via]. `m1`/`m2` shapes carry
    /// `SignalWire` src, cut shapes `Via`.
    fn design_with(
        m1: Vec<ShapeRect>,
        m2: Vec<ShapeRect>,
        cut: Vec<ShapeRect>,
    ) -> DrcDesign {
        DrcDesign {
            routing: vec![LayerShapes::from_rects(m1), LayerShapes::from_rects(m2)],
            cut: vec![LayerShapes::from_rects(cut)],
        }
    }

    fn wire(tech: &TechDb, r: RectDbu, net: &str) -> ShapeRect {
        ShapeRect {
            rect: r,
            src: ShapeSrc::SignalWire,
            net: Some(tech.strings.get(net).unwrap()),
            group: 0,
        }
    }

    fn cut_shape(tech: &TechDb, r: RectDbu, net: &str, group: u32) -> ShapeRect {
        ShapeRect {
            rect: r,
            src: ShapeSrc::Via,
            net: Some(tech.strings.get(net).unwrap()),
            group,
        }
    }

    /// Rule ids / verbatim names / layer / cut / kind — the golden join
    /// key, pinned including the deck's `3c`/`3d`/`5a`/`5b` irregularities.
    #[test]
    fn test_spacing_rules_metadata() {
        assert_eq!(SPACING_RULES.len(), 20);
        let expect: &[(&str, &str, &str, bool, SpacingKind)] = &[
            ("li.3", "Local interconnect spacing < %d (li.3)", "li1", false, SpacingKind::Min),
            ("li.c2", "Core local interconnect spacing < %d (li.c2)", "li1", false, SpacingKind::Min),
            ("mcon.2", "mcon.spacing < %d (mcon.2)", "mcon", true, SpacingKind::Min),
            ("via.2", "Via1 spacing < %d (via.2 - 2 * via.4a)", "via", true, SpacingKind::Min),
            ("via2.2", "via2 spacing < %d (via2.2 - 2 * via2.4)", "via2", true, SpacingKind::Min),
            ("via3.2", "via3 spacing < %d (via3.2 - 2 * via3.4)", "via3", true, SpacingKind::Min),
            ("via4.2", "via4 spacing < %d (via4.2 - 2 * via4.4)", "via4", true, SpacingKind::Min),
            ("met1.2", "Metal1 spacing < %d (met1.2)", "met1", false, SpacingKind::Min),
            ("met2.2", "Metal2 spacing < %d (met2.2)", "met2", false, SpacingKind::Min),
            ("met3.2", "Metal3 spacing < %d (met3.2)", "met3", false, SpacingKind::Min),
            ("met4.2", "Metal4 spacing < %d (met4.2)", "met4", false, SpacingKind::Min),
            ("met5.2", "Metal5 spacing < %d (met5.2)", "met5", false, SpacingKind::Min),
            ("met1.3a", "Spacing of metal1 features attached to and within 0.28um of large metal1 < %d (met1.3a)", "met1", false, SpacingKind::WideAttached),
            ("met1.3b", "Metal1 > 3um spacing to unrelated m1 < %d (met1.3b)", "met1", false, SpacingKind::WideUnrelated),
            ("met2.3a", "Spacing of metal2 features attached to and within 0.28um of large metal2 < %d (met2.3a)", "met2", false, SpacingKind::WideAttached),
            ("met2.3b", "Metal2 > 3um spacing to unrelated m2 < %d (met2.3b)", "met2", false, SpacingKind::WideUnrelated),
            ("met3.3c", "Spacing of metal3 features attached to and within 0.40um of large metal3 < %d (met3.3c)", "met3", false, SpacingKind::WideAttached),
            ("met3.3d", "Metal3 > 3um spacing to unrelated m3 < %d (met3.3d)", "met3", false, SpacingKind::WideUnrelated),
            ("met4.5a", "Spacing of metal4 features attached to and within 0.40um of large metal4 < %d (met4.5a)", "met4", false, SpacingKind::WideAttached),
            ("met4.5b", "Metal4 > 3um spacing to unrelated m4 < %d (met4.5b)", "met4", false, SpacingKind::WideUnrelated),
        ];
        for (got, &(rule, name, layer, cut, kind)) in SPACING_RULES.iter().zip(expect) {
            assert_eq!(got.rule, rule);
            assert_eq!(got.rule_name, name);
            assert_eq!(got.layer, layer);
            assert_eq!(got.cut, cut);
            assert_eq!(got.kind, kind);
        }
    }

    /// met2.2 bound 140: a different-net pair flags at gap 139, not at
    /// 140 or 141 (strict `<`).
    #[test]
    fn test_gap_bound_strictness() {
        let tech = s_tech();
        let pair = |gap| {
            design_with(
                vec![],
                vec![
                    wire(&tech, RectDbu::new(0, 0, 100, 100), "n1"),
                    wire(&tech, RectDbu::new(100 + gap, 0, 200 + gap, 100), "n2"),
                ],
                vec![],
            )
        };
        assert_eq!(check_spacing(&pair(139), &tech).len(), 1);
        assert!(check_spacing(&pair(140), &tech).is_empty());
        assert!(check_spacing(&pair(141), &tech).is_empty());
    }

    /// `touching_ok`: different-net pairs at distance 0 — edge-abutting,
    /// corner-touching, overlapping — never flag.
    #[test]
    fn test_touching_ok_exempts_distance_zero() {
        let tech = s_tech();
        for b in [
            RectDbu::new(100, 0, 200, 100),  // edge abut
            RectDbu::new(100, 100, 200, 200), // corner touch
            RectDbu::new(50, 50, 200, 150),  // overlap
        ] {
            let d = design_with(vec![], vec![
                wire(&tech, RectDbu::new(0, 0, 100, 100), "n1"),
                wire(&tech, b, "n2"),
            ], vec![]);
            assert!(check_spacing(&d, &tech).is_empty(), "rect {b:?} must be exempt");
        }
    }

    /// Euclidean, not Manhattan: `drc euclidean on` means a diagonal
    /// corner gap of `sqrt(gx²+gy²)` counts — `max(gx,gy)` would flag
    /// the 99/99 pair, the squared test does not.
    #[test]
    fn test_euclidean_gap() {
        let tech = s_tech();
        let pair = |d| {
            design_with(vec![], vec![
                wire(&tech, RectDbu::new(0, 0, 100, 100), "n1"),
                wire(&tech, RectDbu::new(100 + d, 100 + d, 200 + d, 200 + d), "n2"),
            ], vec![])
        };
        // 99²·2 = 19602 ≥ 19600 = 140² → exempt; 98²·2 = 19208 < 19600 → flags.
        assert!(check_spacing(&pair(99), &tech).is_empty());
        assert_eq!(check_spacing(&pair(98), &tech).len(), 1);
    }

    /// Same-net touching merges transitively: `A—B—C` is one island
    /// even though `A`/`C` are a sub-bound gap apart; the same pair
    /// without the connector is two islands and flags `met2.2`;
    /// abutting same-net stubs merge.
    #[test]
    fn test_same_net_island_merge() {
        let tech = s_tech();
        let a = RectDbu::new(0, 0, 100, 100);
        let c = RectDbu::new(130, 0, 230, 100); // 30 gap from A
        let bridged = design_with(vec![], vec![
            wire(&tech, a, "n1"),
            wire(&tech, RectDbu::new(100, -5, 130, 105), "n1"), // touches both
            wire(&tech, c, "n1"),
        ], vec![]);
        assert!(check_spacing(&bridged, &tech).is_empty());

        let separated = design_with(vec![], vec![
            wire(&tech, a, "n1"),
            wire(&tech, c, "n1"),
        ], vec![]);
        let v = check_spacing(&separated, &tech);
        assert_eq!(v.len(), 1, "separated same-net islands must be checked");
        assert_eq!(v[0].rule, "met2.2");

        let abutting = design_with(vec![], vec![
            wire(&tech, a, "n1"),
            wire(&tech, RectDbu::new(100, 0, 200, 100), "n1"),
        ], vec![]);
        assert!(check_spacing(&abutting, &tech).is_empty());
    }

    /// Via-instance `group` merges unconditionally — two cuts of one
    /// instance 30 apart are one island; the same pair ungrouped flags
    /// `via.2` (same-net separated islands are checked).
    #[test]
    fn test_via_instance_group_merge() {
        let tech = s_tech();
        let cut = |group| {
            design_with(vec![], vec![], vec![
                cut_shape(&tech, RectDbu::new(0, 0, 100, 100), "n1", group),
                cut_shape(&tech, RectDbu::new(130, 0, 230, 100), "n1", group),
            ])
        };
        assert!(check_spacing(&cut(7), &tech).is_empty());
        let v = check_spacing(&cut(0), &tech);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "via.2");
        assert_eq!(v[0].bound_dbu, 170);
    }

    /// `*b`: a `min(w,h) > 3000` side vs unrelated metal fires
    /// `met1.3b` at 200 (past base 140); at 100 both `met1.2` and
    /// `met1.3b` — independent statements. `min == 3000` is not wide
    /// (strict `>`, the `RANGE 3.001` reading).
    #[test]
    fn test_wide_unrelated() {
        let tech = s_tech();
        let rules = |v: &[Violation]| -> Vec<&'static str> {
            let mut r: Vec<_> = v.iter().map(|v| v.rule).collect();
            r.sort();
            r
        };
        let design = |wide_min, gap| {
            design_with(vec![
                wire(&tech, RectDbu::new(0, 0, wide_min, wide_min), "n1"),
                wire(&tech, RectDbu::new(wide_min + gap, 0, wide_min + gap + 140, 140), "n2"),
            ], vec![], vec![])
        };
        assert_eq!(rules(&check_spacing(&design(3100, 200), &tech)), ["met1.3b"]);
        assert_eq!(
            rules(&check_spacing(&design(3100, 100), &tech)),
            ["met1.2", "met1.3b"]
        );
        assert_eq!(rules(&check_spacing(&design(3000, 100), &tech)), ["met1.2"]);
    }

    /// `*a`: a non-wide stub in a wide plate's island (within
    /// `window` = bound of the plate) must keep the wide bound from
    /// unrelated metal. Disjoint from `*b`: the plate-vs-unrelated
    /// pair is `3b`'s, and a stub outside `window` unmarks.
    #[test]
    fn test_wide_attached() {
        let tech = s_tech();
        let rules = |v: &[Violation]| -> Vec<&'static str> {
            let mut r: Vec<_> = v.iter().map(|v| v.rule).collect();
            r.sort();
            r
        };
        // plate (wide, min 3100) + touching stub, same net → one island;
        // unrelated narrow 200 from the stub, 340 from the plate.
        let attached = design_with(vec![
            wire(&tech, RectDbu::new(0, 0, 3100, 3100), "n1"),
            wire(&tech, RectDbu::new(3100, 100, 3240, 240), "n1"),
            wire(&tech, RectDbu::new(3440, 100, 3580, 240), "n2"),
        ], vec![], vec![]);
        assert_eq!(rules(&check_spacing(&attached, &tech)), ["met1.3a"]);

        // Same stub 281 from the plate — outside `window` = 280, and a
        // separated same-net island anyway: no wide member in its
        // island, nothing fires.
        let detached = design_with(vec![
            wire(&tech, RectDbu::new(0, 0, 3100, 3100), "n1"),
            wire(&tech, RectDbu::new(3381, 100, 3521, 240), "n1"),
            wire(&tech, RectDbu::new(3721, 100, 3861, 240), "n2"),
        ], vec![], vec![]);
        assert!(check_spacing(&detached, &tech).is_empty());

        // The wide side itself vs unrelated metal is `3b`, not `3a`.
        let plate_side = design_with(vec![
            wire(&tech, RectDbu::new(0, 0, 3100, 3100), "n1"),
            wire(&tech, RectDbu::new(3300, 100, 3440, 240), "n2"),
        ], vec![], vec![]);
        assert_eq!(rules(&check_spacing(&plate_side, &tech)), ["met1.3b"]);
    }

    /// Layers the LEF lacks (`li1`, `mcon`, `met3+`, `via2+`) contribute
    /// nothing — no panic on absent layers or empty `LayerShapes`.
    #[test]
    fn test_missing_layers_contribute_nothing() {
        let tech = s_tech();
        let d = design_with(
            vec![wire(&tech, RectDbu::new(0, 0, 1, 1), "n1")],
            vec![],
            vec![],
        );
        let reports = check_spacing_report(&d, &tech);
        assert_eq!(reports.len(), 20);
        assert!(reports.iter().all(|r| r.violations.is_empty()));
        assert!(reports.iter().any(|r| r.layer.is_none()));
    }
}
