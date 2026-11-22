# SDC Examples (`docs/sdc-examples.md`)

Minimal working SDCs for the clock topologies seen in this repository's
fixtures. They use the parser-supported SDC subset and are templates: port
names, clock names, periods, and delays must match the design being run.
Every example below is also exercised by the `sdc_examples_parse` test in
`crates/grus-eda/src/main.rs`, which feeds each block through
`grus_core::sta::parse_sdc`.

## One clock plus input/output delays

```sdc
create_clock -name core_clk -period 5.0 [get_ports clk]
set_input_delay 1.0 -clock core_clk [all_inputs -no_clocks]
set_output_delay 1.0 -clock core_clk [all_outputs]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
```

- `create_clock` anchors timing on the `clk` port.
- `all_inputs -no_clocks` keeps the clock port itself out of the input-delay
  selector.
- `set_input_delay` / `set_output_delay` model external register-to-port
  timing relative to `core_clk`.
- `set_input_transition` and `set_load` are optional but present in the
  committed fixture.

Fixture trace: `tests/designs/gcd/gcd.sdc` (verbatim), consumed by the GCD
STA tests and CLI runs.

## N asynchronous clocks

```sdc
create_clock -name cka -period 2.0 [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
set_clock_groups -asynchronous -group [get_clocks cka] -group [get_clocks ckb]
```

- Put each asynchronous domain in its own `-group`. Clocks that must stay
  synchronous share one group; do not put all asynchronous clocks in one
  group.
- For `N` asynchronous domains use one `-group` per domain; related clocks
  inside a domain may share that domain's group.
- Dropping `set_clock_groups` leaves cross-domain launch/capture pairs
  timed — `tests/designs/sta_clocks/t2_int.sdc` is the corresponding
  unconstrained two-clock fixture.
- `tests/designs/sta_clocks/t2_grpa.sdc` uses `-allow_paths` and is the
  contrast case, not the cut-path example.

Fixture trace: `tests/designs/sta_clocks/t2_grp.sdc` (verbatim) on
`tests/designs/sta_clocks/two_clock.def`; the `sta_clocks` scenario tests
compare all committed SDCs against the OpenSTA JSON references.

### What the STA report shows

- On `two_clock.def` the timed endpoint table drops from 8 rows in `t2_int`
  to 5 rows in `t2_grp` — the three cross-domain candidates `u_fb0/D`,
  `u_fx/D`, `u_fy/D` are removed by the asynchronous groups.
- Those intentionally cut endpoints are accounted for under
  `skipped.false_path_endpoints` in the STA JSON report:
  `set_clock_groups -asynchronous` is implemented by desugaring to pairwise
  false-path exclusions.
- `skipped.cross_domain_endpoints` is the defensive diagnostic for an
  ungrouped cross-domain path that cannot be paired. The committed
  scenarios pair successfully, so `t2_grp` stays at
  `cross_domain_endpoints == 0` rather than moving the intentional cuts
  into the `CrossDomain` skip reason.
- The distinction matters: `false_path_endpoints` is an intended constraint
  effect, while a non-zero `cross_domain_endpoints` or
  `skipped_endpoints[].reason == CrossDomain` means an ungrouped path was
  skipped.

## FF clock-enable/select as data

When a mux select or FF clock-enable gates which data reaches a register,
that signal is a data input, not a clock. Constrain it with input delay and
transition like any other data port:

```sdc
create_clock -name core_clk -period 2.0 [get_ports clk]
set_input_delay -min 0.1 -clock core_clk [get_ports sel]
set_input_delay -max 0.3 -clock core_clk [get_ports sel]
set_output_delay -min 0.1 -clock core_clk [get_ports y]
set_output_delay -max 0.7 -clock core_clk [get_ports y]
set_input_transition 0.1 [get_ports sel]
set_load 0.05 [get_ports y]
```

- `sel` is the enable/select port here; on another design it may be named
  `en`.
- The enable is an ordinary data input. It gets an input delay and
  transition, not a clock declaration.
- Do **not** write `create_clock ... [get_ports sel]`, do **not** write
  `set_clock_groups`, and do **not** create a generated clock for a
  D-mux/FF-enable topology.
- The FF `D` pin stays in the `core_clk` domain; the `sel -> mux -> D` path
  is timed like any other same-domain data path, so it should add neither
  `false_path_endpoints` nor `cross_domain_endpoints`.

Fixture trace: `tests/designs/sta_exceptions/base.sdc` on
`tests/designs/sta_exceptions/exceptions.def`, where `sel` drives mux
selects `u_mux/S` and `u_mux2/S` into `u_ff1/D` and `y2`, and only `clk`
has a `create_clock`. The example keeps the exact command forms exercised
by the fixture and its STA tests, minus unrelated `d_in`, second-output,
and clock-calibration lines.
