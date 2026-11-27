# drt-scale-memory — overview

## Problem (external review S2-2)

During `route_nets`, every guided net's full `NetGraph` **and** its
derived `NetTopo` (CSR adjacency + attach sets + relaxed/access vias)
are held simultaneously for the entire rip-up loop, alongside the
occupancy raster and per-net rip-up mark logs. Nothing about the
footprint is measured. On picorv32 (6,744 guided nets) the process
peaks in the hundreds of MB; guide-union-heavy designs multiply both
structures linearly.

## What's actually held (read of the code)

`route_nets_with_progress` (`crates/grus-eda-core/src/drt/maze.rs`
L3917–4276 — the spec's `crates/grus-core` path is stale):

| structure | site | content | element cost |
|---|---|---|---|
| `graphs: &RouteGraphs` | built in `main.rs` L2097, borrowed whole run | per-net `NetGraph`: `nodes` 12 B, `edges` 12 B, `via_edges` 8 B, `pin_nodes` 32 B + inner | ~O(guide-union cells)/net |
| `topos: Vec<NetTopo>` | L3944–3950, kept whole run | `attach` `Vec<Vec<u32>>`, `members` `BTreeMap<u32, Vec<u32>>`, `relax` `Vec<RelaxedVia>` 20 B (≤ `RELAX_K=3` per node), `access` `Vec<Vec<AccessVia>>` 16 B, `cover: PinCover` `BTreeMap<u32, Vec<RectDbu>>`, CSR `adj_to`/`adj_eidx` 8 B/arc ×2 arcs/edge+via+relax | the large per-net term |
| `occ: Occupancy` | L3930 | `lines_x`/`lines_y` dense `Vec<Line>` (48 B + inner `ivs` 24 B/interval) | committed shapes × lines in band |
| `marks: Vec<Vec<Mark>>` | L3963 | rip-up log, `Mark` 32 B; kept whole run, `mem::take` only on rip-up | per committed interval |
| `routes`, `res_of`, `hist` | L3960–3964 | `NetRoute` (wires 24 B, vias 16 B) + `ClosureResiduals` — the required output | per net |
| `outcomes` transient | L4044–4069 | all `bad` nets' `NetOutcome` live at `collect` end — iter 1 = every net | transient peak |
| `NetRun` scratch | L2304 per attempt | ~6 `Vec`s × `n_nodes`×4 B + pins; `pq`/`bfs`/`hits` per `extend_once` round | ≤ threads in flight |
| `DesignDb`/`TechDb`/`tracks`/`vias` | whole | input | fixed base |

## Semantic constraints (decisive for the fix)

- **`NetTopo` cannot be lazily rebuilt.** `NetTopo::new` reads
  occupancy: `project_attach` runs `scan_stub` (L922) and `access_vias`
  admits `b` nodes by a live `scan_stub` (L1684). A mid-route rebuild
  would see committed `Net` marks, not the post-premark state →
  different attach sets → different routes. **All topos must be built
  against the pre-route occupancy and kept while their net can
  re-enter `bad`** — and any routed net can be re-queued by a
  provisional collision up to the last soft iteration. The review's
  "stream-and-drop" option applies to `NetGraph`, not to `NetTopo`.
- **`NetGraph` is occupancy-independent** — `build_route_graphs_ext`
  reads only `db`/`tech`/`tracks`/`join`/`HullMask` (placement data).
  A per-net rebuild is bit-identical (`NetGraph: PartialEq`), so
  graphs may be dropped after the topo pass and rebuilt per attempt.
- **`bad` order fixes the commit stream.** Re-ordering `bad` (e.g.
  largest-first to bound in-flight big graphs) changes the route —
  not byte-identical. Concurrency may only be reduced by narrowing
  the pool or chunking `bad` in order, never by sorting.
- **Post-route, `graphs` is needed only at census level**:
  `apply_routes` iterates `g.net` for the guided-net membership check
  (emit.rs L169–176); `main.rs` reads `hull_masked`, `region_escape`,
  `escapes` (L2201, L2307–2319). The node/edge payload is dead after
  `route_nets` returns — a trivial drop, but the peak is *inside*
  `route_nets`, so this trims the tail, not the headline number.

## Plan shape

1. **Instrument first** (`measurement.md`): `GRUS_DRT_MEM`-gated
   structural byte census (capacity walks + `ru_maxrss` checkpoints)
   plus `/usr/bin/time -l` peak RSS on gcd and picorv32.
2. **Pick by the split** (`options.md`): the driver chooses among —
   flatten `NetTopo`'s maps-of-vecs to CSR + pack `RelaxedVia`;
   drop-and-rebuild `NetGraph` per attempt; chunk the `outcomes`
   collect; pool the per-attempt `NetRun` scratch; occupancy/marks
   slimming if those rows dominate — measured rows decide.
3. **Gate**: `GCD_ROUTE_DIGEST` pin and committed
   `gcd.drt.ours.routed.def` byte-equality stay green; picorv32 DEF
   `cmp`-identical to the step-0 baseline (`tests.md`).
4. **Record** (`docs.md`): dated `results.md` entry with the split
   table, before/after peak RSS, and the honest bound formula.

## Files

- `crates/grus-eda-core/src/drt/maze.rs` — `NetTopo`, `Occupancy`,
  `marks`, `outcomes`, `NetRun`, census hooks
- `crates/grus-eda-core/src/drt/graph.rs` — `RouteGraphs` census fn,
  per-net graph rebuild entry (if the drop-and-rebuild option lands)
- `crates/grus-eda-core/src/drt/mem.rs` — new: structural byte census
  + peak-RSS helper (instrumentation commit)
- `crates/grus-eda-core/src/drt/emit.rs` — `apply_routes` signature
  (only if the guided-list slimming lands)
- `crates/grus-eda-core/src/drt.rs` — `mem` module export
- `crates/grus-eda/src/main.rs` — checkpoint calls, `apply_routes`
  callsite
- `crates/grus-eda-core/tests/drt.rs` — determinism pins, rebuild-
  identity test (conditional)
- `crates/grus-eda-core/Cargo.toml` — `libc` dep (getrusage)
- `Cargo.toml` — workspace version bumps
- `docs/results.md` — dated measurement entry
- `docs/e6-drt.md` — memory-model paragraph + honest bound
- `docs/status.md` — status line
