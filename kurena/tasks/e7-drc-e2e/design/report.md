# e7-drc-e2e — `drc::report`

New module `crates/grus-core/src/drc/report.rs` (registered
`pub mod report;` in `drc.rs`): the single normalized view of the
subset checker's outcome, shared by the CLI and the integration gate
so both read the same numbers through the same join.

## Width gains `check_width_report`

`width.rs` is the only family without a per-rule `RuleReport`. Add the
sibling used everywhere else:

```rust
pub struct RuleReport {
    pub rule: &'static str,
    /// `None` when the rule's layer is absent from the loaded LEF.
    pub layer: Option<DrcLayer>,
    /// Resolved LEF `WIDTH` bound; 0 when undeclared.
    pub bound_dbu: i32,
    /// Rects that passed the checked-src filter (`SignalWire`/
    /// `SpecialWire`/`IoPort` on routing layers, `Via` on cut layers).
    pub candidates: usize,
    pub violations: Vec<Violation>,
}

pub fn check_width_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport>;
```

`check_width` becomes `check_width_report(design, tech).into_iter()
.flat_map(|r| r.violations).collect()` — the rule loop moves into the
report function unchanged (same skip conditions: unresolvable layer →
`layer: None`, `bound == 0` → `candidates = 0`, empty `violations`),
so there is one implementation and the existing tests still exercise
the real path.

`enclosure.rs`: derive `Serialize, Deserialize` on `EncMode` (needed
by `Measured::Enclosure` below). One-line derive change.

## Types

```rust
/// Which family produced an outcome — grouping only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family { Width, Spacing, Enclosure, Area, Hole }

/// The measured quantity, in the bound's units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Measured {
    /// `min(w,h)` of the flagged rect.
    Width { min_side_dbu: i32 },
    /// Per-axis separations; Euclidean gap is sqrt(gx²+gy²), the
    /// check compares squares — no float stored.
    Spacing { gap_x_dbu: i32, gap_y_dbu: i32 },
    /// Same-net side overhangs plus the failing clause.
    Enclosure { enc_x_dbu: i32, enc_y_dbu: i32, failed: EncMode },
    /// Component union area (min-area) or void area (holes).
    Area { area_dbu2: i64 },
}

/// One finding, normalized across families.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Offending geometry in DBU, sorted lexicographically:
    /// width → [rect]; spacing → [a, b]; enclosure → [cut];
    /// area/hole → [region]. This is the list a later `drc listall`
    /// comparison consumes (Magic emits µm; divide by dbu_per_micron).
    pub bboxes: Vec<RectDbu>,
    /// The bound applied, DBU or DBU² per `measured.kind`.
    pub bound: i64,
    pub measured: Measured,
    /// Net names involved (0–2 entries, resolved via the design
    /// interner); empty for untagged shapes.
    pub nets: Vec<String>,
}

/// One implemented rule's outcome — all 50 always present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SubsetReport {
    /// Table order: 13 width, 20 spacing, 9 enclosure, 6 area,
    /// 2 hole — 50 outcomes, deterministic emission order.
    pub rules: Vec<RuleOutcome>,
}
```

`RuleOutcome.bound` uses `Option<i64>` (area/hole bounds are `i64`;
everything else widens) rather than the families' `0`-sentinel — the
report is a boundary artifact, so "no bound" must not read as
"bound 0".

## Runner

```rust
/// Runs all five rule tables over `design` and normalizes the
/// outcomes. `strings` is the *design* interner (`DesignDb::strings`
/// — `ShapeRect.net` ids belong to it, not to `tech.strings`).
pub fn run_all(
    design: &DrcDesign,
    tech: &TechDb,
    strings: &StringInterner,
) -> SubsetReport;
```

Calls `check_width_report`, `check_spacing_report`,
`check_enclosure_report`, `check_area_report`, `check_hole_report`;
converts each family's `Violation` into `Finding` (`a`/`b` → both
bboxes; `cut` → one; `rect`/`region` → one; `net`/`net_a`/`net_b` →
resolved strings, `None` dropped); `layer` resolved through
`tech.strings` (`routing_layers[i].name` / `cut_layers[i].name`). Pure
glue — no checking logic lives here.

## Magic join

```rust
/// First alphanumeric/`.` token inside the last `(...)` of a
/// `magic_per_rule` key — the resolved-key equivalent of `rule`.
/// `None` when the key has no parens.
pub fn first_paren_id(key: &str) -> Option<String>;

/// Per-rule delta vs `magic_per_rule`. A key whose `first_paren_id`
/// equals `outcome.rule` attributes its count to that rule; matching
/// keys are consumed into the delta, unmatched keys returned in
/// `unclaimed` (they are `magic_only` today — the join records, it
/// does not judge).
pub struct RuleDelta {
    pub rule: &'static str,
    pub rule_name: &'static str,
    pub ours: usize,
    pub magic: u64,
}
pub struct MagicDelta {
    pub deltas: Vec<RuleDelta>,
    /// Golden keys no implemented rule claimed, with their counts.
    pub unclaimed: Vec<(String, u64)>,
    /// `magic_total` from the golden, for context printing only —
    /// it includes the `magic_only` master-internal findings and is
    /// never compared to our total.
    pub magic_total: u64,
}
pub fn magic_deltas(report: &SubsetReport, golden: &serde_json::Value)
    -> MagicDelta;
```

First-token attribution is unambiguous by construction: every
implemented `rule` is the first `(id)` token inside its `rule_name`'s
parens, and Magic preserves the paren text verbatim in resolved keys.
The existing tests' local `paren_tokens` (any-token `contains`) stays
as-is for the per-family joins; `first_paren_id` is the single source
for the new report-level join and the gate.

## Deck-split constants

```rust
/// Deck split pinned by `magic_rules_checked` in the committed
/// goldens (identical on both designs): 278 statements / 239 unique
/// names, 55 statements / 50 unique rules `lef_expressible`, 223
/// `magic_only`.
pub const DECK_STATEMENTS: usize = 278;
pub const DECK_UNIQUE_NAMES: usize = 239;
pub const IMPLEMENTED_STATEMENTS: usize = 55;
pub const IMPLEMENTED_RULES: usize = 50;
pub const MAGIC_ONLY_STATEMENTS: usize = 223;

/// One-line claim boundary for CLI/JSON — the sentence a `0
/// violations` report must carry.
pub fn claim_boundary() -> String;
/// Same sentence built from counts derived from a loaded golden —
/// the CLI uses this when `--reference` is supplied so the printed
/// split is measured, not restated.
pub fn claim_boundary_from(deck: usize, le_stmts: usize,
                           le_rules: usize, magic_only: usize) -> String;
```

`claim_boundary()` produces e.g.
`"55 of 278 Magic drc(full) statements implemented (50 unique rules); 223 magic_only statements (wells, diffusion, taps, poly, FETs, caps, angle rules) are not checkable from LEF+DEF and are not claimed"`.

The gate test (`gate.md`) recomputes all four counts from the golden
and asserts equality with the constants, so the pinned numbers can't
drift from the data silently.
