# e7-drc-enc-area — `drc::enclosure`

The enclosure family: 9 `lef_expressible` names checking that a cut is
overlapped by the owning net's metal on the enclosing routing layer.
Bounds are the LEF cut-layer `ENCLOSURE {BELOW|ABOVE} o1 o2` pairs
(`lef-rules.md`); `rule_name` strings are verbatim
`magic_rules_checked.names` templates, joined to `magic_per_rule` by
the first `(id)` token inside the parens — the width/spacing
convention.

## Rule table

```rust
pub enum Side { Below, Above }
pub enum EncMode { Both, One, Full }

pub struct EnclosureRule {
    /// First `(id)` token inside `rule_name`'s parens — join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template.
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
```

`ENCLOSURE_RULES` (9), `lef-rules.md` table order:

| `rule` | `cut` | `side` | `metal` | `mode` | LEF check (DBU) |
|---|---|---|---|---|---|
| `met1.4` | mcon | Above | met1 | Both | enc_x ≥ 30 ∧ enc_y ≥ 30 |
| `met1.5` | mcon | Above | met1 | One | enc_x ≥ 60 ∨ enc_y ≥ 60 |
| `via.5a` | via | Below | met1 | Full | (x,y ≥ 55) ∧ (x ∨ y ≥ 85) |
| `met2.5` | via | Above | met2 | Full | (x,y ≥ 55) ∧ (x ∨ y ≥ 85) |
| `via2.4a` | via2 | Below | met2 | Full | (x,y ≥ 40) ∧ (x ∨ y ≥ 85) |
| `met3.4` | via2 | Above | met3 | Both | enc_x ≥ 65 ∧ enc_y ≥ 65 |
| `via3.5` | via3 | Below | met3 | Full | (x,y ≥ 60) ∧ (x ∨ y ≥ 90) |
| `met4.3` | via3 | Above | met4 | Both | enc_x ≥ 65 ∧ enc_y ≥ 65 |
| `met5.3` | via4 | Above | met5 | Both | enc_x ≥ 310 ∧ enc_y ≥ 310 |

`enc_x` = `min(left, right)` overhang on the metal union;
`enc_y` = `min(bottom, top)`. `Both` and `One` read a single bound
(`min`/`max` of the LEF pair); `Full` evaluates the both-directions
clause at `min` first, then the one-direction clause at `max` — one
violation per (rule, cut), recording the failing clause.

Not implemented (no deck name — recorded in `lef-rules.md`): mcon
BELOW (0 0) and via4 BELOW → met4 (`via4.4`, folded into the `via4.1`
width composite already checked).

## Measurement

For each `ShapeRect` on the rule's cut layer (every cut rect is
`ShapeSrc::Via` — wires cannot exist on cut layers):

1. **Candidates** — `design.routing[metal].iter_overlapping(probe)`
   where `probe = inflate(cut, bmax_needed)`; keep `s.net == cut.net`.
   Strict overlap is correct: a rect ending exactly at the probe edge
   contributes nothing to the enclosure. Net filtering is the
   documented model — the via's own metal rects carry the same net
   tag, and enclosing metal of another net would be a short anyway.
2. **Side overhangs** — for each direction, split the cut's
   perpendicular span into elementary sub-intervals at the candidate
   rects' edge coordinates. In a sub-interval the union x/y-intervals
   are constant, so the overhang is exact:
   `enc_left(y) = cut.xl − a` where `a` is the left edge of the union
   interval covering `cut.xl`; when `cut.xl` is uncovered, `a` is the
   left edge of the first interval right of `cut.xl`, giving a
   *negative* overhang (local protrusion); when no metal covers the
   sub-interval at all, `a = cut.xh` (worst case). The side's overhang
   is the minimum over the span. Symmetric for right/top/bottom.
   Multi-cut via arrays work per cut: an interior cut's side slab is
   covered by the shared metal rect, so only boundary cuts report a
   small overhang — matching Magic's per-cut surround measurement.
3. **Clause evaluation** — `Both`: flag iff `enc_x < b ‖ enc_y < b`;
   `One`: flag iff `enc_x < B && enc_y < B`; `Full`: flag on the
   `Both` clause at `bmin`, else the `One` clause at `bmax` (record
   which). A rule whose `(0,0)` slot resolves — or whose layer is
   absent — contributes nothing, mirroring the `bound == 0` skips in
   width/spacing.

Exact-bound overhang passes (`enc == b` satisfies `>=`): Magic's
`surround < %d` reads as "insufficient when strictly below". The
real generated vias sit exactly at the LEF bound in places (e.g.
`via2_3`'s met2 side 55/85) — a `>` break is the family's cheap
fail-proof candidate.

## API

```rust
pub struct Violation {
    pub rule: &'static str,
    pub rule_name: &'static str,
    /// The metal routing layer measured.
    pub layer: DrcLayer,
    /// The offending cut rect.
    pub cut: RectDbu,
    /// min(left,right) / min(bottom,top) overhang measured on the
    /// owning net's metal union; negative = local protrusion.
    pub enc_x_dbu: i32,
    pub enc_y_dbu: i32,
    /// Bound of the failing clause.
    pub bound_dbu: i32,
    /// Which clause failed — Both or One (Full maps to one of them).
    pub failed: EncMode,
    pub net: Option<SymbolId>,
    /// Owning via-instance id (`ShapeRect::group`).
    pub group: u32,
}

/// Per-rule outcome for the zero-violation census — same shape as
/// `spacing::RuleReport` (`layer`, `bound_dbu`, `candidates`,
/// `violations`), candidates = cut rects evaluated.
pub struct RuleReport { /* rule, layer, bound_dbu, candidates, violations */ }

pub fn check_enclosure_report(design: &DrcDesign, tech: &TechDb) -> Vec<RuleReport>;
pub fn check_enclosure(design: &DrcDesign, tech: &TechDb) -> Vec<Violation>;
```

Efficiency: rules grouped by `(cut_idx, side)` — the 9 names resolve
to 8 distinct (cut, metal) measurement passes (`met1.4`/`met1.5`
share the mcon→met1 pass). Each pass is one probe + one span sweep
per cut rect; picorv32's via counts keep this trivially small.

## Checked sources

Only `ShapeSrc::Via` cut rects generate findings — the only source
that can produce cut-layer shapes. Metal candidates are every
same-net rect on the routing layer regardless of source (`Via`
metals, `SignalWire`/`SpecialWire`, `IoPort`, `MacroPin`) — all real
metal; `MacroObs` is excluded (blockage abstracts, not metal — and a
via never lands inside a cell anyway). A cut with no same-net metal
reaching its edge gets a negative overhang → flags
(`absence_illegal` semantics).

## Files

- `crates/grus-core/src/drc/enclosure.rs`
- `crates/grus-core/src/drc.rs` (module decl)
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
