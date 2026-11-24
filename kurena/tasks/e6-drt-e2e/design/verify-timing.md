# e6-drt-e2e — verify-stage timing (r8, the memo's only item)

> **r8 revision driver** — the r7 review memo: the verify stage died
> on an infra timeout, **not a failure**. `cargo test --workspace`
> was ~112 CPU-min in when the driver's per-invocation window closed
> (`workflow.toml` `timeout_sec = 1800`); prior identical runs on
> this machine took ~110–120 CPU-min and passed. Implementation is
> complete — this file is the only delta vs r7; every other design
> file stands unchanged.

## Symptom

- The debug-mode `drt` tests dominate the suite (the gcd emit legs
  run the real router). Whole-suite cost ≈ 110–120 CPU-min; wall
  time straddles the 30-min verify-command cap depending on machine
  load.
- `[[verify]] name = "test-workspace"` runs `cargo test
  --workspace` inside `timeout_sec = 1800`. A run landing on the
  wrong side of that margin is killed mid-suite — which is what the
  rejected run hit. Nothing in the tree failed.

## Procedure (the memo's directive)

1. **T0 — start the suite immediately, detached**, before any other
   revision work:

   ```sh
   mkdir -p tests/designs/gcd/runs
   nohup cargo test --workspace \
     > tests/designs/gcd/runs/cargo-test-workspace.log 2>&1 &
   echo $! > tests/designs/gcd/runs/cargo-test-workspace.pid
   ```

   `tests/designs/gcd/runs/` is gitignored — the log is a verify
   artifact, never committed. `nohup` + `&` detaches the run from
   the driver's invocation window so the full ~2h the suite can
   need is available regardless of when the verify stage runs.

2. **Do not start a second `cargo test`/`cargo build` while it
   runs.** Cargo serializes on the target-dir build lock and two
   concurrent test passes would contend for CPU, slowing both. This
   applies to the verify driver's own `test-workspace` invocation
   too: if it fires while the background run is still building, it
   stalls on the lock — expected, not a hang. The collected log is
   the evidence; the in-window invocation is best-effort.

3. **Collect at verify time**: `tail` the log for the per-binary
   `test result: ok. …` lines and quote the totals (plus exit
   status — `$?` via `wait` on the recorded pid, or the log's
   trailing summary) in the verify/card report. That completed log
   is the authoritative `cargo test --workspace` evidence for this
   card.

4. **Escalation is a human decision, not a workaround.** If even
   the unbounded background run cannot finish inside the revision
   window — or the human wants the *driver-run* `test-workspace`
   command to pass in-window — the fix is `cargo test --release
   --workspace` or a raised `timeout_sec` in `workflow.toml`. Per
   the memo: **flag it on the card, do not edit `workflow.toml`.**

## What this does not change

- No code, test, script, golden, or doc delta — `8d96fd7` (design)
  on `5d1502b` (implementation) stands. The diff audit in
  `steps.md`'s checklist is unchanged.
- `cargo clippy --workspace --all-targets -- -D warnings` stays a
  normal in-window verify command (compile-only — it fits).
- The picorv32 leg stays `#[ignore]`d-deferred (`steps.md` §E) —
  a plain `cargo test --workspace` run does not touch it.
- `workflow.toml` is not edited.

## Files

- `tests/designs/gcd/runs/` (gitignored — `cargo-test-workspace.log`,
  `cargo-test-workspace.pid`; no committed file changes in r8)
