# e7-drc-geom — `drc::width`: the width rule family

`src/drc/width.rs` — 13 rules from the goal's list, checked per
assembled `ShapeRect`.

## Rule table

Bound = what we check (LEF-derived). Magic `%d` = the techfile
constant Magic prints, recorded for provenance. `rule_name` is the
**verbatim** `magic_rules_checked.names` entry — the join key.

| `rule` (id) | layer | bound (LEF) | `rule_name` (verbatim from golden) | Magic `%d` | note |
|---|---|---|---|---|---|
| `li.c1` | li1 (R) | li1 `WIDTH` = 170 | `Core local interconnect width < %d (li.c1)` | 140 | bound diverges — Magic's 140 is a deck constant; we check the stricter LEF bound |
| `li.1` | li1 (R) | 170 | `Local interconnect width < %d (li.1)` | 170 | equal |
| `li.7` | li1 (R) | 170 | `Local interconnect width < %d (li.7)` | 290 | bound diverges — Magic requires fat li in special contexts; we check 170 (all three li names share the bound, see below) |
| `mcon.1` | mcon (C) | mcon cut `WIDTH` = 170 | `mcon.width < %d (mcon.1)` | 170 | equal |
| `met1.1` | met1 (R) | 140 | `Metal1 width < %d (met1.1)` | 140 | equal |
| `met2.1` | met2 (R) | 140 | `Metal2 width < %d (met2.1)` | 140 | equal |
| `met3.1` | met3 (R) | 300 | `Metal3 width < %d (met3.1)` | 300 | equal |
| `met4.1` | met4 (R) | 300 | `Metal4 width < %d (met4.1)` | 300 | equal |
| `met5.1` | met5 (R) | 1600 | `Metal5 width < %d (met5.1)` | 1600 | equal |
| `via.1a` | via (C) | cut `WIDTH` = 150 | `Via1 width < %d (via.1a + 2 * via.4a)` | 260 | diverges — `+2*via.4a` is enclosure-augmented; we check the cut's LEF WIDTH |
| `via2.1a` | via2 (C) | 200 | `via2 width < %d (via2.1a + 2 * via2.4)` | 280 | diverges, same shape |
| `via3.1` | via3 (C) | 200 | `via3 width < %d (via3.1 + 2 * via3.4)` | 320 | diverges |
| `via4.1` | via4 (C) | 800 | `via4 width < %d (via4.1 + 2 * via4.4)` | 1180 | diverges |

Layer resolution: `li1`..`met5` → `tech.layer_by_name` →
`RoutingLayer.width_dbu`; `mcon`/`via`/`via2`/`via3`/`via4` →
`tech.cut_layer_by_name` → `CutLayer.width_dbu`. A rule whose layer
isn't in the loaded LEF contributes nothing (no error — different
PDKs have different stacks). `width_dbu == 0` (LEF omitted WIDTH)
skips the rule — the bound-parity test on sky130 pins the 13 real
values.

**Divergence record** — required by the acceptance's "record in
`docs/e7-drc.md` that the bound is LEF, not the Magic techfile
constant": the four via rules and `li.c1`/`li.7` bound ≠
Magic's `%d`. This is the spec-mandated semantic, not a parity bug;
the docs table must show both columns exactly as above (Magic `%d`
re-derived by re-reading `sky130A.tech`, not copied from this doc).

## Check semantics

```rust
pub struct Violation {
    /// `(id)` inside the rule_name parens — `magic_per_rule` join key.
    pub rule: &'static str,
    /// Verbatim `magic_rules_checked.names` template — `%d` intact.
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// The offending shape (not a merged Magic region).
    pub rect: RectDbu,
    pub measured_dbu: i32,  // min(rect.w, rect.h)
    pub bound_dbu: i32,
    pub net: Option<SymbolId>,
}

pub fn check_width(design: &DrcDesign, tech: &TechDb) -> Vec<Violation>
```

Per rule × per rect on the rule's layer: `min(w, h) < bound` →
violation carrying the rect. Strict `<` — a rect exactly at the bound
passes (matches Magic's `width < %d` reading; the 140-wide met1 io
port and 170 li1 cut rects must pass).

Deliberate simplification vs Magic: Magic computes width on the merged
per-layer polygon (a wire's union with via metals/pins can enlarge its
"measured" width); we check each `ShapeRect` independently. On the
clean routed DEFs every shape independently meets bound → both give 0.
Known false-positive risk: a legitimately-merged shape that is
sub-bound standalone — the source tag (`ShapeSrc`) exists so `results.md`
can name the source class if the 0-gate ever fails. Recorded here so a
later reviewer doesn't rediscover it.

## Name-parity join (test-side)

The golden (`<d>.drc.reference.json`) has
`magic_rules_checked.names` (templates) +
`magic_rules_checked.classification` (lef_expressible/magic_only) +
`magic_per_rule` (instantiated names → counts). Test helper:

1. `first_paren_id(name)` — first `(x.y)` token inside parens →
   `HashSet` of lef_expressible ids.
2. Every implemented `WIDTH_RULES[i].rule` ∈ that set **and**
   `WIDTH_RULES[i].rule_name` ∈ `names` verbatim — catches a typo'd
   template or a wrong-family copy (e.g. `via3.1` also appears in the
   magic_only `MiM cap contact width` statement — verbatim template
   matching disambiguates).
3. Clean-design gate: `check_width(...)` empty ⇔ every implemented id
   has no `magic_per_rule` entry — asserted jointly so a nonzero Magic
   run would surface (the goldens carry none for these 13 ids — the
   only `magic_per_rule` keys are `nwell.*`).

## Files

- `crates/grus-core/src/drc/width.rs`
- `crates/grus-core/src/drc.rs`
- `docs/e7-drc.md`
