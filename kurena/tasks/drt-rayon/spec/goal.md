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
3. `route_nets` itself: the honest scheme is parallel search over
   an occupancy snapshot + deterministic serial commit in fixed
   net order (snapshot-stale collisions defer to the next
   iteration — the loop converges anyway). Searches then see a
   different occupancy than serial order would, so routed geometry
   may legitimately differ from the serial baseline: the gate for
   the loop is **deterministic given a fixed thread count +
   reproducible run-to-run + collision-free end state**, not
   byte-identical-to-serial (that requirement degenerates to
   serial). A scheme that keeps literal serial-equivalence, or one
   that can't keep even fixed-thread determinism, is a non-goal —
   record which. Disjoint-region batching is allowed if coverage
   is measured honestly on our designs.
4. Determinism gate per scope item: build/emit must be
   byte-identical to serial (set `RAYON_NUM_THREADS=1` in a test);
   the route loop's bar is the weaker-but-honest contract above —
   pin the net ordering any parallel fold depends on either way.
   Note ordering: this lands after `drt-occupancy-ds` — the scan
   share (~70% of route CPU) shrinks the serial critical path
   before threads multiply it.
5. `rayon` becomes a real dependency — add via `cargo add` with a
   pinned version, not a floating range.

## Files

- `crates/grus-eda-core/src/drt/graph.rs`, `drt/maze.rs`,
  `drt/emit.rs` (as scoped)
- `crates/grus-eda-core/Cargo.toml` (rayon)
- `crates/grus-eda-core/tests/drt.rs` (determinism pin incl. forced
  single-thread)
- `docs/results.md` dated entry (phase timings before/after)
