# perf-slewonly-power — implementation order and gates

One commit: functional change + tests + docs + version bump together
(repo rule: version in the same commit). ~150–220 net lines of
`analysis.rs` plus ~120 lines of tests.

## 0. Baseline (before editing)

`cargo build --release` on the clean worktree and, if a fresh before
column is wanted for `power` zero-wire / `power --vcd` gcd
([`measurement.md`](measurement.md) §Before), run `REPS=5 sh
kurena/tasks/perf-profile/measure.sh` or the subset now — record
which. `cargo test --workspace` green first.

## 1. Refactor, no behavior change

- `prepare_sta` gains `for_slew: bool`; guard becomes
  `any_propagated || (!for_slew && !graph.pulse_width_checks.is_empty())`.
  `run_sta_detail` passes `false`.
- Extract `eval_sta`'s seed block → `eval_seed_state(prepared,
  allowed)`; `eval_sta` calls it under `sta.eval.seeds`.
- Add `eval_state_full(prepared) -> Result<Vec<PinTimingState>>` —
  the partition-dispatch/else-`eval_sta` state dispatcher the fallback
  uses (same branch condition as `run_sta_detail`).

Gate: `cargo test --workspace` + `cargo clippy --workspace
--all-targets -- -D warnings` green — pure move, zero output change.

## 2. `slew_eval` + new `sta_pin_slews`

Per [`api.md`](api.md) §3–4: `slew_eval` level walk under
`sta.slew.walk` (seed replay under `sta.slew.seeds`), `reached`
markers on `arrival_max`, `uncovered_borrow` predicate →
`eval_state_full` fallback, `PinSlew` projection, doc comment rewrite.

Gate: same green; `power` outputs unchanged (the new path is
identical-by-construction, not yet proven — §3 proves it).

## 3. Parity tests + fail-proof

- `assert_pin_slew_parity` helper + the matrix in
  [`verification.md`](verification.md) §1 — start with
  gcd/picorv32/ibex × `{None, Placement}` + `*.propagated.sdc` +
  microbench + `sta_exceptions` + `sta_edge/latch_borrow`; add
  aes/jpeg `Placement` if the release-suite budget allows.
- Record which fixture fires `uncovered_borrow` (expect
  `latch_borrow.def`; if none, say so and mark it defensive).
- Fail-proof per §3: clock-net-guard drop → red → restore;
  `uncovered_borrow = false` forced → latch fixture red → restore.
  Record reds in `docs/results.md`.

Gate: `cargo test --workspace`, `cargo test --release -p grus-core
sta` green; `git status` clean under `tests/designs/**`.

## 4. Measurement

`REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-slewonly-power.log` on the final release binary, idle
machine. Extract power-cell medians and nested phase medians
(`power.slews`, `sta.slew.walk`, `sta.slew.seeds`,
`sta.prepare.clock.*` presence).

## 5. Docs + version + commit

- `docs/results.md` entry per the
  [`measurement.md`](measurement.md) skeleton: before/after table,
  phase line with the moved keys, achieved-vs-plan, parity/fail-proof
  evidence, "what was run".
- `docs/roadmap.md` N8: `perf-slewonly-power` → done with the jpeg
  number; `power.slews` no longer inherits `sta.*` eval phases.
- `Cargo.toml` `0.3.3` → `0.3.4`, `Cargo.lock` follows.
- Commit (Conventional Commits + agent trailer), e.g.
  `perf: Run pin-slew-only STA path for power`.
