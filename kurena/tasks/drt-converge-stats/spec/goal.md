# drt-converge-stats: iteration/convergence statistics

External review: `MAX_ITERS=8` with a hard final iteration means a
net that still fails (or still collides provisionally) at the cap
is permanently unrouted — and the operator cannot see how close
the run was to converging. Per-iteration visibility is what turns
"8 iters wasn't enough" from a guess into a measurement.

## Scope

1. Record per-iteration counts in `RouteDb` (or a sibling stats
   struct): nets attempted, nets newly routed, provisional
   collision count at iteration end. Keep it small — one
   fixed-size array or a `Vec` capped at `max_iters`.
2. Print a compact convergence line in the `== drt emit:` census
   (e.g. `iter_progress=6700,410,38,6,0,...` one number per
   iteration: bad-set size entering that iteration) — enough to
   see whether the tail was still falling or flatlined.
3. Include the stats in the serialized digest (they are
   deterministic outputs of the run) — update the digest test
   expectation in the same commit.
4. No new output channel: extends the existing census block, does
   not overlap `drt-progress`'s periodic stderr line.

## Files

- `crates/grus-core/src/drt/maze.rs` (`RouteDb` + collection at
  the iteration loop + digest)
- `crates/grus-eda/src/main.rs` (census print)
- `crates/grus-core/tests/drt.rs` (digest/print expectations)
- `docs/results.md` dated entry

Note: queue after `e6-drt-e2e` lands — it is rewriting `maze.rs`.
Related to `drt-progress` (periodic vs. final output — do not
merge them).
