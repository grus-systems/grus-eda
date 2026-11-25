# e6-drt-e2e — verify-stage timing (r10, bound raised; the memo's only item)

> **r10 revision driver** — the human answered the `workflow.toml`
> question r9 flagged. Root cause was **config, not code**:
> `timeout_sec` was 1800 while dev-profile `cargo test --workspace`
> needs ~2200 s on this host (`drt.rs` alone measured 2196 s — the
> 1800 s window structurally could not fit the suite). The human
> raised it to **7200 on dev (`63735c5`)** — the main checkout's
> `workflow.toml` now carries the bound.
>
> **Where the bound actually comes from** (verified against kurena
> source, `kurena-local`): `Ctx::open` resolves the repo via
> `git rev-parse --show-toplevel` in the invocation cwd and
> requires `.kurena/` there, so drives always run against the
> **main checkout** (the worktree has no `.kurena/`); `Runner`'s
> per-invocation timeout is `cfg.kurena.timeout_sec` from the main
> checkout's `workflow.toml` — already 7200. The worktree's
> `workflow.toml` is a branch file the driver never consults: it
> still reads 1800 and that is fine. (The human's uncommitted
> worktree mirror was present during planning-1, then restored to
> the branch's committed 1800 by the planning guard —
> `revert_outside` undoes every change outside `design/` at stage
> end.) **Do not re-apply or commit a worktree `workflow.toml`
> edit**: a dirty file at a checkpoint is swept into the branch by
> `commit_all` (`git add -A`) and then collides textually with dev
> `63735c5` at merge — same line rewritten differently (dev's
> carries the 3-line comment). Leaving the branch file untouched
> merges clean and takes dev's 7200.
>
> Directive: **re-run verify under the new bound — nothing else to
> implement.** The r9 collect-only procedure is superseded.

## Bound math

- `drt.rs` alone: 2196 s measured (the completed r8 detached run).
  The other five binaries add seconds-to-minutes — whole-suite wall
  time ~2200–2400 s, comfortably inside 7200.
- Dev profile is back in-window; the r9 `--release` workaround
  (~121 s test time) is no longer needed as primary evidence.

## Verify-time procedure (r10 — normal in-window run)

1. **`cargo test --workspace`** — the driver's literal dev-profile
   `test-workspace` command now runs in-window under
   `timeout_sec = 7200`. Expect ~40 min wall; green shape is
   **601 passed / 0 failed / 6 ignored** (the 6 = the designed
   picorv32 deferrals — a `--ignored` run fails loudly on the
   absent golden by design).
2. **`cargo clippy --workspace --all-targets -- -D warnings`** —
   the second `[[verify]]` command, in-window, clean.
3. **Corroborating logs already on disk** — cite, don't re-derive:
   - `tests/designs/gcd/runs/cargo-test-workspace.log` — the r8
     detached **dev-profile** suite, 601/0/6, every binary `ok`
     (post-`5d1502b` tree).
   - `/tmp/e2e-ws-release-test.log` — the r9 detached **release**
     suite, 601/0/6. May be copied to
     `tests/designs/gcd/runs/release-test-workspace.log`
     (gitignored) as a card artifact — optional now that the
     in-window dev-profile run is the primary evidence.
4. **If the suite still hits the bound** — 7200 is ~3× the measured
   need, so a timeout now means a real regression or machine-load
   anomaly, **not** the r9 structural misfit: capture the log tail
   and report; do not fix-loop or re-touch `workflow.toml`.

## What this does not change

- No code, test, script, golden, or doc delta — implementation
  head `5d1502b` stands; only design files move this revision.
- The picorv32 leg stays `#[ignore]`d-deferred (`steps.md` §E) —
  the same 6 ignored tests are the designed state.
- `workflow.toml` is not touched anywhere. The driver reads the
  main checkout's copy (7200); the worktree's stays at the
  branch's committed 1800, and merge into dev resolves the line
  to 7200 cleanly (mechanism note above).
- `steps.md`'s sha join, diff audit, and golden audit are
  unchanged.

## Files

- `tests/designs/gcd/runs/` (gitignored — verify-time logs only;
  no committed file changes in r10)
