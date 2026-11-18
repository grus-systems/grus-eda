# Goal

Make LEF reader skips honest and countable, mirroring the STA
`SkipReport` pattern (`crates/grus-core/src/sta/analysis.rs:216`).

Today `parse_lef_bytes`/`parse_lef`
(`crates/grus-core/src/readers/lef.rs`) return `Result<()>` and
silently skip both unmodelled blocks (`skip_block`: VIA, VIARULE,
NONDEFAULTRULE, PROPERTYDEFINITIONS, SPACING) and unknown top-level
statements (the `skip_to_semi` fall-through). Counts are invisible —
the same gap E3's F3 closed for arcs.

Interface (decided):

- New `pub struct LefSkipReport { pub skipped: BTreeMap<String, usize> }`
  in `readers/lef.rs`, counting skipped top-level statements by
  uppercased keyword (VIA counts once per VIA block, unknown words
  count once per statement). `skipped` is empty when nothing was
  skipped.
- `parse_lef_bytes` and `parse_lef` return `Result<LefSkipReport>`
  instead of `Result<()>`. `TechDb` is untouched (serde/caches safe).
- Callers merge reports across the `.tlef` + `.lef` reads;
  `crates/grus-eda/src/main.rs` prints one stderr summary after LEF
  loading, only when non-empty:
  `lef: skipped N statements (VIA x2, PROPERTYDEFINITIONS x1)`.
- Update every `parse_lef*` call site and test for the new return
  type.

Tests (`readers/lef.rs` unit tests):

- LEF containing an unknown `FOO 42 ;` statement and one `VIA`
  block → report counts both, parse still succeeds.
- A fully-known LEF (`UNITS`/`LAYER`/`MACRO`/`END` only) → empty
  report.
- Existing tests updated for the new signature.

Keep under ~120 lines; no behavior change beyond counting + the
stderr line.
