# seed-sta-smoke — Steps

## Step 1 — Append the comment

`crates/grus-core/src/lib.rs` is currently 22 lines ending with:

```rust
pub use placement::*;
pub use readers::*;
pub use tech_db::*;
```

Append one line so the file ends with:

```rust
pub use placement::*;
pub use readers::*;
pub use tech_db::*;

// seed/STA smoke test
```

Keep the blank line between the `pub use` block and the comment, and a
trailing newline at end of file. No other edit anywhere.

## Step 2 — Commit

One commit, explicit path staging only (never `git add -A`):

- `crates/grus-core/src/lib.rs`
- `kurena/tasks/seed-sta-smoke/` (design docs + spec)

The pipeline's commit convention is `kurena: implement seed-sta-smoke`
with the agent trailer. A plain `test:`/`chore:` conventional message is
acceptable if the driver commits manually. No version bump
(non-functional change).

## Verify — what `sta-regression` exercises

`cargo test --release -p grus-core sta` inside the task worktree. The
matching tests read:

- `tests/designs/**` — gitignored artifacts delivered by
  `worktree_seed` (already verified present in this worktree),
- `$PDK_ROOT` or `~/.ciel` — the sky130 PDK, host capability `eda`.

Tests assert against committed `*.reference.json` goldens; nothing
shells out to Docker/OpenSTA inside the test run.

## Failure reading

If `sta-regression` fails in-pipeline, the panic/assertion tells you
which layer broke — do not patch code to make it pass:

- `required test input missing: <worktree>/tests/designs/...` → the
  seed did not deliver `tests/designs/` into the worktree. That is the
  pipeline defect this smoke test exists to catch; report it, do not
  "fix" it by regenerating artifacts.
- `cannot locate the PDK (set PDK_ROOT)` → the verify ran on a host
  without the `eda` capability (no `~/.ciel`). Runner-selection
  problem, not a code problem.
- A golden assertion mismatch → a real STA regression on `dev`; report
  the failing test and diff verbatim.

## Definition of done

- `crates/grus-core/src/lib.rs` ends with `// seed/STA smoke test`.
- `sta-regression` verify reports pass in the pipeline run.
- Diff contains no other change.
