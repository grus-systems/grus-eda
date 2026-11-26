//! The unified subset report: one normalized `SubsetReport` over all
//! five rule tables, shared by the `grus-eda drc` CLI and the
//! integration gate so both read the same numbers through the same
//! `magic_per_rule` join. Pure glue — no checking logic lives here.
//!
//! Count units are named, not hidden: our finding count is
//! rect/pair/component-based; Magic's `magic_per_rule` is merged error
//! regions. Equal at zero, divergent when nonzero (the met2 sanity
//! control: Magic 3 regions vs our pair count). `Finding.bboxes` is
//! the list a later `drc listall` comparison consumes.

use crate::drc::enclosure::EncMode;
use crate::drc::{DrcDesign, DrcLayer, area, enclosure, spacing, width};
use crate::interner::{StringInterner, SymbolId};
use crate::tech_db::{RectDbu, TechDb};
use serde::Serialize;

/// Which family produced an outcome — grouping only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Width,
    Spacing,
    Enclosure,
    Area,
    Hole,
}

/// The measured quantity, in the bound's units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Measured {
    /// `min(w,h)` of the flagged rect.
    Width { min_side_dbu: i32 },
    /// Per-axis separations; Euclidean gap is sqrt(gx²+gy²), the
    /// check compares squares — no float stored.
    Spacing { gap_x_dbu: i32, gap_y_dbu: i32 },
    /// Same-net side overhangs plus the failing clause.
    Enclosure {
        enc_x_dbu: i32,
        enc_y_dbu: i32,
        failed: EncMode,
    },
    /// Component union area (min-area) or void area (holes).
    Area { area_dbu2: i64 },
}

/// One finding, normalized across families.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    /// Offending geometry in DBU: width → [rect]; spacing → [a, b]
    /// (`a` is the earlier-`xl` member, ties by rects index — the
    /// sweep order, not a full lexicographic sort); enclosure →
    /// [cut]; area/hole → [region]. This is the list a later
    /// `drc listall` comparison consumes (Magic emits µm; divide by
    /// dbu_per_micron).
    pub bboxes: Vec<RectDbu>,
    /// The bound applied, DBU or DBU² per `measured.kind`.
    pub bound: i64,
    pub measured: Measured,
    /// Net names involved (0–2 entries, resolved via the design
    /// interner); empty for untagged shapes.
    pub nets: Vec<String>,
}

/// One implemented rule's outcome — all 50 always present.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleOutcome {
    pub family: Family,
    /// `(id)` token — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template.
    pub rule_name: &'static str,
    /// Resolved LEF layer name; `None` when unresolvable.
    pub layer: Option<String>,
    /// `None` when the rule is unresolvable or its bound is
    /// undeclared (matches the families' `layer: None` / `bound: 0`
    /// convention, folded into one optional).
    pub bound: Option<i64>,
    pub candidates: usize,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SubsetReport {
    /// Table order: 13 width, 20 spacing, 9 enclosure, 6 area,
    /// 2 hole — 50 outcomes, deterministic emission order.
    pub rules: Vec<RuleOutcome>,
}

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

fn nets(strings: &StringInterner, ids: impl IntoIterator<Item = Option<SymbolId>>) -> Vec<String> {
    ids.into_iter()
        .flatten()
        .map(|s| strings.resolve(s).to_string())
        .collect()
}

