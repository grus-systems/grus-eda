# e7-drc-spacing — Overview

E7 ticket 3, second implementation ticket. `e7-drc-geom` landed the
geometry substrate (`drc::geom` per-layer `LayerShapes`,
`iter_overlapping` spatial index, `ShapeRect` source+net tags) and the
width family. This ticket lands the **spacing** family: 12
minimum-spacing rules and 8 wide-metal spacing rules, joined 1:1 to
`magic_per_rule` names, gated at 0 violations on both routed DEFs.

## What ships

1. **LEF spacing data.** `parse_layer` currently `skip_to_semi`s both
   `SPACING` and `SPACINGTABLE` inside `LAYER` blocks — sky130's
   routing layers carry their rules in `SPACINGTABLE
   PARALLELRUNLENGTH` (the plain `SPACING` lines are commented out),
   cut layers use plain `SPACING`. Both parse into `TechDb`
   (`RoutingLayer.spacing_table`, `RoutingLayer.spacing_dbu`,
   `CutLayer.spacing_dbu`). See `lef-spacing.md`.
2. **Via-instance identity for merge.** `ShapeRect` gains `group: u32`
   — all rects emitted by one `expand_via` call share an id. Without
   it, the 5 cuts of a `via2_3_1600_480_1_5_320_320` array (cut gap
   170 < LEF bound 200) are separate same-net islands and
   false-flag — Magic merges them through the via's metal. See
   `merge.md`.
3. **`drc::spacing`.** Same-net connectivity islands via union-find
   (same-layer touching merge + instance-group merge), per-rule
   candidate pairs from `iter_overlapping` on the bound-inflated
   probe, integer-exact Euclidean gap test (`drc euclidean on` is in
   the committed reference script), `touching_ok` exemption at
   distance 0. 20 rules total. See `spacing.md`.
4. **Verification.** Zero-violation gate on `gcd.routed.def` /
   `picorv32.routed.def` joined to the goldens; the committed
   `sanity_control.mutation` reproduced golden-driven (exactly
   `met2.2`); synthetic island/wide fixtures; merge-disable fail-proof;
   docs updates. See `tests.md`.

## Key decisions

- **Merge = connectivity islands, not net-name equality.** Two
  same-net shapes merge iff connected — same-layer touching/overlap
  (corner touch counts) chains transitively, via-instance rects merge
  unconditionally across layers. A pair flags only when the two rects
  sit in *different* islands at `0 < euclid < bound`: different nets,
  or same-net *separated* islands — both are different Magic nodes.
  Touching/abutting different-net pairs are exempt (`touching_ok` on
  every deck statement). See `merge.md`.
- **Bounds from LEF, names from Magic** — same convention as width.
  Base bound = plain `SPACING`, else the lowest-width-threshold
  `SPACINGTABLE` row; wide bound = the largest-threshold row (`WIDTH 3`
  → 0.28/0.40). The four via-cut bounds diverge from Magic's `%d`
  (LEF 170/200/200/800 vs Magic cut-plane 60/120/80/420) — LEF is the
  documented claim boundary; the docs table carries both columns.
  See `lef-spacing.md`.
- **Euclidean, integer-exact.** The reference ran `drc euclidean on`;
  the gap test is `gx*gx + gy*gy < bound*bound` in `i64`, and
  `gx > 0 || gy > 0` separates "touching" (exempt) from a real gap.
- **Wide-metal is two disjoint statements.** `*b`/`*d` ("MetalN > 3um
  spacing to unrelated mN") fires when a side has `min(w,h) > 3000`
  DBU (LEF `WIDTH 3`; Magic `widespacing` uses 3005); `*a`/`*c`/`*5a`
  ("features attached to and within 0.28/0.40um of large metal") fires
  on pairs where a side is a non-wide island member within the wide
  bound of a same-island wide rect — the `cifspacing
  mN_large_halo` interpretation. See `spacing.md`.
- **Checked sources mirror width's choice** — routing layers check
  `SignalWire | SpecialWire | Via | IoPort`, cut layers check `Via`.
  `MacroPin`/`MacroObs` never generate findings (LEF abstracts, not
  the fabricated GDS Magic measures) but net-tagged shapes still join
  the union-find so pin metal connects same-net islands.
- **Zero is scoped to implemented rules**, as before: `magic_total`
  (305/8049) is the magic_only nwell deck, not our claim.

## Files — delta vs spec

Spec's `## Files` lists `drc.rs`/`drc/geom.rs` hooks, `spacing.rs`,
docs, and Cargo files. Implementation also touches
`readers/lef.rs` + `tech_db.rs` (LEF `SPACING`/`SPACINGTABLE`
retention — the goal mandates LEF-provenance thresholds and neither is
parsed today) and `tests/drc.rs` (the gate + mutation). `ShapeRect`'s
new `group` field breaks literal construction → version `0.9.0 →
0.10.0` (0.x minor per AGENTS §5).

## Non-goals (from the spec)

- Enclosure/overlap, min-area, min-hole-area — `e7-drc-enc-area`.
- No CLI — `e7-drc-e2e`.
- `NONDEFAULTRULES` per-net spacing overrides — both routed DEFs have
  zero occurrences (verified 2026-11-23); recorded, not modeled.
- Parallel-run-length dimension of `SPACINGTABLE` — sky130 uses
  `PARALLELRUNLENGTH 0` (vacuous); rows are kept but the PRL column is
  not modeled (recorded; ignoring PRL can only over-require).

## Doc map

| Doc | Content |
|---|---|
| `lef-spacing.md` | `SPACING`/`SPACINGTABLE` grammar as exercised, `TechDb` fields, bound-resolution helpers, LEF-vs-Magic bound table |
| `merge.md` | Island/union-find semantics, `ShapeRect.group`, `touching_ok`, Euclidean gap, checked-source set |
| `spacing.md` | 20-rule table (verbatim `rule_name`s, kinds), `check_spacing` algorithm, wide-metal model + provenance |
| `tests.md` | Unit + integration inventory, golden-driven mutation, fail-proof |
| `docs.md` | `e7-drc.md`/`results.md`/`status.md`/`roadmap.md` updates, version bump |
| `steps.md` | Ordered steps, commit split, done-when map, watch-items |

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/drc/spacing.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
