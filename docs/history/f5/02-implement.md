# Ticket 02 (F5): spatial index for legalizer refinement pass 1

## Goal

Implement `docs/history/f5-pass1-speed.md` including the review resolutions in
its Section 5 (external review): replace
pass 1's all-pairs scan with a per-round spatial index (bands by sub-row y,
entries `(x, movable index, cell)` sorted by `(x, movable index)`), candidates
collected per cell into a reusable scratch Vec, filtered to movable index
greater than the current cell and the same width, sorted by movable index,
and re-checked against live `design_db` positions exactly as the current
inner loop does. Accepted swaps update the index (remove both cells first,
then insert both). A hard `assert!` at index build enforces that every
movable cell's y is a band y, with the invariant documented in a comment.

## Files

- `crates/grus-core/src/placement/legalize.rs` (pass 1 of the refinement
  rounds, plus one unit test)
- `docs/results.md` (F5 follow-up entry: before/after legalization times on
  gcd and picorv32 as far as this machine allows, and what remains for the
  maintainer's mini acceptance)
- `Cargo.toml` (version bump; this is a functional change) and `Cargo.lock`
  (version lines only)

Current version is 0.1.22, so the bump is to 0.1.23.

## Interface

No public API changes. `legalize_placement` keeps its signature and its
result. Passes 2 and 3, the resync, the round count (4), and all thresholds
(25,000 dbu, 4 site heights, equal width only) are untouched.

## Acceptance

Run by you (this machine has no PDK; the PDK-dependent tests fail on missing
files here — expected, report them as such):

- [ ] New unit test (synthetic, no PDK): a layout where a cell's beneficial
      equal-width swap partner is far away in `movable_cells` order (more
      than half the list apart) and inside the spatial window, asserting the
      swap happens; and a case where the only same-width partner is outside
      the window, asserting no swap. Prove the test can fail (e.g. make the
      index return only index-neighbours, see red, restore) and say so in
      the job report.
- [ ] Existing PDK-free placement tests pass (`cargo test -p grus-core
      --lib -- placement::` filtered to the ones not needing the PDK; name
      them in the report), and the whole `legalize` module compiles with the
      existing tests unchanged.
- [ ] `cargo clippy -- -D warnings` and `cargo build --release` pass.

Run by the maintainer on the Mac mini from your patch:

- `place` on picorv32 and gcd produces a placed DEF byte-identical to the
  pre-change baseline (md5 recorded by the maintainer).
- Legalization elapsed on picorv32 at most 450 ms (from 1,009 ms).
- Full `cargo test --release` green.

## Notes

- Design first: if the byte-identity check on the mini fails, the maintainer
  will send the first differing cell back; do not adjust thresholds or round
  counts to make it pass.
- The candidate scratch Vec, the `cell -> movable index` array, and the band
  lists are all rebuilt per round; nothing persists across rounds.
- Do not commit; the maintainer commits after the mini acceptance run.
