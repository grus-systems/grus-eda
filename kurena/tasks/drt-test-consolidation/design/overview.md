# drt-test-consolidation — overview

## Problem

`crates/grus-eda-core/tests/drt.rs` re-executes `route_nets` on the
identical gcd input bundle once per assertion site. A dev-profile
gcd route is the dominant cost of the whole workspace suite (the
kurena `workflow.toml` timeout comment records ~2196 s for the
dev-profile drt tests alone on an M4 host); the duplicated runs are
pure waste because every consumer only *reads* the resulting
`RouteDb` (plus the immutable input bundle).

Counted call sites today (gcd, `LOADED_OPTS`, non-ignored tests):

| execution | runs |
|---|---|
| `maze_census("gcd")` | 1 |
| `maze_determinism("gcd")` | 2 |
| `test_maze_occupancy` on-arm (`MazeConfig::default()`) | 1 |
| `test_rayon_route_determinism` (forced threads 1/8/8) | 3 |
| `test_net_graph_rebuild_identity` (forced threads 1/8) | 2 |
| `emit_leg("gcd", default)` callers ×10 | 10 |
| **default-config total** | **19** |
| `test_maze_occupancy` off-arm + `test_emit_fail_proof` arm B (`OccMode::Disabled`, identical pair) | 2 |
| **gcd total per suite run** | **21** |

`#[ignore]`d picorv32 legs (`maze_census`, `maze_determinism`,
`emit_gate`) run 4 default-config routes when invoked with
`--ignored`.

## Approach

Introduce a lazily initialized, process-shared route result — one
`std::sync::OnceLock` cell per `(fixture, config)` pair — holding
the input bundle and its `RouteDb`. Tests borrow
`&'static RoutedLeg` instead of re-loading and re-routing.
`OnceLock` matches the existing tree convention (`src/timing.rs`
uses `OnceLock`); no new dependency.

After consolidation, gcd route executions per suite run: **21 → 9**:

- 1 shared default run (`GCD_LEG`)
- 1 shared occupancy-disabled run (`GCD_OCC_OFF_LEG`)
- 7 retained fresh runs that are themselves the thing under test
  (two determinism second-executions, three forced-thread-count
  arms, two streamed-rebuild arms)

picorv32: 4 → 2 (shared default + one determinism
second-execution).

## Invariants (must not weaken)

- Same assertion count — the edit only re-points `r.`/`db`/`join`
  references; no `assert` is removed. `rg -c 'assert' tests/drt.rs`
  must be identical before/after.
- `GCD_ROUTE_DIGEST` pin, unrouted/census joins, golden compares —
  unchanged; the shared run is bit-identical to any per-test run
  (determinism is already pinned by `test_maze_determinism` and
  `test_rayon_route_determinism`).
- Determinism checks must compare **two independent executions** —
  shared + one fresh run, never shared vs itself (a reflexive
  `assert_eq!` would be vacuous).
- Distinct inputs never share: `GRAPHS_OPTS` fixture runs
  (`spare_above: 0`, synthetic LEF/DEF — microsecond-cost anyway),
  forced `RAYON_NUM_THREADS` arms, `db_blk`/custom DEF arms, and
  non-default `MazeConfig` keep their own `route_nets` calls.
- Red-check evidence: perturbing `GCD_ROUTE_DIGEST` must still turn
  `test_maze_census` red (recorded in `docs/results.md`).

## Non-goals

- No `src/` changes — test-file-only refactor plus the results log.
- No sharing of `RouteGraphs`: `route_nets` rebuilds per-net graphs
  internally, so `RoutedLeg` skips `build_route_graphs_ext`
  entirely (extra savings; `test_mem_census_sanity` and
  `test_net_graph_rebuild_identity` keep `load_routed_inputs` /
  explicit builds).
- No reduction of the ignored picorv32 coverage — same asserts,
  same mechanism.

## Files

- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `Cargo.toml` — workspace `version` patch bump in the same commit
  (AGENTS §5; test-only refactor, no API change)
