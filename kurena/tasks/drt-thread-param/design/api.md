# api — exact surface changes

Every change is mechanical: the count arrives as a `usize`, `0`
means `ThreadPoolBuilder::num_threads(0)` — rayon's default
parallelism, today's unset-`RAYON_NUM_THREADS` behavior.

## `src/drt/pool.rs`

```rust
/// A private pool of `threads` workers; `0` keeps rayon's default
/// (available parallelism). The count is an explicit input — the
/// library never reads the environment; the CLI reads
/// `RAYON_NUM_THREADS` once at its boundary (`main.rs`).
pub(crate) fn drt_pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("drt thread pool")
}
```

The module docstring is rewritten: it currently exists *because* the
global pool would pin `RAYON_NUM_THREADS` on first use — the new
reason for a per-call pool is that each surface's pool size is a
call input, and tests force different counts in the same process.

## `src/drt/maze.rs`

`MazeConfig` (L50–64) gains the field; `Default` sets `0`:

```rust
pub struct MazeConfig {
    pub occ: OccMode,
    /// Iteration cap; the last iteration runs in hard mode (foreign
    /// hits are blocks, not costs).
    pub max_iters: u32,
    /// Rayon pool size for the streamed attach pass and the rip-up
    /// search; `0` keeps rayon's default parallelism.
    pub threads: usize,
}
```

Two pool sites, both already in `cfg` scope — **no signature change**
anywhere in this file:

- `route_nets_with_progress` (L4300): `let pool = drt_pool(cfg.threads);`
- `route_prepared` (L4433): `let pool = drt_pool(cfg.threads);`

The comment at L4297–4299 ("`RAYON_NUM_THREADS` is honored at call
time …") is rewritten — the pool is now sized by `cfg.threads`; the
per-call pool remains because each call site's size is an input, not
because of env pinning.

`route_nets` (L4241), `route_nets_with_progress` (L4261),
`route_prepared` (L4416), and the in-file unit-test call at L5711
(`&MazeConfig::default()` → `threads: 0`) are all untouched.
`pool.current_num_threads()` keeps feeding `nthreads` for the
`threads=N` timing token and the in-flight-rebuild bound.

## `src/drt/graph.rs`

```rust
pub fn build_route_graphs_ext(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    threads: usize,
) -> RouteGraphs
```

L1496 becomes `drt_pool(threads)`. `GraphOpts` is **not** extended —
see `overview.md` (the `route_nets` signature already carries two
config structs; a `GraphOpts.threads` would be silently ignored
there). The docstring gains one clause: `threads` sizes the build
pool, `0` = rayon default.

`build_route_graphs` (L1237) keeps its signature and passes `0` —
its ~15 test callers are unaffected.

## `src/drt/emit.rs`

```rust
pub fn apply_routes(
    db: &mut DesignDb,
    tech: &TechDb,
    guided: &[NetId],
    route_db: &RouteDb,
    via_table: &ViaTable,
    threads: usize,
) -> Result<EmitReport>
```

L120 becomes `drt_pool(threads)`.

```rust
fn nets_section(db: &DesignDb, tech: &TechDb, threads: usize) -> Result<String>
```

L309 becomes `drt_pool(threads)`.

```rust
pub fn write_routed_def(
    db: &DesignDb,
    tech: &TechDb,
    template_def: &Path,
    out_def: &Path,
    threads: usize,
) -> Result<()>
```

passes `threads` into `nets_section`. `preemit_check` is unchanged —
it constructs no drt pool (`run_all` is serial).

## `crates/grus-eda/src/main.rs`

In the `Commands::Drt` arm, after the `GRUS_DRT_TIMING` read (the
same spot the other boundary env reads live):

```rust
// `RAYON_NUM_THREADS` is the CLI's thread knob — read once at the
// boundary; the library takes the count as an explicit input
// (`0` = rayon default).
let threads = std::env::var("RAYON_NUM_THREADS")
    .ok()
    .and_then(|v| v.parse::<usize>().ok())
    .unwrap_or(0);
```

Three consumers:

- `MazeConfig { occ, max_iters: args.max_iters, threads }` (L2145 —
  the literal gains the field)
- `apply_routes(&mut design_db, tech_db, &guided, &route_db, &vias, threads)` (L2156)
- `write_routed_def(&design_db, tech_db, &args.def, &args.out_def, threads)` (L2173)

The `GraphOpts` construction (L2110) is unchanged.

## Compile-blast-radius checklist

Sites the compiler will flag; all resolved per `tests.md`:

- `tests/drt.rs`: `build_route_graphs_ext` ×7 (L1422, L1945, L2106,
  L2160, L2699, L2705, L2728), `apply_routes` ×5 (L2844, L3222,
  L3260, L3263, L3431), `write_routed_def` ×13 (L3008, L3037, L3086,
  L3092, L3180, L3200, L3232, L3233, L3277, L3280, L3338, L3438,
  L3798), `MazeConfig` literals (L1474, L2205 — both use
  struct-update syntax, so they compile unchanged; only the forced
  arms *set* `threads`).
- `main.rs`: the `MazeConfig { occ, max_iters }` literal at L2145 —
  the one literal that must name the new field.
