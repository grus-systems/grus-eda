# Call-site dispositions

Every `route_nets` / `emit_leg` / `load_routed_inputs` site in
`tests/drt.rs`, with its disposition. Line numbers are pre-edit.

## Shared — `gcd_leg()` (gcd, `LOADED_OPTS`, `MazeConfig::default()`)

| test | line | change |
|---|---|---|
| `test_maze_census` → `maze_census("gcd")` | 2294 | `let leg = leg_for(name)`; `r.` → `leg.route.`, `db`/`join` → `leg.*`; `ms=` → `leg.route_ms`; digest pin becomes `leg.name == "gcd"` |
| `test_maze_occupancy` on-arm | 2459 | `on` = `gcd_leg().route`; censuses reuse `gcd_leg()`'s tech/tracks/vias (identical inputs to the off-leg) |
| `test_maze_determinism` → `maze_determinism` r1 | 2512 | r1 = `leg.route`; r2 = `route_fresh(leg)` — still two independent executions |
| `test_rayon_route_determinism` inputs | 2710 | `let leg = gcd_leg()` at top level (ambient env); the three `route_nets` calls inside `with_rayon_threads` stay fresh on `&leg.*` inputs |
| `test_emit_roundtrip_gcd` | 2984 | `emit_leg(gcd_leg())` |
| `test_emit_selfdrc_gcd` → `emit_gate("gcd")` | 2902 | `emit_gate(gcd_leg())` |
| `test_emit_fail_proof` arm A | 3066 | `emit_leg(gcd_leg())`; mutation on the returned owned `db` |
| `test_emit_determinism` | 3137 | pass 1 = `emit_leg(leg)`; pass 2 = `route_fresh(leg)` + clone/apply/write |
| `test_rayon_emit_serial_identity` | 3164 | `leg = gcd_leg()` at top level; `route_nets` dropped — the shared `leg.route` feeds both `apply_routes` arms (apply is what runs under forced threads) |
| `test_preemit_gcd` | 3216 | `emit_leg(gcd_leg())` |
| `test_preemit_foreign_pin` | 3270 | `emit_leg(gcd_leg())`; injected wire pushed into the returned owned `db` |
| `test_e2e_ours_def_deterministic_gcd` | 3710 | `emit_leg(gcd_leg())` |
| `test_emit_via_default_consistency` | 3738 | `emit_leg(gcd_leg())`; `vias` → `&leg.vias` |

## Shared — `gcd_occ_off_leg()` (gcd, `OccMode::Disabled`)

| test | line | change |
|---|---|---|
| `test_maze_occupancy` off-arm | 2469 | `off` = `gcd_occ_off_leg().route` |
| `test_emit_fail_proof` arm B | 3113 | `emit_leg(gcd_occ_off_leg())` |

Two formerly independent executions of the same `(gcd, occ-off)`
pair consolidate to one — allowed: identical inputs, identical
config.

## Kept fresh — distinct inputs or the mechanism under test

| test | line | reason |
|---|---|---|
| `test_maze_cases` (4 runs) | 1619–1700 | synthetic `MAZE_LEF`/`maze_db`, `GRAPHS_OPTS`, `db_blk` variant |
| `test_route_nets_progress_hook` (2 runs) | 1746, 1779 | hooked-vs-unhooked *is* the check; synthetic fixture |
| `test_maze_guide_escape` (7 runs) | 1896–2102 | per-arm `EscapeCfg`/`GraphOpts`, wall/occ DEF variants |
| `test_maze_pin_collision` (2 runs) | 2149, 2178 | `MAZE_PIN_TAIL` fixture |
| `test_emit_unrouted_fixture` | 3338 | `MAZE_DEF_TAIL` fixture |
| `test_net_graph_rebuild_identity` (2 runs) | ~2670 | streamed census under forced `RAYON_NUM_THREADS` 1 vs 8 — forced env is the input |
| `test_rayon_route_determinism` (3 runs) | 2714 | forced thread counts 1/8/8 are the arms' inputs |
| `maze_determinism` r2 | 2515 | the second independent execution the assert compares |
| `test_emit_determinism` pass 2 | 3138 | same |

## Shared — `leg_for("picorv32")` (all `#[ignore]`)

| test | change |
|---|---|
| `test_maze_census_picorv32` | `leg_for(name)` via `maze_census` |
| `test_maze_determinism_picorv32` | shared r1 + `route_fresh` r2 |
| `test_emit_selfdrc_picorv32` | `emit_gate(leg_for("picorv32"))` |

## Unchanged

- `test_mem_census_sanity` (2568) — needs `RouteGraphs`; keeps
  `load_routed_inputs("gcd")` (no route).
- `test_net_graph_rebuild_identity`'s input build — keeps
  `load`/`load_guide`/`build_route_graphs_ext`/`via_table`; only its
  two streamed routes stay (listed above).
- `load_routed_inputs` itself — retained for the graphs consumers.
- All synthetic-fixture maze/emit tests (µs-cost), all pure-JSON
  golden tests (`ours_reference_gate`, digest fail-proofs), and the
  e2e reference gates — no `route_nets` involvement.

## Execution census

| | before | after |
|---|---|---|
| gcd default-config routes | 19 | 1 shared + 7 fresh* |
| gcd occ-off routes | 2 | 1 shared |
| **gcd total** | **21** | **9** |
| picorv32 (`--ignored`) | 4 | 2 |

*fresh: determinism second runs (2: `maze_determinism`,
`test_emit_determinism`) + rayon route arms (3) +
streamed-rebuild arms (2) — all same-config but distinct
*purpose*: each retained run is itself the mechanism under test
(second execution or forced thread count).
