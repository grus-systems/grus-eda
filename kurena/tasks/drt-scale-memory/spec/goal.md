# drt-scale-memory: memory at scale

External review S2-2: every guided net's full route graph plus a
per-net `NetTopo` (CSR adjacency) is held simultaneously for the
whole run — guide-union-heavy designs multiply both. Nothing is
measured; picorv32 already peaks in the hundreds of MB.

## Scope

1. Measure first: peak RSS of `grus-eda drt` on gcd and picorv32
   (`/usr/bin/time -l` or equivalent), plus the per-structure split
   (route graphs vs NetTopo vs occupancy vs design_db) — a counter
   or allocation hook is fine; record real numbers.
2. Decide the fix by measurement: stream-and-drop graphs after
   routing (rebuild for emit), share immutable graph structure
   across nets, arena/pool the NetTopo allocations, or cap
   concurrency on the biggest graphs — driver picks after seeing
   what dominates.
3. Correctness gate: routed output byte-identical; the reduction
   must not trade determinism for memory.
4. Honest bound in docs: state the measured peak before/after and
   the design size where the new bound would still be a concern
   (e.g. linearity constant × net count).

## Files

- `crates/grus-eda-core/src/drt/` (as scoped by the measurement)
- `docs/results.md` dated entry (RSS before/after, split table)
