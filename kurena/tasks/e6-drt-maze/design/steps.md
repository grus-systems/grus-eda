# e6-drt-maze — Implementation steps

Sized ~700–900 lines across `maze.rs` + `drt.rs` wiring + tests +
docs. If it grows past that, split into two commits: (1–3) the model
(occupancy + search + extraction) with unit tests, then (4–6)
integration tests + docs. Order:

1. **Occupancy** (`drt/maze.rs`) — `Occupant`, `Occupancy`
   (`lines_x`/`lines_y` `BTreeMap`s), `rasterize` (strict
   `w/2 + s` band, integer half-width), placed-cell OBS pre-mark
   (replicate the `drc::geom` source-4 loop — gate + `orient_rect` +
   translate — OBS rects only), candidate-edge `check` returning
   hit occupants. Unit tests: band boundaries (abut-at-`s` is
   legal), perpendicular crossing, adjacent-line band on a
   sub-`w+s`-pitch layer.
2. **Attach + search** — `attach_set` (strict `pin_nodes`, else the
   `search.md` §2 projection over `pin_shapes` + `region`), CSR
   adjacency per net, the multi-source Dijkstra round with
   `(cost, node)` PQ order and parent tracking, tree extension loop,
   `FailKind` classification. Costs per the constant table; via
   edges gated on `pairs[k].default.is_some()`. Unit tests: PQ tie
   order, projection on an escaped pin, simple-path invariant.
3. **Sequential driver + extraction** — the `search.md` §5
   iteration loop (join order, soft/hard modes, provisional
   collisions, rip-up + `hist` escalation, cap), commit (wire +
   via-footprint rasterization), `NetRoute` extraction (per-line
   interval merge, canonical sorts), `overlap_census`,
   `route_digest`, `MazeConfig`/`OccMode`. `drt.rs`: `pub mod
   maze;` + `pub use` + doc line.
4. **Synthetic fixture** (`tests/drt.rs`) — one LEF/DEF pair +
   per-case guide strings covering the four acceptance cases
   (straight / forced via / OBS detour / named unroutable) —
   `tests.md` §"Synthetic fixture".
5. **Real-design tests** — `test_maze_census` (run
   `-- --nocapture`, collect the printed census for `results.md`;
   pin unrouted set + classifications after the first real run —
   planning bound gcd ≤3 / picorv32 ≤209, `overview.md`),
   `test_maze_occupancy` fail-proof, `test_maze_determinism`.
   `picorv32.placed.def` must exist (generate via
   `tests/designs/picorv32/generate_ref.sh` if absent — container
   run, mention before running).
6. **Docs + version** — `docs/e6-drt.md` §7 + §4/§5 pointer updates
   (`docs.md`), `docs/results.md` dated entry with **actual** run
   numbers (never planning estimates), `docs/status.md`,
   `docs/roadmap.md`; `Cargo.toml` workspace version
   `0.12.3 → 0.12.4` in the same commit as the functional change.

## Verification

- `cargo test -p grus-core --lib maze` — unit tests.
- `cargo test -p grus-core --test drt -- --nocapture` — the census
  print (both designs) for `results.md`; confirm the runtime line
  for picorv32 is measured and recorded (the acceptance requires it
  complete in-test).
- `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` — record
  outcomes.
- Prove a test can fail (AGENTS §7.7): with occupancy disabled the
  detour/ census assertions or the digest comparison must go red —
  demonstrate once during development and note it in `results.md`.
- Runtime watch: per-net Dijkstra + per-edge interval queries on
  ~28M picorv32 nodes is the risk point; if the census test runs
  hot, first check per-net region sizes (the big clock nets), then
  the per-line `Vec` scan — record whatever optimization was needed,
  do not silently change semantics.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
