# e5-cts-ref — `<d>.cts.reference.json` schema and sources

Flat-with-subobjects JSON, emitted by the script's python3 step from
the `runs/cts/` transcript + census files. Every field names its
source; the transcript parser is the only hand-written logic and it
aborts on an unmatched line rather than defaulting.

```json
{
  "design": "gcd",
  "clock": "core_clk",
  "cts_root_buf": "sky130_fd_sc_hd__clkbuf_16",
  "cts_buf_list": ["sky130_fd_sc_hd__clkbuf_8",
                   "sky130_fd_sc_hd__clkbuf_4",
                   "sky130_fd_sc_hd__clkbuf_2"],
  "cts_wire_unit": 20,
  "inserted_instances": 0,
  "tree_buffers": {"sky130_fd_sc_hd__clkbuf_16": 0},
  "dummy_loads": {"sky130_fd_sc_hd__clkbuf_4": 0},
  "clock_subnets": 0,
  "sinks": 0,
  "tree_depth": 0,
  "avg_sink_wirelength_um": 0.0,
  "clock_net_wirelength_um": 0.0,
  "clock_net_wirelength_basis": "report_cts|hpwl",
  "skew_setup_ns": 0.0,
  "skew_hold_ns": 0.0,
  "latency_max_ns": 0.0,
  "latency_min_ns": 0.0,
  "def_components": 0,
  "def_signal_nets": 0
}
```

## Per-field source

| field | source |
|---|---|
| `design`, `clock` | script arg; `report_clock_skew` table's clock column (assert equal) |
| `cts_root_buf`, `cts_buf_list`, `cts_wire_unit` | `report_cts_config` block in `cts.log` |
| `inserted_instances` | odb census — insts absent from the pre-CTS snapshot |
| `tree_buffers` | per-master histogram of inserted insts minus `dummy_loads`, cross-checked against `report_cts` buffer counts |
| `dummy_loads` | TritonCTS log/`report_cts` dummy count per cell type; DEF cross-check: inserted clkbuf whose output net has ≤ 1 ITerm |
| `clock_subnets` | `report_cts` |
| `sinks` | `report_cts` (cross-check: `all_registers` `is_clock` pins — 35 on gcd) |
| `tree_depth` | `report_cts` max-level line |
| `avg_sink_wirelength_um` | `report_cts`/TritonCTS log (P-CTS: 120.66) |
| `clock_net_wirelength_um` | `report_cts` total wirelength if the table carries it; else per-clock-net HPWL via `getTermBBox` in the census Tcl — `*_basis` records which |
| `skew_setup_ns`, `skew_hold_ns` | `report_clock_skew -setup/-hold` table max-`Skew` column; `worst_clock_skew` line is the cross-check |
| `latency_max_ns`, `latency_min_ns` | `report_clock_skew` table's per-pin `Latency` column, max and min (insertion-delay anchor) |
| `def_components`, `def_signal_nets` | census scalars (sanity: `inserted_instances` + placed components) |

If `worst_clock_skew` returns the value in Tcl (probe first —
`set x [worst_clock_skew -setup]`), prefer it for `skew_*` and keep the
report table as cross-check; record which surface won in
`docs/results.md`.

## P-CTS scoping numbers to confirm or correct

The `docs/results.md` entry carries a measured-vs-scoped table:

| quantity | P-CTS (2026-11-22) | measured |
|---|---|---|
| tree buffers | 5 × `clkbuf_16` | |
| dummy loads | 3 (`clkbuf_4`×1, `clkbuf_8`×2) | |
| sinks | 35 | |
| tree depth | 2 | |
| avg sink wirelength | 120.66 µm | |
| clock-pin latency | ≈ 0.31 ns | |
| skew setup/hold | ≈ 0 ps | |

The scoping run used identical flags, so any divergence is a finding —
name the cause, do not silently overwrite.

## `docs/results.md` entry (`e5-cts-ref`, dated)

- Script path and its steps (A: CTS+reports+census+`write_def`; B:
  python assembly; C: propagated STA golden).
- The measured-vs-scoped table above, confirmed or corrected.
- Artifact sizes in bytes and SHA-256 first-16 for every new artifact
  (committed and gitignored alike).
- Which surface supplied `skew_*`/wirelength/`dummy_loads` (report vs
  return value vs census).
- Host + invocation (`generate_cts_ref.sh gcd [picorv32]` inside the
  pinned image), number of runs, `git diff` state on rerun.
- picorv32: included numbers or the explicit deferral reason.
