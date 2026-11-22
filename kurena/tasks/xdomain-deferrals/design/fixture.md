# xdomain-deferrals — Fixture changes

All new SDCs live in `tests/designs/sta_clocks/` and run on the existing
DEFs — no new DEF is required (the optional latch flop-launch variant in
`probe.md` is the only exception, and only if item 2's port-launch probe is
ambiguous).

## New SDC files

### `t2_u1.sdc` — item 1, unclocked-launch cone (on `two_clock.def`)

```sdc
create_clock -name cka -period 2.0 [get_ports clk1]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

`u_fy/D` is the corner: cka-captured, launched by unclocked `u_fb0/Q`.
`u_fx/D`, `u_fb0/D`, `u_fb1/D`, `yb` are unclocked-side controls.

### `t2_u2.sdc` — item 1, `-to`-covered form

`t2_u1.sdc` plus:

```sdc
set_max_delay 1.5 -to [get_pins u_fy/D]
set_min_delay 0.5 -to [get_pins u_fy/D]
```

Does the delay-check surface report `u_fy/D` anyway? (cf. `t2_mdx`'s
pin-startpoint form — a `-to`-only check searches fan-in startpoints.)

### `t2_xlat.sdc` / `t2_xlat2.sdc` — item 2 (on `two_clock_latch.def`)

`t2_latch.sdc` with `d2` re-anchored on `cka`; `t2_xlat2` raises `-max` to
1.3 so the arrival moves deeper into/past the ckb borrow window:

```sdc
create_clock -name cka -period 2.0 [get_ports clk1]
create_clock -name ckb -period 1.0 [get_ports clk2]
set_clock_latency 0.6 [get_clocks cka]
set_clock_latency 0.2 [get_clocks ckb]
set_clock_uncertainty -setup 0.05 [get_clocks cka]
set_clock_uncertainty -setup 0.15 [get_clocks ckb]
set_input_delay -max 0.7 -clock cka [get_ports d2]
set_input_delay -min 0.1 -clock cka [get_ports d2]
set_input_delay -max 0.5 -clock cka [get_ports d1]
set_input_delay -min 0.1 -clock cka [get_ports d1]
set_output_delay -max 0.5 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_input_transition 0.1 [all_inputs]
set_load 0.05 [all_outputs]
```

(`t2_xlat2`: `set_input_delay -max 1.3 -clock cka [get_ports d2]`.)

### `t2_lcm.sdc` / `t2_lcm2.sdc` — item 4 (on `two_clock.def`)

`t2_int.sdc` with the second clock's period changed:

- `t2_lcm`: `create_clock -name ckb -period 1.7320508 [get_ports clk2]`
- `t2_lcm2`: `create_clock -name ckb -period 1.999999 [get_ports clk2]`

## `probe.sh` changes

- `run_report` gains an optional 4th argument — a wall-clock bound in
  seconds, applied as `timeout <s> openroad ...` — used by the `t2_lcm*`
  calls so a non-convergent OpenSTA search records as a timeout in the
  artifact instead of stalling the probe.
- New list after `MEASURE_ONLY`:

  ```sh
  # xdomain-deferrals evidence: each probes one parked CrossDomain corner.
  # run_report only — nothing folds into the goldens unless the item is
  # implemented (then the scenario moves to SCENARIOS).
  XDEFER="t2_u1 t2_u2 t2_lcm t2_lcm2"
  for name in $XDEFER; do
      run_report "two_clock.def" "$name.sdc" "probe_$name.txt" 300
  done
  ```

- Item 2 runs on the latch DEF — extend the latch block:

  ```sh
  run_sta "two_clock_latch.def" "t2_latch.sdc" "ref_t2_latch.json"
  run_report "two_clock_latch.def" "t2_latch.sdc" "probe_t2_latch.txt"
  # xdomain-deferrals: latch-D with a foreign (cka) launch.
  for name in t2_xlat t2_xlat2; do
      run_report "two_clock_latch.def" "$name.sdc" "probe_$name.txt"
  done
  ```

- If an item is implemented: move its scenario into `SCENARIOS` (or the
  latch `run_sta`/`run_report` pair) in the same change so `ref_*.json`
  folds into `two_clock.reference.json` /
  `two_clock_latch.reference.json`.

## Files table

| path | status |
|---|---|
| `t2_u1.sdc`, `t2_u2.sdc`, `t2_lcm.sdc`, `t2_lcm2.sdc` | new, on `two_clock.def` |
| `t2_xlat.sdc`, `t2_xlat2.sdc` | new, on `two_clock_latch.def` |
| `probe_t2_u1.txt`, `probe_t2_u2.txt`, `probe_t2_lcm.txt`, `probe_t2_lcm2.txt`, `probe_t2_xlat.txt`, `probe_t2_xlat2.txt` | new committed artifacts |
| `probe.sh` | `run_report` timeout arg, `XDEFER` list, latch block loop |
| `two_clock_latchx.def` | optional — only if item 2 needs a flop-launch discriminator |
