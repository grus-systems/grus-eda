# e6-drt-e2e — verify-stage timing (r9, the memo's only item)

> **r9 revision driver** — the second driver-timeout on the verify
> stage. The memo rules it **structural, not a code issue**:
> dev-profile `cargo test --workspace` needs ~2h+ on this machine
> (the `drt` tests dominate — the gcd emit legs run the real
> router; `drt.rs` alone measured 2195.68 s in the completed r8
> background run), while `workflow.toml` gives each `[[verify]]`
> command `timeout_sec = 1800`. Directive: **do not re-run it
> inside the verify step.** The r8 detached-suite procedure is
> superseded — this file is collect-only.

## Evidence base — nothing to launch

1. **Recorded clean runs.** `docs/results.md` (dated 2026-11-24,
   what-ran): `cargo test --workspace` and `cargo clippy
   --workspace --all-targets -- -D warnings` clean at `0d113ba`
   and confirmed post-`5d1502b`. `5d1502b` is a pure
   rename/refactor over `0d113ba` — byte-equal emit is re-proved
   by `test_e2e_ours_def_deterministic_gcd` inside the suite.
2. **The r8 detached dev-profile run completed green** —
   `tests/designs/gcd/runs/cargo-test-workspace.log` (gitignored;
   `nohup` at r8-T0, pid file's process exited): every binary
   `test result: ok.` — 550 lib + 18 `drc.rs` + 24 `drt.rs` +
   2 `irdrop.rs` + 7 `grus-eda` unittests + 0 doc-tests =
   **601 passed, 6 ignored** (the picorv32 companions, ignored by
   design). A real post-`5d1502b` dev-profile full-suite pass on
   the committed tree — only design docs sit on top.
3. **The memo's designated artifact — release profile, already
   complete.** `cargo test --workspace --release` ran detached,
   log `/tmp/e2e-ws-release-test.log`: `Finished release profile
   in 20.77 s`, then **601 passed / 0 failed / 6 ignored** across
   the same six binaries in ~121 s of test time. Green at
   2026-11-25 01:41. Release is the profile that fits: compile
   ~15 min cold (20 s warm here), suite in minutes.

## Verify-time procedure (collect-only)

1. **Collect `/tmp/e2e-ws-release-test.log`.** Copy it to
   `tests/designs/gcd/runs/release-test-workspace.log`
   (gitignored — the card's artifact next to
   `cargo-test-workspace.log`) and quote the per-binary
   `test result: ok.` totals + the run's completion in the
   verify/card report. If a future verify finds it still running,
   wait on the pid — minutes, not hours; never start a competing
   `cargo` invocation while it holds the `target/` lock. If found
   dead mid-log, restart it the same way it was started
   (`nohup cargo test --workspace --release > … 2>&1 &`) —
   **release profile only, never a dev-profile re-run.**
2. **`cargo clippy --workspace --all-targets -- -D warnings`** —
   stays a normal in-window verify command (compile-only, fits;
   run it after the release artifact lands so it does not queue
   behind the `target/` lock).
3. **The driver's literal `test-workspace` invocation.** If the
   pipeline still executes `cargo test --workspace` (dev profile)
   in-window it will exceed 1800 s — that kill is the known
   structural limit, **not a test failure**, and no code change
   can fix it. Per the memo: **flag it on the card for a human
   `workflow.toml` decision** — candidate changes for the human:
   point `test-workspace` at `cargo test --workspace --release`
   (warm `target/release` ⇒ minutes in-window), raise
   `timeout_sec`, or declare the collected-artifact convention —
   rather than looping the timeout through `max_verify_attempts`.
   We do not edit `workflow.toml`, and we do not substitute
   dev-profile runs of our own.

## What this does not change

- No code, test, script, golden, or doc delta — `5d1502b`
  (implementation) stands; only design files move this revision.
- The picorv32 leg stays `#[ignore]`d-deferred (`steps.md` §E) —
  both collected logs show the same 6 ignored tests, as designed.
- `workflow.toml` is not edited.
- `steps.md`'s sha join, diff audit, and golden audit are
  unchanged.

## Files

- `tests/designs/gcd/runs/` (gitignored — `release-test-workspace.log`
  copied next to the completed `cargo-test-workspace.log`;
  `cargo-test-workspace.pid`; no committed file changes in r9)
