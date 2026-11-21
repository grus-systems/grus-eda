# deferred-check-arcs — Fixture `tests/designs/sta_edge/check_arcs/`

New sta_edge case directory: one DEF, three SDCs, one `generate_ref.sh`
emitting the golden JSON plus verbatim probe text per mode (`async_arcs`
convention; CI's `sta_edge/*/generate_ref.sh` loop picks it up). Hand-
written, `sky130_fd_sc_hd` cells only. Header (VERSION/NAMESCASESENSITIVE/
DIVIDERCHAR/BUSBITCHARS/DIEAREA/ROW/TRACKS/`unithd` site) copied verbatim
from `async_arcs.def`.

## 1. `check_arcs.def` — topology

Ports (`+ LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED`, `async_arcs` style,
spread down the left edge): `clk`, `rst_n`, `set_n`, `d_in`, `te` (INPUT),
`y` (OUTPUT). Every port nets out to a buffer or cell pin.

### Components (14)

| inst | cell | role |
|---|---|---|
| `u_clkb` | `clkbuf_2` | `clk -> c1` clock buffer |
| `u_rst` | `clkbuf_2` | `rst_n -> rst_b` |
| `u_set` | `clkbuf_2` | `set_n -> set_b` |
| `u_dbuf` | `buf_2` | `d_in -> d1` |
| `u_teb` | `buf_2` | `te -> te1` enable buffer |
| `u_fbp` | `dfbbp_1` | dual async pins: `non_seq_*` pair, `clear`+`preset` arcs, rising recovery/removal on both `SET_B`/`RESET_B` |
| `u_fbn` | `dfbbn_1` | same set on `CLK_N` (falling recovery/removal variant) |
| `u_ebuf` | `ebufn_1` | `three_state_*` on `Z` rel `TE_B` (buffer, enable-lo) |
| `u_einv` | `einvn_1` | same (inverter, enable-lo) |
| `u_evp` | `einvp_1` | same (inverter, enable-hi `TE`) |
| `u_cap1` | `dfxtp_1` | `D <- u_ebuf/Z` — capture behind enable arc |
| `u_cap2` | `dfxtp_1` | `D <- u_einv/Z` |
| `u_cap3` | `dfxtp_1` | `D <- u_evp/Z` |
| `u_ff` | `dfxtp_1` | ordinary endpoint, `Q -> y` |

### Nets (16)

```
- clk   ( PIN clk )  ( u_clkb A )                                              + USE SIGNAL ;
- c1    ( u_clkb X ) ( u_fbp CLK ) ( u_fbn CLK_N ) ( u_cap1 CLK )
                     ( u_cap2 CLK ) ( u_cap3 CLK ) ( u_ff CLK )                + USE SIGNAL ;
- rin   ( PIN rst_n ) ( u_rst A )                                              + USE SIGNAL ;
- rst_b ( u_rst X )  ( u_fbp RESET_B ) ( u_fbn RESET_B )                       + USE SIGNAL ;
- sin   ( PIN set_n ) ( u_set A )                                              + USE SIGNAL ;
- set_b ( u_set X )  ( u_fbp SET_B ) ( u_fbn SET_B )                           + USE SIGNAL ;
- din   ( PIN d_in ) ( u_dbuf A )                                              + USE SIGNAL ;
- d1    ( u_dbuf X ) ( u_fbp D ) ( u_fbn D ) ( u_ebuf A ) ( u_einv A )
                     ( u_evp A ) ( u_ff D )                                    + USE SIGNAL ;
- tin   ( PIN te )   ( u_teb A )                                               + USE SIGNAL ;
- te1   ( u_teb X )  ( u_ebuf TE_B ) ( u_einv TE_B ) ( u_evp TE )              + USE SIGNAL ;
- z1    ( u_ebuf Z ) ( u_cap1 D )                                              + USE SIGNAL ;
- z2    ( u_einv Z ) ( u_cap2 D )                                              + USE SIGNAL ;
- z3    ( u_evp Z )  ( u_cap3 D )                                              + USE SIGNAL ;
- qb1   ( u_fbp Q )                                                           + USE SIGNAL ;
- qb2   ( u_fbn Q_N )                                                         + USE SIGNAL ;
- y     ( u_ff Q )   ( PIN y )                                                + USE SIGNAL ;
```

`rst_b`/`set_b` come from *different* ports so a non_seq data check between
`SET_B` and `RESET_B` gets a distinct slack from the recovery/removal check
(probe Q1). `qb1`/`qb2` single-pin dangling nets keep the flop outputs
connected (DEF has no unconnected-pin marker; `async_arcs` did the same via
downstream loads — here the loads would add noise, so the outputs dangle).
If the DEF reader or OpenSTA rejects a single-pin net, tie `qb1`/`qb2` into
`y`'s fan-in-free sink set instead (a second `dfxtp` `D` is not wanted — it
would add an endpoint); record the choice in `results.md`.

