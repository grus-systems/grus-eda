# async-latch-delay — Remaining steps (post-review fixup)

The implementation landed as `e530643` and passed its own verification
(`docs/results.md` "path delays on async-check and latch-D endpoints
(v0.1.95)": 32 `sta_edge` tests STRICT-green, workspace suite green,
clippy clean, fail-proof recorded). The human review rejected on a
single docstring sign error. Everything below is that delta — do not
touch fixtures, goldens, docs, or the version.

## Step 1 — Fix the docstring

`crates/grus-core/src/sta/analysis.rs:7754`, the doc comment on
`test_sta_edge_latch_borrow_delay_max` currently reads:

```rust
/// `[N + lat0 - unc, + pulse + t_setup]` on the max side and
```

The bound is `pulse - t_setup`, not `pulse + t_setup` — the compose
comment at ~3609 (`max_borrow = pulse - t_setup`), `docs/path-delays.md`
§1 (`base + pulse - t_setup`), and the `delay_allregs` golden
(`u_ln2/D`: `0.9 + 3.4 - 0.1645 = 3.235511`) all agree. Minimal fix:
change `+ pulse + t_setup` to `+ pulse - t_setup`. Nothing else in the
docstring moves.

## Step 2 — Optional fmt nit

`cargo fmt` **only** the hunks `e530643` introduced — the review marked
this "if cheap". `analysis.rs` has pre-existing non-fmt-clean spots (17
flagged before the commit; `rustfmt --edition 2024 --check` shows them);
reformatting the whole file adds unrelated churn. rustfmt's
`--file-lines` range mode is nightly-only, so either hand-apply the
suggested formatting inside the new hunks (`eval_launch` ~2488, the
borrow/`disabled_borrow` edits ~2678/2850, req compose ~3453-3715,
`Worst` ~4072, test block ~7685) or skip — do not let this step touch
pre-existing code.

## Step 3 — Verify & commit

- `cargo test -p grus-core sta_edge` — comment-only change, still cheap.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- One `docs:` commit, explicit path staging, agent trailer. No version
  bump: the change is comment-only, and `0.1.95` already shipped in the
  same commit as the functional change (acceptance criterion met).

## Full procedure (executed — kept for the record)

Step 0 (fixtures + committed `generate_ref.sh` regen in the container),
Step 1 (`api.md` implementation), Step 2 (11 STRICT `compare_sta_edge`
tests + fail-proof), Step 3 (`docs/results.md` entry), Step 4
(`docs/path-delays.md` §1/§3/§5 + `0.1.95` bump) all ran in `e530643`;
the regenerated goldens agreed with `probe.md`'s record. If a future
change invalidates the fixture, rerun those steps in that order —
evidence first (`goal.md` "Measure first"), implementation second.
