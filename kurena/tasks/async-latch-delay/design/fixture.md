# async-latch-delay — Fixture & golden layout

Both fixtures already carry the DEF, baseline SDCs, `generate_ref.sh`, and
`*.reference.json`. This task adds delay-check scenario SDCs (the probe
questions committed as files) and extends each `generate_ref.sh` with a
scenario list. The committed set is verdict-independent: an "ignored"
verdict produces a golden section byte-identical to the baseline —
permanent, reproducible evidence — so the same scenarios ship either way.

## 1. New SDC scenarios

Each file = the baseline SDC verbatim plus the delay commands. Basenames
become the golden section keys, matching the `sta_edge` descriptive naming
(`ideal.sdc`, `propagated.sdc`, `viol.sdc`).

### `tests/designs/sta_edge/async_reset/` (base `ideal.sdc` unless noted)

| file | added commands | probes |
|---|---|---|
| `delay_max.sdc` | `set_max_delay 1.0 -to [get_pins u_ff0/RESET_B]` | A1 |
| `delay_min.sdc` | `set_min_delay 0.5 -to [get_pins u_ff0/RESET_B]` | A2 |
| `delay_from.sdc` | `set_max_delay 1.0 -from [get_ports rst_n] -to [get_pins u_ff0/RESET_B]` | A3 |
| `delay_cell.sdc` | `set_max_delay 1.0 -to [get_cells u_ff0]` | A4 |
| `delay_prop.sdc` | base `propagated.sdc` + `set_max_delay 1.0 -to [get_pins u_ff0/RESET_B]` | A6 |

### `tests/designs/sta_edge/latch_borrow/` (base `latch.sdc` unless noted)

| file | added commands | probes |
|---|---|---|
| `delay_max.sdc` | `set_max_delay 1.0 -to [get_pins u_lp/D]` | B1 |
| `delay_min.sdc` | `set_min_delay 0.5 -to [get_pins u_lp/D]` | B2 |
| `delay_ln.sdc` | `set_max_delay 1.5 -to [get_pins u_ln/D]` | B3 |
| `delay_from.sdc` | `set_max_delay 1.0 -from [get_ports p_lp] -to [get_pins u_lp/D]` | B4 |
| `delay_allregs.sdc` | `set_max_delay 1.0 -to [all_registers]` | B5 |
| `delay_prop.sdc` | base `prop.sdc` + `set_max_delay 1.0 -to [get_pins u_lp/D]` | B6 |

Negative-control probes (`-from [get_ports d_in]`, `-from [get_pins
u_ff0/CLK]`) stay ad-hoc: they assert *no* change, which the applied
scenarios already pin down; record their output in `results.md` without
committing goldens for them.

## 2. `generate_ref.sh` changes

Both scripts today call `run_sta` twice and merge `ideal`+`propagated`.
Extend each to a scenario list in the same style:

```bash
run_sta ideal.sdc ideal.json
run_sta propagated.sdc propagated.json
run_sta delay_max.sdc delay_max.json
# … one per scenario …
```

then merge each `delay_*.json` as a new top-level key alongside `ideal` /
`propagated` in `*.reference.json` (same `printf`/`cat` assembly, same
intermediate-file cleanup). For `delay_prop.sdc` emit the key `delay_prop`.

If OpenSTA errors on a scenario (errored verdict), that scenario produces
no endpoint JSON: emit a `{"error": "<verbatim STA-#### message>"}`
section for it instead — the `sta_exceptions` fixture already carries
error-record sections (`md_early`, `md_datapath`), so the schema has
precedent. `compare_sta_edge` is only pointed at sections with
`endpoints`; error sections exist for reproduction, not comparison.

## 3. Tests

One `compare_sta_edge` call per scenario section with an `endpoints`
array, all at `Tol::STRICT`, added next to the existing
`test_sta_edge_async_reset_*` (~`analysis.rs:7572`) and
`test_sta_edge_latch_borrow_*` (~`analysis.rs:7513`) tests:

- `test_sta_edge_async_reset_{delay_max,delay_min,delay_from,delay_cell,delay_prop}`
- `test_sta_edge_latch_borrow_{delay_max,delay_min,delay_ln,delay_from,delay_allregs,delay_prop}`

An "ignored" verdict leaves our report identical to baseline (the counted
skip keeps clock-check values), so the same tests pass in every branch —
they pin the evidence, not just the new code.

## 4. Coverage notes

- `delay_allregs.sdc` exercises latch-D *and* latch-async (`RESET_B`)
  endpoints plus FF-D in one run — if the async verdict on `async_reset`
  is "ignored" but `all_registers` moves the latch `RESET_B` rows, record
  the selector-dependence rather than treating the kinds as one.
- `u_iso/D` (baseline slack exactly 0 via the borrow clamp) is the
  sharpest discriminator for whether a delay check on latch-D keeps
  borrow semantics; it sits inside `delay_allregs` coverage.
