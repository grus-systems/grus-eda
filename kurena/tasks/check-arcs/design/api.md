# check-arcs — Rust-side design

All naming follows the existing `analysis.rs` conventions. The measured
rules from `probe.md` pick between the listed candidates — nothing below is
final until the probe answers land.

## 1. Graph model

```rust
/// `min_pulse_width` arc on a clock pin: one Liberty arc carrying both
/// `rise_constraint` and `fall_constraint`, evaluated as two checks
/// (high and low pulse) at `pin`.
#[derive(Clone, Copy, Debug)]
struct PulseWidthCheck {
    pin: PinId,
    lib_arc: u32,
}
```

`TimingGraph` gains `pulse_width_checks: Vec<PulseWidthCheck>`
(`analysis.rs:675-695`). In `build_graph` (`analysis.rs:1060-1155`), add an
explicit arm **before** `t if t.is_deferred_check()`:

```rust
TimingType::MinPulseWidth => {
    graph.pulse_width_checks.push(PulseWidthCheck { pin: dst, lib_arc });
}
```

`pin` is `dst` (the timing group's own pin; `related_pin` = `target_pin` =
clock pin on every sky130 arc). If any sky130 arc resolves `src != dst`,
record it in `results.md` and use `dst` regardless.

`TimingType::is_deferred_check` (`lib_db.rs:54-68`) loses
`Self::MinPulseWidth`; its doc comment is updated to say `min_pulse_width`
is evaluated since this version. `non_seq_*`, `three_state_*`, `preserve`,
`Clear`, `Preset` stay.

## 2. Clock-pin seeding

Pulse-check pins join every place consumer clock pins are listed so a pin
carrying *only* a `min_pulse_width` arc (possible on cells whose clock pin
has no launch/check — e.g. extra clock pins on multi-clock cells) is still
seeded and its net is a clock net:

- `register_clock_nets` / `consumer_nets` chains (`analysis.rs:1202-1235`)
  gain `graph.pulse_width_checks.iter().map(|p| p.pin)`.
- The `consumers`/`clock_pins` chains in `prepare_sta`
  (`analysis.rs:2214-2220` propagated, `2252-2258` ideal) gain the same.

## 3. Report shape

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseEdge { High, Low }

