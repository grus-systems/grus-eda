# e7-drc-enc-area — Documentation updates

All repository text in English; chat in Japanese. Keep the docs in
step with the code in the same change (AGENTS §7.8).

## `docs/e7-drc.md`

Append a new ticket section after ticket 3 (`e7-drc-spacing`,
sections 10–12), continuing the numbering:

- **§13 Enclosure family** — the 9-rule table from `lef-rules.md`
  (`rule`, cut, side→metal, LEF overhangs, verbatim `rule_name`,
  Magic statement + value, divergence column). Notes: composite
  deck names evaluate both clauses; `met1.4`/`met1.5` are two rows
  over one LEF pair; sides with no deck statement (`mcon` BELOW,
  `via4` BELOW → met4, folded into `via4.1`) named; measurement =
  per-side union overhang on the owning net, negative = protrusion.
- **§14 Min-area family** — the 6-rule table (LEF `AREA` == Magic
  `%a` throughout, no divergence), net-agnostic component model,
  drawn-member gate for `MacroPin`-only components.
- **§15 Hole-area family** — the 2-rule table (LEF
  `MINENCLOSEDAREA`, deck-pinned 0.14 µm²), enclosed-void
  extraction, edge-connected free space (point-pinch = hole,
  recorded), OBS discard rationale.
- **§7 family table update** — `enclosure`, `area`, `holes` rows
  change `pending` → **implemented**; the table notes all 50 unique
  `lef_expressible` rules are now covered (13 + 20 + 9 + 6 + 2).
  Magic `%d`/`%a` values re-derived from `sky130A.tech` during the
  doc step, not copied from design notes.

## `docs/results.md`

New dated entry (`e7-drc-enc-area`, `v0.11.0`):

- Per-rule census tables from the integration tests: enclosure
  (candidates = cut rects per rule — `met1.4`/`met1.5` expected 0),
  min-area (evaluated components), holes (enclosed voids detected).
- Mutation records: DEF-VIAS `+ ENCLOSURE` 85/165→10/10 → count +
  rule `via.5a - via.4a` only; `+ RECT met1` island (coordinates
  recorded) → exactly `met1.6`.
- Fail-proof record per family: which break, what went red, counts,
  restore→green.
- Checks run + version-bump rationale (public struct fields added).
- State explicitly what ran vs. what is untested.

## `docs/status.md`

- "Last updated" line: `v0.11.0` ticket `e7-drc-enc-area` — the
  subset checker covers all 50 `lef_expressible` rules; 0 violations
  on both routed DEFs; DEF-VIAS and RECT-island mutations pin the
  positive path; per-family fail-proofs in results.md.
- E7 row: families 33/50 → 50/50; keep the named gaps (`magic_only`
  permanently out of scope, no CLI yet — `e7-drc-e2e`).

## `docs/roadmap.md`

- E7 bullet: add a `e7-drc-enc-area landed` line like the prior
  entries — three families, 0 violations, mutations, fail-proofs;
  all `lef_expressible` rules now implemented; remaining E7 work is
  the CLI (`e7-drc-e2e`).

## Version bump

`Cargo.toml` `[workspace.package] version`: **0.10.1 → 0.11.0**
(0.x minor per AGENTS §5) — `RoutingLayer` and `CutLayer` gain
public fields, breaking literal construction for downstream users of
the structs. `Cargo.lock` regenerates. Bump rides the LEF-retention
commit (same commit as the functional change, per AGENTS).

## Files

- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
