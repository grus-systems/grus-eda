# Probes — new `sta_exceptions` scenarios

Every new file is self-contained: the current shared preamble (the
`base.sdc` body including the `sel` lines) plus the command lines below.
`pmo_*` files additionally carry `set_propagated_clock [get_clocks core_clk]`
before the delay command — the `pmd_*`/`pm2_*` shape. Two files change one
preamble *value* locally (marked "preamble edit"); nothing else in the
directory is modified.

## 1. Max side — N sweep on `-to [get_ports y]`

| file | extra lines | measures |
|---|---|---|
| `mdo_max05.sdc` | `set_max_delay 0.5 -to [get_ports y]` | req slope/intercept at N = 0.5 |
| `mdo_max1.sdc` | `set_max_delay 1.0 -to [get_ports y]` | N = 1.0 (anchor point for `pmo_max1`) |
| `mdo_max2.sdc` | `set_max_delay 2.0 -to [get_ports y]` | N = 2.0 |
| `mdo_max3.sdc` | `set_max_delay 3.0 -to [get_ports y]` | loose-replace probe: delay req (~2.25) is *looser* than the ordinary output req (1.95); if OpenSTA kept the ordinary check when the delay check loses, the golden would show 1.95 |

Ad hoc `md_toy*` measurements say `req = N - output_delay_max - setup_unc`
(edge reference 0); the committed sweep re-verifies on the golden — the
reported `y.max_req_ns` should be `N - 0.75` across all four points, and
`max_arr_ns`/`max_slew_ns` must equal `base`'s `y` values (candidate keeps
its ordinary arrival).

## 2. Min side — sweep plus constant attribution

| file | extra lines | measures |
|---|---|---|
| `mdo_min0.sdc` | `set_min_delay 0.0 -to [get_ports y]` | raw additive constant (req at N = 0) |
| `mdo_min05.sdc` | `set_min_delay 0.5 -to [get_ports y]` | slope on N |
| `mdo_min1.sdc` | `set_min_delay 1.0 -to [get_ports y]` | slope on N |
| `mdo_mindl.sdc` | preamble edit: `set_output_delay -min 0.4` (was 0.1); `set_min_delay 0.0 -to [get_ports y]` | isolates a `- output_delay_min` term |
| `mdo_minhu.sdc` | preamble edit: `set_clock_uncertainty -hold 0.35` (was 0.1); `set_min_delay 0.0 -to [get_ports y]` | isolates a `+ hold_uncertainty` term |

Why the two attribution probes: the preamble has `output_delay_min = 0.1`
and `hold_unc = 0.1`, so candidate formulas cancel — `req = N` and
`req = N - output_delay_min + hold_unc` both read 0.0 at N = 0. Decision
table for `mdo_min0`'s `y.min_req_ns` vs the variants:

| formula | `mdo_min0` | `mdo_mindl` (delay_min 0.4) | `mdo_minhu` (hold unc 0.35) |
|---|---|---|---|
| `N` | 0.0 | 0.0 | 0.0 |
| `N - delay_min + hold_unc` | 0.0 | -0.3 | +0.25 |
| `N - delay_min` | -0.1 | -0.4 | -0.1 |
| `N + hold_unc` | +0.1 | +0.1 | +0.35 |

Any value involving `latency` (0.7 ideal) is ruled out by the `~= N` ad-hoc
note; if `mdo_min0` lands outside this table, record the value in
`docs/results.md` and re-probe rather than guess.

## 3. Selector and `-from` coverage

| file | extra lines | measures |
|---|---|---|
| `mdo_all.sdc` | `set_max_delay 1.0 -to [all_outputs]` | `all_outputs` selector resolves to `{y}` |
| `mdo_from1.sdc` | `set_max_delay 1.0 -from [get_cells u_ff1] -to [get_ports y]` | positive `-from`: `u_ff1/Q` is `y`'s only launch |
| `mdo_from0.sdc` | `set_max_delay 1.0 -from [get_cells u_ff0] -to [get_ports y]` | negative `-from`: `u_ff0`'s fanout ends at `u_ff1/D`; `y` must keep `base`'s ordinary output-check numbers |
| `mdo_from2.sdc` | `set_max_delay 1.0 -from [get_cells {u_ff0 u_ff1}] -to [get_ports y]` | mixed two-object `-from`: covers `y` via `u_ff1` while listing a non-launching cell — the spec's `{u_ff0 u_ff2}` corrected (see `overview.md`; that form would be a second pure negative) |

`mdo_from0` is the load-bearing negative: the golden should equal `base`'s
`y` entry exactly — the delay rule is not in `u_ff1`'s signature class, so
no delay check may appear at `y`. If the golden instead shows a delay
check, OpenSTA's `-from` coverage at output ports is broader than the
launch-source model and the plan needs revisiting before implementation.