#[derive(Clone, Debug, Serialize)]
pub struct PulseWidthCheckReport {
    pub pin: String,
    pub edge: PulseEdge,
    pub required_ns: f32,
    pub actual_ns: f32,
    pub slack_ns: f32,
}
```

`StaReport` (`analysis.rs:406-418`) gains
`pub pulse_width_checks: Vec<PulseWidthCheckReport>` — serialized into
`--json` automatically; excluded from `endpoints`, WNS/TNS, and
setup/hold violation counts (probe Q7 confirms OpenSTA's exclusion;
record the measured verdict). `merge_corner_reports`
(`analysis.rs:4094-4103`) merges by `(pin, edge)` keeping the smaller
slack. `EndpointKind` (`analysis.rs:201-208`) gains `MinPulseWidth`
(`min_pulse_width`) — used only by `SkippedEndpoint` when a check pin is
unreached.

## 4. Evaluation

One helper, called with the same post-eval state the skip pass uses
(`state` in `collect_endpoints`, `merged` in `run_sta_partitioned` — clock
pins carry identical clock state in every signature class):

```rust
fn eval_pulse_width_checks(
    cx: &Context,
    graph: &TimingGraph,
    state: &[PinTimingState],
    skipped: &mut SkipReport,
    out: &mut Vec<PulseWidthCheckReport>,
)
```

Per check entry (`pin`, `arc`):

- **Unreached pin** (either transition unreached in `state[pin]`):
  `skipped.unreached_endpoints += 1` + `SkippedEndpoint {
  kind: MinPulseWidth, reason: NoClock, related: Some(pin_name) }` —
  same convention as an unreached check clock (`analysis.rs:3744-3755`).
- **Widths** — edge pair = `(open, close)` = `(rise, fall)` for `high`,
  `(fall, rise)` for `low`:

  `width = arrive(close) − arrive(open)`, wrapped `+period` once when
  `width <= 0` (the next edge of that polarity; inverting pins reorder
  first-occurrence arrivals — probe Q8). `arrive` picks `arrival_max` or
  `arrival_min` per the probe Q4 measured corner rule (default hypothesis
  `arrival_max` — `min_pulse_width_mode : "max"`).
- **Required** — `eval_nldm_constraint` on the arc's
  `rise_constraint`/`fall_constraint` table with the probe Q5 measured
  slew argument, at the probe Q3 measured high/low mapping; the probe Q6
  result decides whether a `sdc.derate(.., DERATE_CHECK, ..)` factor or
  uncertainty term applies (default hypothesis: neither).
- **slack** = `actual − required` (hold-style: negative = violated),
  matching the report's `Slack` column sign convention.

Call sites: `collect_endpoints` returns the rows alongside `entries` (or
fills `out` directly); `run_sta_partitioned` calls the helper once on
`merged` (`analysis.rs:3447-3452`) with the composed `skipped`.
`assemble_sta_report` (`analysis.rs:2675`) takes the rows into `StaReport`
sorted by `(pin, edge)` — pin name ascending, `High` before `Low` — same
ordering the golden uses.

## 5. Counting conventions

- `skipped.total_endpoints += <rows per arc>` (`2 × arcs` if OpenSTA emits
  both pulses per pin — probe Q2; adjust to match the measured surface).
- `arcs_skipped()`/`nonzero()`/`is_clean`: untouched — the arc is
  evaluated, so it no longer appears in `recognized_check_arcs`; the
  remaining deferred types still count.
- `expected_latch_skips` (`analysis.rs:4670-4733`) treats `MinPulseWidth`
  like the modelled kinds (excluded from the recount since it is
  evaluated, not dropped).
- `test_skip_report_counts_latch_arcs` (`analysis.rs:4742-4808`): expected
  maps lose `MinPulseWidth` (`edge_latch`: `{}`; `ibex`: `{Clear: 1655,
  Preset: 9}`). Extend the same test to assert
  `report.pulse_width_checks.len()` on both designs (edge_latch: 2 latch
  arcs → 2 pins × 2 pulses = 4 rows; ibex: 3602 arcs → up to 2·3602 rows
  minus unreached — take the measured count).

## 6. CLI

`main.rs`: the STA summary line (`main.rs:735-741`) stays; add one line
under it: `pulse-width checks: {n} ({v} violating)` — or fold into the
existing summary. `print_skip_summary` unchanged (skipped pulse pins flow
through `skipped_endpoints` already). `--strict` unchanged.

## 7. Tests

- New `compare_sta_edge`-style assertions for the `pulse_width` golden
  section: extend `compare_sta_edge` with an optional compare or add a
  sibling that checks `report.pulse_width_checks` against the golden rows
  at `Tol::STRICT` on `required_ns`/`actual_ns`/`slack_ns` (and exact
  pin/edge names). Tests: `test_sta_edge_min_pulse_width_{ideal,
  propagated,viol}`.
- Update `test_skip_report_counts_latch_arcs` expectations (above).
- Fail-proof (lesson 7): after green, deliberately break one measured
  rule (swap rise/fall constraint mapping or use the wrong corner arrival)
  and confirm `viol`/`propagated` go red; restore.

## 8. Docs touched

`docs/results.md` (dated entry: probe surface + rules + comparison),
`docs/roadmap.md` N3 (deferred note → `min_pulse_width` evaluated,
remaining types still counted), `docs/e3-sta.md` §-line ~220 (no longer
"ignored" — one sentence on the new endpoint family), `README.md` skip
paragraph (`min_pulse_width` leaves the deferred list). `Cargo.toml`
`0.1.92` → `0.1.93` in the same commit as the functional change.
