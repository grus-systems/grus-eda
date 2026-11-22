# check-arcs — Fixture `tests/designs/sta_edge/min_pulse_width/`

New sta_edge case directory (goal: a new endpoint *kind*, not an SDC
exception — do not grow `sta_exceptions`). Hand-written DEF + three SDCs +
`generate_ref.sh` + `min_pulse_width.reference.json`, matching the
`hold_violation`/`propagated_clock` conventions (CI regenerates every
`tests/designs/sta_edge/*/generate_ref.sh` automatically — the new dir is
picked up for free).

## 1. `mpw.def`

A small clkbuf-heavy clock tree with an inverting branch and one latch —
the `propagated.def` shape stripped to what the checks need. Ports `clk`,
`din`, `y`; all cells `sky130_fd_sc_hd`.

| inst | cell | role |
|---|---|---|
| `u_dbuf` | `buf_2` | `din -> d1` data buffer |
| `u_clkb` | `clkbuf_2` | `clk -> c1` |
| `u_clkinv` | `clkinv_2` | `c1 -> c2` inverting branch |
| `u_clkb2` | `clkbuf_4` | `c1 -> c3` second buffered branch |
| `u_ff0` | `dfxtp_1` | `CLK` on `c1`, `D <- d1`, `Q -> y` |
| `u_ff1` | `dfxtp_1` | `CLK` on `c3`, `D <- d1` |
| `u_ff2` | `dfxtp_1` | `CLK` on `c2` (inverted), `D <- d1` |
| `u_dfr` | `dfrtn_1` | `CLK_N` on `c1`, `D`/`RESET_B <- d1` |
| `u_lt` | `dlxtn_1` | `GATE_N` on `c3`, `D <- d1` |

Nets: `clk = (PIN clk) (u_clkb A)`; `c1 = (u_clkb X) (u_ff0 CLK)
(u_dfr CLK_N) (u_clkinv A) (u_clkb2 A)`; `c2 = (u_clkinv Y) (u_ff2 CLK)`;
`c3 = (u_clkb2 X) (u_ff1 CLK) (u_lt GATE_N)`; `din = (PIN din) (u_dbuf A)`;
`d1 = (u_dbuf X) (u_ff0 D) (u_ff1 D) (u_ff2 D) (u_dfr D) (u_dfr RESET_B)
(u_lt D)`; `y = (u_ff0 Q) (PIN y)`. `RESET_B` is driven (from `d1`) so its
recovery/removal check is a live endpoint, not skip noise. ROW/TRACKS/DIEAREA
copied from `propagated.def` with placements spread along ROW_0.

Check-pin coverage: 3 arrival profiles (`c1` direct, `c2` inverted,
`c3` double-buffered), 3 cell shapes (`dfxtp`, `dfrtn` falling-edge,
`dlxtn` latch enable) → 5 check pins × 2 pulse rows per scenario.

## 2. SDC scenarios

Common preamble for all three: io delays and loads as in
`sta_edge/propagated_clock/edge.sdc` (`set_input_delay 0.5 -clock core_clk
[all_inputs -no_clocks]`, `set_output_delay 0.5`, `set_input_transition 0.15`,
`set_load 0.05`, `set_clock_uncertainty 0.1`). **No `-waveform`** — our
`create_clock` parser refuses unknown flags (`sdc.rs:1767-1768`), so duty
cycle is fixed at 50% and violations come from period + slew-driven
required width instead.

- **`ideal.sdc`** — `create_clock -period 5.0`, `set_clock_latency 1.0`,
  `set_clock_latency -source 0.2`, `set_clock_transition 0.1`. Ideal-clock
  baseline: pulse width 2.5 ns everywhere, required ≈ 0.29 ns (slew 0.1)
  → all rows MET, ordinary endpoints healthy.
- **`propagated.sdc`** — `ideal.sdc` + `set_propagated_clock`, plus the
  discriminators:
  ```
  set_timing_derate -early 0.80 -late 1.20 -clock -cell_delay
  set_timing_derate -early 0.90 -late 1.10 -cell_check
  ```
  Real skewed high/low widths per pin (rise/fall delay asymmetry +
  `clkinv` branch), a min/max arrival spread for the corner rule
  (probe Q4), and a `-cell_check` pair for the required-derate rule
  (probe Q6).
- **`viol.sdc`** — `ideal.sdc` preamble but `create_clock -period 0.6` and
  `set_clock_transition 0.3`. Pulse = 0.3 ns; required at slew 0.3 ≈
  0.56 ns (`dfxtp` interp: `0.1687 + (0.29/0.49)·(0.8333−0.1687)`) →
  slack ≈ −0.26, every row `VIOLATED` on both `high` and `low`.
  Ordinary endpoints also violate — expected; the golden records it.
  **Contingency:** after the first regen, if any row's |slack| < ~0.05 ns
  the margin is too thin — drop the period or raise `set_clock_transition`
  and regen (record the final values).

## 3. Golden JSON schema — `min_pulse_width.reference.json`

Same per-mode envelope as `hold_violation.reference.json` plus a
`pulse_width` array (the probe decides which command emits the rows):

```json
{
  "design": "sta_edge_min_pulse_width",
  "ideal": {
    "wns_max_ns": ..., "tns_max_ns": ..., "wns_min_ns": ..., "tns_min_ns": ...,
    "setup_violations": ..., "hold_violations": ...,
    "endpoints": [ {"pin": ..., "max_arr_ns": ..., ...} ],
    "pulse_width": [
      {"pin": "u_dfr/CLK_N", "edge": "high", "required_ns": ..., "actual_ns": ..., "slack_ns": ...},
      {"pin": "u_dfr/CLK_N", "edge": "low",  "required_ns": ..., "actual_ns": ..., "slack_ns": ...}
    ]
  },
  "propagated": { ... },
  "viol": { ... }
}
```

`edge` mirrors OpenSTA's `(high)`/`(low)` row suffix. `pulse_width` rows
sorted by `pin` then `edge` (`lsort -dictionary`), one row per check the
golden surface emits (probe Q2 fixes whether that is both pulses per pin).
If `find_timing_paths -path_delay min_pulse_width` yields PathEnds, the
generator extracts the fields it actually provides (probe Q1) and the
schema above is adjusted to match — `actual`/`required` may arrive as
`data_arrival_time`/`data_required_time` of a close/open edge pair, which
the generator converts to widths before emitting; record the conversion
in the script comment.

## 4. `generate_ref.sh`

Clone `propagated_clock/generate_ref.sh` (container self-re-exec, PDK mount,
`openroad -no_init -no_splash` heredoc). Loop the three scenarios
(`read_def`/`read_sdc` per scenario; collect endpoints JSON with the
existing `collect_endpoints_json` procs), and per scenario run the
measured pulse-width surface. If the surface is `report_check_types` text,
parse rows with `regexp` on `^\s*(\S+) \((high|low)\)\s+(\S+)\s+(\S+)\s+(\S+)\s+\((\w+)\)`
inside the same process — every committed value still comes from the
container run. Emit 6-digit decimals like the other generators. The
committed script must reproduce the committed golden byte-for-byte; run it
as written and `git diff` before committing (AGENTS.md §7.1).
