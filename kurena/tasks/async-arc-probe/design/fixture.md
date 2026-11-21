# async-arc-probe — Fixture `tests/designs/sta_edge/async_arcs/`

One DEF, two SDCs, one `generate_ref.sh` (emits the golden JSON *and* the
verbatim probe text — the `sta_edge` convention; CI's `sta_edge/*/
generate_ref.sh` loop then reproduces both). All hand-written; only
`sky130_fd_sc_hd` cells, matching the other `sta_edge` fixtures.

## 1. `async_arcs.def` — topology

Same skeleton as `async_reset.def` (4 ROWs, TRACKS, `unithd` site, DIEAREA
20000x20000) — copy the header rows verbatim.

### Ports (PINS)

`clk`, `aclk`, `rst_n`, `set_n`, `d_in` (INPUT), `y` (OUTPUT) — same
`+ LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED` style as `async_reset.def`,
spread down the left edge.

`aclk` is the discriminator port: it is connected in the DEF but **no
`create_clock` references it** in any SDC.

### Components (14)

Clock-tree / reset-tree cells:

- `u_clkb` `clkbuf_2` — clock buffer
- `u_rst`  `clkbuf_2` — reset buffer
- `u_set`  `clkbuf_2` — set buffer

Clocked check-variant flops (CLK/CLK_N on `c1`):

- `u_ff0` `dfrtp_1` — control: rising recovery/removal on `RESET_B` +
  `clear` arcs
- `u_fp`  `dfstp_1` — set-only: rising recovery/removal on `SET_B` +
  `preset` arc
- `u_fbp` `dfbbp_1` — dual pins: rising recovery/removal on `RESET_B` and
  `SET_B`, `clear`+`preset` arcs, `non_seq_*` pair
- `u_fbn` `dfbbn_1` — same as `u_fbp` on `CLK_N`: **falling** recovery/removal
  on the remaining inverting-clock variant family
- `u_ff2` `dfxtp_1` — ordinary register-to-register endpoint behind `u_ff0`

Async-only discriminators (CLK on `aclk`, no clock declared):

- `u_ar` `dfrtp_1` — `RESET_B` driven; `Q -> u_ds/D` (`clear` arc)
- `u_ap` `dfstp_1` — `SET_B` driven; `Q -> u_dp/D` (`preset` arc)
- `u_aq` `dfrbp_1` — `RESET_B` driven; `Q_N -> u_dq/D` (the `preset` arc on
  the complementary output)

Capture flops (clocked normally):

- `u_ds`, `u_dp`, `u_dq` `dfxtp_1` — each `D` is reachable *only* through an
  async arc

### Nets (13)

```
- clk   ( PIN clk )   ( u_clkb A )                                                   + USE SIGNAL ;
- c1    ( u_clkb X )  ( u_ff0 CLK ) ( u_fp CLK ) ( u_fbp CLK ) ( u_fbn CLK_N )
                      ( u_ff2 CLK ) ( u_ds CLK ) ( u_dp CLK ) ( u_dq CLK )           + USE SIGNAL ;
- aclk  ( PIN aclk )  ( u_ar CLK ) ( u_ap CLK ) ( u_aq CLK )                        + USE SIGNAL ;
- rst   ( PIN rst_n ) ( u_rst A )                                                  + USE SIGNAL ;
- rst_b ( u_rst X )   ( u_ff0 RESET_B ) ( u_fbp RESET_B ) ( u_fbn RESET_B )
                      ( u_ar RESET_B ) ( u_aq RESET_B )                             + USE SIGNAL ;
- snet  ( PIN set_n ) ( u_set A )                                                  + USE SIGNAL ;
- set_b ( u_set X )   ( u_fp SET_B ) ( u_fbp SET_B ) ( u_fbn SET_B )
                      ( u_ap SET_B )                                              + USE SIGNAL ;
- din   ( PIN d_in )  ( u_ff0 D ) ( u_fp D ) ( u_fbp D ) ( u_fbn D )
                      ( u_ar D ) ( u_ap D ) ( u_aq D )                              + USE SIGNAL ;
- d1    ( u_ff0 Q )   ( u_ff2 D )                                                  + USE SIGNAL ;
- a1    ( u_ar Q )    ( u_ds D )                                                   + USE SIGNAL ;
- a2    ( u_ap Q )    ( u_dp D )                                                   + USE SIGNAL ;
- a3    ( u_aq Q_N )  ( u_dq D )                                                   + USE SIGNAL ;
- y     ( u_ff2 Q )   ( PIN y )                                                    + USE SIGNAL ;
```

(Net names `rst`/`snet`/`set_b` avoid colliding with the port names
`rst_n`/`set_n`; `d1`/`a1`/`a2`/`a3`/`y` follow `async_reset.def` style.)

## 2. SDCs

`ideal.sdc` — copy `async_reset/ideal.sdc` and add the `set_n` input delay
(drop the `d_in`-on-`clk` line if copied — keep exactly these):

```
create_clock -name core_clk -period 4.0 [get_ports clk]
set_clock_latency 0.6 [get_clocks core_clk]
set_clock_latency -source 0.1 [get_clocks core_clk]
set_clock_transition 0.1 [get_clocks core_clk]
set_clock_uncertainty -setup 0.05 [get_clocks core_clk]
set_clock_uncertainty -hold 0.1 [get_clocks core_clk]
set_input_delay 0.5 -clock core_clk [get_ports rst_n]
set_input_delay 0.5 -clock core_clk [get_ports set_n]
set_input_delay 0.7 -clock core_clk [get_ports d_in]
set_output_delay 0.3 -clock core_clk [get_ports y]
set_input_transition 0.1 [all_inputs]
set_load 0.05 [all_outputs]
```

`propagated.sdc` = `ideal.sdc` + `set_propagated_clock [get_clocks core_clk]`
(same shape as `async_reset/propagated.sdc`).

`aclk` deliberately gets only the `all_inputs` transition — no clock, no
input delay.

## 3. `generate_ref.sh`

Clone `async_fall/generate_ref.sh` (container preamble + `run_sta` +
merge block) with two additions:

1. `run_sta ideal.sdc ideal.json` and `run_sta propagated.sdc
   propagated.json`, merged into `async_arcs.reference.json` under keys
   `ideal` / `propagated` with `"design": "sta_edge_async_arcs"`.
2. `run_probe` (the `sta_clocks/probe.sh` `run_report` pattern): one
   `openroad` run per SDC writing `probe_async_<mode>.txt` — `report_checks
   -unconstrained`, the `-from`/`-through` find_timing_paths lists, the
   command-discovery output, and `report_checks -to ... -format full` for
   the check pins. The exact Tcl is specified in `probe.md`; the file names
   are `probe_async_ideal.txt` / `probe_async_propagated.txt`.

If the pulse-width collector from `min_pulse_width/generate_ref.sh`
(`collect_pulse_width_json`, `report_pulse_width_checks -digits 6`) is cheap
to add, include a `"pulse_width"` section per mode — `u_fbn/CLK_N` pulse rows
on an inverting clock pin are new coverage. Do not drop it if OpenSTA emits
rows; `compare_sta_edge` only checks the section when present, and
absent-vs-present is itself a measured fact to state in `results.md`.

Predicted golden endpoint set (verify, do not assert — this is what the probe
settles): `u_ff0/D`, `u_ff0/RESET_B`, `u_fp/D`, `u_fp/SET_B`, `u_fbp/D`,
`u_fbp/RESET_B`, `u_fbp/SET_B`, `u_fbn/D`, `u_fbn/RESET_B`, `u_fbn/SET_B`,
`u_ff2/D`, `y` — plus `u_ds/D`/`u_dp/D`/`u_dq/D` iff OpenSTA propagates async
arcs, and `non_seq` check pins iff OpenSTA reports them.

## 4. Artifacts committed

- `async_arcs.def`, `ideal.sdc`, `propagated.sdc`, `generate_ref.sh`
- `async_arcs.reference.json` (emitted by the script inside the container —
  never hand-edited, AGENTS.md §7.1)
- `probe_async_ideal.txt`, `probe_async_propagated.txt` (verbatim probe text,
  < 1 MB)

After generating, rerun `bash tests/designs/sta_edge/async_arcs/generate_ref.sh`
and confirm `git status` on the dir is clean before committing (CI's
regenerate-and-diff step requires byte-reproducibility).
