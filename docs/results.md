# Verification Results (`docs/results.md`)

Every comparison against the reference tools (`OpenROAD` and `OpenSTA` in `ghcr.io/librelane/librelane:3.0.16`) is recorded here, including runs that did **not** yet meet the target thresholds and the root cause identified.

## Summary of Stage Targets

| Stage | Reference Tool | Target ("Done When") | Status |
|---|---|---|---|
| **E0** (Readers) | OpenROAD / OpenSTA | Cell, net, pin, and row counts match exactly on every test design | Pending |
| **E1** (Congestion) | OpenROAD `global_route` | Overflow ranks the test variants in the same order | Pending |
| **E2** (Placement) | OpenROAD `global_placement` | OpenROAD's legalizer accepts the result; HPWL within 5% | Pending |
| **E3** (STA) | OpenSTA (`sta`) | Worst slack and every endpoint's arrival within 1% or 5 ps | Pending |

---

## Verification Log

### 2026-11-09 — E0.3 — `sky130_fd_sc_hd__tt_025C_1v80.lib` (Liberty Reader & Binary Cache)
- **Reference Command**: `sta -no_init -no_splash` (running `read_liberty`, `get_lib_cells`, `get_lib_pins` inside `ghcr.io/librelane/librelane:3.0.16`)
- **Our Command**: `cargo run --release -p grus-eda -- check-e0 --lib ~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib`
- **Our Runtime**: Cold parse: `5.35 ms` (test) / `6.11 ms` (CLI); Warm mmap cache load: `3.12 ms` (test) / `3.47 ms` (CLI) (target: `< 10 ms`)

| Metric | Reference (`OpenSTA`) | Ours (`grus-eda`) | Diff (Abs / %) | Pass? |
|---|---|---|---|---|
| Library Cells | 428 | 428 | 0 (0.0%) | PASS |
| Library Total Pins | 1,897 | 1,897 | 0 (0.0%) | PASS |
| Sequential Cells (Registers / Latches) | 63 (45 FF, 18 Latch) | 63 (45 FF, 18 Latch) | 0 (0.0%) | PASS |
| Warm Cache Load Time | N/A | 3.47 ms | N/A (< 10 ms) | PASS |
| Table Dimensions (`inv_1` fall delay) | 7x7 (49 values) | 7x7 (49 values) | 0 (0.0%) | PASS |
| Table Value Sample (`inv_1` [0,0]) | 0.0143656 ns | 0.0143656 ns | 0 (0.0%) | PASS |
| Table Value Sample (`inv_1` [6,6]) | 1.2016104 ns | 1.2016104 ns | 0 (0.0%) | PASS |

- **Notes / Root Cause Analysis on Sequential Cell Count**:
  - An initial naive textual count in `generate_ref.sh` counted 87 blocks via `grep -c "ff ("` (69) + `grep -c "latch ("` (18) = 87.
  - However, in OpenSTA and the cell library structure, there are 428 cells total. Exactly 24 cells are scan flip-flops (`sdf*` and `sedf*`), each containing both an `ff ("IQ", "IQ_N")` block at the cell level and a duplicate `ff` block inside its `test_cell ()` view.
  - OpenSTA ignores `test_cell ()` views, yielding exactly 45 unique flip-flop cells with `IQ` pins and 18 unique latch cells with `IQ` pins = 63 sequential cells total.
  - `grus-core` Liberty reader matches OpenSTA's semantic interpretation exactly: 428 cells, 1,897 pins, and 63 sequential cells (with 87 syntax groups in source text).

