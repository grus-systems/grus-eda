# e5-cts-tree — Implementation Steps

Two commits, mirroring `e5-insert-api`'s split (code+tests, then docs).
Version `0.4.0 → 0.5.0` rides in commit 1 per §5's same-commit rule.

## Commit 1 — `feat: add clock-tree constructor (grus-eda cts)`

Files: `crates/grus-core/src/cts/{mod,tree}.rs` (new),
`crates/grus-core/src/lib.rs`, `crates/grus-core/src/sta/analysis.rs`
(visibility only), `crates/grus-eda/src/main.rs`, `Cargo.toml`.

Order inside the module:

1. `CtsConfig` + `CtsReport` (schema-first so tests compile against the
   report shape).
2. `clock_roots` + T1.
3. `partition` + the level loop + `pick_buffer`/`buffer_signal_pins` +
   `legalize_inserted_only` → `run_cts` → T2, T4, T7.
4. `write_def_full` emission → T3, T6 (roundtrip + legality both designs).
5. `sta/analysis.rs` accessor → T5 (propagated-clock informational).
6. `main.rs` `cts` arm + CLI parse/e2e tests.
7. `Cargo.toml` version bump.
8. Fail-proof mutation run (tests.md) — record red lines for
   `docs/results.md`.
9. `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`.

Commit body records: algorithm choice (count-equalized median bisection
vs PD-tree — why), `max_loads = 8` and its TritonCTS-fanout grounding,
pin-and-place legalization (why — the byte-identity criterion), the
`insert_buffer_on_net` deviation (k-way fanout needs primitive ops),
the minor-vs-patch discrepancy (acceptance says minor; §5 says patch —
minor taken per the explicit acceptance, same as `e5-insert-api`).

## Commit 2 — `docs: record e5-cts-tree results and stage doc`

Files: `docs/e5-cts.md` (new), `docs/results.md`, `docs/status.md`,
`docs/roadmap.md`.

### `docs/e5-cts.md` sections

- Inputs & reproduction commands (the exact `grus-eda cts` CLI lines for
  gcd + picorv32; seeds: `*.placed.def`, `*.cts.sdc`, sky130 files).
- Algorithm (pointer to `kurena/tasks/e5-cts-tree/design/algorithm.md`
  summary: partition rule, sizing rule, pin-and-place, naming).
- Report schema (`report.md` table).
- Ours-vs-reference census tables (measured numbers from the test run;
  gcd and picorv32 side by side with `*.cts.reference.json`).
- Recorded limitations & deferred work: `+ SOURCE TIMING`,
  `+ USE CLOCK`, `+ NONDEFAULTRULE`, TRACKS/GCELLGRID/VIAS/SPECIALNETS
  emission gaps; no dummy loads / `repair_clock_nets`; clock-as-data and
  pre-existing-fragment semantics; pin-and-place divergence from
  OpenROAD's full legalize; `run_sta_with_state` visibility change;
  the e2e STA gate deferred to `e5-cts-e2e` (ticket 4) — **stated, not
  claimed**, per the goal.

### `docs/results.md` entry

New dated `e5-cts-tree` section beside the `e5-cts-ref` entry: what ran
(exact test names), census tables ours-vs-reference for both designs,
the informational STA numbers (spread, insertion delay vs 0.31 ns /
34 ps refs), byte-identity check result, fail-proof red lines, the
version discrepancy note, and what was **not** run (no OpenROAD re-run —
references committed in e5-cts-ref are reused as-is).

### `docs/status.md`

New completed row/entry in the E5 section; "clock tree insertion" no
longer listed as absent in the stage summary; remaining E5 gap narrowed
to the e2e STA comparison (ticket 4).

### `docs/roadmap.md`

Update the E5 checklist line for ticket 3 (constructor + census done);
keep ticket 4 (`e5-cts-e2e` e2e comparison) as the next item — do not
mark E5 complete.

## Risks & contingencies

- **Legalizer density**: if Abacus can't place a buffer near its seed at
  picorv32 utilization, legalize reports it — inspect before widening
  anything; the fix is seed quality, not loosening legality.
- **Sizing `max_cap` missing** on a Liberty pin: fallback is the
  strongest `buf_list` entry (recorded rule), not a crash.
- **Depth > expectation on picorv32**: `max_loads` is the knob; a bump
  to e.g. 10 re-centers the census — change the config constant, rerun
  T6, record the retune in the commit + results entry.
- **`write_def_full` name-escaping** on `clknet_*` is already exercised
  by I6 tests; if a DEF keyword collision appears, name it rather than
  papering over it.

## Done-when (this ticket)

All `acceptance.md` items: `grus-eda cts` runs on both designs; emitted
DEF re-parses and legalizes; clock nets decomposed into buffer-driven
segments; report JSON per run with the census schema; `docs/results.md`
companion beside the TritonCTS census; propagated-clock STA executes in
`cargo test` with gcd informational spread/insertion recorded; non-clock
DEF content byte-identical with control-net HPWL unchanged; `cargo test`
+ `clippy -D warnings` green; fail-proof mutation recorded;
`docs/e5-cts.md` + status + roadmap updated; version bumped.
