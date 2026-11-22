# Fixture changes — `tests/designs/sta_exceptions/`

## 1. `exceptions.def`

Add one `mux2` component, one output pin, and one net; extend three
existing nets. Resulting blocks:

```
COMPONENTS 8 ;
- u_clkb sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 0 ) N ;
- u_clkb2 sky130_fd_sc_hd__clkbuf_2 + PLACED ( 4600 2720 ) N ;
- u_clkb3 sky130_fd_sc_hd__clkbuf_2 + PLACED ( 9200 2720 ) N ;
- u_ff0 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 9200 0 ) N ;
- u_ff1 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 13800 0 ) N ;
- u_ff2 sky130_fd_sc_hd__dfxtp_1 + PLACED ( 18400 0 ) N ;
- u_mux sky130_fd_sc_hd__mux2_1 + PLACED ( 11500 2720 ) N ;
- u_mux2 sky130_fd_sc_hd__mux2_1 + PLACED ( 11500 5440 ) N ;
END COMPONENTS
```

```
PINS 5 ;
- clk + NET clk + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 0 ) N ;
- d_in + NET din + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 2720 ) N ;
- sel + NET sel + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 4080 ) N ;
- y + NET y + DIRECTION OUTPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 5440 ) N ;
- y2 + NET y2 + DIRECTION OUTPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 6800 ) N ;
END PINS
```

```
NETS 11 ;
- clk ( PIN clk ) ( u_clkb A ) ( u_clkb2 A ) + USE SIGNAL ;
- c1 ( u_clkb X ) ( u_ff0 CLK ) ( u_ff1 CLK ) + USE SIGNAL ;
- c2 ( u_clkb2 X ) ( u_clkb3 A ) + USE SIGNAL ;
- c3 ( u_clkb3 X ) ( u_ff2 CLK ) + USE SIGNAL ;
- din ( PIN d_in ) ( u_ff0 D ) ( u_ff2 D ) + USE SIGNAL ;
- sel ( PIN sel ) ( u_mux S ) ( u_mux2 S ) + USE SIGNAL ;
- d0 ( u_ff0 Q ) ( u_mux A1 ) ( u_mux2 A1 ) + USE SIGNAL ;
- d2 ( u_ff2 Q ) ( u_mux A0 ) ( u_mux2 A0 ) + USE SIGNAL ;
- d1 ( u_mux X ) ( u_ff1 D ) + USE SIGNAL ;
- y ( u_ff1 Q ) ( PIN y ) + USE SIGNAL ;
- y2 ( u_mux2 X ) ( PIN y2 ) + USE SIGNAL ;
END NETS
```

Notes:

- `u_ff0 -> A1`, `u_ff2 -> A0` **deliberately mirrors `u_mux`'s post-swap
  orientation**: `A1`'s larger input capacitance gives the `u_ff0` path the
  larger pure delay (~13.5 ps measured at `u_mux`), while `u_ff2`'s launch
  lands ~69 ps later under `set_propagated_clock` (extra `u_clkb3` stage).
  That reproduces the diverging-winner geometry at a port — ideal winner
  `u_ff0`, propagated arrival winner `u_ff2` (see `overview.md`).
- `u_mux2/S` shares `sel` (per the goal): one more load on `sel`, and `sel`
  becomes a third — port-launched, uncovered — launch at `y2`. It is a
  losing competitor at the chosen delay values (`overview.md` masking
  analysis); a separate select was rejected as it adds an input port that
  every `.sdc` file would also have to constrain.
- `u_mux2` sits at (11500, 5440): row 2 is entirely empty, so no overlap;
  placements are wire-delay-irrelevant anyway (zero parasitics in this
  flow — the test passes `parasitics = None` and the golden runs no
  `estimate_parasitics`). `y2` at (0, 6800) is on the left edge, 1360 DBU
  above `y`, clear of all existing ( -70 -70 ) ( 70 70 ) pin shapes.

## 2. `y2` constraints in the shared preamble

Every `.sdc` file in the directory is self-contained: the first lines are
the shared `base.sdc` preamble (plus per-file exception lines; the
`base_prop`/`pmd_*`/`pm2_*`/`pmo_*`/`pml_*` files additionally carry
`set_propagated_clock`). Insert in **all 97 existing files** — every
`SCENARIOS` member plus `exc_diag.sdc` and `md_pin.sdc` — and in the
seven new files:

- after `set_output_delay -max 0.7 -clock core_clk [get_ports y]`:

  ```
  set_output_delay -min 0.1 -clock core_clk [get_ports y2]
  set_output_delay -max 0.7 -clock core_clk [get_ports y2]
  ```

- after `set_load 0.05 [get_ports y]`:

  ```
  set_load 0.05 [get_ports y2]
  ```

The insertion is mechanical (`sed` on the two anchor lines). Both anchors
must be verified present in all files first — `mdo_mindl.sdc` edits only
the `-min` output-delay value and `mdo_minhu.sdc`/`mhu_*.sdc` edit
uncertainty lines, so neither anchor moves; confirm with
`grep -cL`/`grep -c` before and after.

Why `set_load 0.05`: the goal mandates the `set_output_delay` pair (same
values as `y`, 0.1/0.7) so `y2` is a real checked endpoint. The load is a
deliberate extra: it gives `y2`'s reported slew the same external-load
term `y` carries instead of a zero-capacitance port slew. Values still
come entirely from the regen, so this is a symmetry choice, not an
accuracy fix — record it in `docs/results.md`.

## 3. New scenario files

Each file is the updated `base.sdc` preamble (including the `y2` lines)
plus the lines below. `pml_*` files additionally include
`set_propagated_clock [get_clocks core_clk]` before the delay command,
same as `pm2_*.sdc`/`pmo_*.sdc`.

| file | extra lines |
|---|---|
| `ml_both.sdc` | `set_max_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_ports y2]` |
| `ml_max0.sdc` | `set_max_delay 1.0 -from [get_cells u_ff0] -to [get_ports y2]` |
| `ml_max2.sdc` | `set_max_delay 1.0 -from [get_cells u_ff2] -to [get_ports y2]` |
| `ml_all.sdc` | `set_max_delay 1.0 -to [all_outputs]` |
| `ml_min.sdc` | `set_min_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_ports y2]` |
| `pml_both.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_max_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_ports y2]` |
| `pml_min.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_min_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_ports y2]` |

Rationale:

- `ml_both`/`pml_both` use a **single** `-from` command covering both
  cells — the signature-class form that actually merges the two launches
  in one eval run (see `overview.md`).
- `ml_max0`/`ml_max2` are the per-source probes the merged report
  decomposes against: `ml_both.y2` should equal the winner's `ml_max*`
  row.
- `set_min_delay 1.0` (not the `m2_*`/`md_min` 0.5): at a port the delay
  req is `N` itself and `sel`'s uncovered min check (~+0.2 slack) would
  out-report a +0.4-slack delay check — see the masking analysis in
  `overview.md`. `N = 1.0` puts the covered slack at ≈ −0.1 so the merged
  min rule is the reported candidate.
- `pml_min` carries the same two-source `-from` (merged min rule under
  skewed launches — `pm2_min2` covered only the single-source skewed
  case at an FF).

## 4. `generate_ref.sh`

Append the seven names to `SCENARIOS` (line ~133), keeping existing order
and adding at the end:

```
... pmo_max05 pmo_max1 pmo_min0 pmo_min05 ml_both ml_max0 ml_max2 ml_all ml_min pml_both pml_min
```

No other script change — scenario count grows from 95 to 102 and
`exceptions.reference.json` embeds all of them.
