# LefSkipReport — overview

## Problem

`parse_lef_bytes`/`parse_lef` (`crates/grus-core/src/readers/lef.rs`) return
`Result<()>` and silently drop two classes of input:

1. Unmodelled top-level blocks routed to `skip_block`: `VIA`, `VIARULE`,
   `NONDEFAULTRULE`, `PROPERTYDEFINITIONS`, `SPACING`.
2. Unknown top-level statements that fall through to `skip_to_semi`
   (e.g. `VERSION`, `BUSBITCHARS`, `DIVIDERCHAR`, and anything else the open
   LEF keyword set adds).

This is the same gap E3's F3 closed for STA arcs (`SkipReport` in
`crates/grus-core/src/sta/analysis.rs:216`): skips must be counted and shown,
not invisible.

## Design

Mirror the `SkipReport` pattern at reader scope:

- New `pub struct LefSkipReport { pub skipped: BTreeMap<String, usize> }` in
  `readers/lef.rs`. Keys are uppercased top-level keywords; values count
  skipped statements. `BTreeMap` gives deterministic ordering for free.
- `parse_lef_bytes` and `parse_lef` return `Result<LefSkipReport>` instead of
  `Result<()>`. `TechDb` is untouched — no serde or cache impact.
- `load_tech_db` merges the `.tlef` and `.lef` reports and returns
  `Result<(TechDb, LefSkipReport)>` so callers get one combined report.
- `crates/grus-eda/src/main.rs` prints a single stderr line after LEF loading,
  only when the merged report is non-empty:
  `lef: skipped N statements (VIA x2, PROPERTYDEFINITIONS x1)`.

## Counting semantics (decided)

Counting happens only in the **top-level** dispatch loop of
`parse_lef_bytes`. Statements skipped inside `LAYER`/`MACRO`/`PIN`/`OBS`/`SITE`
bodies are not counted — they are per-construct details, not top-level
statements.

- `VIA`, `VIARULE`, `NONDEFAULTRULE`: each block counts once under its own
  uppercased keyword (`VIA x2` means two VIA blocks).
- `PROPERTYDEFINITIONS`, `SPACING`: each block counts once under its keyword.
- Any other unrecognised top-level word: counts once under
  `word.to_uppercase()` (so `FOO 42 ;` counts `FOO x1`; `VERSION`,
  `BUSBITCHARS`, etc. count under their own names).
- `END` and stray `;` tokens are not counted.

Note: the sample fixtures (`SAMPLE_TLEF`, real sky130 `.tlef`) contain
`VERSION`/`BUSBITCHARS`/`DIVIDERCHAR`, so real runs will produce a non-empty
report — that is intended, not a test failure. The "empty report" test must use
a fixture containing only modelled constructs (`UNITS`/`SITE`/`LAYER`/`MACRO`/`END`).

## Non-goals

- No behaviour change to what is parsed or skipped — only counting plus the
  stderr line.
- No `--strict` interaction, no JSON output, no per-file breakdown in the
  summary (the merged report is what is shown).
- No change to `TechDb`, caches, or binary formats.
