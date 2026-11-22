# ux-sta-summary — API & schema

All library changes are in `crates/grus-core/src/sta/analysis.rs` plus the
re-export list in `crates/grus-core/src/sta/mod.rs`. Stdout changes are in
`crates/grus-eda/src/main.rs`.

## 1. Report types (public, `Serialize`)

```rust
/// One clock domain's slack roll-up. `clock` is the `create_clock` name;
/// `null` marks the unattributed row (endpoints whose reported check has
/// no clock anchor — internal delay-check leaves, unclocked captures).
#[derive(Clone, Debug, Serialize)]
pub struct DomainSummary {
    pub clock: Option<String>,
    /// Endpoints whose *reported* check anchors on this domain.
    pub endpoints: u32,
    /// Same folds as `StaReport`'s globals, over this row's subset:
    /// min(`worst_max_slack_ns`), Σ min(0, `worst_max_slack_ns`), and the
    /// hold-side twins. `wns` is `INFINITY` on an empty row (serializes
    /// as `null`, same convention as the globals).
    pub wns_max_ns: f32,
    pub tns_max_ns: f32,
    pub wns_min_ns: f32,
    pub tns_min_ns: f32,
    pub setup_violations: u32,
    pub hold_violations: u32,
}

/// Slack bucket key of `slack_histogram`: the endpoint's
/// `worst_max_slack_ns` expressed as a fraction of its capture period.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackBucketKind {
    /// `s < 0` — count equals `setup_violations` by construction.
    Negative,
    /// `0 <= s < 0.2 * P`
    P0To20,
    /// `0.2 * P <= s <= 0.5 * P` — the 0.5 boundary lands here.
    P20To50,
    /// `s > 0.5 * P`
    Over50,
    /// No period reference (`domain_period_ns` is `None`) or no max-side
    /// candidate to bucket.
    NoPeriod,
}

#[derive(Clone, Debug, Serialize)]
pub struct SlackBucket {
    pub bucket: SlackBucketKind,
    pub count: u32,
}
```

`StaReport` gains, always serialized (`#[serde(default)]`, no
`skip_serializing_if`):

```rust
    /// One row per `sdc.clocks` entry, in file order, plus a trailing
    /// `clock: null` row iff any endpoint is unattributed. Empty array
    /// when no clocks are declared.
    #[serde(default)]
    pub domains: Vec<DomainSummary>,
    /// Worst max-slack distribution over `endpoints`, bucketed by share
    /// of the capture period. Always all five buckets, `Negative` first,
    /// `NoPeriod` last; counts sum to `endpoints.len()`.
    #[serde(default)]
    pub slack_histogram: Vec<SlackBucket>,
```

## 2. `Worst` and `EndpointReport` bookkeeping (crate-internal, never serialized)

```rust
struct Worst {
    arr: f32, dly: f32, req: f32, slack: f32, slew: f32,
    class: u16,
    from_pin: u32,
    /// `Sdc::clocks` row of the check/anchor that produced this
    /// candidate; `u8::MAX` = no clock anchor. Same bookkeeping family
    /// as `class`/`from_pin` — drives `domains` via the reported
    /// candidate, never serialized.
    dom: u8,
}

pub struct EndpointReport {
    // ... existing fields ...
    /// `dom` of the reported candidate (picked `max` `Worst`, `min`'s as
    /// fallback) — `None` when the reported check has no clock anchor.
    /// Drives `domains`/`slack_histogram`; never serialized.
    #[serde(skip)]
    pub(crate) domain_row: Option<usize>,
    /// `sdc.clocks[row].period_ns` at `domain_row`, carried so
    /// `merge_corner_reports` can bucket without an `Sdc`.
    #[serde(skip)]
    pub(crate) domain_period_ns: Option<f32>,
}
```

- `dom` is set at every `Worst` construction site (audit list in
  `steps.md`): check/gate candidates take their in-scope capture
  variable (`cap`, `en_dom` — already `u8` with the `u8::MAX`
  convention); port anchor entries take `out_row as u8`; sites built
  with `..w` inherit; `req = N` leaf checks and test helpers set
  `u8::MAX`. A delay-check rewrite (`set_max_delay`/`set_min_delay`)
  keeps the anchor row of the check it replaced — the `..w` spreads do
  this for free; full literals use the in-scope `cap_row`/`out_row`.
