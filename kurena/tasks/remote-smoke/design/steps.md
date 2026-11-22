# RemoteSmoke — steps

Single-step task. One commit.

## 1. Create `src/lib.rs` and commit

- Create `src/lib.rs` at the repository root (new file — the repo currently
  has no root `src/` directory) with exactly this content:

  ```rust
  // remote-drive smoke test
  ```

  One line, one trailing newline, nothing else.
- Do not modify any other file. In particular do **not** touch
  `crates/grus-core/src/lib.rs`, `Cargo.toml` (no version bump — not a
  functional change), or anything under `kurena/`.
- Verify: `tail -1 src/lib.rs` prints `// remote-drive smoke test`, and
  `git status --porcelain` shows only `?? src/lib.rs` (plus the committed
  `kurena/tasks/remote-smoke/` pipeline artifacts, which are expected and
  not part of this change).
- The workflow verify commands (`cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`) should pass
  unchanged since the new file belongs to no crate; no need to run anything
  beyond the pipeline's own verify.
- Commit with `chore:` subject, English, agent trailer, e.g.
  `chore: Add remote-drive smoke test marker`. Stage `src/lib.rs` explicitly
  (never `git add -A`).

## Files

- `src/lib.rs`
