# e7-drc-spacing — Documentation updates

What each doc gains, and the honesty constraints on the wording.

## `docs/e7-drc.md`

Append a ticket-3 section after the ticket-2 sections (keeps §
numbering stable):

- **Family table update** — `spacing` → **implemented** (`drc::spacing`,
  20 rules, 0 violations on both routed DEFs); `enclosure`, `area`,
  `holes` stay pending; the `magic_only` row unchanged.
- **Spacing rule/bound table** — `lef-spacing.md`'s table verbatim
  (id, layer, LEF bound, verbatim `rule_name`, Magic `%d`, divergence
  note). Magic `%d` is re-derived from `sky130A.tech` at doc-writing
  time — do not copy from the design doc.
- **Merge-semantics paragraph** — the `merge.md` contract in doc form:
  connectivity islands (same-net touching + via-instance `group`),
  separated same-net islands still checked, `touching_ok` exemption
  at distance 0, Euclidean metric (`drc euclidean on`, integer
  `gx²+gy² < bound²`), checked-source set (MacroPin/Obs never
  generate findings; named divergence vs Magic's GDS truth).
- **Wide-metal paragraph** — `*b`/`*d` = `widespacing` reading
  (`min(w,h) > threshold`), `*a`/`*c`/`*5a` = `cifspacing
  large_halo` reading (non-wide island member within `window` of a
  same-island wide rect; `window` = wide bound per the rule text's
  "within 0.28/0.40um"); rect-local vs union-local width divergence
  recorded.
- **`li.c2` collapse + via-bound divergences** — name them: LEF
  170/200/200/800 vs Magic cut-plane 60/120/80/420 is the documented
  claim boundary, not a parity bug.

## `docs/results.md` — dated entry

`### YYYY-MM-DD — e7-drc-spacing — same-layer spacing rules (LEF bounds, Magic names)`

Contents (mirroring prior entries):

- **Commands run** — `cargo test -p grus-core --test drc`,
  `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`. No container steps run — goldens reused, not
  regenerated (state it explicitly).
- **Spacing table** — 20 rules × {gcd, picorv32}: LEF bound,
  candidate different-island pairs within reach, violations (all 0),
  `magic_per_rule` expectation (absent = 0). Rules with zero
  candidates named honestly (`li.3`, `li.c2`, `mcon.2` if empty —
  assert what the census prints, do not assume).
- **Mutation record** — `sanity_control.mutation` fields replayed
  (net `_001_`, `NEW met2 ( 10520 48450 ) ( 10520 48950 )`, 30 DBU
  edge gap): our verdict = N violating pairs all `met2.2`, Magic's
  recorded verdict = `new_rules_vs_clean ["Metal2 spacing < 0.14um
  (met2.2)"]` / `magic_total` 305 → 307. Same rule, different count
  unit (pairs vs merged regions — the §1 convention).
- **Fail-proof record** — merge-disabled red excerpt (unit fixture +
  gcd count), restore, green rerun; plus the `<`→`<=` observation
  whichever way it lands.
- Runtime: assemble + check per design — measured, not estimated.

## `docs/status.md`

Advance the E7 row: "width + **spacing** families landed (`v0.10.0`
ticket `e7-drc-spacing`)" — 20 spacing rules with same-net island
merge, LEF bounds, 0 violations on both designs, `met2.2` mutation
control; remaining pending = enclosure/area/holes.

## `docs/roadmap.md`

One progress line under the E7 block (after the `e7-drc-geom` line,
~line 814): what landed, the 20-rule 0-violation verdict on both
designs, the `met2.2` mutation control, merge-disable fail-proof,
pointer to the results date.

## Version bump

`[workspace.package] version` `0.9.0 → 0.10.0` — `ShapeRect.group` is
a required-field addition to a public struct (breaking for literal
construction); `RoutingLayer`/`CutLayer` field additions ride the
same minor bump. Bump lands in the same commit as the model change;
`Cargo.lock` regenerates via the build.

## Files

- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
