# Fixture changes — `tests/designs/sta_exceptions/`

No DEF or preamble changes: `exceptions.def` is untouched and every `pcr_*`
file is the full `base_prop.sdc` text plus the two derate lines plus its own
commands. The derate lines go after `set_propagated_clock` (order is
irrelevant, but keep them grouped before the exception/delay command for
readability).

## 1. Shared `pcr_` spread block

```
set_timing_derate -early 0.80 -clock -cell_delay
set_timing_derate -late 1.20 -clock -cell_delay
```

Both flag/value spellings are already exercised inside LibreLane OpenSTA by
`sta_edge/timing_derate/propagated.sdc`, and our parser covers
`-early`/`-late`, `-clock`, `-cell_delay` (`sdc.rs` `set_timing_derate`).
`-net_delay` is deliberately absent — this flow passes no parasitics, so net
delays are identically zero and the selector would measure nothing.

Effect on this fixture: spread `S(pin) = 0.40 * d(cell)` at each clock-net
driver — `u_clkb/X` (~`0.4 * 0.070` ~ 28 ps, from the `0.169918` `c1` early
arrival in `docs/results.md`), `u_clkb2/X`, `u_clkb3/X` — and zero at the
`clk` root port (source latency is not a cell/net delay).

## 2. Scenario files

`pcr_clk.sdc` — `base_prop.sdc` + spread block only.

`pcr_clkfp.sdc` — `base_prop.sdc` + spread block +

```
set_false_path -from [get_cells u_ff2] -to [get_cells u_ff1]
```

Why the false path: `u_ff1/D`'s max report is otherwise won by the `u_ff2`
path (`0.796505` vs `u_ff0`'s `0.742708` in `base_prop`), whose launch clock
path (`clk -> u_clkb2 -> c2 -> u_clkb3 -> c3`) shares only the `clk` root
with the capture path — spread 0 there, so nothing about the credit is
visible in the reported slack. Removing it makes the `u_ff0` path (launch
`c1` = capture `c1`) the winner. `sel` is left alone: its max arrival
(~`0.3` + `sel -> u_mux/S -> X`) stays well below `u_ff0`'s. Min side at
`u_ff1/D` keeps `sel` as winner — both engines credit it 0, consistent.

`pcr_max.sdc` — `base_prop.sdc` + spread block +

```
set_max_delay 1.0 -from [get_cells u_ff0] -to [get_cells u_ff1]
```

`pmd_basic`/`pm2_prop` shape; the covered `u_ff0` delay check stays the
reported max candidate (un-credited slack ~0.24 vs ~1.2 clock checks).

`pcr_min.sdc` — `base_prop.sdc` + spread block +

```
set_min_delay 0.5 -from [get_cells u_ff0] -to [get_cells u_ff1]
```

`pmd_min` shape; covered check wins the min report (`-0.155849` un-spread).

`pcr_off.sdc` — `base_prop.sdc` + spread block +

```
set_max_delay 1.0 -from [get_cells u_ff2] -to [get_cells u_ff1]
```

Control: covered launch on `c3` shares only the `clk` root port with `c1`.

## 3. `generate_ref.sh`

Append to `SCENARIOS`, after `pmp_alls`:

```
pcr_clk pcr_clkfp pcr_max pcr_min pcr_off
```

No other script change (106 -> 111 scenarios).

## 4. Predicted golden behaviour (verify before running the test)

Predictions use `S` ~ `0.4 * d(u_clkb)` ~ 28 ps and the derate shifts
(`early` paths -0.2*d, `late` +0.2*d on `c1`; `u_ff2`'s `c3` launch shifts
+0.2*(d2+d3) ~ +20-30 ps more than `u_ff0`'s):

- `pcr_clk` vs `base_prop`: `u_ff1/D` `max_req` drops ~`-0.2*d` (~-14 ps),
  `min_req` rises `+0.2*d` — the **opposite-direction** pair is the recorded
  proof that early/late spread exists (a single-corner derate would move
  both the same way). `u_ff1/D` max winner remains `u_ff2`-launched
  (arrival ~`0.7965 + 0.2*(d2+d3)`); that path's credit is ~0 (common point
  = `clk` root). Same opposite-direction pair at `u_ff0/D` (`c1`) and
  `u_ff2/D` (`c3`, shifts ~2x `c1`'s: two derated buffers on its capture
  path).
- `pcr_clkfp`: `u_ff1/D` `max_arr` ~ `0.742708 + 0.2*d` ~ `0.757`,
  `max_slack` = un-credited `~1.25` **plus `S` iff OpenSTA credits the
  ordinary check per-path** (expected: `~1.28`; ours reports `~1.25` ->
  red by ~`S`). `u_ff1/D` min winner stays `sel` (credit 0 both sides).
- `pcr_max`: `u_ff1/D` `max_req` ~ `1.013333 - 0.2*d (+ S if credited)` —
  i.e. ~`0.999` un-credited, ~`1.027` credited; `max_arr` ~`0.757`; slack
  ~`0.242` vs ~`0.270`.
- `pcr_min`: `u_ff1/D` `min_req` ~ `0.748340 + 0.2*d (- S if credited)` —
  ~`0.762` un-credited, ~`0.734` credited; `min_arr` ~`0.593 - 0.2*d` ~
  `0.579`; slack ~`-0.18` vs ~`-0.16`.
- `pcr_off`: `u_ff1/D` `max_arr` ~ `0.796505 + 0.2*(d2+d3)` ~ `0.82`; req as
  `pcr_max` un-credited (`~0.999`) **even if delay checks are credited** —
  the `c3`-launched path shares only the `clk` root (S = 0 there). Slack
  ~`0.18`, still the min-slack winner.
- `y`/`y2` rows shift only via the derated FF launch arrivals; port
  launches (`d_in`, `sel`, `clk`) are not derated in either engine.
- Non-`pcr_*` rows: **byte-identical** — scenarios run in independent
  OpenSTA processes; a diff on existing keys means nondeterminism, stop and
  record it.

## 5. Margin contingency

If the regenerated `pcr_clk` shows the opposite-direction req shift < ~10
ps (thin discriminator), widen the derates to `0.70`/`1.30` in all five
files and regen — do not thin the claim or hand-adjust numbers.
