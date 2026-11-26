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

## Files

- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md` dated entry

## Verify

- `cargo test --workspace --profile verify`
- `cargo clippy --workspace --all-targets -- -D warnings`
