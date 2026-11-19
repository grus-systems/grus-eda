# load-selectors — Overview

## Goal

Close the `docs/roadmap.md` N2 gap: `set_load` accepts only port selectors
and `set_fanout_load` is not in the SDC command table. A stock
`write_sdc`-style file that annotates loads on nets or pins must parse; an
annotation the timing model can honour must change timing the same way
OpenSTA does, and anything else must become a named skip instead of a
whole-file parse failure.

## Current state

- `Sdc::loads` is `Vec<(PortSel, f32)>` (`sdc.rs`); `set_load` calls
  `value_and_ports`, which requires `[numeric, Value::Ports]`.
- `Value` has `Ports`, `Cells`, `Pins`, `Clocks`, `Str`, and `Opaque`, but
  no net collection. `get_nets`, `all_fanouts`, and `all_fanins` fall
  through `is_object_selector` to `Value::Opaque`; `get_pins` already
  produces `Value::Pins`.
- `set_load <v> [get_pins ...]` therefore fails the whole SDC on the type
  mismatch, `set_load <v> [get_nets ...]` fails through `Opaque`, and
  `set_fanout_load` fails as an unsupported command.
- Timing application is also port-shaped: `Context::sink_cap` calls
  `Sdc::output_load_pf` only for top-level output-port pins, and
  `Context::net_load` adds that pin capacitance to the sink sum.
  `driver_response` reads the lumped load in zero-wire mode or the pi
  model in parasitic mode; `wire_delays` and `pi_model` both consume the
  same per-pin capacitance vector.
- `SkipReport::sdc_ignored_commands` is already the right surface for a
  command the parser accepts but cannot apply. It is copied into the
  report and printed by `nonzero()`; `is_clean()` currently exempts only
  the `set_max_*` design rules.

## Non-negotiables

- Measure first. OpenSTA source is GPL and must not be read; use the
  tool's output inside the pinned LibreLane image. Record the before
  failure mode and every measured rule in `docs/results.md`.
- Do not silently drop a selection. Every accepted-but-unapplied command
  or unresolved object gets a named count.
- A selector outside the modelled set skips the whole `set_load` command.
  Do not partially apply an opaque cone query.
- Preserve the current output-port behaviour and file-order precedence.
- Keep symbolic selectors symbolic until `DesignDb` is available; do not
  flatten names into opaque strings earlier than needed.
- Skip diagnostics must survive the unpartitioned and exception-
  partitioned STA paths and `merge_skip_reports`.

## Design shape

1. Extend the parser's typed value space with net collections and replace
   `Vec<(PortSel, f32)>` with load rules carrying a `LoadSel`
   (`Ports`/`Pins`/`Nets`). `get_nets` emits `Value::Nets` for plain name
   lists and `Value::Opaque` for flags or nested selections.
2. `set_load` accepts exactly one numeric value plus one object
   collection. `Ports`, `Pins`, and `Nets` become symbolic rules;
   `Opaque`, clock/cell collections, and unsupported qualifiers make the
   whole command a named `ignored_commands["set_load"]` skip. Malformed
   commands still error.
3. `set_fanout_load` joins the command table with the same value+object
   shape. Unless the probe shows an effect the delay model can reproduce,
   it is accepted and counted as `ignored_commands["set_fanout_load"]`.
4. At `prepare_sta`, resolve load rules before pi-model and wire-delay
   construction. Resolution produces per-pin/per-net capacitance
   assignments plus an `unresolved_load_selectors` count for names that do
   not exist. The same resolved caps feed `sink_cap`, `net_load`,
   `pi_model`, and `elmore_ns`, so zero-wire and parasitic modes cannot
   diverge silently.
5. Extend `SkipReport` for resolution-time misses and merge it through
   `nonzero()`, `is_clean()`, `merge_skip_reports()`, and the partitioned
   report path.

## Rules the probes must settle

- `get_nets`: whether the value is added to the existing net load,
  replaces wire capacitance, or replaces the net's total capacitance; and
  where it sits in the RC model when parasitics are present.
- `get_pins`: whether the value is added to or replaces the pin's Liberty
  capacitance, whether it attaches to the pin itself or the attached net,
  and what a driver-pin selection does.
- Multi-object and repeated commands: last-wins, accumulation, and what
  happens when part of a collection is missing.
- `all_fanouts`/`all_fanins`: accepted spellings and options, object
  type returned, and whether `set_load` accepts the collection.
- `set_fanout_load`: whether OpenSTA 3.0.16 accepts it, accepted object
  sets/options, and whether it changes endpoint timing or only fanout
  design-rule reporting.

Until those answers are recorded, the timing application point remains a
candidate, not a decision. The implementation must select the measured
mapping in `api.md` and record the rejected alternatives in
`docs/results.md`.

## Strictness policy

- Parse-time unsupported object selectors (`Opaque`, `Value::Cells`,
  `Value::Clocks`) count as `sdc_ignored_commands` and make `--strict`
  fail: the selection could have changed an arrival.
- Resolve-time missing net/pin names count as
  `unresolved_load_selectors` and make `--strict` fail.
- `set_fanout_load` counts under `sdc_ignored_commands`. Add it to the
  `is_clean()` design-rule exemption only if the probe proves it cannot
  alter the reported endpoint/arrival surface (for example, it only feeds
  `report_check_types -max_fanout`).
- If OpenSTA rejects a selector or command outright, our parser may still
  accept it as a counted skip; the golden records the reference error and
  the diagnostics test asserts our count rather than endpoint numbers.

## Files

- `crates/grus-core/src/sta/sdc.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/sta_exceptions/`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/timing-exceptions.md`
- `docs/e3-sta.md`
- `Cargo.toml`
- `Cargo.lock`
