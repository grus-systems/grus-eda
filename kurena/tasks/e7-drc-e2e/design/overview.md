# e7-drc-e2e — overview

E7 stage gate: wrap the finished subset checker in a `grus-eda drc`
CLI and gate the complete `lef_expressible` set against the committed
Magic goldens on both routed DEFs. Depends on `e7-drc-enc-area`; adds
no new rule checks.

## What exists today

- `drc::geom::assemble(&DesignDb, &TechDb) -> Result<DrcDesign>` —
  per-layer shape index over all six `ShapeSrc` classes.
- Four rule families, all 50 unique `lef_expressible` names covered:
  `drc::width` (13 rules, flat `check_width` only — **no**
  `RuleReport`), `drc::spacing` (20, `check_spacing_report`),
  `drc::enclosure` (9, `check_enclosure_report`), `drc::area` (6 + 2
  holes, `check_area_report` / `check_hole_report`).
- `crates/grus-core/tests/drc.rs` — per-family zero-violation gates,
  per-family golden joins, three mutation tests (met1.1, met2.2 probe,
  via.5a, met1.6).
- Goldens `tests/designs/<d>/<d>.drc.reference.json`:
  `magic_per_rule` (resolved keys: `"N-well spacing < 1.27um
  (nwell.2a)": 120`), `magic_rules_checked.names` (278 statements,
  identical on both designs), `classification` (239 unique keys; 50
  `lef_expressible` ↔ 55 statements, 189 `magic_only` ↔ 223),
  `sanity_control` (the met2 probe Magic flagged as `met2.2`, count 3).

## What this ticket adds

1. `drc::report` — one normalized `SubsetReport` over all five rule
   tables: per-rule `{family, rule, rule_name, layer, bound,
   candidates, findings}` where each finding carries its offending
   geometry as DBU bboxes. Plus the `magic_per_rule` delta join and the
   deck-split constants.
2. `grus-eda drc` — `--tlef/--lef/--def`, optional `--reference`,
   optional `--json`. Stdout: per-rule counts, finding bboxes, the
   claim-boundary line, per-rule deltas when a reference is given.
3. Gate additions in `crates/grus-core/tests/drc.rs`: exact
   implemented-set == `lef_expressible`-set coverage, per-rule count ==
   `magic_per_rule` on both designs, met2 probe parity at report level.
4. Docs + version bump to `0.12.0` (new CLI surface, minor).

## Decisions

- **`check_width` gains a `check_width_report` sibling** instead of
  reconstructing width candidates inside `report.rs`. Every other
  family already exposes `check_*_report`; duplicating the layer
  resolution + src filter in `report.rs` would create a second source
  of truth for what "evaluated" means (E0-lesson 8). `check_width`
  delegates to it via `flat_map` — one implementation.
- **`--json <path>`** for the machine-readable report — the
  `sta`/`power` convention. Spec's "`--out`/`--json`" is satisfied by
  `--json`; no `--out` alias (one flag, one meaning).
- **Findings are data, not an exit status**: exit 0 with violations
  printed (a DRC checker reporting violations is success). Nonzero
  only on I/O/parse failure. No `--strict` — spec doesn't ask.
- **Claim boundary is printed always.** With `--reference` the
  278/55/223 split is derived from `magic_rules_checked`; without it,
  the same line comes from pinned constants in `report.rs`
  (`DECK_STATEMENTS` etc.) whose provenance is the committed golden —
  the gate test pins them, so drift fails loudly.
- **Count unit named, not hidden**: our finding count is rect/pair/
  component-based; Magic's `magic_per_rule` is merged error regions.
  Both record zero today on the clean DEFs, so the gate asserts
  equality; the docs + CLI name the unit difference so a future
  nonzero isn't misread.
- **No new fixtures.** The met2 parity test mutates DEF text in
  memory, as the existing mutation tests do; `tests/designs/README.md`
  unchanged.

## Acceptance traceability

| Item | Where |
|---|---|
| `grus-eda drc` on both DEFs, per-rule counts + summary JSON, `--reference` deltas | `cli.md` |
| Coverage: every `lef_expressible` name implemented + golden count | `report.md` (`run_all`, `magic_deltas`), `gate.md` (`test_subset_*`) |
| Claim boundary in CLI output (55/278, magic_only out of scope) | `report.md` (constants), `cli.md` (stdout line) |
| met2 sanity-control mutation flags `met2.2` | `gate.md` (report-level extension of `test_spacing_mutation_met2_probe`) |
| `cargo test` / `clippy` | `steps.md` verify step |
| `docs/results.md` dated entry, `docs/status.md` honest E7 line | `docs.md` |

## Non-goals

`magic_only` rules (223 statements: wells/diffusion/taps/poly/FET/cap/
angle — never derivable from LEF+DEF), antenna checks, `drc listall`
bbox-for-bbox comparison against Magic (the emitted bbox format is
*compatible* with that later comparison; the comparison itself is not
in this ticket), any new rule implementation, any speed work.
