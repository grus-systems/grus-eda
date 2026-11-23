# e7-drc-enc-area — Overview

E7 ticket 4, third implementation ticket. `e7-drc-geom` landed the
geometry substrate (`DrcDesign`, `LayerShapes`, `ShapeRect` source/net/
group tags) and the width family; `e7-drc-spacing` landed connectivity
islands and the 20-rule spacing family. This ticket lands the
**remaining 17 `lef_expressible` rules** — via enclosure/overlap (9
names), minimum area (6), minimum hole area (2) — completing all 50
unique `lef_expressible` rules. Gate: 0 violations on both routed DEFs,
joined to `magic_per_rule` by verbatim rule-name template.

## What ships

1. **LEF rule retention, second pass.** `parse_layer` currently
   `skip_to_semi`s `AREA`, `MINENCLOSEDAREA` and `ENCLOSURE` inside
   `LAYER` blocks — the sky130 techlef carries all three (`AREA 0.083`
   on met1, `MINENCLOSEDAREA 0.14` on met1/met2, `ENCLOSURE
   {BELOW|ABOVE} o1 o2` on every CUT layer). Parse into `TechDb`:
   `RoutingLayer.{area_dbu2, min_enclosed_area_dbu2}`,
   `CutLayer.{enc_below_dbu, enc_above_dbu}`. See `lef-rules.md`.
2. **`drc::enclosure`.** 9 rules over the assembled cut rects: for
   each cut, measure the four side overhangs of the owning net's metal
   union on the rule's routing layer, then evaluate the rule's clause —
   both-directions `>= min(o1,o2)`, one-direction `>= max(o1,o2)`, or
   both clauses for the composite deck names (`via.5a - via.4a` etc.).
   See `enclosure.md`.
3. **`drc::union` + `drc::area`.** A rect-union sweep per connected
   component (merged polygon area) plus free-space component extraction
   (enclosed voids = holes). `drc::area` then checks the 6 min-area
   rules and the 2 hole-area rules on top. See `area.md`.
4. **Verification.** Zero-violation gates + `rule_name`/`lef_expressible`
   join on both goldens; a DEF-VIAS `+ ENCLOSURE` mutation flagging
   exactly `via.5a - via.4a`; a `+ RECT` island mutation flagging
   exactly `met1.6`; a synthetic donut fixture flagging `met1.7`;
   per-family fail-proofs; docs updates. See `tests.md`.

## Key decisions

- **Enclosure bounds are LEF `ENCLOSURE` pairs, not Magic `%d`.** The
  techlef pins every overhang in comments (`# Via 4a / Via 5a`):
  `ENCLOSURE o1 o2` reads as `min(o1,o2)` required on *both* axes plus
  `max(o1,o2)` on *at least one* axis. Where `o1 == o2` the rule is a
  single both-direction check. Magic's `surround` bounds differ
  (30 directional vs LEF 55/85 etc.) — LEF is the documented claim
  boundary, same convention as width/spacing; the docs table carries
  both columns. See `lef-rules.md`.
- **Composite deck names get composite checks.** Magic emits one
  `surround` statement per (cut, metal) pair whose name joins two
  sky130 ids (`(via.5a - via.4a)`). We implement one rule row per deck
  name; for the asymmetric-overhang names the check evaluates *both*
  clauses and flags once per cut. The `(id)` join key is the first
  token, matching the existing `paren_tokens` convention. mcon is the
  exception: the deck uses two separate names (`met1.4` both-dir,
  `met1.5` one-dir) — two rule rows over the same LEF overhang pair.
- **Enclosure metal = the owning net's same-layer shapes.** Cut rects
  are measured against union coverage by `net == cut.net` rects
  overlapping the `bmax`-inflated cut bbox — the via's own metals plus
  whatever wire/pin/port metal the net draws there. No union-find:
  metal covering an enclosure slab is connected to the via through the
  net's shapes by construction; a different net's metal would be a
  short anyway. Side overhang = cut edge to the union's boundary,
  computed per elementary y/x sub-interval (negative = local
  protrusion). See `enclosure.md`.
- **Min-area polygons are net-agnostic components.** Magic merges all
  touching paint on the plane regardless of net label — components =
  closed-touch connectivity over `SignalWire | SpecialWire | Via |
  IoPort | MacroPin` (per-layer, `for_each_pair_within(1)` like the
  spacing pass). Findings require >= 1 drawn-source member
  (`MacroPin`-only components are LEF abstracts, not the GDS Magic
  measures — skipped, recorded, consistent with width/spacing).
- **Holes = enclosed free-space components.** Within each component
  bbox, subtract all member rects (any component, any net — a foreign
  wire inside a donut shrinks the void), union the x-slab free
  intervals into edge-connected components; a component that never
  touches the bbox boundary is enclosed by metal => a hole. Corner-only
  contact does *not* merge free space (point-pinched channel counts as
  a hole — recorded). Voids touching a `MacroObs` rect are discarded:
  obs abstracts hide real cell content, so a void walled by obs can
  only over-report. See `area.md`.
- **Below-side via4 enclosure has no deck name.** `met4` over `via4`
  (LEF `ENCLOSURE BELOW 0.19 0.19`) has no `surround v4/m4` statement —
  Magic folds it into `via4.1` (`width 1180 = 800 + 2*190`), which the
  width family already covers at the LEF cut width. Same folding for
  `via.1a`/`via2.1a`/`via3.1`. mcon `BELOW 0 0` (li1 side) is a zero
  overhang — no rule. See `enclosure.md`.

## Non-goals (from the spec)

- CLI surface — `e7-drc-e2e`.
- Antenna checks — `check_antennas` is a separate surface per the
  `docs/e7-drc.md` taxonomy.
- `magic_only` statements — permanently out of scope per the taxonomy.
- Per-axis `ENCLOSURE` (o1 pinned to x, o2 to y) — the deck's own
  mapping and OpenDB's generated-via expansion both treat the pair as
  min-both/max-one; recorded interpretation.

## Doc map

| Doc | Content |
|---|---|
| `lef-rules.md` | `AREA`/`MINENCLOSEDAREA`/`ENCLOSURE` grammar as exercised, `TechDb` fields, full LEF-vs-Magic bound table for all 17 rules |
| `enclosure.md` | 9-rule table, side-overhang measurement, clause evaluation, checked sources, API |
| `area.md` | `drc::union` component + sweep + void extraction, min-area and hole rule tables, OBS semantics, API |
| `tests.md` | Unit + integration inventory, the three mutation/synthetic fixtures, per-family fail-proofs |
| `docs.md` | `e7-drc.md`/`results.md`/`status.md`/`roadmap.md` updates, version bump |
| `steps.md` | Ordered steps, commit split, done-when map, watch-items |

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/enclosure.rs`
- `crates/grus-core/src/drc/union.rs`
- `crates/grus-core/src/drc/area.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
