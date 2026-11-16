# Step 4 ticket 08: ibex stage tests (E0-E3)

Repository: grus-eda (this repo). Work on `dev`. Follow
`AGENTS.md` (English only, Conventional Commits, stage explicit
paths). Design: `docs/history/step4-designs.md` including the
review resolutions and the 2026-11-14 measurement notes. The jpeg
tests (ticket 05) are the pattern: mirror them for ibex, changing
only what the design changes. The ibex golden files
`ibex.reference.json` and `ibex.gcell_usage.txt` are committed;
the large generated inputs (including `ibex.sta.reference.json`,
`ibex.orient.pins.txt`) are git-ignored, produced by
`tests/designs/ibex/generate_ref.sh`, and tests that need them use
`test_support::require` like the jpeg tests. Design facts: top
`ibex_core`, 8.0 ns clock on `clk_i`, floorplan core utilisation
50, placement target density 0.6, layer adjustment 0.2 (ORFS
`sky130hd/ibex/fastroute.tcl`), 16,644 instances, 3,702 STA
endpoints per mode.

Two ibex-specific cautions:

- The SDC is ORFS `sky130hd/ibex/constraint.sdc` verbatim: it sets
  `set_clock_latency 1.095` on the design clock and defines I/O
  delays against a **virtual clock** (`vclk_core_clock`, created
  with no source). No earlier design exercised either construct.
  If the STA comparison fails in a way that points at clock
  latency or the virtual clock, measure and report it; do not fix
  the STA or the SDC parser under this ticket (a follow-up ticket
  will own any fix, like ticket 06 did for async endpoints).
- Synthesis used the slang frontend, so `ibex.synth.json` comes
  from yosys-slang. If the yosys_json reader trips on a construct
  the plain-yosys designs never produced, report the exact failure
  instead of working around it in the reader.

## Task

1. **E0 for ibex**: mirror the gcd/aes/jpeg integration checks
   against `ibex.reference.json` wherever a per-design test
   exists (netlist cell/net/pin counts, DEF row/component counts,
   SPEF presence, orientation pins against
   `ibex.orient.pins.txt`). Reuse existing helpers; generalise
   only by adding the ibex calls.
2. **E3 for ibex**: add `compare_design_with_opensta` runs for
   ibex (zero-wire, placement RC, SPEF) with a per-design `Tol`
   following the picorv32/aes/jpeg pattern. The STA golden is a
   generated file here, so load it via `test_support` paths like
   jpeg does. Report the measured worst endpoint differences per
   mode in your report (arrival/slack/required/slew, WNS/TNS); do
   not widen any shared tolerance.
3. **E1 map test for ibex** (`test_ibex_congestion_map`): mirror
   the implemented aes/jpeg map test (same file format, same
   Spearman computation with average ranks, same negative
   controls: one-cell x-shift and the 20-permutation p95 with
   fixed seeds, same fixed-cut hot set at ratio >= 0.9 on both
   sides) on `ibex.util65.def` with a uniform **0.2** layer
   adjustment against `ibex.gcell_usage.txt`. Assert the grids
   agree; assert per direction rho >= 0.60, rho >= null + 0.15,
   hot-set overlap >= 0.50. Print all values for the record.
4. **E2 for ibex**: mirror the aes/jpeg placement test on the ibex
   floorplan: GP + legalization, assert `verify_legality` passes
   on the legalized result, compare total HPWL against
   `ibex.reference.json` (report the gap; assert within 5% only
   if it holds). Per the design doc, if GP does not converge at
   target density 0.6, apply the density step-down (0.55, then
   0.5) and print which density was used.

## Files

- Modify: `crates/grus-core/src/congestion/route.rs` (test module only)
- Modify: `crates/grus-core/src/placement/legalize.rs` (test module only)
- Modify: `crates/grus-core/src/readers/def.rs` (test module only)
- Modify: `crates/grus-core/src/readers/spef.rs` (test module only)
- Modify: `crates/grus-core/src/readers/verilog.rs` (test module only)
- Modify: `crates/grus-core/src/readers/yosys_json.rs` (test module only)
- Modify: `crates/grus-core/src/sta/analysis.rs` (test module only)

## Acceptance

- `cargo clippy --all-targets -- -D warnings` is clean (note the
  `--` before `-D warnings`).
- The new tests compile; on this VM the PDK/input-dependent ones
  will fail with missing-input panics from `test_support` — that
  is expected and must be the only failure mode you leave behind.
  The maintainer runs the real acceptance on the Intel host with
  the generated inputs present.
- Final report: the measured values you could obtain, any
  deviation from the jpeg test structure, and any failure that
  looks like a product bug (especially around the SDC latency /
  virtual clock and the slang-produced JSON) with the exact error
  text.
