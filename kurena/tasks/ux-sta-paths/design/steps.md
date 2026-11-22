# ux-sta-paths — Implementation steps

Ordered so every step compiles and is independently checkable. Steps 1–5 are
code; 6–8 verification & docs. Commit per `AGENTS.md` §5 (one logical unit per
commit — suggested split marked with `commit:`).

## 0. Probe (no code)

- Inside `ghcr.io/librelane/librelane:3.0.16`: `sta -no_init -no_splash` →
  `report_checks -help`; confirm `-endpoint_path_count`,
  `-unique_paths_to_endpoint`, `-group_path_count`, `-path_delay min|max`,
  `-fields`, `-digits`, `-format full`. Record the exact flag surface in the
  script header comment (`golden.md` §1).

## 1. Report types + `StaReport` field — `commit:` `feat: Add critical path report types`

- `PathEdge`, `PathStage`, `CriticalPath` per `api.md` §1;
  `StaReport.critical_paths` with the serde gate; `merge_corner_reports`
  keeps `max` rows from setup + `min` rows from hold.
- Fix the `StaReport` constructor sites (`assemble_sta_report` — empty vec;
  `merge_corner_reports`; any test literals).
- `sta/mod.rs` re-exports.
- Verify: `cargo test --workspace` — pure additive, everything still green.

## 2. `Worst` tags + plumbing — same commit or `feat: Tag endpoint candidates for path tracing`

- `Worst` gains `class: u16` (`u16::MAX`) and `from_pin: u32` (`u32::MAX`);
  update the ~6 constructor sites (`collect_endpoints` ×3, partitioned
  compose folds, leaf fold, deferred union compose).
- `pin_segment_arrival` also returns the winning pin; stamp `from_pin` at the
  two rewrite sites.
- `assemble_sta_report` second return becomes `Vec<(PinId, EndpointPolarities)>`;
  adjust `run_sta_detail`, `run_sta_with_state`, `run_sta_partitioned`.
- Verify: `cargo test --workspace` — behavioural no-op.

## 3. Tracer — `commit:` `feat: Trace worst-N critical paths in sta`

- `trace_path` + predecessor-candidate helpers per `trace.md` §2–§5
  (sink/arc/seed candidates, borrow replay, `TRACE_EPS_NS`, stage reversal,
  startpoint/endpoint/clock naming, `note` honesty).
- `select_critical_paths` (rank endpoints, pick polarity, call `trace_path`
  per corner).
- `run_sta_inner`/`run_sta_detail`/`run_sta_partitioned` gain `path_count`;
  `run_sta_with_paths`/`run_sta_quiet_with_paths` wrappers; attach
  `report.critical_paths`. Partitioned branch: keep `class_states` + the
  per-class `disabled_borrow` Vec alive until after tracing.
- Verify: `cargo test --workspace` (paths code exercised only by new tests,
  next step).

## 4. Unit tests — same commit

- `test_critical_paths_structure_gcd` and `test_critical_paths_exceptions`
  per `golden.md` §2 (exception fixture: pick an `sta_exceptions` SDC that
  leaves ≥3 reported endpoints).
- Verify: green.

## 5. CLI — `commit:` `feat: Add sta --paths N path dump`

- `StaArgs.paths`, `*_with_paths` dispatch, stdout dump format, `--lib-hold`
  merge path.
- Verify manually on gcd:
  `cargo run --release -p grus-eda -- sta --lib ... --tlef ... --lef ...
   --def tests/designs/gcd/gcd.placed.def --sdc tests/designs/gcd/gcd.sdc
   --spef tests/designs/gcd/gcd.routed.spef --paths 3 --json /tmp/sta.json`
  — eyeball the dump and `critical_paths` in the JSON.

## 6. Golden — `commit:` `test: Add gcd report_paths golden for critical paths`

- Write `tests/designs/gcd/generate_paths_ref.sh` (step-0 flags); run it in
  the pinned container (Colima up per `AGENTS.md` §2); commit
  `gcd.paths_max.txt` / `gcd.paths_min.txt`; `git diff` the outputs on a
  re-run to prove the script reproduces them.
- `test_gcd_critical_paths_vs_opensta` per `golden.md` §2 — pin sequences
  verbatim, per-stage delay/arrival `max(5 ps, 1%)`, transitions, path-level
  values.
- If a sequence or stage diverges: name it in `docs/results.md` with the
  root cause before adjusting any bound (E0 lesson 5).

## 7. Fail-proof + full verify — same commit as 6

- Corrupt winner selection → confirm `test_gcd_critical_paths_vs_opensta`
  red → restore → green. Record both runs in `docs/results.md`.
- `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`.

## 8. Docs + version — `commit:` `docs: Document critical path reporting (E9.1)`

- `docs/e9-reporting.md` (schema + semantics per `docs.md`), `docs/README.md`
  index line, `docs/results.md` dated entry (commands run, residuals,
  fail-proof log), `docs/status.md` E9 row.
- `Cargo.toml` `0.5.1 -> 0.6.0` + `Cargo.lock`.

## Watch-items

- `gcd.paths_min.txt`: if OpenSTA reports no hold paths (`Path Type: min`
  empty), assert the file shape honestly instead of inventing checks — gcd
  does have `min_*` endpoints in `gcd.sta.reference.json`, so this should
  not trigger.
- `parse_paths` may need a `Path Type: min`/`(hold)` tweak — read it before
  writing the test.
- `EndpointPolarities`-aligned vectors: after step 2 the endpoints and
  polarities must stay index-aligned — `assemble_sta_report` unzips them
  together, so sort order is preserved; do not re-sort paths afterwards.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/mod.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/gcd/generate_paths_ref.sh` (new)
- `tests/designs/gcd/gcd.paths_max.txt` (new, generated)
- `tests/designs/gcd/gcd.paths_min.txt` (new, generated)
- `docs/e9-reporting.md` (new)
- `docs/README.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
- `Cargo.lock`
