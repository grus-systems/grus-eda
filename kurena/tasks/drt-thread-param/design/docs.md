# docs — e6-drt.md, results.md, status.md, version

## `docs/e6-drt.md` — "Thread control" paragraph (≈L775–781)

Current text describes `drt_pool` reading `RAYON_NUM_THREADS` at
call time. Rewrite to:

> **Thread control**: every drt parallel section — graph build,
> `NetTopo` build, soft-iteration search, `apply_routes`,
> `nets_section` — runs inside a per-call `rayon::ThreadPool`
> (`drt::pool::drt_pool`) whose size is an explicit `threads` input:
> `MazeConfig.threads` covers both route legs (attach pass and
> rip-up loop), while `build_route_graphs_ext`, `apply_routes`, and
> `write_routed_def` take a `threads: usize` parameter. `0` keeps
> rayon's default parallelism. The library never reads the
> environment — `grus-eda drt` reads `RAYON_NUM_THREADS` once at the
> `Commands::Drt` boundary and passes the count down, so CLI behavior
> is unchanged and tests force per-call counts without touching
> process state. `GRUS_DRT_TIMING` reports the pool size as a
> `threads=N` token on each timing line.

## `docs/results.md` — dated entry

New dated section (2026-11-27 or the run date), following the file's
entry conventions — record:

- **What changed**: `drt_pool()` → `drt_pool(threads)`; the
  `RAYON_NUM_THREADS` read moved from the library to the
  `Commands::Drt` CLI boundary; `MazeConfig.threads` field added;
  `build_route_graphs_ext` / `apply_routes` / `write_routed_def`
  gained a `threads: usize` parameter (`0` = rayon default).
  Breaking API change → workspace `0.15.2 → 0.16.0`.
- **Test change**: `with_rayon_threads`/`THREAD_ENV_LOCK` deleted;
  the four forced arms (`test_rayon_graphs_serial_identity`,
  `test_net_graph_rebuild_identity`, `test_rayon_route_determinism`,
  `test_rayon_emit_serial_identity`) pass 1/8 explicitly — same
  assertions.
- **Evidence**: `cargo test --workspace --profile verify` result;
  clippy `-D warnings` clean; the `rg` greps from `tests.md`; route
  digest `0xef2b3ff70271a2e9` unchanged (`test_maze_census`); DEF
  byte-equality vs committed `gcd.drt.ours.routed.def` unchanged
  (`test_e2e_ours_def_deterministic_gcd`); state explicitly what was
  run (AGENTS §7.9) — e.g. whether the full verify suite or a
  filtered `cargo test -p grus-eda-core --test drt` run produced the
  numbers.
- **Why**: card #60 review deferred item — env mutation in tests was
  unsound under edition 2024 (concurrent environ readers through the
  old `drt_pool`), panic-leaky, and clobbered a pre-existing
  `RAYON_NUM_THREADS`.

## `docs/status.md`

Update the `Last updated:` header line — prepend the
`v0.16.0 drt-thread-param` token with a one-sentence summary
(explicit `threads` inputs replace the `RAYON_NUM_THREADS` read in
`drt_pool`; CLI reads the var at the boundary; output byte-identical,
digest/goldens unchanged), preserving the existing "Previous:"
nesting convention.

## `Cargo.toml` / `Cargo.lock`

`[workspace.package] version` `0.15.2 → 0.16.0` — breaking public
surface change while in 0.x bumps **minor** per AGENTS §5. Regenerate
`Cargo.lock` via the build (or `cargo check`) so the two `grus-eda*`
package entries move.

## Commit

One commit — the API change and its test/docs fallout are a single
logical unit (the tree must compile). Suggested message:

```
refactor!: Take drt thread count as an explicit parameter

- drt_pool(threads): drop the RAYON_NUM_THREADS read; 0 = rayon
  default; the count is a call input everywhere
- MazeConfig.threads covers both route pools; build_route_graphs_ext,
  apply_routes, write_routed_def gain a threads parameter
- grus-eda drt reads RAYON_NUM_THREADS once at the Commands::Drt
  boundary — CLI behavior unchanged
- tests: delete with_rayon_threads/THREAD_ENV_LOCK (unsound env
  mutation under edition 2024); forced arms pass 1/8 explicitly
- BREAKING: MazeConfig new field; three public functions new
  required parameter → workspace 0.15.2 → 0.16.0
```

plus the agent trailer per AGENTS §5.
