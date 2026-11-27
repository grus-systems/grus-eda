# drt-thread-param — overview

## Problem

`with_rayon_threads` in `crates/grus-eda-core/tests/drt.rs` (L2674–2682)
mutates `RAYON_NUM_THREADS` around forced-thread test arms.
`THREAD_ENV_LOCK` serializes those arms against each other but not
against the rest of the binary: every other test in the same process
reads the environ block concurrently through `drt_pool()`'s
`std::env::var` — the exact race that makes `set_var` `unsafe` in
edition 2024. Secondary defects named in the spec: a panic inside the
closure leaks the forced value; a pre-existing `RAYON_NUM_THREADS` is
clobbered by `remove_var`, not restored. Any process-global knob —
env var, atomic, thread-local — keeps the same structural property:
two tests wanting different counts cannot coexist without
serialization.

The fix removes the global entirely: thread count becomes an explicit
input to every drt parallel surface, `0` meaning rayon's default
parallelism (unchanged behavior).

## Current state (read of the code)

Five `drt_pool()` construction sites, all calling
`pub(crate) fn drt_pool() -> rayon::ThreadPool`
(`crates/grus-eda-core/src/drt/pool.rs:9`), which reads
`RAYON_NUM_THREADS` at call time and falls back to `num_threads(0)`:

| site | enclosing public surface |
|---|---|
| `graph.rs:1496` | `build_route_graphs_ext` (also reached via `build_route_graphs`) |
| `maze.rs:4300` | `route_nets_with_progress` — the streamed attach pass |
| `maze.rs:4433` | `route_prepared` — the rip-up loop (called by `route_nets_with_progress` and by in-file unit tests) |
| `emit.rs:120` | `apply_routes` |
| `emit.rs:309` | `nets_section` — private, called by `write_routed_def` |

`preemit_check` (`emit.rs:255`) runs `drc::report::run_all`, which
does not use rayon — no pool, no parameter needed. `route_nets` is a
thin forwarder into `route_nets_with_progress`.

Test-side surfaces today: `THREAD_ENV_LOCK` + `with_rayon_threads`
(`tests/drt.rs:2669–2682`) wrap four arms —
`test_rayon_graphs_serial_identity` (build at 1 vs 8),
`test_net_graph_rebuild_identity` (streamed census at 1 vs 8),
`test_rayon_route_determinism` (route digest at 1/8/8), and
`test_rayon_emit_serial_identity` (apply + write bytes at 1 vs 8).

The CLI never spells the count: `Commands::Drt` in
`crates/grus-eda/src/main.rs` calls `route_nets_with_progress`
(L2138), `apply_routes` (L2156), and `write_routed_def` (L2173), and
the env var is honored only because `drt_pool` reads it deep inside
the library.

## Mechanism decision

**Explicit `threads: usize` per surface, `0` = rayon default** —
the same convention `ThreadPoolBuilder::num_threads(0)` and the
current `unwrap_or(0)` already use.

- `drt_pool(threads: usize)` — signature change, env read deleted;
  `pub(crate)` unchanged.
- `MazeConfig.threads: usize` (default `0`) — covers **both** maze
  pool sites: `cfg` is already in scope at `maze.rs:4300` and is
  already a `route_prepared` parameter at `maze.rs:4426`. The
  `route_nets`/`route_nets_with_progress`/`route_prepared` signatures
  do not change; `MazeConfig::default()` and struct-update-syntax
  callers are untouched.
- `build_route_graphs_ext(.., opts, threads: usize)` — explicit
  trailing parameter. `build_route_graphs` keeps its signature and
  passes `0`.
- `apply_routes(.., via_table, threads: usize)` and
  `write_routed_def(.., out_def, threads: usize)` — explicit trailing
  parameters; `nets_section` gains the same parameter privately.
