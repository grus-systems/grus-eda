# drt-rayon: parallel route/build/emit

External review: `route_nets` is single-threaded; occupancy is
shared so naive per-net rayon is unsafe, but graph construction
(per-net, read-only inputs) and emit (per-net geometry assembly)
parallelize freely, and even the maze loop may admit
parallel-search-then-serial-commit or snapshot-occupancy schemes.

## Scope

1. Profile first — name the phase split (build_route_graphs /
   route_nets iterations / emit+selfdrc) with timings on gcd;
   parallelize where the split pays, not everywhere.
2. `build_route_graphs` per-net construction is the safe first
   target (independent per net). Emit/next.
3. `route_nets` itself: only if a scheme keeps the shared-occupancy
   invariant honest (e.g. disjoint net batches on disjoint
   occupancy, or collect-then-commit); otherwise record it a
   non-goal with the reason.
4. Determinism is the gate: routed geometry must be byte-identical
   to the serial implementation regardless of thread count (set
   `RAYON_NUM_THREADS=1` in a test to prove equivalence; pin the
   ordering any parallel fold depends on).
5. `rayon` becomes a real dependency — add via `cargo add` with a
   pinned version, not a floating range.

## Files

- `crates/grus-core/src/drt/graph.rs`, `drt/maze.rs`,
  `drt/emit.rs` (as scoped)
- `crates/grus-core/Cargo.toml` (rayon)
- `crates/grus-core/tests/drt.rs` (determinism pin incl. forced
  single-thread)
- `docs/results.md` dated entry (phase timings before/after)
