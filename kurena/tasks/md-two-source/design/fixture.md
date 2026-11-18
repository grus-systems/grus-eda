# Fixture changes — `tests/designs/sta_exceptions/`

## 1. `exceptions.def`

Three structural edits, nothing else. Resulting blocks:

```
COMPONENTS 6 ;
- u_clkb sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 0 ) N ;
- u_clkb2 sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 2720 ) N ;
- u_ff0 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 9200 0 ) N ;
- u_ff1 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 13800 0 ) N ;
- u_ff2 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 18400 0 ) N ;
- u_mux sky130_fd_sc_hd__mux2_1 + PLACED ( 11500 2720 ) N ;
END COMPONENTS
```

```
PINS 4 ;
- clk + NET clk + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 0 ) N ;
- d_in + NET din + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 2720 ) N ;
- sel + NET sel + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 4080 ) N ;
- y + NET y + DIRECTION OUTPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 5440 ) N ;
END PINS
```

```
NETS 9 ;
- clk ( PIN clk ) ( u_clkb A ) ( u_clkb2 A ) + USE SIGNAL ;
- c1 ( u_clkb X ) ( u_ff0 CLK ) ( u_ff1 CLK ) + USE SIGNAL ;
- c2 ( u_clkb2 X ) ( u_ff2 CLK ) + USE SIGNAL ;
- din ( PIN d_in ) ( u_ff0 D ) ( u_ff2 D ) + USE SIGNAL ;
- sel ( PIN sel ) ( u_mux S ) + USE SIGNAL ;
- d0 ( u_ff0 Q ) ( u_mux A0 ) + USE SIGNAL ;
- d2 ( u_ff2 Q ) ( u_mux A1 ) + USE SIGNAL ;
- d1 ( u_mux X ) ( u_ff1 D ) + USE SIGNAL ;
- y ( u_ff1 Q ) ( PIN y ) + USE SIGNAL ;
END NETS
```

Notes:

- `u_ff0 -> A0`, `u_ff2 -> A1` deliberately: A1's larger input capacitance
  (0.001884 pF vs A0's 0.001558 pF) gives the `u_ff2` path the slightly
  larger pure data delay, which is the delay-domain winner candidate while
  `u_ff2` itself launches earlier (its `u_clkb2` sees one load vs `u_clkb`'s
  two).
- Coordinates are wire-delay-irrelevant (zero parasitics in this flow) but
  must stay inside the 20000x20000 die on the site grid; the row-1
  placements above satisfy that. `sel` at (0, 4080) sits on the left edge
  between `d_in` and `y` without overlapping their ( -70 -70 ) ( 70 70 )
  shapes.
- `din` keeps feeding `u_ff0/D` and `u_ff2/D` — those endpoints are
  unchanged.

## 2. `sel` constraints in the shared preamble

Every `.sdc` file in the directory is a self-contained file whose first 12
lines are the shared `base.sdc` preamble (plus per-file constraint lines;
`base_prop.sdc`/`pmd_*`/`pm2_*` additionally carry
`set_propagated_clock`). Insert, in all of them — `base.sdc`,
`base_prop.sdc`, all existing scenario files and all new ones:

- after `set_input_delay -max 0.7 -clock core_clk [get_ports d_in]`:

  ```
  set_input_delay -min 0.1 -clock core_clk [get_ports sel]
  set_input_delay -max 0.3 -clock core_clk [get_ports sel]
  ```

- after `set_input_transition 0.1 [get_ports d_in]`:

  ```
  set_input_transition 0.1 [get_ports sel]
  ```

The insertion is mechanical (`sed` on the two anchor lines — both are
present in all 67 files; `mhu_base.sdc`'s differing uncertainty line is not
an anchor).

Why these values: `sel` must be a *live but losing* competitor. Estimated
`sel -> u_mux/S -> X -> u_ff1/D` arrival is ~`0.7 + d + 0.1..0.2`. With
`-min 0.1` its hold slack stays clearly positive (~+0.1 ns) so it does not
dominate min-side reports and the `m2_min2`/`pm2_min2` delay checks
(~ -0.3 ns slack) still win; `-min 0.0` like `d_in` would leave the slack
~0 and risk `sel` masking the min probes. With `-max 0.3` its setup slack
(~1.4 ns ideal / ~0.9 ns propagated) loses to every `set_max_delay` check
(~0.3–0.5 ns) and keeps `u_ff1/D`'s `base` max report on an FF path. These
are estimates — implementation verifies the winner identities in the
regenerated golden and adjusts only if a new scenario's delay check is not
the reported path (see `steps.md`).

Leaving `sel` unconstrained in legacy files was rejected: its ~0 launch
would then win every min-side report at `u_ff1/D` and nullify the existing
`md_min*`/`mhu_min*`/`mc_hold*` probes.

## 3. New scenario files

Each file is the updated `base.sdc` preamble (including the `sel` lines)
plus the lines below. `pm2_*` files additionally include
`set_propagated_clock [get_clocks core_clk]` before the delay command, same
as `pmd_*.sdc`.

| file | extra lines |
|---|---|
| `m2_max0.sdc` | `set_max_delay 1.0 -from [get_cells u_ff0] -to [get_cells u_ff1]` |
| `m2_max2.sdc` | `set_max_delay 1.0 -from [get_cells u_ff2] -to [get_cells u_ff1]` |
| `m2_both.sdc` | `set_max_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_cells u_ff1]` |
| `m2_min2.sdc` | `set_min_delay 0.5 -from [get_cells u_ff2] -to [get_cells u_ff1]` |
| `pm2_prop.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_max_delay 1.0 -from [get_cells u_ff0] -to [get_cells u_ff1]` |
| `pm2_min2.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_min_delay 0.5 -from [get_cells u_ff2] -to [get_cells u_ff1]` |
| `pm2_both.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_max_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_cells u_ff1]` |

`m2_both`/`pm2_both` use a **single** `-from` command covering both cells —
that is what puts `u_ff0` and `u_ff2` in one signature class so the delay
domain merges their launches (see `overview.md`). Two separate commands
would split them into two classes and never merge; `pmd_multi` already
covers the two-command propagated shape for free after the DEF change.

## 4. `generate_ref.sh`

Append to `SCENARIOS` (line 133), keeping the existing order and adding the
new names at the end:

```
... md_early md_datapath m2_max0 m2_max2 m2_both m2_min2 pm2_prop pm2_min2 pm2_both
```

No other script change — scenario count grows from 65 to 72 and
`exceptions.reference.json` embeds all of them.