- `crates/grus-eda/src/main.rs` — the `Commands::Drt` arm reads
  `RAYON_NUM_THREADS` once (`var → parse → unwrap_or(0)`, the same
  parse `pool.rs` ran) and feeds `MazeConfig.threads`,
  `apply_routes`, `write_routed_def`. CLI behavior is preserved
  verbatim; the library no longer touches the environment.

### Why a `MazeConfig` field but bare parameters elsewhere

`route_nets(db, tech, tracks, join, opts, vias, cfg)` already carries
two config structs. Putting `threads` on `GraphOpts` would give that
signature two thread sources — `cfg.threads` (used) and
`opts.threads` (silently ignored) — a silent-ignore footgun.
`MazeConfig` is the route's execution config (it already holds
`max_iters` and `occ`), so `threads` belongs there and only there.

`GraphOpts` configures graph *semantics* (`spare_above`, `escape`);
the pool size of one `build_route_graphs_ext` call is an execution
input, so it goes in as a trailing parameter — the same shape
`apply_routes`/`write_routed_def` get. Consistent rule: the count
rides the one config struct that already configures that stage's
execution (maze), and is an explicit parameter everywhere else.

### Rejected alternatives

- **`GraphOpts.threads` field** — the two-sources footgun above.
- **`EmitOpts { threads }` struct** — one field, no second consumer;
  a bare parameter is the honest surface. If emit grows more options
  later it can introduce the struct then.
- **`Option<usize>` / `NonZeroUsize`** — `0` already means "rayon
  default" at every layer this value passes through (the builder's
  own `num_threads(0)` contract). An option type adds a wrap/unwrap
  step without new information.
- **Crate-level `AtomicUsize` or `OnceLock<ThreadPool>`** — the
  global-knob disease the spec names; tests with different counts
  could not coexist, and a shared pool would re-introduce the
  first-use pinning `drt_pool` was written to avoid.
- **Keep `drt_pool()` env-reading as a fallback** — the spec's core
  requirement is no env read in the library; a fallback keeps the
  unsafe reader alive for every test in the binary.

## Scope boundary

- `GRUS_DRT_TIMING`, `GRUS_DRT_MEM` — unrelated observability knobs,
  unchanged (they configure output, not correctness inputs, and are
  read once into locals).
- Non-drt rayon uses (`sta`, `placement`, `congestion`) — out of
  scope; they do not call `drt_pool`.
- No new `--threads` CLI flag — `RAYON_NUM_THREADS` remains the CLI
  knob, now read at the boundary. Adding a flag is a separate
  product decision.

## Breaking-change note

`MazeConfig` gains a field (literal constructions break; update
syntax `..Default::default()` is unaffected), and three public
functions gain a required parameter — a breaking API change.
Per AGENTS §5 the 0.x workspace bumps **minor**: `0.15.2 → 0.16.0`,
in the same commit, recorded in the commit message.

## Files

- `crates/grus-eda-core/src/drt/pool.rs` — `drt_pool(threads)`; env
  read deleted
- `crates/grus-eda-core/src/drt/maze.rs` — `MazeConfig.threads`; two
  `drt_pool(cfg.threads)` sites; comment updates
- `crates/grus-eda-core/src/drt/graph.rs` — `build_route_graphs_ext`
  `threads` parameter
- `crates/grus-eda-core/src/drt/emit.rs` — `apply_routes` /
  `write_routed_def` / `nets_section` `threads` parameters
- `crates/grus-eda/src/main.rs` — boundary `RAYON_NUM_THREADS` read;
  three call sites updated
- `crates/grus-eda-core/tests/drt.rs` — delete
  `with_rayon_threads`/`THREAD_ENV_LOCK`; forced arms pass counts
  explicitly; mechanical `threads`/`0` additions at all other sites
- `Cargo.toml` — workspace version `0.15.2 → 0.16.0`
- `Cargo.lock` — version bump regeneration
- `docs/e6-drt.md` — Thread-control paragraph rewrite
- `docs/results.md` — dated entry (API change + evidence)
- `docs/status.md` — `Last updated` line, `v0.16.0` token
