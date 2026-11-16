# Ticket 17 — DMP vs `report_dcalc` on the picorv32 routed SPEF (76 rows)

Step 7 (E3.5, `docs/e3-sta.md` Section 5). Ticket 16 separated the picorv32 wire-mode error into an
estimator side (~1.4 ns median, not ours to fix) and an engine residual (median 6.2 ps, with a slack
tail: 726 of 1,798 endpoints outside the strict criterion, and a ~13%-low slew family on one register
bank). E3.5's named instrument for the engine side is a `report_dcalc` table. The placed-SPEF version
exists (`test_dmp_ceff_matches_opensta_report_dcalc`, 6 arcs, Ceff asserted at 6%). This ticket builds
the routed-SPEF version at scale. **Measurement only: no model change, no tolerance change, no
`RD_SCALE` change in this ticket.** The maintainer decides the follow-up from the printed table.

## Reference data (already collected, committed with this ticket)

`tests/designs/picorv32/dcalc_routed_ref.txt` — 76 rows from OpenSTA's
`report_dcalc -from <in> -to <out> -max -digits 6` on the placed DEF + `picorv32.rcx.spef`
(pinned LibreLane image, maintainer-collected 2026-11-15). The file header records the provenance and
the column layout: from/to pins, arc sense, output transition, input slew (ns), pi model
(C1 far pF, Rpi kohm, C2 near pF — OpenSTA's naming), Ceff (pF), delay (ns), table slew (ns), driver
waveform slew (ns). Arcs are the stages of the worst paths to `_11265_/D`, `_11072_/D` and
`_11317_/D`, plus the three arcs of the placed-SPEF comparison. Non-unate arcs appear once per sense;
the key (from, to, sense, output transition) is unique across the file. The three CLK->Q rows have
input slew 0.

## Files

- `crates/grus-core/src/sta/analysis.rs` (one new test; helpers local to it)

Nothing else. In particular not `sta/dmp.rs`: if `solve_dmp`'s result does not expose the arc delay
and slew the engine actually propagates, say so in the job report instead of widening scope.

## Work

Add `test_dmp_matches_opensta_report_dcalc_routed`, modelled on
`test_dmp_ceff_matches_opensta_report_dcalc` (same context construction), with these differences:

1. Parasitics come from the **routed** SPEF (`picorv32.rcx.spef`, loaded the same way
   `Rc::RoutedSpef` loads it) instead of the placement-estimate SPEF.
2. Cases come from parsing `dcalc_routed_ref.txt` (via `design_dir("picorv32")`); `#` lines are
   comments. Panic loudly if the file is missing or a row is malformed (no silent skips).
3. For each row, find the graph edge from -> to whose Liberty arc matches the row's **sense** and
   output transition. How the reader/graph represents sense is for you to determine from the code;
   match on what is actually stored. A row with no matching edge is **unmodelled**: print it as such
   (with the row's key) and count it — do not drop it silently and do not merge senses.
4. For matched rows compute, exactly as the existing test does: our pi model for the output pin's net
   (sink caps for the row's output transition) and `solve_dmp` at the row's input slew. Print one
   table (via the test's stdout under `--nocapture`) with, per row: the key, and the signed relative
   errors in percent for pi C1, Rpi, C2, Ceff, delay, slew; plus the gap between OpenSTA's driver
   waveform slew and our slew (percent). End with a summary line: rows total / matched / unmodelled,
   and the median and max |error| per quantity over matched rows.
5. Assertions: the row count equals the number of data rows in the file and is at least 60; every row
   is matched or printed unmodelled. **No error magnitude is asserted** (this is the measurement).
   The existing 6-arc test is untouched.

## Acceptance (run by the maintainer; the job worktree has no PDK)

- The new test passes on the M4 with the generated picorv32 inputs present, and its printed table is
  complete (76 rows accounted for).
- Full suite and clippy stay green; the placed-SPEF dcalc test is unchanged.
- The job report quotes the summary line (matched/unmodelled counts, median/max errors per quantity)
  as far as it can be produced without the PDK inputs — if it cannot run there, it says so.