/// Runs all five rule tables over `design` and normalizes the
/// outcomes. `strings` is the *design* interner (`DesignDb::strings`
/// — `ShapeRect.net` ids belong to it, not to `tech.strings`).
pub fn run_all(design: &DrcDesign, tech: &TechDb, strings: &StringInterner) -> SubsetReport {
    let mut rules = Vec::with_capacity(IMPLEMENTED_RULES);

    for rep in width::check_width_report(design, tech) {
        let rule_name = width::WIDTH_RULES
            .iter()
            .find(|r| r.rule == rep.rule)
            .map(|r| r.rule_name)
            .expect("width report ids come from WIDTH_RULES");
        rules.push(RuleOutcome {
            family: Family::Width,
            rule: rep.rule,
            rule_name,
            layer: rep.layer.map(|l| layer_name(tech, l)),
            bound: (rep.bound_dbu != 0).then_some(rep.bound_dbu as i64),
            candidates: rep.candidates,
            findings: rep
                .violations
                .iter()
                .map(|v| Finding {
                    bboxes: vec![v.rect],
                    bound: v.bound_dbu as i64,
                    measured: Measured::Width {
                        min_side_dbu: v.measured_dbu,
                    },
                    nets: nets(strings, [v.net]),
                })
                .collect(),
        });
    }

    for rep in spacing::check_spacing_report(design, tech) {
        let rule_name = spacing::SPACING_RULES
            .iter()
            .find(|r| r.rule == rep.rule)
            .map(|r| r.rule_name)
            .expect("spacing report ids come from SPACING_RULES");
        rules.push(RuleOutcome {
            family: Family::Spacing,
            rule: rep.rule,
            rule_name,
            layer: rep.layer.map(|l| layer_name(tech, l)),
            bound: (rep.bound_dbu != 0).then_some(rep.bound_dbu as i64),
            candidates: rep.candidates,
            findings: rep
                .violations
                .iter()
                .map(|v| Finding {
                    bboxes: vec![v.a, v.b],
                    bound: v.bound_dbu as i64,
                    measured: Measured::Spacing {
                        gap_x_dbu: v.gap_x_dbu,
                        gap_y_dbu: v.gap_y_dbu,
                    },
                    nets: nets(strings, [v.net_a, v.net_b]),
                })
                .collect(),
        });
    }

    for rep in enclosure::check_enclosure_report(design, tech) {
        let rule_name = enclosure::ENCLOSURE_RULES
            .iter()
            .find(|r| r.rule == rep.rule)
            .map(|r| r.rule_name)
            .expect("enclosure report ids come from ENCLOSURE_RULES");
        rules.push(RuleOutcome {
            family: Family::Enclosure,
            rule: rep.rule,
            rule_name,
            layer: rep.layer.map(|l| layer_name(tech, l)),
            bound: (rep.bound_dbu != 0).then_some(rep.bound_dbu as i64),
            candidates: rep.candidates,
            findings: rep
                .violations
                .iter()
                .map(|v| Finding {
                    bboxes: vec![v.cut],
                    bound: v.bound_dbu as i64,
                    measured: Measured::Enclosure {
                        enc_x_dbu: v.enc_x_dbu,
                        enc_y_dbu: v.enc_y_dbu,
                        failed: v.failed,
                    },
                    nets: nets(strings, [v.net]),
                })
                .collect(),
        });
    }

    let area_outcome = |family: Family, rep: area::RuleReport| RuleOutcome {
        family,
        rule: rep.rule,
        rule_name: area::AREA_RULES
            .iter()
            .chain(area::HOLE_RULES.iter())
            .find(|r| r.rule == rep.rule)
            .map(|r| r.rule_name)
            .expect("area report ids come from AREA_RULES or HOLE_RULES"),
        layer: rep.layer.map(|l| layer_name(tech, l)),
        bound: (rep.bound_dbu2 != 0).then_some(rep.bound_dbu2),
        candidates: rep.candidates,
        findings: rep
            .violations
            .iter()
            .map(|v| Finding {
                bboxes: vec![v.region],
                bound: v.bound_dbu2,
                measured: Measured::Area {
                    area_dbu2: v.area_dbu2,
                },
                nets: nets(strings, [v.net]),
            })
            .collect(),
    };
    rules.extend(
        area::check_area_report(design, tech)
            .into_iter()
            .map(|r| area_outcome(Family::Area, r)),
    );
    rules.extend(
        area::check_hole_report(design, tech)
            .into_iter()
            .map(|r| area_outcome(Family::Hole, r)),
    );

    SubsetReport { rules }
}

