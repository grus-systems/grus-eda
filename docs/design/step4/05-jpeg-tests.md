# Step 4 ticket 05: jpeg stage tests (E0-E3)

Repository: grus-eda (this repo). Work on `dev`. Follow
`AGENTS.md` (English only, Conventional Commits, stage explicit
paths). Design: `docs/design/step4-designs.md` including the
review resolutions and the 2026-11-14 measurement notes. The aes
tests (ticket 03) are the pattern: mirror them for jpeg, changing
only what the design changes. The jpeg golden files
`jpeg.reference.json` and `jpeg.gcell_usage.txt` are committed;
the large generated inputs (including
`jpeg.sta.reference.json`, `jpeg.orient.pins.txt`) are
git-ignored, produced by `tests/designs/jpeg/generate_ref.sh`,
and tests that need them use `test_support::require` like the
picorv32 tests. Design facts: top `jpeg_encoder`, 5.0 ns clock,
floorplan core utilisation 50, placement target density 0.6,
layer adjustment 0.2 (ORFS `sky130hd/jpeg/fastroute.tcl`),
61,569 instances, 4,467 STA endpoints per mode.

## Task

1. **E0 for jpeg**: mirror the gcd/aes integration checks
   against `jpeg.reference.json` wherever a per-design test
   exists (netlist cell/net/pin counts, DEF row/component
   counts, SPEF presence, orientation pins against
   `jpeg.orient.pins.txt`). Reuse existing helpers; generalise
   only by adding the jpeg calls.
2. **E3 for jpeg**: add `compare_design_with_opensta` runs for
   jpeg (zero-wire, placement RC, SPEF) with a per-design `Tol`
   following the picorv32/aes pattern. The STA golden is a
   generated file here, so load it via `test_support` paths
   like picorv32 does. Report the measured worst endpoint
   differences per mode in your report (arrival/slack/required/
   slew, WNS/TNS); do not widen any shared tolerance.
3. **E1 map test for jpeg** (`test_jpeg_congestion_map`):
   mirror the implemented aes map test (same file format, same
   Spearman computation with average ranks, same negative
   controls: one-cell x-shift and the 20-permutation p95 with
   fixed seeds, same fixed-cut hot set at ratio >= 0.9 on both
   sides) on `jpeg.util65.def` with a uniform **0.2** layer
   adjustment against `jpeg.gcell_usage.txt`. Assert the grids
   agree; assert per direction rho >= 0.60, rho >= null + 0.15,
   hot-set overlap >= 0.50. Print all values for the record.
4. **E2 for jpeg**: mirror the aes placement test on the jpeg
   floorplan: GP + legalization, assert `verify_legality`
   passes on the legalized result, compare total HPWL against
   `jpeg.reference.json` (report the gap; assert within 5%
   only if it holds). Per the design doc, if GP does not
   converge at target density 0.6, apply the density step-down
   (0.55, then 0.5) and print which density was used.

## Files

- Modify: `crates/grus-core/src/congestion/route.rs` (test module only)
- Modify: `crates/grus-core/src/placement/legalize.rs` (test module only)
- Modify: `crates/grus-core/src/readers/def.rs` (test module only)
- Modify: `crates/grus-core/src/readers/spef.rs` (test module only)
- Modify: `crates/grus-core/src/readers/verilog.rs` (test module only)
- Modify: `crates/grus-core/src/readers/yosys_json.rs` (test module only)
- Modify: `crates/grus-core/src/sta/analysis.rs` (test module only)

## Acceptance

- `cargo test --release` passes locally on this VM except the
  PDK/design-input tests, which may only fail with the loud
  `test_support` missing-input panic (this VM has no PDK and
  only some generated inputs); list which tests those are.
- `cargo clippy --all-targets -- -D warnings` is clean.
- The report states the measured E3 differences per mode, the
  E1 map values per direction (rho, both nulls, overlaps, hot
  counts), the E2 outcome (convergence, density used, HPWL
  gap), and the E0 counts compared.
- No golden file changes; no tolerance changes outside the new
  jpeg `Tol` entry.
