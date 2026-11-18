# LefSkipReport — API

## `crates/grus-core/src/readers/lef.rs`

```rust
/// Top-level LEF statements the reader skipped, counted by uppercased keyword.
/// `skipped` is empty when nothing was skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LefSkipReport {
    pub skipped: BTreeMap<String, usize>,
}

impl LefSkipReport {
    /// Adds `other`'s counts into `self` (used to merge .tlef + .lef reports).
    pub fn merge(&mut self, other: &LefSkipReport);

    /// Sum of all per-keyword counts.
    pub fn total(&self) -> usize;
}
```

`is_empty()` can be `self.skipped.is_empty()` at call sites; add a method only
if it reads better — keep the struct minimal.

### Signature changes

```rust
pub fn parse_lef_bytes(
    bytes: &[u8],
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<LefSkipReport>;

pub fn parse_lef(
    path: &Path,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<LefSkipReport>;

pub fn load_tech_db(
    tlef_path: &Path,
    lef_path: &Path,
    interner: &mut StringInterner,
) -> Result<(TechDb, LefSkipReport)>;
```

- `parse_lef` returns the report from `parse_lef_bytes` after the existing
  `strings`/`rebuild_maps` steps.
- `load_tech_db` calls `parse_lef` twice, merges the two reports, and returns
  `(tech_db, merged_report)`.
- On `Err`, no report is returned (unchanged error paths).

### Counting insertion points in `parse_lef_bytes`

Inside the existing `while let Some(tok)` top-level loop, keep a local
`let mut report = LefSkipReport::default();`:

- `PROPERTYDEFINITIONS` arm → after `skip_block`, `*report.skipped.entry("PROPERTYDEFINITIONS".into()).or_default() += 1;`
- `VIA`/`VIARULE`/`NONDEFAULTRULE` arm → key is `word.to_uppercase()` of the
  matched keyword (not the block name that follows).
- `SPACING` arm → `"SPACING"`.
- Final fall-through (unknown `Token::Word`) → key `word.to_uppercase()`, then
  the existing `skip_to_semi`. The trailing `tok != Token::Semi` guard is dead
  code (only `Word` reaches the chain); it may be simplified to a plain
  `else { ... }` while touching this code.

A small private helper keeps this tidy, e.g.
`fn count_skip(report: &mut LefSkipReport, keyword: &str)`.

`skip_block` itself is unchanged — counting happens at the dispatch site, once
per block.

## `crates/grus-eda/src/main.rs`

Add one private helper and use it at all six `load_tech_db` call sites
(`check-e0` ~L334, `congestion` ~L407, `floorplan` ~L460, `place` ~L530,
`sta` ~L645, `power` ~L742):

```rust
/// Loads the tech DB and reports skipped top-level LEF statements on stderr.
fn load_lef_tech_db(
    tlef: &Path,
    lef: &Path,
    interner: &mut StringInterner,
) -> anyhow::Result<TechDb> {
    let (tech_db, report) = grus_core::readers::load_tech_db(tlef, lef, interner)?;
    print_lef_skip_summary(&report);
    Ok(tech_db)
}

fn print_lef_skip_summary(report: &LefSkipReport) {
    if report.skipped.is_empty() {
        return;
    }
    let total = report.total();
    // Count descending, keyword ascending for determinism.
    let mut parts: Vec<_> = report.skipped.iter().collect();
    parts.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let detail = parts
        .iter()
        .map(|(k, v)| format!("{k} x{v}"))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("lef: skipped {total} statements ({detail})");
}
```

Each call site changes only the function name
(`load_tech_db` → `load_lef_tech_db`); arguments are identical.

## Call-site updates in `grus-core`

All `load_tech_db` callers are tests; they need the tuple's `.0` (or a
destructure). ~70 sites across:

- `readers/lef.rs` (test at ~L952)
- `readers/def.rs` (~L1074–1480, 8 sites)
- `readers/verilog.rs` (~L733–901, 4 sites) plus `parse_lef_bytes` at ~L985
- `readers/yosys_json.rs` (~L347–515, 4 sites)
- `sta/analysis.rs` (~L3382–6982, 15 sites), `sta/parasitics.rs` (~L542–1074, 7 sites)
- `placement/{wirelength,nesterov,legalize,density}.rs`, `floorplan.rs`,
  `congestion/{rsmt,route,gcell}.rs` (test sites)

Mechanical fix pattern: `load_tech_db(...).expect("...")` → append `.0`, or
`let (tech, _report) = ...` where the report might be worth asserting later.
`parse_lef_bytes` test sites: `.unwrap()` still compiles (returns the report,
discarded); `lef_error`'s `Ok(())` arm becomes `Ok(_)`; truncation tests need
no change (`is_err()`/`let _ =` already fit).
