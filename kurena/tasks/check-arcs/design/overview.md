# check-arcs — Overview

## Goal

Promote `min_pulse_width` from a counted `recognized_check_arcs` skip to an
evaluated check: for every clock pin carrying a `min_pulse_width` Liberty arc,
evaluate the high pulse (fall arrival − rise arrival) and the low pulse
(next-edge rise arrival − fall arrival) against the arc's required widths and
report one row per (pin, pulse), matched against OpenSTA at STRICT tolerance.
`non_seq_*`, `three_state_*`, `preserve`, and async `Clear`/`Preset` stay
counted-deferred; the census (`ibex` reports only `Clear`/`MinPulseWidth`/
`Preset`, every other design fewer) shows no instantiated instances.

## What exists today

- `TimingType::MinPulseWidth` already exists (`lib_db.rs:31-33`) and
  `is_deferred_check()` (`lib_db.rs:54-68`) routes it into
  `SkipReport::recognized_check_arcs` in `build_graph` (`analysis.rs:1094-1099`),
  exempt from `--strict` via `is_clean`.
- `SkippedEndpoint::kind` (`EndpointKind`, `analysis.rs:201-208`) has
  `Check`/`ClockGate`/`OutputPort`; a new `MinPulseWidth` variant is needed for
  unreached pulse-check pins.
- Clock-pin arrivals are already computed for every launch/check/latch-enable/
  clock-gate pin in both modes: ideal seeds rise at `ideal_latency` and fall at
  `ideal_latency + period/2` with parity tracking (`analysis.rs:2259-2277`);
  `set_propagated_clock` fills them from `propagate_clock` (`analysis.rs:2197-2239`).
  Pins on clock nets keep that state through data eval (`analysis.rs:2513-2515`).
- `run_sta_detail` (`analysis.rs:2002-2019`) has two paths: unpartitioned
  (`eval_sta` → `collect_endpoints`, `analysis.rs:3671`) and partitioned
  (`run_sta_partitioned`, `analysis.rs:3025`, with a merged union state at
  `analysis.rs:3447-3452`). Pulse-width checks are clock-state-only, so one
  helper called from both sites sees identical inputs.

## Measured library facts (sky130_fd_sc_hd__tt_025C_1v80)

- `min_pulse_width` arcs sit on the clock pin itself (`related_pin` =
  `target_pin` = `CLK`; `CLK_N` on `dfrtn`/`dlxtn`-family cells). 113 timing
  groups in the library; `ibex` instantiates 3602 arcs.
- Each arc carries `rise_constraint` and `fall_constraint` on a 1-D
  `constraint_3_0_1` template (`variable_1 : related_pin_transition`, index
  `0.01 / 0.5 / 1.5`). `dfxtp_1`: `0.1687/0.8333/2.5` rise vs
  `0.2082/0.8333/2.5` fall — required widths grow steeply with clock-pin slew
  and differ between the two tables, so the golden `Required` column
  identifies the high/low mapping unambiguously.
- The library declares `min_pulse_width_mode : "max"` (line 149) — the
  candidate rule "actual width uses late (max) arrivals for both edges". The
  derated propagated scenario discriminates this against the early/early and
  mixed alternatives.
- `violation_delay_degrade_pct : 10` on each arc is documentation metadata,
  not part of the check.

## OpenSTA surface (from public docs, to confirm by probe)

`report_check_types -min_pulse_width` is the documented command
(`report_pulse_width_checks` is deprecated in its favour). Its table carries
`Pin` (`<inst>/<pin> (high|low)`), `Required Width`, `Actual Width`,
`Slack`, and a `MET`/`VIOLATED` marker. Whether `find_timing_paths
-path_delay min_pulse_width` returns PathEnd objects in 3.0.16 is a probe
question (`probe.md`).

## Design shape (decisions taken before measurement)

- **Separate report list.** `StaReport` gains
  `pulse_width_checks: Vec<PulseWidthCheckReport>` (`{pin, edge, required_ns,
  actual_ns, slack_ns}`), *not* merged into `endpoints`. OpenSTA's `-path_delay
  min`/`max` endpoint set excludes these checks (different check family), so
  folding them into `endpoints` would break every existing golden's endpoint
  count; WNS/TNS/`setup_violations`/`hold_violations` keep their meaning
  (probe confirms OpenSTA does not fold pulse slack into `worst_slack -min`).
- **The arc leaves `is_deferred_check()`** — it is evaluated now, and the
  arm order in `build_graph` keeps the `MinPulseWidth` arm first. The
  `recognized_check_arcs` map shrinks (ibex → `Clear=1655, Preset=9`); the
  counting convention for the still-deferred types is unchanged.
- **One lib arc → two rows** (high and low pulse) at its pin, matching the
  `(high)`/`(low)` row suffixes OpenSTA prints; `total_endpoints` grows
  accordingly (see `probe.md` Q3 for the exact count rule).
- **No `-waveform` support is added.** `create_clock` accepts only
  `-name`/`-period` and bails on unknown flags (`sdc.rs:1767-1768`), so the
  fixture tightens pulses with `create_clock -period` and drives the required
  width up with `set_clock_transition` (the slew-indexed constraint table),
  not a custom duty cycle.

## Open questions the probe must answer (probe.md)

1. Which command/fields emit the checks programmatically (PathEnd accessors
   vs `report_check_types` text).
2. Row granularity: rows per (pin, pulse), or worst-only.
3. `rise_constraint`/`fall_constraint` ↔ high/low pulse mapping.
4. Corner rule for `Actual Width`: late/late, early/early, or mixed.
5. Which slew feeds `related_pin_transition`.
6. Whether clock uncertainty or `-cell_check` derates touch `Required`.
7. Whether pulse slack feeds `worst_slack`/TNS/`report_checks` endpoint sets.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/lib_db.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/sta_edge/min_pulse_width/`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/e3-sta.md`
- `README.md`
- `Cargo.toml`
- `Cargo.lock`
