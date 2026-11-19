# Fixture — `tests/designs/sta_clocks/`

New hand-written fixture directory. Reusing `sta_exceptions` was
rejected: it has one clock port (`clk` feeds both `c1` and `c3`
through buffers — same domain), and editing `exceptions.def` would
invalidate all committed scenario goldens. A second domain needs a
second clock port, hence a new DEF.

## 1. `two_clock.def`

Six `dfxtp_1` FFs in two groups, each clocked through its own
`clkbuf_2` off its own input port, with a cross-domain path in each
direction:

```
VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN sta_clocks ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 20000 20000 ) ;
ROW ROW_0 unithd 0 0 N DO 43 BY 1 STEP 460 2720 ;
ROW ROW_1 unithd 0 2720 N DO 43 BY 1 STEP 460 2720 ;
ROW ROW_2 unithd 0 5440 N DO 43 BY 1 STEP 460 2720 ;
TRACKS X 170 DO 100 STEP 340 LAYER met1 ;
TRACKS Y 140 DO 100 STEP 340 LAYER met2 ;
COMPONENTS 8 ;
- u_clka sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 0 ) N ;
- u_clkb sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 2720 ) N ;
- u_fa0 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 9200 0 ) N ;
- u_fa1 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 13800 0 ) N ;
- u_fx sky130_fd_sc_hd__dfxtp_1 + PLACED ( 13800 2720 ) N ;
- u_fy sky130_fd_sc_hd__dfxtp_1 + PLACED ( 9200 2720 ) N ;
- u_fb0 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 9200 5440 ) N ;
- u_fb1 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 13800 5440 ) N ;
END COMPONENTS
PINS 5 ;
- clk1 + NET clk1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 0 ) N ;
- clk2 + NET clk2 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 2720 ) N ;
- d_in + NET din + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 4080 ) N ;
- ya + NET ya + DIRECTION OUTPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 5440 ) N ;
- yb + NET yb + DIRECTION OUTPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 6800 ) N ;
END PINS
NETS 11 ;
- clk1 ( PIN clk1 ) ( u_clka A ) + USE SIGNAL ;
- ca ( u_clka X ) ( u_fa0 CLK ) ( u_fa1 CLK ) ( u_fy CLK ) + USE SIGNAL ;
- clk2 ( PIN clk2 ) ( u_clkb A ) + USE SIGNAL ;
- cb ( u_clkb X ) ( u_fb0 CLK ) ( u_fb1 CLK ) ( u_fx CLK ) + USE SIGNAL ;
- din ( PIN d_in ) ( u_fa0 D ) ( u_fb0 D ) + USE SIGNAL ;
- qa0 ( u_fa0 Q ) ( u_fa1 D ) ( u_fx D ) + USE SIGNAL ;
- qb0 ( u_fb0 Q ) ( u_fb1 D ) ( u_fy D ) + USE SIGNAL ;
- ya ( u_fa1 Q ) ( PIN ya ) + USE SIGNAL ;
- yb ( u_fb1 Q ) ( PIN yb ) + USE SIGNAL ;
- qx ( u_fx Q ) + USE SIGNAL ;
- qy ( u_fy Q ) + USE SIGNAL ;
END NETS
END DESIGN
```

Topology and what it exercises:

| path | launch domain | capture domain | shape |
|---|---|---|---|
| `d_in -> u_fa0/D` | cka (port anchored to cka) | cka | external launch, same domain |
| `u_fa0/Q -> u_fa1/D` | cka | cka | FF->FF same domain |
| `u_fa0/Q -> u_fx/D` | **cka** | **ckb** | cross A->B |
| `d_in -> u_fb0/D` | cka | ckb | external launch into B |
| `u_fb0/Q -> u_fb1/D` | ckb | ckb | FF->FF same domain |
| `u_fb0/Q -> u_fy/D` | **ckb** | **cka** | cross B->A |
| `u_fa1/Q -> ya` | cka | output check on cka | output port, domain A |
| `u_fb1/Q -> yb` | ckb | output check on ckb | output port, domain B |

One scenario file exercises both cross directions at once — no
period-swap variant needed. `qx`/`qy` are stub nets so every DEF pin
is connected. `d_in` is anchored to `cka` in every file, so
`u_fb0/D` is a third pairing shape (external launch into domain B).

## 2. Shared SDC preamble

Every `t2_*.sdc` is self-contained (same convention as
`sta_exceptions`). Preamble:

