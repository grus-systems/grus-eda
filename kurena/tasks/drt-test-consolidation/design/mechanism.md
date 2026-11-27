# Mechanism — the shared `RoutedLeg`

## Type and cells

Place near `load_routed_inputs` (the "Maze router" section header):

```rust
/// One `(fixture, config)` pair's shared route: the immutable input
/// bundle plus its `RouteDb`, resolved once per test process. Tests
/// that only read the route borrow `&'static RoutedLeg`; tests that
/// mutate emit geometry clone `leg.db` (the `&'static` borrow can
/// never yield `&mut`). `route_ms` is measured inside the
/// initializer — it is the single real execution's time, whatever
/// `Instant` would read in the resolving thread.
struct RoutedLeg {
    name: &'static str,
    tech: TechDb,
    db: DesignDb,
    tracks: Vec<LayerTracks>,
    join: GuideJoin,
    vias: ViaTable,
    cfg: MazeConfig,
    route: RouteDb,
    route_ms: u128,
}

static GCD_LEG: OnceLock<RoutedLeg> = OnceLock::new();
static GCD_OCC_OFF_LEG: OnceLock<RoutedLeg> = OnceLock::new();
static PICORV32_LEG: OnceLock<RoutedLeg> = OnceLock::new();

fn shared_leg(
    name: &'static str,
    occ: OccMode,
    cell: &'static OnceLock<RoutedLeg>,
) -> &'static RoutedLeg {
    cell.get_or_init(|| {
        let (tech, db, tracks, _) = load(name);
        let (_, join) = load_guide(&db, &tech, name);
        let vias = via_table(&tech, &db).expect("via_table");
        let cfg = MazeConfig {
            occ,
            ..MazeConfig::default()
        };
        let t0 = std::time::Instant::now();
        let route = route_nets(&db, &tech, &tracks, &join, LOADED_OPTS, &vias, &cfg);
        RoutedLeg {
            name,
            tech,
            db,
            tracks,
            join,
            vias,
            cfg,
            route,
            route_ms: t0.elapsed().as_millis(),
        }
    })
}

fn gcd_leg() -> &'static RoutedLeg {
    shared_leg("gcd", OccMode::Enabled, &GCD_LEG)
}

fn gcd_occ_off_leg() -> &'static RoutedLeg {
    shared_leg("gcd", OccMode::Disabled, &GCD_OCC_OFF_LEG)
}

fn leg_for(name: &'static str) -> &'static RoutedLeg {
    match name {
        "gcd" => gcd_leg(),
        "picorv32" => shared_leg("picorv32", OccMode::Enabled, &PICORV32_LEG),
        _ => panic!("no shared leg for {name}"),
    }
}
```

Notes:

- `OnceLock` (not `once_cell`/`lazy_static`) — the tree already uses
  `std::sync::OnceLock` in `src/timing.rs`; no new dependency.
- The init deliberately **skips** `build_route_graphs_ext`:
  `route_nets` rebuilds per-net graphs internally (the `opts`
  parameter only documents the build options), and every current
  route call site destructures the graphs element as `_`.
  `load_routed_inputs` stays for the two consumers that need real
  `RouteGraphs` (`test_mem_census_sanity`,
  `test_net_graph_rebuild_identity`).
- `name` is stored so `{name}`-parameterized helpers (`maze_census`,
  `maze_determinism`, `emit_gate`) keep their printed labels and the
  `if name == "gcd"` digest pin becomes `if leg.name == "gcd"`.

## Sync

`static OnceLock<RoutedLeg>` compiles only when `RoutedLeg: Send +
Sync` — all fields are plain owned data. Mirror the tree's existing
compile-time assertion convention (`src/drt/graph.rs` `need::<HullMask>`):

```rust
/// The shared legs are read across test threads — a `Cell`/`RefCell`
/// regression inside any field fails here naming the type.
const _: fn() = || {
    fn need<T: Sync + Send>() {}
    need::<RoutedLeg>();
};
```

## Emit leg reshape

`emit_leg` currently returns `(TechDb, DesignDb, RouteDb, ViaTable,
EmitReport)` — five owned values the callers then re-borrow. New
shape: the leg supplies everything immutable; only the `db` that
`apply_routes` mutates is cloned per call.

```rust
/// Clone `leg.db`, apply the shared `leg.route` into it — the emit
/// half of the gate. Route re-execution is never part of emit.
fn emit_leg(leg: &'static RoutedLeg) -> (DesignDb, EmitReport) {
    let mut db = leg.db.clone();
    let guided: Vec<NetId> = leg.join.nets.iter().map(|&(n, _)| n).collect();
    let report =
        apply_routes(&mut db, &leg.tech, &guided, &leg.route, &leg.vias).expect("apply_routes");
    (db, report)
}
```

Call sites change shape, e.g.:

```rust
let leg = gcd_leg();
let (db, report) = emit_leg(leg);
// was: report.routed == route_db.nets.len()  →  leg.route.nets.len()
// was: write_routed_def(&db, &tech, …)       →  &leg.tech
```

`emit_gate(name)` becomes `emit_gate(leg: &'static RoutedLeg)`
(callers pass `gcd_leg()` / `leg_for("picorv32")`).

## Fresh-run helper for determinism arms

A determinism check must compare two independent executions.
Shared + one fresh run on the shared inputs:

```rust
fn route_fresh(leg: &'static RoutedLeg) -> RouteDb {
    route_nets(
        &leg.db,
        &leg.tech,
        &leg.tracks,
        &leg.join,
        LOADED_OPTS,
        &leg.vias,
        &leg.cfg,
    )
}
```

- `maze_determinism`: `assert_eq!(leg.route, route_fresh(leg))`.
- `test_emit_determinism`: `emit_leg(leg)` for pass 1; `route_fresh`
  + a second `leg.db.clone()` + `apply_routes` + `write_routed_def`
  for pass 2. `assert_eq!(leg.route, r2)` still compares two
  independent executions; net-geoms/byte pins unchanged.

## Hazards (named, not assumed)

1. **Never resolve a leg inside `with_rayon_threads`.**
   `RAYON_NUM_THREADS` is process-global and read at route time; a
   cell resolved inside a forced-thread closure would bind the
   shared run's thread count to test-order luck. Rule: `*_leg()`
   calls happen at test top level only. `test_rayon_route_determinism`
   and `test_net_graph_rebuild_identity` use `leg.db`/`leg.tracks`/
   `leg.join`/`leg.vias` for inputs but call `route_nets` directly
   inside the forced closures. (The route digest is pinned
   thread-count-independent, so a violation would not corrupt
   outputs — it is still a named hygiene rule.)

2. **No `&mut` can escape a leg.** `&'static RoutedLeg` yields only
   shared borrows; mutation-needing tests (`test_emit_fail_proof`,
   `test_preemit_foreign_pin`) necessarily go through
   `leg.db.clone()` — compile-enforced, no discipline required.

3. **`ms=` print semantics.** `maze_census`'s `ms=` field becomes
   `leg.route_ms` — the measured time of the single real execution
   at init, not a per-test measurement. `docs/results.md` picorv32
   runtime provenance stays honest (the ignored run still executes
   the route exactly once, inside `get_or_init`).

4. **Reflexive-assert trap.** Wherever an assert compared two
   formerly-independent `route_nets` results, the second operand
   must come from `route_fresh` (or the test's own forced-env
   runs), never the leg again.

5. **Env-var leak.** `GRUS_DRT_TIMING`/`GRUS_DRT_MEM` are read at
   call time; whichever test resolves a cell first inherits ambient
   env — affects only eprintln output, never `RouteDb` content.
