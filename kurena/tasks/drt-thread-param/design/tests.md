# tests — `tests/drt.rs` refactor

Two kinds of edits: (a) delete the env-mutation apparatus and rewrite
the four forced-thread arms to pass counts explicitly; (b) append
`threads` / `0` at every other call site of the three changed
functions. Assertions, fixtures, and digest pins are untouched.

## Deletions

- `THREAD_ENV_LOCK` and `with_rayon_threads` (L2669–2682) — the
  static, the helper, and its doc comment go entirely.
- The pinning comments that justified env control:
  - Section header (L2656–2666): rewrite — the drt parallel sections
    run inside a per-call pool whose size is an explicit `threads`
    input (`MazeConfig.threads` for the route legs, a `threads`
    parameter on `build_route_graphs_ext`/`apply_routes`/
    `write_routed_def`); `0` is rayon's default. Keep the existing
    sentences about the route loop's contract (determinism under a
    fixed count, collision-free end state, serial identity a named
    non-goal) — they are still true.
  - `test_rayon_route_determinism` L2775–2778 ("a `*_leg()` call
    inside a `with_rayon_threads` closure would pin the shared
    route's thread count"): the hazard it describes no longer
    exists — delete; the leg still resolves first because its route
    is shared, not because of pinning.
  - `test_rayon_emit_serial_identity` L3248–3251: same deletion; the
    shared `leg.route` still feeds both apply arms.

## The four forced arms

Same assertions, same digest pins — only the count's path changes.

**`test_rayon_graphs_serial_identity`** (L2689): wrap the
`build_route_graphs_ext` call in a `|threads: usize|` closure that
passes `threads` as the new trailing argument; compare `graphs(1)`
vs `graphs(8)` — the same `{:?}` full-Debug equality as today:

```rust
let graphs = |threads: usize| {
    format!(
        "{:?}",
        grus_eda_core::drt::graph::build_route_graphs_ext(
            &db, &tech, &tracks, &join,
            grus_eda_core::drt::graph::GraphOpts {
                spare_above: 1,
                escape: Some(grus_eda_core::drt::graph::EscapeCfg::DEFAULT),
            },
            threads,
        )
    )
};
assert_eq!(graphs(1), graphs(8), "graph build diverged under 8 threads");
```

**`test_net_graph_rebuild_identity`** (L2720): the bulk reference
build passes `0` (`build_route_graphs_ext(..., opts, 0)`). The
streamed `for threads in [1, 8]` loop keeps its shape; the count now
rides the config: `&MazeConfig { threads, ..MazeConfig::default() }`
in place of `&MazeConfig::default()` — no closure, no lock.

**`test_rayon_route_determinism`** (L2774): the `route` closure
becomes

```rust
let route = |threads: usize| {
    route_nets(
        &leg.db, &leg.tech, &leg.tracks, &leg.join,
        LOADED_OPTS, &leg.vias,
        &MazeConfig { threads, ..leg.cfg },
    )
};
```

(`leg.cfg` is `Copy`; the update syntax keeps `occ`/`max_iters`.)
`route(1)`, `route(8)`, `route(8)` — identical assertions.

**`test_rayon_emit_serial_identity`** (L3247): the forced env
wrappers become literal parameters — `apply_routes(..., &leg.vias, 1)`
into `db1`, `apply_routes(..., &leg.vias, 8)` into `db2`;
`write_routed_def(&db1, tech, &placed, &out1, 1)` and
`write_routed_def(&db2, tech, &placed, &out8, 8)`. The report-field,
`net_geoms`, and DEF-byte equality assertions are unchanged.

## Mechanical sites (non-forced, pass `0`)

Every other caller of the three changed functions gets `0` — the
current default behavior:

| function | sites (current line numbers) |
|---|---|
| `build_route_graphs_ext` | L1422, L1945, L2106, L2160, L2728 |
| `apply_routes` | L2844, L3222, L3431 |
| `write_routed_def` | L3008, L3037, L3086, L3092, L3180, L3200, L3232, L3233, L3338, L3438, L3798 |

`route_nets`/`route_nets_with_progress` callers are untouched —
`MazeConfig::default()` and `MazeConfig { occ, .. }` update-syntax
constructions (L1474, L2205) compile as-is with `threads: 0`.
`GRAPHS_OPTS`/`LOADED_OPTS` consts unchanged (`GraphOpts` has no new
field). The `emit_leg` helper (L2844 site) passes `0` — shared-leg
emit keeps default parallelism.

## Greps that must come back empty / clean

- `rg "set_var|remove_var|THREAD_ENV_LOCK|with_rayon_threads" crates/` → no hits
- `rg "RAYON_NUM_THREADS" crates/grus-eda-core/src/` → no hits
- `rg "RAYON_NUM_THREADS" crates/` → only `crates/grus-eda/src/main.rs`
- `rg "env::var" crates/grus-eda-core/src/drt/` → only the surviving
  observability reads (`GRUS_DRT_TIMING`, `GRUS_DRT_MEM`)

## Fail-ability note

The forced arms' prove-it-red evidence was established when they
landed (`drt-rayon`; the surviving assertions are byte/digest
equality across pool sizes). This refactor does not change what the
assertions measure — the count reaching `ThreadPoolBuilder` is now
compile-checked: delete `cfg.threads` plumbing and the code does not
build. No new prove-red arm is required; if the reviewer wants one,
passing `threads: 1` vs `8` through `route_prepared`'s `threads=N`
timing token (`GRUS_DRT_TIMING=1`) is the observational check.