```tcl
create_clock -name cka -period <Pa> [get_ports clk1]
create_clock -name ckb -period <Pb> [get_ports clk2]
set_input_delay -min 0.1 -clock cka [get_ports d_in]
set_input_delay -max 0.7 -clock cka [get_ports d_in]
set_output_delay -min 0.1 -clock cka [get_ports ya]
set_output_delay -max 0.7 -clock cka [get_ports ya]
set_output_delay -min 0.1 -clock ckb [get_ports yb]
set_output_delay -max 0.7 -clock ckb [get_ports yb]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports ya]
set_load 0.05 [get_ports yb]
```

## 3. Scenario files

| file | periods / extra lines | question it answers |
|---|---|---|
| `t2_int.sdc` | Pa=2.0, Pb=1.0 | baseline: are cross-domain paths timed at all; integer-ratio capture edges; the grus-eda break point |
| `t2_frac.sdc` | Pa=2.0, Pb=1.4 | non-integer ratio: which launch/capture edge pairs win; search horizon |
| `t2_unc.sdc` | Pa=2.0, Pb=1.4 + `set_clock_uncertainty -setup 0.05 [get_clocks cka]`, `-setup 0.20 [get_clocks ckb]`, `-hold 0.10 [get_clocks cka]` | per-clock uncertainty: same-domain vs cross-domain composition (max? sum? capture-side?) |
| `t2_uncx.sdc` | `t2_unc` + `set_clock_uncertainty -setup 0.30 -from [get_clocks cka] -to [get_clocks ckb]` | inter-clock uncertainty override on the A->B cross paths |
| `t2_lat.sdc` | Pa=2.0, Pb=1.4 + `set_clock_latency 0.6 [get_clocks cka]`, `set_clock_latency 0.2 [get_clocks ckb]`, `set_clock_latency -source 0.1 [get_clocks cka]` | per-clock latency: launch uses launch clock's, capture uses capture clock's |
| `t2_md.sdc` | `t2_int` + `set_max_delay 1.0 -from [get_cells u_fa0] -to [get_cells u_fx]` and `set_min_delay 0.5 -from [get_cells u_fb0] -to [get_cells u_fy]` | the §5 question: delay-check capture anchor on a second-domain endpoint, both directions |
| `t2_mdl.sdc` | `t2_md` + `set_clock_latency 0.6 [get_clocks cka]`, `0.2 [get_clocks ckb]`, distinct per-clock setup/hold uncertainties | which domain's latency/uncertainty the delay-check anchor picks up (t2_md has both at 0) |
| `t2_mdo.sdc` | `t2_int` + `set_max_delay 0.8 -to [get_ports yb]` and `set_min_delay 0.2 -to [get_ports yb]` | output-port delay check anchored to `ckb` |
| `t2_prop.sdc` | `t2_int` + `set_propagated_clock [all_clocks]` | per-domain propagation |
| `t2_prop1.sdc` | `t2_int` + `set_propagated_clock [get_clocks cka]` | mixed ideal/propagated domains |
| `t2_grp.sdc` | `t2_int` + `set_clock_groups -asynchronous -group [get_clocks cka] -group [get_clocks ckb]` | cross paths dropped (expected); informs the later ticket |
| `t2_mcp.sdc` | `t2_int` + `set_multicycle_path -setup 2 -from [get_cells u_fa0] -to [get_cells u_fx]` and `-hold 1` variant | cross-domain MCP: which period each side's multiplier applies to (stretch — cheap) |

Exact command lines may be adjusted in implementation if OpenSTA
rejects a form (record the rejection — that is itself a finding).

## 4. `probe.sh`

Mirror `tests/designs/sta_exceptions/generate_ref.sh`: same container
self-invocation (`ghcr.io/librelane/librelane:3.0.16`, `PDK_ROOT`
override, `~/.ciel` default), same `collect_endpoints_json` /
`count_violations` procs, one `openroad` process per scenario (read_sdc
accumulates). Two outputs per scenario:

- `ref_<name>.json` — wns/tns/violations + per-endpoint max/min
  arr/req/slack/slew (the existing JSON shape), consolidated into
  `two_clock.probe.json` and the temporaries deleted.
- `probe_<name>.txt` — a second `openroad` invocation per scenario
  whose stdout is captured: `report_checks -path_delay max -format
  full -endpoint_path_count 1 -group_path_count 100000`, the same for
  `min`, plus `report_checks -unconstrained` (proves which paths are
  untimed). The full-format report prints the launch/capture clock
  name and edge times per path — the pairing evidence the endpoint
  table alone does not carry.

Both artifact kinds are small (8 endpoints, ~11 scenarios) and
committed — they are the `results.md` evidence trail.

## 5. `tests/designs/README.md`

New catalog entry next to `sta_edge`/`sta_exceptions`: hand-written
fixture authored in this repository, `sky130_fd_sc_hd` cells only,
used for the multi-clock SDC probe (`two_clock.probe.json` +
`probe_*.txt` generated by `probe.sh`, committed, < 1 MB).
