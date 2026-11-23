# e7-drc-spacing — `drc::spacing`: the spacing rule family

`src/drc/spacing.rs` — 20 rules: 12 minimum-spacing + 8 wide-metal,
checked over connectivity islands (`merge.md`), bounded by LEF
(`lef-spacing.md`).

## Rule table

`rule` = `(id)` join key, `rule_name` = verbatim
`magic_rules_checked.names` template — same convention as
`WIDTH_RULES`. `kind` selects the check; bounds resolve per layer via
`lef-spacing.md` helpers.

| `rule` | `rule_name` (verbatim) | layer | cut | kind |
|---|---|---|---|---|
| `li.3` | `Local interconnect spacing < %d (li.3)` | li1 | — | Min |
| `li.c2` | `Core local interconnect spacing < %d (li.c2)` | li1 | — | Min |
| `mcon.2` | `mcon.spacing < %d (mcon.2)` | mcon | ✓ | Min |
| `via.2` | `Via1 spacing < %d (via.2 - 2 * via.4a)` | via | ✓ | Min |
| `via2.2` | `via2 spacing < %d (via2.2 - 2 * via2.4)` | via2 | ✓ | Min |
| `via3.2` | `via3 spacing < %d (via3.2 - 2 * via3.4)` | via3 | ✓ | Min |
| `via4.2` | `via4 spacing < %d (via4.2 - 2 * via4.4)` | via4 | ✓ | Min |
| `met1.2`–`met5.2` | `MetalN spacing < %d (metN.2)` | met1–5 | — | Min |
| `met1.3a` | `Spacing of metal1 features attached to and within 0.28um of large metal1 < %d (met1.3a)` | met1 | — | WideAttached |
| `met1.3b` | `Metal1 > 3um spacing to unrelated m1 < %d (met1.3b)` | met1 | — | WideUnrelated |
| `met2.3a`/`met2.3b` | `... within 0.28um ... (met2.3a)` / `Metal2 > 3um ... (met2.3b)` | met2 | — | same pair |
| `met3.3c` | `Spacing of metal3 features attached to and within 0.40um of large metal3 < %d (met3.3c)` | met3 | — | WideAttached |
| `met3.3d` | `Metal3 > 3um spacing to unrelated m3 < %d (met3.3d)` | met3 | — | WideUnrelated |
| `met4.5a` | `Spacing of metal4 features attached to and within 0.40um of large metal4 < %d (met4.5a)` | met4 | — | WideAttached |
| `met4.5b` | `Metal4 > 3um spacing to unrelated m4 < %d (met4.5b)` | met4 | — | WideUnrelated |

