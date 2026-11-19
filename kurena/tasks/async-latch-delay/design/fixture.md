# async-latch-delay — Fixture & golden layout

Both fixtures carry the DEF, baseline SDCs, `generate_ref.sh`, and
`*.reference.json` — nothing else. The scenario SDCs below **must be
created** (the aborted run's copies are gone; this table is the spec). Each
file = its base SDC verbatim plus the delay command; basenames become the
golden section keys, matching the `sta_edge` descriptive naming.

## 1. Scenario SDCs (to create)

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

Negative controls (`-from [get_ports d_in]`, `-from [get_pins
u_ff0/CLK]`) stay ad-hoc — they assert *no* change, which the applied
scenarios already pin down; their output goes in `results.md` without
goldens (`probe.md` §Residual).

## 2. `generate_ref.sh` extension

Both scripts today call `run_sta` twice and merge two JSONs. Extend each:

```bash
run_sta ideal.sdc ideal.json          # latch_borrow: latch.sdc
run_sta propagated.sdc propagated.json # latch_borrow: prop.sdc
run_sta delay_max.sdc delay_max.json
# … one per scenario, same basename → same key …
```

then merge each `delay_*.json` as a new top-level key alongside `ideal` /
`propagated` in `*.reference.json`, reusing the existing `printf`/`cat`
assembly and intermediate-file cleanup. Two per-script details:

- `async_reset` keys: `delay_max`, `delay_min`, `delay_from`,
  `delay_cell`, `delay_prop`.
- `latch_borrow` keys: `delay_max`, `delay_min`, `delay_ln`,
  `delay_from`, `delay_allregs`, `delay_prop`. **Watch the emit key**:
  this script's `run_sta` writes `"latch_borrows"` where `async_reset`'s
  writes `"hold_violations"` — copy nothing across scripts; extend each
  in its own style.
- Scenario files sit in the same dir; the container `-w` and the
  `read_sdc …/$sdc_file` line already resolve any basename — no path
  changes needed.

If a scenario raises inside OpenSTA (none expected — the recorded probe
showed applied), emit `{"error": "<verbatim STA-####>"}` for its section
instead of endpoint JSON (`sta_exceptions` `md_early`/`md_datapath`
precedent) and point `compare_sta_edge` only at sections with
`endpoints`.

## 3. Tests

One `compare_sta_edge` call per scenario section, all `Tol::STRICT`, next
to the existing tests (`test_sta_edge_latch_borrow_*` ~line 7513,
`test_sta_edge_async_reset_*` ~line 7572 in `analysis.rs`). Signature
(existing, ~line 7136):
`compare_sta_edge(case, "case/case.def", sdc_file, None, golden_key,
Tol::STRICT)`.

- `test_sta_edge_async_reset_{delay_max,delay_min,delay_from,delay_cell,delay_prop}`
- `test_sta_edge_latch_borrow_{delay_max,delay_min,delay_ln,delay_from,delay_allregs,delay_prop}`

## 4. Coverage notes

- `delay_allregs` exercises latch-D *and* latch `RESET_B` endpoints plus
  FF-D in one run — `all_registers` resolves to check pins of every
  sequential cell, so selector coverage is measured at scale.
- `u_iso/D` (baseline slack exactly 0 via the borrow clamp) is the
  sharpest discriminator of the clamp formula's upper bound.
- `delay_cell` covers `u_ff0/D` (FF-D, long supported) and
  `u_ff0/RESET_B` in one rule — a mixed-kind golden in one section.
