# Step 4 ticket 06 — E3: asynchronous set/reset pins as timing endpoints

## Background

The jpeg E3 tests (`test_jpeg_zero_wire_matches_opensta`, `test_jpeg_placement_rc_matches_opensta`,
`test_jpeg_placed_spef_matches_opensta`, added by ticket 05) fail before any value comparison:
our STA reports 4,401 endpoints, the OpenSTA golden (`jpeg.sta.reference.json`) reports 4,467.
Maintainer diagnosis (Intel, 2026-11-14): all 66 missing endpoints are asynchronous set/reset
pins — `_84344_/RESET_B` … `_88182_/RESET_B` (64 RESET_B + 2 SET_B in the first 40 shown), zero
extra endpoints on our side. OpenSTA's `find_timing_paths` reports paths ending at async pins;
our STA does not.

## Root cause (already established, do not re-derive)

- `crates/grus-core/src/sta/analysis.rs`, `collect_endpoints` (line ~420): endpoints come only
  from `graph.checks` (D pins with setup/hold arcs) and primary outputs.
- `build_graph` (line ~187) handles `TimingType::SetupRising` / `HoldRising` into `graph.checks`
  and drops everything else (`_ => {}`), including `TimingType::RecoveryRising` /
  `TimingType::RemovalRising`.
- The Liberty parser already maps `recovery_rising` / `removal_rising`
  (`crates/grus-core/src/readers/liberty.rs`, lines ~218–219), so the arcs are in `cx.lib.arcs`.
- Arrival propagation to the async pins already happens (they are sinks of nets), so only the
  endpoint collection and the required-time computation are missing.

## Files

- `crates/grus-core/src/sta/analysis.rs` (only file; `build_graph`, `RegisterCheck`,
  `collect_endpoints`).

## Work

1. **Measure first.** From `tests/designs/jpeg/jpeg.sta.reference.json` (mode `zero_wire`),
   read the `max_req_ns` / `min_req_ns` of one async endpoint (e.g. `_84344_/RESET_B`) and of
   one D pin of the same cell, and confirm the required-time semantics before coding:
   recovery is expected to behave like setup (`req_max = period + clk_arr_min - recovery_time -
   uncertainty`) and removal like hold (`req_min = clk_arr_max + removal_time + uncertainty`),
   with the same clock as the cell's D-pin check. Print the comparison and put the numbers in
   the final message. If the golden's numbers do not fit that semantics, stop and report the
   measured values instead of guessing.
2. Extend the graph/checks so async pins (pins carrying a recovery or removal arc in the
   Liberty data) become endpoints, with the required times from step 1. Reuse the existing
   worst-per-polarity selection and `endpoint_report` shape; do not change the D-pin or
   primary-output paths.
3. Keep the change to the STA engine; do not change any golden file, test, or tolerance.

## Acceptance

- `cargo test --release test_jpeg_zero_wire_matches_opensta` (and the other two jpeg E3 tests,
  run on a machine with the PDK and the jpeg generated inputs — the maintainer will run them)
  must reach the value comparison: endpoint count 4,467 on all three modes.
- Report, per mode, the `max relative diff per field` line the comparator prints, and the
  WNS/TNS comparison. If a wire mode (placement RC / SPEF) fails the gcd tolerance, do **not**
  widen the tolerance and do **not** ignore the test yourself: report the measured diffs per
  field and leave the tolerance call to the maintainer (aes precedent: systematic bias is
  `#[ignore]`d with a recorded table, never fitted).
- `cargo clippy --all-targets -D warnings` clean. No other test may change its pass/fail state
  (the maintainer runs the full suite on acceptance).
- Final message: the step-1 numbers, the per-mode max-diff lines, and the exact commit the
  work is based on.
