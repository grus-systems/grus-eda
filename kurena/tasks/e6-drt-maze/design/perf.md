# e6-drt-maze — Runtime record

The reviewer's "slow or stuck" question resolved as **slow** — the
`c1832dc` rewrite lands the levers below and gcd's census leg went
**203 s → 9.7 s** (measured, debug test profile). HEAD `cf34607` is
docs-only on top — same binary. picorv32 (28.4 M nodes / 54.6 M
edges, ~37× gcd's graph) is still unmeasured at design time; the
earlier partially-optimized run needed 25+ CPU-min, which the
reviewer pre-accepted if it recurs — record the measured number.

## What landed (`c1832dc`)

All semantics-preserving; `RouteDb` output bytes unchanged.

- **L1 — scratch reuse**: `edge_cost` takes `&mut Vec<Hit>` (the
  `hits` buffer), no per-relaxation alloc; `route_one_net` reuses a
  `hits` buffer for collision extraction.
- **L2 — `NetTopo` cache**: attach sets + `projected`/`attachable` +
  `host_cells` + `members` + `relaxed_vias` + CSR arcs
  (`adj_start`/`adj_to`/`adj_eidx`) built once per net in
  `route_nets`, shared across rip-up iterations. Kills the ~8×
  rebuild of 55 M arcs / 28 M `BTreeMap` inserts on picorv32.
  `NetRun` keeps per-attempt state only. Arc order byte-identical.
- **L3 — chunked skip index**: per-line `Vec<Interval>` is
  `lo`-sorted with `max_hi` per `SKIP_CHUNK = 16` chunk rebuilt
  eagerly from the dirty chunk. `scan_wire`/`scan_rect` skip chunks
  with `max_hi ≤ qlo` and stop at `lo ≥ qhi`. Hit set identical;
  consumers order-insensitive.
- **L5 — goal-directed wave** (landed beyond the original lever
  list): per round, unit-weight BFS from all remaining targets fills
  `hop`; the PQ keys `(g + hop, node)` — the wave stops at the first
  target pop instead of flooding the region. Consistent heuristic
  (every edge ≥1 hop) ⇒ same minimum-`g` target as Dijkstra.
- **L4 — profile-level tuning**: *not taken*. The debug profile
  stays stock; no `[profile.test]` override, no `#[ignore]`, no
  `max_iters` shrink — recorded per the lever rule.

## Measured numbers (debug test profile)

| run | before (`2c3c713`) | after (`c1832dc`) |
|---|---|---|
| gcd `test_maze_census` leg | 203 s (48 unrouted) | **9,704 ms** (269/269, 0 unrouted, 0 collisions) |
| gcd `test_maze_occupancy` (2 runs) | — | 17.6 s total |
| picorv32 census leg | >21 CPU-min (killed) | **unmeasured — record measured ms** |

The gcd figure includes the full `route_nets`: `premark_obs` +
`NetTopo` build + 8 iterations + `overlap_census` (0 collisions
means the census still ran). `GRUS_DRT_TIMING=1` breaks the phases
down per iteration (`bad=`/`failed=`/`prov=`/ms) — optional rerun
evidence for `results.md`.

## Budget restated

- picorv32 `test_maze_census` leg **completes** under the debug
  profile — acceptance: "completes in a test, not a timeout — state
  the measured number". The reviewer pre-accepted a 25+ CPU-min
  figure *if that is the measured number*; it must be stated
  explicitly in `docs/results.md`, not rounded or omitted.
- `test_maze_determinism` runs picorv32 twice; `test_maze_occupancy`
  is gcd-only. Total picorv32 `route_nets` invocations across the
  `drt` binary: 3 (census 1 + determinism 2). The workspace gate's
  wall time inherits that; it is the honest cost, not a reason to
  weaken tests.
- The measured per-design `ms=` from the census print goes into
  `docs/results.md` verbatim — no estimates (AGENTS §2).

## If picorv32 still misses a practical bound

Options in order (recorded, none taken yet):

1. Accept and record — the reviewer's stated fallback.
2. `GRUS_DRT_TIMING=1` to name the dominant phase in `results.md`
   (premark vs topos vs iterations vs census) — evidence, not a fix.
3. `[profile.test] opt-level` — last resort, recorded tradeoff
   (`perf.md` L4) — changes the whole suite's build/test balance;
   only with an explicit note in `docs/results.md`.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/results.md`
- `Cargo.toml`
