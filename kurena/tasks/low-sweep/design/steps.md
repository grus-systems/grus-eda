# low-sweep — implementation steps

Order is code-first, docs-second: the code edits are what the test gates
check, and the doc edits are pure text with no gate. One commit at the
end (acceptance: version bump in the same commit).

## 1. Code nits

`code-nits.md` C1–C4, in file order:

- `sta/analysis.rs` — extract `slew_eval_state`, rewire
  `sta_pin_slews` + `assert_pin_slew_parity`.
- `sta/dmp.rs` — split the `memo_table` borrow.
- `placement/legalize.rs` — `new_y` contract comment, `pin_count`
  gates in the three accept loops, `debug_assert` the pass-1 width
  invariant.
- `bool_expr.rs` — one doc line on `sensitivity`.

Check: `cargo test -p grus-core` + `cargo clippy -p grus-core
--all-targets -- -D warnings`. The slew-parity tests
(`..._parity_committed_designs`, `..._latch_borrow_fallback`) and
`test_incremental_extrema_matches_full_rebuild` are the direct
regression coverage for C1 and C3(b).

## 2. `LibSeq` parser tests

`libseq-tests.md` — four tests in `readers/liberty.rs` `mod tests`.
Run `cargo test -p grus-core readers::liberty` (module path
`readers::liberty::tests`); all four must be named in the output —
no silent skips.

## 3. Doc fixes

`docs-fixes.md` R1–R7 + K-items — pure text edits to `docs/results.md`
and the six kurena design docs. After editing, `git diff` each hunk
against its item: replacements are one-for-one sentences/tables, no
other drift. The R6 table is precomputed — paste it, do not re-measure.

## 4. Version bump

`Cargo.toml` `[workspace.package] version` 0.3.7 → 0.3.8;
`cargo check` to refresh `Cargo.lock`. Stage `Cargo.toml` + `Cargo.lock`
explicitly (never `git add -A`).

## 5. Verify

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --release -p grus-core sta` (the sta-regression leg —
  needs the seeded `tests/designs/` artifacts; worktree is seeded)
- Golden identity: `git status --short tests/designs/` is empty —
  nothing under it may be touched. No `cmp` runs needed: the change is
  provably output-neutral (comments, a dead guard removal, a borrow
  split, a shared dispatch, test additions, doc text).

## 6. Commit

One commit, e.g. `refactor: Sweep low-severity review findings from
perf and activity tickets`, body listing the disposition table's fixes
(one bullet per R/C item) plus the version bump line, agent trailer per
AGENTS §5. The R6 note about `sta.prepare.clock.walk` and the R4
precision note are doc-text, not commit-message material.
