# e7-drc-geom — Documentation updates

What each doc gains, and the honesty constraints on the wording.

## `docs/e7-drc.md`

New section (append after the taxonomy — keeps § numbering stable by
appending as the next section):

- **Width family status table** — one row per family:
  `width` = implemented (`drc::width`, this ticket) / `spacing`,
  `enclosure`, `area`, `holes` = pending / `magic_only` statement
  classes = out of scope (nwell-in-master etc., 305/8049 merged
  violations on the clean decks).
- **Rule/bound table** — `width.md`'s table verbatim (id, LEF bound,
  verbatim `magic_rules_checked.names` template, Magic `%d`,
  divergence note). Magic `%d` column is re-derived from
  `sky130A.tech` inside the container at doc-writing time — do not
  copy from this design doc.
- **Provenance note** — bounds come from LEF `WIDTH` / via cut
  `WIDTH`, not from the Magic techfile `%d`; name the divergent rules
  (`li.c1`, `li.7`, `via.*`). State the join convention: `rule` id =
  first `(id)` in the template; `rule_name` = template verbatim.
- **Check-semantics caveat** — per-rect min-dim vs Magic's
  merged-polygon width; per-statement-position rules `li.c1`/`li.7`
  collapse onto the same li1 bound in our model.
- Update the § status header if it currently says "reference only".

## `docs/results.md` — dated entry

`### YYYY-MM-DD — e7-drc-geom — LEF+DEF geometry model + width family`

Contents (mirroring existing entries):

- **Commands run** — `cargo test -p grus-core drc`,
  `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`. No container steps run (record that explicitly —
  the goldens are reused, not regenerated).
- **Per-layer census table** — both designs: wire rects by layer
  (signal vs special), via instances by name, expanded via rects by
  layer, macro pin/obs/io counts — as printed by the census test.
- **Width table** — 13 rules × {gcd, picorv32}: bound, violations
  (all 0), `magic_per_rule` expectation (absent = 0) joined by id.
- **Mutation record** — fixture edit (`NEW met1 480` → `NEW met1
  100`, one rail), expected 1 × `met1.1` measured 100 vs actual.
- **Fail-proof record** — the `<` → `<=` break, the red output
  excerpt, restore, green rerun.
- Runtime (parse + assemble + check per design) — measured, not
  estimated.

## `docs/status.md`

Add/advance the E7 row: "subset checker — geometry substrate + width
family landed; spacing/enclosure/area/holes pending". Link
`docs/e7-drc.md`.

## `docs/roadmap.md`

One progress line under the existing E7 block (~line 796), same style
as the `e6-drt-ref` line: what landed, the 0-violation counts on both
designs, mutation + fail-proof done, pointer to the results date.

## Version bump

`[workspace.package] version` `0.8.0 → 0.9.0` — `PdnVia.geom` is a
required-field addition to a public struct (breaking for literal
construction) per AGENTS §5's 0.x rule (breaking → minor). Same
commit as the model change. `Cargo.lock` regenerates via the build.

## Files

- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