Notes on the deck's id irregularities — pin them in the table, do not
"fix": the met3 pair is `3c`/`3d` (not `3a`/`3b`), met4 is
`5a`/`5b`, met5 has no wide rules, and each `*b`/`*d` name appears
twice in `names` (the deck's `widespacing allmN` + `widespacing
*obsmN` statements — one rule row covers both since our
checked-source model has no obs plane).

## Types

```rust
pub enum SpacingKind { Min, WideUnrelated, WideAttached }

pub struct SpacingRule {
    pub rule: &'static str,        // `(id)` join key
    pub rule_name: &'static str,   // verbatim template, `%d` intact
    pub layer: &'static str,
    pub cut: bool,
    pub kind: SpacingKind,
}

pub struct Violation {
    pub rule: &'static str,
    pub rule_name: &'static str,
    pub layer: DrcLayer,
    /// The offending pair (not a merged Magic region).
    pub a: RectDbu,
    pub b: RectDbu,
    /// Per-axis separations; euclidean gap is `sqrt(gx²+gy²)`, and the
    /// integer-exact test compares the squares — no float stored.
    pub gap_x_dbu: i32,
    pub gap_y_dbu: i32,
    pub bound_dbu: i32,
    pub net_a: Option<SymbolId>,
    pub net_b: Option<SymbolId>,
}

pub const SPACING_RULES: &[SpacingRule]; // 20 rules, table order above
pub fn check_spacing(design: &DrcDesign, tech: &TechDb) -> Vec<Violation>;
```

## Algorithm

```
islands = union_find(design)               // merge.md: same-net
                                           // touching + via groups
for rule in SPACING_RULES:
    (layer, bound) = resolve(rule, tech)   // min_spacing_dbu /
                                           // wide_spacing; absent
                                           // layer or bound==0 → skip
    if rule.kind != Min:
        wide_mark = mark_wide(design[layer], islands, threshold, bound)
    for (i, a) in design[layer].rects where checkable(a.src):
        probe = inflate(a.rect, bound)
        for (j, b) in layer.iter_overlapping(probe) where ptr(b)>ptr(a)
                                                      && checkable(b.src):
            if find(a) == find(b): continue            // same island
            (gx, gy) = axis_separations(a.rect, b.rect)
            if gx == 0 && gy == 0: continue            // touching_ok
            if gx²+gy² >= bound²: continue             // i64
            if !pair_qualifies(rule.kind, a, b, wide_mark): continue
            emit Violation
```

`pair_qualifies` per kind (dedup per rule only — a pair can flag
under both `met1.2` and `met1.3b`, matching Magic's independent
statement evaluation):

- `Min` — always.
- `WideUnrelated` — `max(min(a), min(b)) > width_threshold` (min
  dimension of at least one side exceeds 3000 DBU).
- `WideAttached` — at least one side carries `wide_mark` *and*
  neither side is inherently wide (the wide-vs-* case is `*b`'s).
  `wide_mark` per island: member rects with `min(w,h) <= threshold`
  whose distance to any same-island wide member is `<= window`.
  `window` = the rule's `bound` (280/400) — the names literally say
  "within 0.28/0.40um of large metalN"; the window and the bound are
  the same deck constant, recorded.

## Wide-metal provenance

Magic's deck statements (sky130A.tech `drc` section, verbatim):

```
widespacing allm1 3005 allm1,*obsm1,m1fill 280 touching_ok "(met1.3b)"
widespacing *obsm1 3005 allm1 280 touching_ok "(met1.3b)"   # why the
                                                           # name ×2
cifspacing m1_large_halo m1_large_halo 280 touching_ok "(met1.3a)"
```

Our rect-model reading (recorded interpretation, not a copied
algorithm — `cifspacing` works on a derived halo plane):

- `*b`/`*d` ≈ `widespacing`: a ≥3µm-wide shape near *unrelated* metal
  needs the wide bound. Trigger `min(w,h) > threshold` — LEF says
  `WIDTH 3`, Magic says 3005; we use LEF per the provenance rule.
- `*a`/`*c`/`*5a` ≈ `cifspacing <large_halo>`: the halo wraps the
  large plate plus `window` around it; a *non-wide* member of the
  plate's island inside the halo must keep the wide bound from
  unrelated metal. Disjoint from `*b` by construction (the pair's
  inherently-wide side is absent) — a stub-attached-to-plate vs
  another plate flags under both names, which is also what two
  independent deck statements would do.

**Known divergence — rect-local vs union-local width.** Magic's
`widespacing` marks any *merged* region ≥3µm thick; we test
`min(w,h)` per `ShapeRect`. An L/T-corner union locally exceeding
3µm with sub-threshold member rects is wide in Magic but not in our
model — under-reports `*b`, conservative. Recorded in
`docs/e7-drc.md`.

## Layer resolution

- routing rules: `tech.layer_by_name[rule.layer]` →
  `min_spacing_dbu()` (Min) or `wide_spacing()` (Wide*); absent layer
  or bound 0 → rule contributes nothing.
- cut rules (`mcon`/`via`/`via2`/`via3`/`via4`):
  `tech.cut_layer_by_name` → `cut_layers[i].spacing_dbu`; only `Via`
  shapes are checkable there (mirrors width's source filter).
- The wide pair of a layer shares one `wide_spacing()` row:
  `(threshold, bound)` → `*b` checks `min > threshold`, `*a` uses
  `bound` as its check bound *and* its `window`. If a layer's LEF
  has no wide row, both wide rules skip — no guessed constants
  (goal's "else Magic-deck constant" clause: sky130 resolves
  everything; the docs table records Magic's values as provenance
  regardless).

## Complexity

Merge passes are per layer O(n log n + k). Per rule, each rect probes
a bound-inflated window — O(n log n + matches). 20 rules × ~100k
rects stays comfortably in the existing drc-test budget; measure and
record per-design runtime in `results.md` (no estimate).

## Files

- `crates/grus-core/src/drc/spacing.rs`
- `crates/grus-core/src/drc.rs`
- `docs/e7-drc.md`