/// First alphanumeric/`.` token inside the last `(...)` of a
/// `magic_per_rule` key — the resolved-key equivalent of `rule`.
/// `None` when the key has no parens.
pub fn first_paren_id(key: &str) -> Option<String> {
    let paren = &key[key.rfind('(')?..];
    paren
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .find(|t| !t.is_empty())
        .map(str::to_string)
}

/// One rule's count delta vs `magic_per_rule`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuleDelta {
    pub rule: &'static str,
    pub rule_name: &'static str,
    pub ours: usize,
    pub magic: u64,
}

/// Per-rule delta vs `magic_per_rule`. A key whose `first_paren_id`
/// equals `outcome.rule` attributes its count to that rule; matching
/// keys are consumed into the delta, unmatched keys returned in
/// `unclaimed` (they are `magic_only` today — the join records, it
/// does not judge).
#[derive(Clone, Debug, PartialEq)]
pub struct MagicDelta {
    pub deltas: Vec<RuleDelta>,
    /// Golden keys no implemented rule claimed, with their counts.
    pub unclaimed: Vec<(String, u64)>,
    /// `magic_total` from the golden, for context printing only —
    /// it includes the `magic_only` master-internal findings and is
    /// never compared to our total.
    pub magic_total: u64,
}

pub fn magic_deltas(
    report: &SubsetReport,
    golden: &serde_json::Value,
) -> anyhow::Result<MagicDelta> {
    let mut deltas: Vec<RuleDelta> = report
        .rules
        .iter()
        .map(|o| RuleDelta {
            rule: o.rule,
            rule_name: o.rule_name,
            ours: o.findings.len(),
            magic: 0,
        })
        .collect();
    let mut unclaimed = Vec::new();
    let per_rule = golden["magic_per_rule"].as_object().ok_or_else(|| {
        anyhow::anyhow!("reference `magic_per_rule` missing or not an object")
    })?;
    for (key, count) in per_rule {
        let magic = count.as_u64().ok_or_else(|| {
            anyhow::anyhow!("reference `magic_per_rule[{key}]` is not an unsigned integer")
        })?;
        match first_paren_id(key).and_then(|id| deltas.iter_mut().find(|d| d.rule == id)) {
            Some(d) => d.magic += magic,
            None => unclaimed.push((key.clone(), magic)),
        }
    }
    let magic_total = golden["magic_total"].as_u64().ok_or_else(|| {
        anyhow::anyhow!("reference `magic_total` missing or not an unsigned integer")
    })?;
    Ok(MagicDelta {
        deltas,
        unclaimed,
        magic_total,
    })
}

/// Deck split pinned by `magic_rules_checked` in the committed
/// goldens (identical on both designs): 278 statements / 239 unique
/// names, 55 statements / 50 unique rules `lef_expressible`, 223
/// `magic_only`. The gate test recomputes all four from the golden so
/// the pinned numbers cannot drift silently.
pub const DECK_STATEMENTS: usize = 278;
pub const DECK_UNIQUE_NAMES: usize = 239;
pub const IMPLEMENTED_STATEMENTS: usize = 55;
pub const IMPLEMENTED_RULES: usize = 50;
pub const MAGIC_ONLY_STATEMENTS: usize = 223;

/// One-line claim boundary for CLI/JSON — the sentence a `0
/// violations` report must carry.
pub fn claim_boundary() -> String {
    claim_boundary_from(
        DECK_STATEMENTS,
        IMPLEMENTED_STATEMENTS,
        IMPLEMENTED_RULES,
        MAGIC_ONLY_STATEMENTS,
    )
}

/// Same sentence built from counts derived from a loaded golden —
/// the CLI uses this when `--reference` is supplied so the printed
/// split is measured, not restated.
pub fn claim_boundary_from(
    deck: usize,
    le_stmts: usize,
    le_rules: usize,
    magic_only: usize,
) -> String {
    format!(
        "{le_stmts} of {deck} Magic drc(full) statements implemented \
         ({le_rules} unique rules); {magic_only} magic_only statements \
         (wells, diffusion, taps, poly, FETs, caps, angle rules) are \
         not checkable from LEF+DEF and are not claimed"
    )
}
