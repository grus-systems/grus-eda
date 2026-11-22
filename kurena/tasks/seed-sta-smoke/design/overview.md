# seed-sta-smoke — Overview

## Goal

Prove the kurena seed + host-runner verify path end to end: a task worktree
must receive the gitignored `tests/designs/` generated artifacts via
`worktree_seed`, and the `sta-regression` verify must pass inside the
pipeline — not just in a hand-made worktree.

The code change is deliberately a no-op: append the one-line comment
`// seed/STA smoke test` at the end of `crates/grus-core/src/lib.rs`
(see "Path note" below). Nothing else changes.

## What is actually under test

Not the comment — the pipeline machinery around it:

1. **`worktree_seed`** (`workflow.toml`): `["tests/designs/"]` copies the
   gitignored generated artifacts from the main checkout into each task
   worktree. Without them the STA golden tests panic via
   `test_support::require` (loud failure, no silent skip).
2. **`sta-regression` verify** (`workflow.toml`):
   `cargo test --release -p grus-core sta` with `runner = "eda"` — the
   `eda` host token restricts `push --remote` to hosts advertising the
   capability (PDK at `$PDK_ROOT`/`~/.ciel`, CPU for a release build).
   In a local worktree push it just runs on this host.
3. **Test coverage of the filter `sta`**: matches the `sta::` module
   tests plus every `*_matches_opensta` golden comparison
   (`test_picorv32_*`, `test_gcd_*`, `test_aes_*`, `test_jpeg_*`,
   `test_ibex_*`, `sta_edge`/`sta_exceptions` fixtures) and the two
   `power.rs` `*_matches_opensta` tests. These read
   `tests/designs/<name>` via `CARGO_MANIFEST_DIR/../../tests/designs`
   — i.e. the worktree's own copy, which is exactly what the seed must
   populate.

## Evidence already observed in this worktree

This planning stage itself runs inside `.kurena/wt/seed-sta-smoke`, so the
seed already ran here:

- Gitignored artifacts are present: `picorv32.synth.v`,
  `picorv32.sta.reference.json`, `picorv32.routed.spef`,
  `jpeg.sta.reference.json`, `aes.synth.v` (all confirmed via
  `git check-ignore -v`).
- `du -sh tests/designs/` → 255 MB; `git status --short` is clean (the
  seeded files are ignored, as intended).
- `~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd` exists on this host, so the
  PDK half of the `eda` capability is satisfied locally.

So the expected outcome is: the trivial edit lands, `sta-regression`
runs in-pipeline, and passes. If it fails, the failure mode is
diagnostic — see `steps.md` §"Failure reading".

## Path note

The spec writes `src/lib.rs`; the repository has no root `src/` — the
only `lib.rs` is `crates/grus-core/src/lib.rs`, which is also the crate
the verify command exercises (`-p grus-core`). Interpret `src/lib.rs` as
`crates/grus-core/src/lib.rs`. Do **not** create a stray `src/lib.rs` at
the repo root — it would be an untracked, unreferenced file and a
different change than intended.

## Non-goals

- No version bump: a trailing comment is not a functional change
  (AGENTS.md §5 ties bumps to functional changes).
- No docs/results.md entry: nothing was measured against a reference
  tool.
- No changes to `workflow.toml`, `.gitignore`, tests, or fixtures.

## Files

- `crates/grus-core/src/lib.rs`
