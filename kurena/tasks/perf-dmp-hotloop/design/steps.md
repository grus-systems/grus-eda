# perf-dmp-hotloop — implementation order and gates

One commit at the end: mechanism + `#[cfg(test)]` hook + docs +
version bump (`0.3.6` → `0.3.7`, `[workspace.package]` +
`Cargo.lock` together — repo rule). The counters patch is never
committed; the design-doc "chosen mechanism" edit rides in the
same commit.

## 0. Baseline (before editing)

`cargo build --release` on the clean worktree;
`cp target/release/grus-eda /tmp/grus-eda.before`;
`cargo test --workspace` green. Then:

```sh
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-dmp-before.log
```

and the two `--json` baseline files per
[`verification.md`](verification.md) §2
(`/tmp/{jpeg,ibex}.sta.before.json`) with
`/tmp/grus-eda.before`.

## 1. Count (ephemeral — do not commit)

Apply the instrumentation from [`counts.md`](counts.md) as an
uncommitted patch: walk-tag TLS cell, per-walk `driver_response`
total/unique `HashSet`s, intra-call `table(c)` counter,
`rc_crossing` closed/brent split, `eval_2d` total — all dumped by
`eprintln!` at command end. Build release, run the jpeg `sta` +
`power` and ibex `sta` commands listed there, save output to
`/tmp/dmp-counts.txt`, `git checkout` the counters away (keep
`/tmp/dmp-counters.patch` until the results entry is written).

Gate: numbers recorded; decision rule applied; `overview.md` /
`memoization.md` updated to name the chosen mechanism(s).

## 2. Implement

Per [`memoization.md`](memoization.md):

- `thread_local!` `DRV_MEMO` map + `DRV_MEMO_EPOCH` counter +
  `DrvKey`;
- `Context.memo_epoch` filled at every `Context { .. }` site
  (`prepare_sta` + test literals);
- `driver_response`: key → lookup → miss computes the unchanged
  body → insert;
- `#[cfg(test)]` hit/miss stat cells + the committed unit test
  ([`verification.md`](verification.md));
- M2 / M4 only if the counts gated them in.

Gate: `cargo test --workspace` and `cargo clippy --workspace
--all-targets -- -D warnings` green.

## 3. Verification battery

In order, per [`verification.md`](verification.md):

1. `--json` `cmp` jpeg + ibex, before vs after binaries;
2. `RAYON_NUM_THREADS` = 1 / 4 / 10 `cmp` on jpeg
   `sta.placement` `--json`;
3. fail-proof mutation → confirm red → restore;
4. `cargo test --release -p grus-core sta` green (the strict
   SPEF/DEF consumers).

## 4. Measure + record + commit

`REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-dmp-after.log` on the final binary, same idle session
as the before run. Write the `docs/results.md` entry from the
skeleton in [`measurement.md`](measurement.md) — counts block
verbatim from `/tmp/dmp-counts.txt`, before/after phase + wall
tables, achieved vs residual. Update `docs/roadmap.md` §N8.
Bump `0.3.6` → `0.3.7` in root `Cargo.toml` (Cargo.lock follows).
Single commit; stage explicit paths only.

## If the counts kill M1

`R` below threshold with no other mechanism clearing the bar →
the ticket records the negative result: results.md logs the
counts and the residual analysis, the memo code is not added,
and the commit is docs + version bump only. Do not ship an
unsupported cache.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/dmp.rs`
- `crates/grus-core/src/sta/nldm.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-dmp-hotloop/design/`
- `Cargo.toml`
- `Cargo.lock`
