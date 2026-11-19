# RemoteSmoke — overview

## Purpose

Smoke-test the remote drive path of the kurena pipeline with a trivial,
zero-risk change. There is no functional intent; the change only exists to
prove that the executor can modify a file in the task worktree and have the
result reviewed.

## Change

Create `src/lib.rs` at the repository root containing exactly one line:

```rust
// remote-drive smoke test
```

(with a single trailing newline).

## Path resolution (decided)

The spec names `src/lib.rs`. The repository has no root `src/` directory —
the only `lib.rs` is `crates/grus-core/src/lib.rs`. The acceptance criterion
reads "`src/lib.rs` ends with the `// remote-drive smoke test` comment", which
is only satisfied verbatim by a file at the literal repo-relative path
`src/lib.rs`. So the implementation **creates** `src/lib.rs`; it does not
touch `crates/grus-core/src/lib.rs`.

If the card reviewer actually meant the crate root, the fix is a one-line
path change — reject with that memo rather than expanding this task.

## Impact

- `src/lib.rs` is not referenced by any workspace member (`grus-core`,
  `grus-eda`), so `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` are unaffected.
- No version bump: this is not a functional change, and the acceptance
  criterion requires that nothing else changes.
- No commit convention concerns beyond the usual: English Conventional
  Commit (`chore:` fits), agent trailer appended.

## Files

- `src/lib.rs`
