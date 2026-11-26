# ux-sta-paths — API & schema

All library changes are in `crates/grus-core/src/sta/analysis.rs` plus the
re-export list in `crates/grus-core/src/sta/mod.rs`. CLI changes are in
`crates/grus-eda/src/main.rs`.

## 1. Report types (public, `Serialize`)

```rust
/// How this stage's pin was reached from the previous stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathEdge {
    /// Register/latch clock (or latch enable) pin — path start, carries the
    /// seeded/propagated clock arrival as both delay and arrival.
    Clock,
    /// Latch enable pin of a `LatchOpen` launch (data-clock, not a flop CK).
    LatchOpen,
    /// Top-level input port launch anchor (one stage folding clock latency +
    /// edge offset + input delay + driving-cell/wire delay).
    Port,
    /// `-from get_pins` segment-check startpoint.
    Pin,
    /// Combinational or clk->Q cell arc (`driver_response` delay, derated).
    CellArc,
    /// Net traversal driver -> sink (Elmore wire delay; `ln2 * Elmore` for a
    /// port-driven net; 0 under the zero-wire model).
    Net,
}

#[derive(Clone, Debug, Serialize)]
pub struct PathStage {
    /// `inst/pin` for cell pins, bare port name for top-level pins (`pin_name`).
    pub pin: String,
    pub edge: PathEdge,
    /// Transition at this pin: `"rise"` | `"fall"`.
    pub transition: PathTransition,       // or rename_all snake_case on RISE/FALL
    /// This stage's own delay contribution, ns (derated cell arc, wire
    /// delay, or the launch/external anchor value — see `edge`).
    pub delay_ns: f32,
    /// Cumulative arrival at `pin`, ns — the recorded `state` extreme.
    pub arrival_ns: f32,
    /// `state.slew_{max|min}[pol]`; `null` where unreached.
    pub slew_ns: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CriticalPath {
    /// `"max"` (setup/late) or `"min"` (hold/early) — matches the
    /// `max_*`/`min_*` naming everywhere else in the report.
    pub corner: PathCorner,               // enum {Max, Min}, snake_case
    pub startpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startpoint_clock: Option<String>,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_clock: Option<String>,
    /// The reported check values (`Worst`), not re-derived from stages.
    pub data_arrival_ns: f32,
    pub required_ns: f32,
    pub slack_ns: f32,
    /// Ordered launch -> endpoint, `trace.md` §5.
    pub stages: Vec<PathStage>,
    /// Honest record of anything untraceable: un-traced predecessor,
    /// ambiguous anchor, undeclared launch, `-from` segment, `pin_from` start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
```

`StaReport` gains:

```rust
    /// Worst-`--paths` traces per corner; absent from JSON when empty —
    /// either `--paths` was not passed or no endpoint produced a trace.
    /// `merge_corner_reports` keeps setup's `max` rows + hold's `min` rows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub critical_paths: Vec<CriticalPath>,
```

`merge_corner_reports` fills it as
`setup.critical_paths(corner == Max) ++ hold.critical_paths(corner == Min)` —
mirroring its wns/tns field source rule. Every constructor site of `StaReport`
(`assemble_sta_report`, `merge_corner_reports`, test literals if any) sets the
field; `assemble_sta_report` takes it empty (paths are attached after, by
`run_sta_detail`).

## 2. `Worst` bookkeeping fields (private, never serialized)

```rust
struct Worst {
    arr: f32, dly: f32, req: f32, slack: f32, slew: f32,
    /// Partitioned-run signature class whose `class_states` produced this
    /// candidate; `u16::MAX` = trace against the state handed to the tracer.
    class: u16,
    /// Winning `-from get_pins` startpoint pin of a segment delay check;
    /// `u32::MAX` = none (ordinary launch).
    from_pin: u32,
}
```

- `collect_endpoints` sites set `class: u16::MAX, from_pin: u32::MAX`.
- `run_sta_partitioned` stamps `w.class = ci` at the two folds
  (`trace.md` §6); the deferred-endpoint union compose leaves `u16::MAX`.
