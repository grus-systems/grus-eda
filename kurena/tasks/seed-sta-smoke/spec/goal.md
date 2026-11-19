# Goal

Smoke-test the seed + host-runner verify path end to end: a task
worktree must receive the gitignored `tests/designs/` generated
artifacts via `worktree_seed`, and the `sta-regression` verify
(`cargo test --release -p grus-core sta`, `runner = "eda"` host
token) must pass inside the pipeline — not just in a hand-made
worktree.

The code change itself is deliberately trivial: add a one-line
comment `// seed/STA smoke test` at the end of `src/lib.rs`
(create no other change).

## Files

- src/lib.rs

## Verify

sta-regression
