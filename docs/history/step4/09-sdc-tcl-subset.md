# Step 4 ticket 09 — SDC Tcl subset and clock latency (ibex E3 unblocker)

## Background

The ibex E3 tests (`test_ibex_{zero_wire,placement_rc,placed_spef}_matches_opensta`)
all fail at SDC load: `parse_sdc` bails on line 1 of the verbatim ORFS SDC
(`tests/designs/ibex/ibex.sdc`) with `unsupported command 'current_design'`.
The earlier designs' SDCs are flat command files; the ibex one is real Tcl:

```
current_design ibex_core

set clk_name core_clock
set clk_port_name clk_i
set clk_period 8.0
set clk_io_pct 0.2

set clk_port [get_ports $clk_port_name]

create_clock -name $clk_name -period $clk_period $clk_port
set clk_io_name vclk_$clk_name
create_clock -name $clk_io_name -period $clk_period
set_clock_latency 1.095 [get_clocks $clk_name]
set_clock_latency 1.095 [get_clocks $clk_io_name]

set non_clock_inputs [all_inputs -no_clocks]

set_input_delay [expr $clk_period * $clk_io_pct] -clock $clk_io_name $non_clock_inputs
set_output_delay [expr $clk_period * $clk_io_pct] -clock $clk_io_name [all_outputs]
```

The three ibex STA tests are `#[ignore]`d with a pointer to this ticket; this
landing un-ignores them. No value comparison has been possible yet, so the
latency semantics below are pinned by the golden at maintainer acceptance.

## Files

- Modify: `crates/grus-core/src/sta/sdc.rs`
- Modify: `crates/grus-core/src/sta/analysis.rs` (latency application only)
- Modify: `crates/grus-core/src/sta/analysis.rs` (test module only — un-ignore the three ibex STA tests)

## Work

1. **Tcl subset in `parse_sdc`** (keep the existing flat commands working
   unchanged; add a unit test on the ibex SDC text itself, which needs no
   PDK): a variable environment for `set <name> <value>`; `$var` substitution
   including concatenation (`vclk_$clk_name`); `[expr ...]` with the four
   arithmetic operators, parentheses, numbers and `$var` references;
   command substitution for `[get_ports <name>]`, `[get_clocks <name>]`,
   `[all_inputs]`, `[all_outputs]`, `[all_inputs -no_clocks]` (the
   `-no_clocks` selector must exclude the clock port from input delays);
   `current_design` accepted and ignored. Keep bailing loudly on any
   construct outside this subset.
2. **Clock latency**: record `set_clock_latency <value> [get_clocks <name>]`
   per clock. `Sdc` gains `clock_latency_ns` for the design clock and
   `io_clock_latency_ns` for the virtual IO clock (the `-clock` argument of
   `set_input_delay`/`set_output_delay` selects which clock the delay is
   referenced to; default to the design clock when the flag is absent).
3. **Apply latency in the analysis** in the standard OpenSTA way: a clock's
   edges are delayed by its latency wherever that clock is referenced.
   Concretely: seed the clock network's source arrival with
   `clock_latency_ns`; seed an input port's data arrival with
   `io_clock_latency_ns + input_delay`; an output port's required time is
   `io_clock_latency_ns + period_ns - output_delay` (and the early-time
   analogue). Internal register-to-register paths then see the latency
   cancel (same clock both sides); IO-boundary paths see the difference.
   With both ibex latencies at 1.095 the zero-wire result should match a
   no-latency parse — the golden decides.

## Acceptance

- `parse_sdc` on the ibex SDC yields: period 8.0, clock name `core_clock`,
  clock port `clk_i`, input delay 1.6 on non-clock inputs, output delay 1.6
  on outputs, both latencies 1.095. (Unit test, no PDK.)
- The gcd/aes/jpeg/picorv32 SDCs parse to the same values as before the
  change (regression tests or a recorded before/after — the maintainer
  re-runs the full suite on Intel, where those designs pass).
- `cargo clippy --all-targets -- -D warnings` is clean (note the `--`
  before `-D warnings`).
- The three ibex STA tests are un-ignored in this landing.
- The maintainer runs the real acceptance on Intel: ibex zero-wire must
  pass the strict tolerance; the two wire modes are measured and recorded,
  and if they fail they are `#[ignore]`d by the maintainer with the
  measured tables (the aes/jpeg precedent) — do not widen any shared
  tolerance in this ticket.
- Final report: the parsed values, any deviation from the semantics in
  this ticket, and anything the golden contradicts.