- `pin_segment_arrival` is extended to return `(winning_pin, segment)` (or a
  `pin_segment_winner` sibling); the two call sites (~4689, ~4735) store
  `from_pin` on the rewritten `Worst`. All other sites pass `u32::MAX`.

## 3. Function plumbing

```rust
// New public entry points (mod.rs re-exports):
pub fn run_sta_with_paths(
    design: &DesignDb, lib: &LibDb, sdc: &Sdc,
    parasitics: Option<&NetParasitics>, path_count: usize,
) -> Result<StaReport>;

pub fn run_sta_quiet_with_paths(
    design: &DesignDb, lib: &LibDb, sdc: &Sdc,
    parasitics: Option<&NetParasitics>, path_count: usize,
) -> Result<StaReport>;
```

- `run_sta`/`run_sta_quiet` become `run_sta_inner(.., emit_warnings, 0)`;
  the `*_with_paths` pair passes `path_count`. `run_sta_inner` and
  `run_sta_detail` gain the `path_count: usize` parameter and
  `run_sta_detail` attaches `report.critical_paths`.
- `assemble_sta_report` keeps `PinId` attached to the polarities it returns:
  its second return value becomes `Vec<(PinId, EndpointPolarities)>` (it
  already has the pin in `entries`; just keep it). `run_sta_detail` returns
  that tuple through; `run_sta_with_state` discards it as today.
- `run_sta_partitioned` gains `path_count`, produces paths inside (needs
  `class_states` + the per-class `disabled_borrow` sets — keep the latter in
  a parallel `Vec` while composing) and returns them via the same channel.
- `trace_path` and the candidate/winner helpers are private `fn`s; nothing in
  the trace path is `pub` beyond the report types and the two entry points.

## 4. CLI (`crates/grus-eda/src/main.rs`)

```rust
#[arg(
    long,
    default_value_t = 0,
    help = "Also emit the worst-N startpoint->endpoint path traces per \
            corner (human dump on stdout and `critical_paths` in --json)"
)]
paths: usize,
```

- `StaArgs.paths`; when `> 0`, the sta dispatch calls
  `run_sta_with_paths` / `run_sta_quiet_with_paths` (the hold leg of the
  `--lib-hold` pair uses the quiet variant), then `merge_corner_reports` as
  today — the merge carries the paths across per §1.
- Stdout dump after the existing summary + skip summary, before the `--json`
  write. Compact form (implementer may adjust cosmetics; keep ≤ ~2 + N lines
  of header plus one line per stage):

```
paths (worst 3 per corner):
  [max] 1. _16708_/CLK -> _16945_/D   slack -4.524066  (core_clk -> core_clk)
           0.000000 +0.000000 clock    _16708_/CLK ^
           1.447068 +1.447068 cell_arc _16708_/Q ^
           1.470467 +0.023399 net      _08249_/A ^
           ...
           8.002586           required 3.478520  slack -4.524066
```

  Cumulative arrival, `+delay`, edge kind, pin, `^`/`v`. The `required/slack`
  footer per path is optional if the header carries slack — pick one, keep it
  small.
- `--json` needs no new code: `serde_json::to_string_pretty(&report)` picks up
  `critical_paths` automatically. `--paths 0` (default) is byte-identical
  output to today.

## 5. `sta/mod.rs` re-exports

```rust
pub use analysis::{
    CriticalPath, PathEdge, PathStage, /* PathCorner, */
    EndpointKind, EndpointReport, PinSlew, SkipReason, SkippedEndpoint, StaReport,
    merge_corner_reports, run_sta, run_sta_quiet, sta_pin_slews,
    run_sta_quiet_with_paths, run_sta_with_paths,
};
```

## 6. Version

`[workspace.package] version` `0.5.1` -> `0.6.0` in root `Cargo.toml`
(minor — new CLI flag + new JSON surface, matching the `v0.4.0`/`v0.5.0`
precedent), same commit, `Cargo.lock` regenerated.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs`
- `Cargo.toml`
- `Cargo.lock`