- `endpoint_report` gains an `sdc: &Sdc` parameter and derives
  `domain_row` = `max.or(min).and_then(|w| (w.dom != u8::MAX).then_some(
  w.dom as usize))` and `domain_period_ns` = the row's `period_ns`.
  Call sites: `record_endpoint` (has `cx.sdc`) and the partitioned
  entries push at ~5561 (has `sdc`).

## 3. The summary helper

```rust
/// `domains` + `slack_histogram` from the endpoint list alone:
/// `clock_names[row]` is the JSON/stdout name of `domain_row == row`,
/// in `sdc.clocks` order. `assemble_sta_report` passes the Sdc names;
/// `merge_corner_reports` passes the clock names of `setup.domains`
/// (same row order — see §4). Pure: identical inputs give identical
/// rows, which is what the consistency test re-derives.
fn summarize_domains(
    endpoints: &[EndpointReport],
    clock_names: &[String],
) -> (Vec<DomainSummary>, Vec<SlackBucket>);
```

Grouping is by `domain_row` index, **not** by name — two `create_clock`s
may share a `-name` (`t2_clkdup` territory), and each is still its own
row.

## 4. `assemble_sta_report` and `merge_corner_reports`

- `assemble_sta_report` gains `sdc: &Sdc`; after the existing folds it
  calls `summarize_domains(&endpoints, &clock_names)` with
  `clock_names = sdc.clocks.iter().map(|c| c.name.clone()).collect()`.
  Both callers (`run_sta_detail` ~2530, `run_sta_partitioned` ~5865)
  have `sdc` in scope.
- `merge_corner_reports` builds merged `endpoints` first — each merged
  `EndpointReport` copies `domain_row`/`domain_period_ns` as:
  `e.max_slack_ns.is_some()` → setup's `(domain_row, domain_period_ns)`
  (the merged max side *is* setup's reported check and its anchor);
  else `h.domain_row.or(e.domain_row)` with the matching period (the
  merged min side comes from the hold run; setup's value is the
  fallback when hold's reported check carries no anchor). Then calls
  `summarize_domains(&endpoints, &names)` where `names` is the
  `clock` column of `setup.domains` minus the trailing `null` row —
  `names = setup.domains.iter().filter_map(|d| d.clock.clone())
  .collect()` works because `domains` emits every `sdc.clocks` row in
  order and the unattributed row is last.
- The endpoint-set equality assertion is unchanged; the domain merge is
  a pure function of the merged endpoint list.

## 5. `sta/mod.rs` re-exports

```rust
pub use analysis::{
    DomainSummary, SlackBucket, SlackBucketKind, /* existing names */
};
```

## 6. Stdout (`crates/grus-eda/src/main.rs`)

Printed after the existing `STA (...)` wns/tns `println!` and the
pulse-width line, before `print_skip_summary(&report)` (~line 1014).
Layout (cosmetics adjustable; keep ≤ ~20 lines):

```text
domains:
  clock        endpoints    wns_max     tns_max    wns_min     tns_min  v_setup  v_hold
  cka                  4    -0.123456  -1.234567   0.012345   0.000000        1       0
  ckb                  4     0.234567   0.000000   0.045678   0.000000        0       0
  (none)               0          ...        ...        ...        ...        0       0
slack histogram (share of capture period): <0: 1   0-20%: 3   20-50%: 2   >50%: 2   no_period: 0
```

- Render the `wns_*` of an empty domain row as `inf`/`null`-tolerant —
  `{:+.6}` on `INFINITY` prints `inf`, acceptable and self-explanatory.
- Print the blocks unconditionally when `report.domains` is non-empty;
  the `(none)` row only exists when the JSON has one (same source).
- `--json` needs no new code: `serde_json::to_string_pretty(&report)`
  picks up both fields.

## 7. Version

`[workspace.package] version` `0.6.0` -> `0.6.1` in root `Cargo.toml`
(**patch** — additive JSON fields and stdout text only: no new CLI flag,
no API signature change beyond private types, no consumer of `StaReport`
breaks; the `v0.4.0`/`v0.5.0`/`v0.6.0` minors each added a command or
flag, this ticket adds none), same commit, `Cargo.lock` regenerated.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs`
- `Cargo.toml`
- `Cargo.lock`
