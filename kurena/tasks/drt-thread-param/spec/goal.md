# drt-thread-param: explicit thread count instead of env mutation

Card #60 review medium (deferred): `with_rayon_threads` in
`crates/grus-eda-core/tests/drt.rs` mutates the process environment
to control rayon pool sizes in tests. The unsafe justification is
dishonest — `THREAD_ENV_LOCK` serializes the forced closures against
each other, but every other test in the same binary reads the environ
block concurrently through `drt_pool()`'s `env::var` (this is exactly
why `set_var` is unsafe in edition 2024). A panic inside the closure
leaks the forced value, and a pre-existing `RAYON_NUM_THREADS` is
clobbered by `remove_var` instead of restored. Any global knob
(env var or atomic) has the same structural problem: tests needing
different thread counts cannot coexist without serialization.

The correct shape: no global mutable state at all. Thread count
becomes an explicit input to every parallel surface — graph build,
routing, emit — with `0`/absent meaning current default behavior.
Tests pass the count directly; nothing mutates the process
environment, and `with_rayon_threads`/`THREAD_ENV_LOCK` are deleted.

## Scope

1. Thread the thread-count through the parallel surfaces. Today
   `drt_pool()` (crates/grus-eda-core/src/drt/pool.rs) reads
   `RAYON_NUM_THREADS` at call time in graph build
   (`graph.rs`), routing (`maze.rs`, two sites), and emit
   (`emit.rs`, two sites). The driver picks the mechanism — a field
   on the existing config structs (`MazeConfig`, `GraphOpts`, emit's
   parameter surface) or explicit parameters — but the count must
   reach every `drt_pool` construction site without any global.
   Default behavior unchanged: unset means rayon's default
   parallelism; the CLI keeps honoring `RAYON_NUM_THREADS` by
   reading it once at the boundary in `main.rs`.
2. Tests: delete `with_rayon_threads` and `THREAD_ENV_LOCK` entirely.
   The forced-thread determinism arms (1 vs 8) pass the count
   explicitly instead. No `set_var`/`remove_var` anywhere in tests.
3. This changes the public library surface of grus-eda-core — bump
   minor per AGENTS §5 (0.x breaking). Record the API change in the
   commit message.
4. Determinism contract unchanged: output byte-identical across
   thread counts, digest pins and goldens untouched. Prove the
   forced-arm tests still exercise what they did (route graph
   identity, route determinism, emit identity at 1 vs 8).

## Files

- `crates/grus-eda-core/src/drt/pool.rs`
- `crates/grus-eda-core/src/drt/maze.rs` / `graph.rs` / `emit.rs` (as scoped)
- `crates/grus-eda-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs` (CLI boundary env read)
- `Cargo.toml` (minor bump)
- `docs/e6-drt.md` / `docs/results.md` dated entry
