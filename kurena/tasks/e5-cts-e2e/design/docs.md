# e5-cts-e2e — Documentation updates

Four doc files record the verdicts. All text from the gate/decomposition
`--nocapture` prints — nothing transcribed by memory (E0 lesson 1).

## `docs/results.md` — new "E5 cts-e2e" section

Appended after the existing `e5-cts-tree` entry, one block:

- Header: date, environment (`mise`/container versions, commit hash),
  exact commands run (`generate_cts_ours.sh <d>`, the two `cargo test
  -- --nocapture test_<d>_cts_e2e` invocations, `generate_cts_ref.sh
  picorv32` rerun for `picorv32.cts.def` — naming that only that step
  produced a new artifact, E0 lesson 9).
- Artifacts table: path, size, sha256 first-16, committed/gitignored
  (the `picorv32.cts.def` precedent).
- Per design, the four criteria as measured:
  1. skew: `our_spread_ps`, `ref_spread_ps`, diff, bound `max(25, 25%)`,
     pass/fail;
  2. insertion: `our_lat_max`, `ref latency_max_ns`, diff%, bound 15%;
  3. e2e STA: per-field max diff table (8 endpoint fields + wns/tns)
     and the worst-5 rows from the gate print;
  4. honesty: the census table (`CtsReport` vs `CtsRefJson` side by
     side: inserted/tree/dummy/subnets/sinks/depth/HPWL/avg-sink —
     `avg_sink_wirelength_um` flagged "different quantity, see
     e5-cts.md §3").
- Decomposition table: the four axes' per-field max diffs and the
  named cause assigned to every outside-bound value (tree-shape /
  engine / reference-setup, per `decomposition.md`'s attribution rule).
- Fail-proof record: which mutation was run, which assertion went red.
- Failures stay in the log with root cause (§2).

## `docs/status.md` — E5 row

Current row: `E5.3 CTS tree` etc. Update/append the strict-comparison
status:

- Per-criterion verdict per design: `met` or `named residual: <axis> —
  <cause>` (e.g. expected picorv32 skew entry: "named residual:
  tree-shape — depth-balanced tree equalizes buffer count, not per-pin
  RC; 70.6 ps vs ref 36.2 ps, engine residual on the same tree ≤
  <measured> ps" — actual numbers, not this sketch).
- Actual tolerances used (the `CtsTol` values, including any
  floor-holding `*_CTS_*` constants and their causes).
- Coverage: 53/1,798 endpoints, 35/1,597 clock pins; known gaps
  carried forward (`+ SPECIAL` PINS emission gap, no dummy loads).

## `docs/e5-cts.md`

- §1 status table: `strict e2e comparison vs committed goldens` row →
  `done (v0.5.1)` with the per-design verdict summary.
- New short section (after §3's metric-comparison note): the e2e
  harness shape — where the tests live, which SDC/RC the comparison
  runs, the skew surface definition (per-pin `arrival_max_rise`
  spread, why not `worst_clock_skew`), and the decomposition axes.
  Keep the spec in step with the code (E0 lesson 8).
- §6 version line → `0.5.1` with the patch-vs-minor choice recorded
  ("patch: additive tests/artifacts/docs, no production-API change").

## `docs/roadmap.md`

E5 line: `E5 CTS (placed DEF + SDC → legal CTS DEF)` progress —
`e5-cts-e2e` done, strict comparison verdicts recorded; any residual
named (e.g. picorv32 skew, `+ SPECIAL` pins) carried to the open-gaps
list rather than hidden.
