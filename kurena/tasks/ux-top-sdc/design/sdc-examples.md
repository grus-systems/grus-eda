# `docs/sdc-examples.md` design

## Shape of the document

Create one concise user-facing page, `docs/sdc-examples.md`, with:

1. A short scope note: examples use the parser-supported SDC subset and are templates; port names, clock names, periods, and delays must match the design being run.
2. Three labeled sections, each with exactly one fenced block tagged `sdc`.
3. A fixture citation and explanation after each block.
4. A short "what the STA report shows" subsection for the asynchronous example.

Keep exactly three ` ```sdc` fences in the page so the parse test can require `blocks.len() == 3` and fail if a malformed or extra example is added.

## Example 1: one clock plus I/O delays

Use `tests/designs/gcd/gcd.sdc` verbatim:

```sdc
create_clock -name core_clk -period 5.0 [get_ports clk]
set_input_delay 1.0 -clock core_clk [all_inputs -no_clocks]
set_output_delay 1.0 -clock core_clk [all_outputs]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
```

Explain:

- `create_clock` anchors timing on the `clk` port.
- `all_inputs -no_clocks` keeps the clock port itself out of the input-delay selector.
- `set_input_delay` and `set_output_delay` model external register-to-port timing relative to `core_clk`.
- `set_input_transition` and `set_load` are optional but present in the committed fixture.

Fixture trace: `tests/designs/gcd/gcd.sdc`, consumed by existing GCD STA tests and CLI runs.

## Example 2: asynchronous clock domains

Use `tests/designs/sta_clocks/t2_grp.sdc` verbatim, including its two `create_clock` commands and one `set_clock_groups -asynchronous` declaration:

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

Explain:

- Put each asynchronous domain in its own `-group`. Clocks that must remain synchronous may be placed in the same group; do not put all asynchronous clocks in one group.
- For `N` asynchronous domains, use one `-group` per domain. Additional related clocks inside one domain may share that domain's group.
- Removing `set_clock_groups` leaves valid cross-domain launch/capture pairs timed. `tests/designs/sta_clocks/t2_int.sdc` is the corresponding unconstrained two-clock fixture.
- `tests/designs/sta_clocks/t2_grpa.sdc` uses `-allow_paths` and therefore is not the cut-path example.

Fixture traces: `tests/designs/sta_clocks/t2_grp.sdc` and `tests/designs/sta_clocks/two_clock.def`. Existing scenario coverage reads all committed `sta_clocks` SDCs and compares behavior to the committed OpenSTA JSON references.

### Observable report effect

Name the fields rather than using a loose phrase:

- On `two_clock.def`, the timed endpoint table drops from 8 rows in `t2_int` to 5 rows in `t2_grp` because the three cross-domain candidates (`u_fb0/D`, `u_fx/D`, `u_fy/D`) are removed by the asynchronous groups.
- Those intentionally cut endpoints are accounted for under `skipped.false_path_endpoints` in the STA JSON report; `set_clock_groups` is implemented by desugaring to pairwise false-path exclusions.
- `skipped.cross_domain_endpoints` is the defensive diagnostic for an ungrouped cross-domain path that cannot be paired; committed scenarios pair successfully, so `t2_grp` remains at `cross_domain_endpoints == 0` rather than moving the intentional cuts into the `CrossDomain` skip reason.
- This distinction matters: `false_path_endpoints` is an intended constraint effect, while a non-zero `cross_domain_endpoints` or `skipped_endpoints[].reason == CrossDomain` means an ungrouped path was skipped.

## Example 3: FF clock-enable/select as data

Use the relevant lines from `tests/designs/sta_exceptions/base.sdc`; `sel` in `exceptions.def` drives mux selects `u_mux/S` and `u_mux2/S`, including the data path into `u_ff1/D`, while `u_ff0`/`u_ff1`/`u_ff2` are clocked by the real `clk` domain. Show the same constraint shape:

```sdc
create_clock -name core_clk -period 2.0 [get_ports clk]
set_input_delay -min 0.1 -clock core_clk [get_ports sel]
set_input_delay -max 0.3 -clock core_clk [get_ports sel]
set_output_delay -min 0.1 -clock core_clk [get_ports y]
set_output_delay -max 0.7 -clock core_clk [get_ports y]
set_input_transition 0.1 [get_ports sel]
set_load 0.05 [get_ports y]
```

Explain:

- `sel` is the enable/select port here; on another design this may be named `en`.
- The enable is an ordinary data input. It gets an input delay and transition, not a clock declaration.
- Do not write `create_clock ... [get_ports sel]`, do not write `set_clock_groups`, and do not create a generated clock for a D-mux/FF-enable topology.
- The FF `D` pin remains in the `core_clk` domain; the `sel -> mux -> D` path is timed like any other same-domain data path, so it should not add intentional `false_path_endpoints` or `cross_domain_endpoints`.

Fixture traces: `tests/designs/sta_exceptions/base.sdc` and `tests/designs/sta_exceptions/exceptions.def`. The example omits unrelated `d_in`, second-output, and clock-calibration lines but keeps the exact command forms exercised by the fixture and its STA tests.

## Parse coverage

Add a `main.rs` unit test that treats the document as the test input, for example:

```rust
#[test]
fn sdc_examples_parse() {
    let doc = include_str!("../../../docs/sdc-examples.md");
    let blocks: Vec<&str> = doc
        .split("```sdc")
        .skip(1)
        .map(|rest| rest.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 3);
    for block in blocks {
        grus_core::sta::parse_sdc(block).expect("documented SDC example must parse");
    }
}
```

This proves every documented code block is syntactically valid without adding a new fixture or a separate parser test file. Existing fixture tests already exercise each command form semantically:

- `gcd.sdc` through GCD STA tests.
- `t2_grp.sdc` through `test_sta_clocks_match_opensta` scenario coverage.
- `base.sdc` through the `sta_exceptions` fixture tests.

## Files

- `docs/sdc-examples.md`
- `crates/grus-eda/src/main.rs`
