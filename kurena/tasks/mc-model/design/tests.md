# mc-model — Test plan

Every existing golden stays byte-identical — no `*.reference.json`
diffs, no tolerance change. The changes below are all in
`crates/grus-core/src/sta/sdc.rs::tests` unless noted.

## 1. Mechanical updates (assert identical values through accessors)

| test | update |
|---|---|
| `parses_the_gcd_constraints`, `flat_sdcs_keep_their_values`, `parses_stock_write_sdc_unchanged` | `sdc.clock_port` → `clock_port()`, `period_ns` → `period_ns()`, `clock_name` → `clock_name()` |
| `parses_the_ibex_tcl_sdc` | `clock_latency_ns` → `sdc.design_clock().unwrap().latency_ns`; `io_clock_latency_ns` → `sdc.clock("vclk_core_clock").unwrap().latency_ns` |
| `propagated_clock_*`, `clock_reference_falls_back_to_the_source_port_name`, `parses_the_write_sdc_output`, `keeps_negative_values_and_the_known_flags`, `clock_uncertainty_setup_hold_and_clock_list` | field reads → accessors (`propagated_clock()`, `clock_transition_ns()`, `setup/hold_uncertainty_ns()`, `design_clock()` fields for latency/source) |
| analysis.rs test-only sites (~7218, ~8372) | `sdc.propagated_clock()` / `sdc.is_clock_port(port_name)` |

## 2. Replaced tests (the pinned bail/bug surfaces that no longer exist)

| old test | new test | asserts |
|---|---|---|
| `rejects_a_second_design_clock` | `second_design_clock_appends_a_row` | two `create_clock`s → `clocks.len() == 2`, per-row `name`/`port`/`period_ns` correct |
| `rejects_a_virtual_clock_with_another_period` | `virtual_clock_keeps_its_own_period` | `create_clock -name vclk -period 4.0` parses; row carries `period_ns == 4.0`, `port == None` |
| `uncertainty_on_virtual_clock_writes_design_fields` (bug pin) | `uncertainty_lands_on_the_named_row` | `set_clock_uncertainty -setup 0.20 [get_clocks vclk]` → vclk row `setup_uncertainty_ns == 0.20`, **design row stays 0.0** — the silent wrong-target regression |

## 3. New accepted-surface tests

- `set_clock_latency 0.5 [get_clocks {core_clk vclk}]` → both rows'
  `latency_ns == 0.5`; `-source` variant writes `source_latency_ns`.
- `set_clock_latency 0.5 [get_clocks {cka ckb}]` on a two-design-clock
  SDC → both rows.
- `set_clock_uncertainty -setup/-hold` per name on a two-design-clock
  SDC → each row gets its own value (`-setup 0.05` on cka,
  `-setup 0.20` on ckb — the `t2_unc` shape).
- `set_clock_transition 0.1 [get_clocks {core_clk vclk}]` → both rows
  `transition_ns == 0.1`; `ignored_options` has no
  `set_clock_transition (virtual clock)` entry.
- `set_propagated_clock [get_clocks cka]` on a two-design-clock SDC →
  only row 0 `propagated`; `[all_clocks]` sets every port-bearing row
  and still bails on a virtual row.
- `-clock ckb` on `set_output_delay` resolves: set a latency on ckb,
  assert `output_clock_latency_ns("yb")` reads row 1 (proves
  `DelayRule.clock` is a real row index, not a two-bucket enum).

## 4. Still-bailing surface (`multi_clock_bails` updated)

Keep verbatim-message assertions for:

- `set_clock_latency -early 0.5 [get_clocks core_clk]` →
  ``unsupported option `-early` ``
- `set_clock_uncertainty -setup 0.30 -from [get_clocks a] -to [get_clocks b]` →
  `set_clock_uncertainty takes a value and optional clock names`
- `create_clock -waveform {0 0.5} -period 2.0 [get_ports other]` →
  ``unsupported option `-waveform` ``
- `set_clock_groups -asynchronous …` →
  ``unsupported command `set_clock_groups` ``
- `set_input_delay 1.0 -clock other …` → ``unknown clock `other` ``
- `set_propagated_clock [get_clocks other]` → ``unknown clock `other` ``
- `set_propagated_clock [get_clocks vclk]` (virtual) →
  `set_propagated_clock does not apply to the virtual clock`
- `create_clock -period 0` (any row) → `finish()` period error

Plus one standalone test (the `multi_clock_bails` fixtures always
declare a clock first, so this cannot be another case row):

- `clock_commands_before_create_clock_bail`: `set_clock_latency 0.5`
  and `set_clock_uncertainty 0.5` on an empty table →
  `no design clock declared yet` — pins the ordering edge (a no-target
  clock command needs a port-bearing row at apply time; previously it
  wrote the scalar fields unconditionally).

## 5. Eval gate (analysis.rs tests)

```rust
#[test]
fn sta_bails_on_a_two_clock_sdc() {
    // load sky130_hd liberty + tech, parse_def(two_clock.def),
    // load_sdc(t2_int.sdc) — SDC parses (two rows), run_sta errors.
    let err = run_sta(&design, &lib, &sdc, None).unwrap_err();
    assert!(format!("{err:#}").contains("design clocks"), "{err:#}");
}
```

Same fixture pattern as `for_each_pins_design` (`design_dir` +
`sky130_hd` + `StringInterner` + `parse_def`). One test on `run_sta`
covers the funnel; `sta_pin_slews`/`run_sta_quiet` share
`run_sta_detail`. Record the verbatim CLI message in `docs/results.md`
(the acceptance item is the CLI run, not just the unit test).

## 6. Verification checklist

- `cargo test --workspace` — green, all reference fixtures untouched.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- Break-it check (AGENTS.md §7 rule 7): remove the gate →
  `sta_bails_on_a_two_clock_sdc` fails; restore.
- Golden identity: `git status tests/designs/` shows no modified
  `*.reference.json` / `*.txt` artifacts after the test run.
