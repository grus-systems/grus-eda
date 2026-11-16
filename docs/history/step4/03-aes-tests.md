# Step 4 ticket 03: aes stage tests, the E1 map test, picorv32 E2

Repository: grus-eda (this repo). Work on `dev`. Follow
`AGENTS.md` (English only, Conventional Commits, stage explicit
paths). Design: `docs/history/step4-designs.md` including the
review resolutions and the 2026-11-14 measurement note. The aes
golden files are committed; the large generated inputs
(`aes.synth.*`, `aes.*.def`, `aes.routed.spef`) are git-ignored
and produced by `tests/designs/aes/generate_ref.sh` — tests that
need them use `test_support::require` like the picorv32 tests.

## Task

1. **Congestion API**: extend `CongestionReport` with per-edge
   usage-ratio vectors `h_usage_ratio` (indexed like the grid's
   horizontal edges) and `v_usage_ratio`, filled during the
   existing aggregation in `route.rs`. No behaviour change to the
   existing fields.
2. **E0 for aes**: mirror the gcd integration checks against
   `aes.reference.json` (Liberty/LEF counts are shared; netlist
   cell/net/pin counts, DEF row/component counts, SPEF presence)
   wherever gcd has a per-design test. Reuse existing helpers;
   generalise only by adding the aes calls.
3. **E3 for aes**: add `compare_design_with_opensta` runs for aes
   (zero-wire, placement RC, SPEF) with a per-design `Tol`
   following the picorv32 pattern. Report the measured worst
   endpoint differences per mode in your report.
4. **E1 map test for aes** (`test_aes_congestion_map`): parse
   `tests/designs/aes/aes.gcell_usage.txt` (header + `H/V x y
   usage capacity` lines). Run `estimate_congestion` on
   `aes.util65.def` with a uniform 0.3 layer adjustment. Assert
   the grids agree (origin, tile, nx, ny from the dump header vs
   our grid). Per direction, compute over the dump's edges:
   Spearman rank correlation (average ranks for ties) between
   OpenROAD's usage ratio (usage/capacity) and ours, and the
   overlap coefficient of the top 5% hottest edges by ratio.
   Negative controls in the same test: the reference map shifted
   by one GCell along x, and a fixed-seed permutation of the
   reference values; take the larger control rho per direction as
   the null. Assert per direction: rho >= 0.60, rho >= null +
   0.15, overlap >= 0.50. Print all values (rho per direction,
   nulls, overlaps, support sizes) for the record. Spearman and
   overlap helpers live in the congestion test module.
5. **E2 for aes**: mirror gcd's placement test on the aes
   floorplan: GP + legalization, assert `verify_legality`
   passes, compare total HPWL against the reference value in
   `aes.reference.json` (report the gap; assert within 5% only
   if it holds — if not, report the measured gap and do not
   assert).
6. **E2 for picorv32**: a test that runs placement end to end on
   the generated picorv32 floorplan and asserts the run completes
   and the legalized result passes `verify_legality`; print
   convergence status, final overflow and HPWL. No HPWL
   threshold (the known state is GP non-convergence).
7. **CI**: add an aes generation step to `.github/workflows/ci.yml`
   mirroring the picorv32 one (generate when `aes.util65.def` is
   missing).

## Files

- Modify: `crates/grus-core/src/congestion/route.rs`
- Modify: the test modules that hold the gcd/picorv32 integration
  tests (congestion, placement, STA, readers as applicable)
- Modify: `.github/workflows/ci.yml`

## Acceptance

- `cargo test --release` passes on a machine with the PDK and the
  generated aes/picorv32 inputs; on a machine without them the
  new tests panic via `require` like the existing ones.
- Report prints: E3 aes worst diffs per mode, E1 map numbers,
  E2 aes HPWL gap, E2 picorv32 outcome.