## 4. Precedence on the port side

| file | extra lines | measures |
|---|---|---|
| `mdo_fp.sdc` | `set_false_path -to [get_ports y]`<br>`set_max_delay 1.0 -to [get_ports y]` | false_path > delay: `y` disappears from `endpoints` entirely (the delay check must not rescue it) — mirrors `md_fp` |
| `mdo_mc.sdc` | `set_multicycle_path -setup 2 -to [get_ports y]`<br>`set_max_delay 1.0 -to [get_ports y]` | delay > MCP: `y.max_req_ns` must be the delay req, not the MCP-shifted output req (~3.95) |
| `mdo_mc_rev.sdc` | `set_max_delay 1.0 -to [get_ports y]`<br>`set_multicycle_path -setup 2 -to [get_ports y]` | order independence — mirrors `md_mc_rev` |
| `mdo_min_mch.sdc` | `set_multicycle_path -hold 1 -to [get_ports y]`<br>`set_min_delay 0.5 -to [get_ports y]` | min-side delay check ignores the MCP shift — mirrors `md_min_mcp_h1` |
| `mdo_min_mcs.sdc` | `set_multicycle_path -setup 2 -to [get_ports y]`<br>`set_min_delay 0.5 -to [get_ports y]` | setup-MCP must not feed the min delay check — mirrors `md_min_mcp_s2` |
| `mdo_both.sdc` | `set_max_delay 1.0 -to [get_ports y]`<br>`set_min_delay 0.5 -to [get_ports y]` | max/min coexist per side — mirrors `md_both`; the uncovered-side question is moot here but each side's req is checked independently |

## 5. Propagated clock — the anchor question

| file | extra lines | measures |
|---|---|---|
| `pmo_max1.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_max_delay 1.0 -to [get_ports y]` | does the max req stay edge-0 (`1.0 - 0.7 - 0.05 = 0.25`), anchor to `-source` latency (0.35), or to the propagated `clk` port-pin arrival? |
| `pmo_max05.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_max_delay 0.5 -to [get_ports y]` | second N point — separates an anchor offset from a slope error |
| `pmo_min0.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_min_delay 0.0 -to [get_ports y]` | min-side constant under propagation; compare against `mdo_min0` — a nonzero delta means a latency term |
| `pmo_min05.sdc` | `set_propagated_clock [get_clocks core_clk]`<br>`set_min_delay 0.5 -to [get_ports y]` | min-side N linearity under propagation |

Fixture limitation: `-source` latency (0.1) and the propagated `clk`
port-pin arrival (~0.1 + near-zero wire) are within tolerance on this DEF,
so option (b) vs (c) cannot be split — record whichever the numbers support
as "latency-or-port-pin anchor" rather than claiming a specific anchor.
Source-vs-network is already distinguished by `latency_ns` (`sdc.rs:330`).

## 6. `generate_ref.sh`

Append the 23 names to `SCENARIOS` (line 133), after `pm2_both`, keeping
this order:

```
mdo_max05 mdo_max1 mdo_max2 mdo_max3 mdo_min0 mdo_min05 mdo_min1
mdo_mindl mdo_minhu mdo_all mdo_from0 mdo_from1 mdo_from2
mdo_fp mdo_mc mdo_mc_rev mdo_min_mch mdo_min_mcs mdo_both
pmo_max05 pmo_max1 pmo_min0 pmo_min05
```

No other script change. `exceptions.reference.json` grows from 72 to 95
entries; the existing 72 entries must come back byte-identical (no SDC, DEF
or script-input change) — `git diff` must show only the new keys.

## 7. Golden inspection checklist (before writing code)

1. `mdo_max*` `y.max_req_ns` = `N - 0.75`? If not, re-derive from the three
   N points.
2. `mdo_min0` `y.min_req_ns` = the raw constant; attribute via
   `mdo_mindl`/`mdo_minhu` per the decision table.
3. `pmo_max1`/`pmo_min0` `y.*_req_ns` vs the ideal values → the anchor.
4. `mdo_from0` `y` entry == `base` `y` entry (all four `*_req`/`*_slack`
   fields); `mdo_from1`/`mdo_from2`/`mdo_all` show the delay req.
5. `mdo_fp`: no `y` endpoint; `mdo_mc*`/`mdo_min_mc*`: delay req wins.
6. `mdo_max3`: req ~2.25 (delay check reported even though looser) — if the
   golden shows 1.95, OpenSTA did *not* replace on a losing delay check
   and composition must special-case that.
