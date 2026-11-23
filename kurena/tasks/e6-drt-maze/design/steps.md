# e6-drt-maze — Implementation steps (resume from `c1832dc`)

The checkpoint compiles; synthetic cases, in-module units, the gcd
census leg, and the occupancy fail-proof are green; the picorv32
census leg is in-flight. Remaining work, in the reviewer's order:

## 0. `docs/e6-drt.md` §7 + §4/§5 amendments

Write the "7. Maze search" section per `docs.md` — content is fully
determined by the committed code (cost constants, occupancy classes,
rip-up rule, hop heuristic); measure nothing new here. Amend §4
claim boundary and §5 router implication to match what shipped
(projection attach + spare layer answered the escape/pocket classes;
relaxed vias bridge the strict-crossing gap).

## 1. Clear the 6 clippy warnings

All mechanical; match the file's existing conventions:

- `drt/maze.rs:706` `scan_stub` — `too_many_arguments (8/7)` →
  `#[allow(clippy::too_many_arguments)]` (same convention as
  `edge_cost`, `extend_once`).
- `drt/maze.rs:729` `edge_collisions` — `(9/7)` → same `#[allow]`.
- `drt/maze.rs:1014` `host_cells.push(c.0 as u32)` —
  `unnecessary_cast` → drop `as u32` (`PinId`/`CellId` `.0` is
  already `u32`).
- `drt/maze.rs:1185` (+use at 1189) `needless_range_loop` — the hop
  BFS indexes only `adj_to` with `pos` → iterate the slice:
  `for &to in &topo.adj_to[range]`; check the PQ loop at ~1218 for
  the same warning after the fix (it uses `pos` for two arrays —
  `adj_to[..].iter().zip(&adj_eidx[..])` if flagged).
- `drt/maze.rs:1272` `route_one_net` — `(10/7)` → same `#[allow]`.
- `tests/drt.rs:1495` `find(p: &mut Vec<usize>, i)` — `ptr_arg` →
  `&mut [usize]`.

`cargo clippy --workspace --all-targets -- -D warnings` → clean.

## 2. Verify runs (numbers feed `results.md`)

- `cargo test -p grus-core --test drt test_maze_census -- --nocapture`
  — collect both designs' census lines verbatim (gcd already
  measured: `routed=269/269 unrouted=0 collisions=0 ms=9704`;
  picorv32's `ms=` is the acceptance number — record it however
  large; if it lands near the earlier 25+ CPU-min bound, state that
  explicitly per the reviewer).
- `cargo test -p grus-core --test drt test_maze_occupancy` — already
  measured (Enabled 0 collisions vs Disabled 6,429; digests differ).
- `cargo test -p grus-core --test drt test_maze_determinism` — 2×
  full runs per design; identical `RouteDb` + nonzero digest.
- `cargo test --workspace` — the whole suite, including the
  picorv32-bound tests (census + determinism = 3 full picorv32
  route_nets runs total across the binary; plan the wall-clock
  envelope accordingly — run with `--release`? **No**: acceptance
  says "completes in a test, not a timeout" under the default
  profile; the debug number is the honest one. If the suite's wall
  time is dominated by the 3 picorv32 runs, that is the recorded
  fact, not a reason to `#[ignore]`).
- `GRUS_DRT_TIMING=1` on a census rerun is optional evidence — the
  per-iteration `bad=`/`prov=` lines document convergence.

## 3. `docs/results.md` + `docs/status.md` + `docs/roadmap.md`

Per `docs.md`: dated entry with the measured census table (both
designs), the occupancy fail-proof numbers, determinism digest,
perf before/after (203 s → measured on gcd), the picorv32 runtime
statement, checks run, version bump. `status.md` head paragraph and
the `roadmap.md` E6 progress line per the existing style.

## 4. Version bump + commit

- `Cargo.toml` `[workspace.package] version 0.12.3 → 0.12.4` (feature,
  patch on 0.x — same commit as the functional change, AGENTS §5).
- One commit on top of the checkpoints:
  `feat: Sequential maze router over the track graph`
  (Conventional Commits, English, agent trailer). Stage explicit
  paths — never `git add -A`.

## Contingencies

- **picorv32 census red on the met3-bridge classifier**: any unrouted
  `NoPath` net outside the met3-bridge class needs its cause named
  and the classifier extended (`tests.md`) — do not weaken the
  assert silently.
- **clippy fix changes output**: none of the six fixes touches
  semantics; if the `RouteDb` digest changes after the edits, that
  is a bug in the fix, not new routing behavior.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
