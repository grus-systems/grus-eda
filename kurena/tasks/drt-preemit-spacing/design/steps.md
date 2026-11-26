# Steps — implementation order, verification, commits

Two commits: (1) check + flag + tests (one coupled functional change),
(2) `docs/results.md` dated entry. The red-proof run of step 4 lands
inside commit 1 as evidence.

## Step 1 — `preemit_check` (emit.rs)

1. `crates/grus-core/src/drt/emit.rs`: add `preemit_check` per
   `semantics.md` (~10 lines + doc comment naming the `checkable`
   boundary); imports `crate::drc::geom::assemble`,
   `crate::drc::report::{run_all, SubsetReport}`.

## Step 2 — flag wiring (main.rs)

2. `DrtArgs.preemit_check` (`cli.md`).
3. Compute site: `Option<SubsetReport>` between `apply_routes` and
   `write_routed_def`.
4. `print_subset_findings` extraction; `preemit_check` print block
   before `if args.self_drc`; `self_drc` re-pointed at the helper
   (output byte-identical — diff a before/after `--self-drc` run on
   gcd if cheap, or trust the existing gate).
5. `preemit_check scope:` line with the report-only caveat.
6. Argv unit test: `--preemit-check` + assert.

## Step 3 — tests (tests/drt.rs)

7. `test_preemit_gcd` — clean, non-vacuity, determinism, IoPort
   presence, `preemit_report == self_drc_report` (tests.md).
8. `test_preemit_foreign_pin` — IoPort-adjacent inject → `≥1` spacing
   finding, pin rect in `bboxes`, port net in `nets`.

## Step 4 — verify

9. `cargo test --release -p grus-core --test drt test_preemit -- --nocapture`
   — both new tests green; capture output for `results.md`.
10. Red-proof: gate `IoPort` out of `assemble` →
    `test_preemit_foreign_pin` goes red → restore. Save the red run.
11. `cargo test --workspace`; `cargo clippy --workspace -- -D warnings`.
12. `Cargo.toml` `workspace.package` patch bump (0.12.13 → 0.12.14).

Commit 1: `feat: Add --preemit-check — 50-rule subset over the
in-memory emit shape set before write_routed_def` — body: mechanism
(reused `assemble`+`run_all`, no third engine), breadth + flag-shape
decisions, the `checkable` boundary (io-port endpoints checkable,
macro pin/obs islands-only — shared under-report vs Magic),
report-only pending `drt-selfdrc-exit`, version bump, agent trailer.

## Step 5 — docs (commit 2)

13. `docs/results.md` dated entry: what ran (both tests + red-proof),
    clean-gcd `findings_total=0`, fixture finding line verbatim, the
    `preemit == self_drc` equivalence assert, the named boundaries
    (MacroPin/MacroObs uncheckable endpoints, distance-0 overlaps
    exempt → `pin_collisions` territory, report-only exit gap pending
    `drt-selfdrc-exit`), picorv32 not run.

Commit 2: `docs: Record drt-preemit-spacing verification entry` —
agent trailer.

## Contingencies

- **`preemit_report != self_drc_report` on gcd**: a real emit/parse
  divergence — do not paper over; the assert is the ticket's proof
  that the checked set is the emitted set. Investigate before
  proceeding; a legitimate difference (none is known) must be named
  per-field in `results.md`.
- **Clean gcd reports >0 pre-emit**: the same finding exists on the
  re-parse path — a routing regression this check caught doing its
  job, not a check bug. Name the rule/site; do not land the check
  over an unexplained firing.
- **No `IoPort` shape on gcd emit**: impossible per `gcd.placed.def`
  (met3/met4/met5 `+ LAYER` rects on every `PINS` record) — but if the
  scan finds none, the fixture must not silently pass; it panics
  first (`expect`), which is the loud-failure behavior.
- **`drt-selfdrc-exit` lands mid-flight**: that ticket absorbs the
  exit-code decision for both flags; this ticket still ships
  report-only and its scope line is updated there, not here.
