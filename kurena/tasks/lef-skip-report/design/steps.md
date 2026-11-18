# LefSkipReport — steps

Ordered so the workspace compiles at each step. One commit per logical unit.

## 1. `LefSkipReport` + `parse_lef*` signatures (`readers/lef.rs`)

- Add `LefSkipReport` (`skipped: BTreeMap<String, usize>`), `merge`, `total`.
- Change `parse_lef_bytes`/`parse_lef` to return `Result<LefSkipReport>`;
  add counting in the top-level dispatch per api.md.
- Change `load_tech_db` to return `Result<(TechDb, LefSkipReport)>`.
- Update in-file tests: `lef_error` `Ok(())` → `Ok(_)`; `load_tech_db` site
  `.0`; truncation tests unchanged.
- `cargo check -p grus-core` will fail on other crates' test call sites — that
  is expected until step 2; use `cargo check -p grus-core --lib` for the
  non-test build.

## 2. Mechanical call-site updates (`grus-core`)

- Every `load_tech_db` test caller: take `.0` of the tuple (or destructure).
- `readers/verilog.rs:985` and `readers/def.rs:1584`
  `parse_lef_bytes(...).expect(...)` — compiles as-is (report discarded);
  leave unless clippy complains.
- `cargo test -p grus-core --no-run` to confirm all test targets compile.

## 3. New unit tests (`readers/lef.rs` tests module)

- `test_skip_report_counts_skips`: LEF text combining modelled constructs with
  `FOO 42 ;`, two `VIA` blocks, one `PROPERTYDEFINITIONS`, one `SPACING`, one
  `VIARULE` → assert `report.skipped` entries and that `TechDb` still parses
  (macro count etc. as in the fixture). Keep the fixture minimal.
- `test_skip_report_empty_when_nothing_skipped`: LEF with only
  `UNITS`/`SITE`/`LAYER`/`MACRO`/`END` (no `VERSION`, `BUSBITCHARS`, etc.) →
  `report.skipped.is_empty()`.
- Case-insensitivity spot check: lowercase `via`/`foo` still count under
  uppercased keys (the dispatch already uses `eq_ignore_ascii_case`; the key
  must be `word.to_uppercase()`).

## 4. CLI summary (`crates/grus-eda/src/main.rs`)

- Add `load_lef_tech_db` + `print_lef_skip_summary` helpers per api.md.
- Swap the six `load_tech_db` call sites to `load_lef_tech_db`.
- Manual smoke: run any subcommand against sky130 LEF and confirm the stderr
  line appears (sky130 `.tlef`/`.lef` contain unmodelled blocks, so a non-empty
  report is expected); confirm silence is impossible to distinguish — instead
  verify the empty path via the unit test only.

## 5. Version bump + verify

- Bump `[workspace.package] version` `0.1.83` → `0.1.84` in root `Cargo.toml`
  (patch: internal refactor / observability, per AGENTS.md) in the same commit
  as the functional change.
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Commit(s) per repo convention (`feat:`/`refactor:`, English, agent trailer).

## Files

- `Cargo.toml`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/verilog.rs`
- `crates/grus-core/src/readers/yosys_json.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/parasitics.rs`
- `crates/grus-core/src/placement/wirelength.rs`
- `crates/grus-core/src/placement/nesterov.rs`
- `crates/grus-core/src/placement/legalize.rs`
- `crates/grus-core/src/placement/density.rs`
- `crates/grus-core/src/floorplan.rs`
- `crates/grus-core/src/congestion/rsmt.rs`
- `crates/grus-core/src/congestion/route.rs`
- `crates/grus-core/src/congestion/gcell.rs`
- `crates/grus-eda/src/main.rs`