## 2. SDCs

`ideal.sdc` — `async_arcs/ideal.sdc` preamble with the fixture's ports and
*distinct* input delays:

```
create_clock -name core_clk -period 4.0 [get_ports clk]
set_clock_latency 0.6 [get_clocks core_clk]
set_clock_latency -source 0.1 [get_clocks core_clk]
set_clock_transition 0.1 [get_clocks core_clk]
set_clock_uncertainty -setup 0.05 [get_clocks core_clk]
set_clock_uncertainty -hold 0.1 [get_clocks core_clk]
set_input_delay 0.5 -clock core_clk [get_ports rst_n]
set_input_delay 0.9 -clock core_clk [get_ports set_n]
set_input_delay 0.7 -clock core_clk [get_ports d_in]
set_input_delay 0.7 -clock core_clk [get_ports te]
set_output_delay 0.3 -clock core_clk [get_ports y]
set_input_transition 0.1 [all_inputs]
set_load 0.05 [all_outputs]
```

`propagated.sdc` = `ideal.sdc` + `set_propagated_clock [get_clocks core_clk]`.

`te_slow.sdc` = `ideal.sdc` with `set_input_delay 2.5` on `te` — the
propagation discriminator (probe Q2): a propagating `TE_B -> Z` arc shifts
`u_cap*/D` max_arr by ~1.8 ns vs `ideal`; an ignored arc leaves the golden
identical to `ideal`'s endpoint block.

## 3. `generate_ref.sh`

Clone `async_arcs/generate_ref.sh` (container self-re-exec, PDK mount,
per-scenario `openroad` processes, `SCRATCH_DIR` intermediates,
`collect_endpoints_json`/`collect_pulse_width_json` reused verbatim):

- `run_sta` × 3 → `ref_{ideal,propagated,te_slow}.json`, merged into
  `check_arcs.reference.json` under keys `ideal` / `propagated` / `te_slow`
  with `"design": "sta_edge_check_arcs"`.
- `run_probe` × 3 → `probe_check_arcs_{ideal,propagated,te_slow}.txt` —
  the full Q0–Q5 command list from `probe.md` (Q0's flag enumeration runs in
  the ideal session only; per-flag `catch` results are identical anyway).
- If the Q1 surface emits data-check rows, add a `collect_data_checks_json`
  (regexp-parse `report_check_types -data_setup/-data_hold` output into
  `{"pin":…, "ref_pin":…, "kind":…, "required_ns":…, "arrival_ns":…,
  "slack_ns":…}` rows, `lsort -dictionary`) and a `"data_checks"` section
  per mode. If the surface is `find_timing_paths`-based, data-check
  PathEnds join `endpoints` through the existing collector and no new
  section is needed — the probe says which.
- `pulse_width` rows: every `c1` clock pin emits them (`u_fbn/CLK_N`
  inverting-pin rows included); keep the collector — do not drop it even if
  it is not this ticket's focus (fixture parity precedent, `async_arcs` §3).

## 4. Artifacts committed

`check_arcs.def`, the three SDCs, `generate_ref.sh`,
`check_arcs.reference.json`, `probe_check_arcs_*.txt` (all < 1 MB). Rerun
the committed script before commit and confirm `git status` on the dir is
clean (AGENTS.md §7.1; CI regenerates and diffs).
