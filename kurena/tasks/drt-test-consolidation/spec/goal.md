# drt-test-consolidation: share route re-execution across test assertions

The `tests/drt.rs` maze/emit tests re-run `route_nets` on the same
gcd inputs once per assertion group (at least four independent
full gcd routes per suite run — maze.rs tests at the `route_nets`
call sites plus the census). In dev profile a single gcd route is
the dominant cost of the workspace suite; the duplication is pure
waste. The assertions are read-only over the route result, so one
run can feed all of them.

## Scope

1. Consolidate the same-config `route_nets` gcd runs in
   `tests/drt.rs` into one shared run per fixture/config pair
   (`once_cell`/manual lazy init — check what the tree already
   uses), with the existing assertions unchanged in content.
   Runs that use genuinely different inputs (`db_blk`, non-default
   `MazeConfig`) keep their own execution — do not share across
   configs.
2. Measure honestly: record the dev-profile `cargo test -p
   grus-core --test drt` wall time before/after in
   `docs/results.md`, plus the per-run route count before/after.
3. Coverage must not shrink: same assertion count, same digest
   pins, same census joins. Prove the tests still fail when the
   route changes (red-check one digest pin by perturbing it,
   restore).

4. Make the `with_rayon_threads` env manipulation sound (card #60
   review medium): the current `THREAD_ENV_LOCK` only serializes the
   rayon-arm tests against each other, while every other test calling
   `drt_pool()` reads the environ block concurrently — the stated
   unsafe safety argument does not cover them, a panic inside the
   closure leaks the forced value, and a pre-existing
   `RAYON_NUM_THREADS` is clobbered by `remove_var` instead of
   restored. Preferred direction: pass the thread count explicitly
   (pool constructor parameter or route config) so tests stop
   mutating the process environment at all; if env manipulation is
   retained, the unsafe comment must state its real coverage, the
   closure must be panic-safe, and any pre-existing value must be
   restored. While there: the stale `route_nets` rustdoc still says
   "Sequential maze routing".

## Files

- `crates/grus-eda-core/tests/drt.rs`
- `crates/grus-eda-core/src/drt/pool.rs` / `maze.rs` (only if the
  explicit-thread-count direction is taken)
- `docs/results.md` dated entry

## Verify

- `cargo test --workspace --profile verify`
- `cargo clippy --workspace --all-targets -- -D warnings`
