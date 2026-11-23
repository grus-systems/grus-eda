# e6-drt-maze — Overview (final revision)

E6 ticket 5 — the sequential maze router over the track graph.
**The implementation is landed.** HEAD is the wip checkpoint
`4ff7ade`, which contains the full `drt::maze` router, the
`docs/e6-drt.md` "7. Maze search" section, a clean clippy run
(the six warnings fixed in place), and the `0.12.3 → 0.12.4`
workspace version bump. The working tree is clean.

This revision covers only the four items the rejection memo left
open — docs evidence, status docs, the verify pass, and the final
commit. No routing-model work is planned unless a verify run
contradicts the measured state below.

## Landed at `4ff7ade`

- `route_nets` — pathfinder-style tree extension per net in
  `join.nets` order over the unified `[in-plane | strict via |
  relaxed via]` edge space, `NetTopo` cached across rip-up
  iterations, goal-directed `(g + hop, node)` PQ, bounded
  backtrack, `absorb_taps`.
- Costs `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 / HIST 8 /
  MAX_ITERS 8`, last iteration hard mode.
- Interval occupancy per track line (`w/2 + s` band, `SKIP_CHUNK`
  skip index) with the `Obs`/`Net`/`Pin` verdict table —
  unconditional host-cell carve, `Pin` never hard-blocks, unmerged
  `obs_rects_raw` marks.
- Projection attachment fallback (strict `pin_nodes` first, ±1
  line margin), `spare_above = 1` region, relaxed via edges with
  committed stub wires.
- Rip-up rule: `bad` = unrouted ∪ both parties of every
  provisional collision; per-net `hist` escalation; cap 8.
- Named per-pin unrouted report (`Unattachable` vs `NoPath`).
- `GRUS_DRT_TIMING` env-gated phase/iteration `eprintln!`
  instrumentation — committed, opt-in, feeds the results entry.
- Tests: `test_maze_cases` (4 synthetic cases), `test_maze_census`
  (gcd + picorv32), `test_maze_occupancy` (gcd fail-proof),
  `test_maze_determinism` (double-run both designs).

## Remaining work (the rejection memo's four items)

1. **`docs/results.md` dated entry** — `### 2026-11-23 — e6-drt-maze:
   sequential maze router (v0.12.4)`. The blocker is the measured
   runtime census on both designs: rerun `test_maze_census` under
   `GRUS_DRT_TIMING=1` and record the real `ms=` numbers verbatim.
   The picorv32 leg ran 30+ CPU-minutes in release mode earlier —
   that is the accepted bound if it recurs; record the measured
   number, never a hand-written one (AGENTS §2).
2. **`docs/status.md` + `docs/roadmap.md`** — head paragraph and
   E6 progress line per `docs.md`.
3. **`cargo test --workspace` verify pass** — the whole suite in
   the default (dev) profile; plus a clippy re-check for the
   record. Record the actual wall-clock time.
4. **Final implementation commit** — the doc changes are the
   remaining delta; commit them as the ticket's deliverable.

## Measured state at design time

- `test_maze_cases` — green (0.00 s).
- `test_maze_census` gcd leg — green: `guided=269 routed=269
  pins_connected=931 projected=17 unrouted=0 collisions=0
  iterations=8 ms=9704 digest=0xfbb3073487860974` (dev profile).
- `test_maze_occupancy` — green (17.6 s): Enabled digest
  `0xfbb3073487860974` census=0 vs Disabled digest
  `0x349f82f53a0c7143` census=6,429 — occupancy measurably
  constrains.
- `test_maze_census` picorv32 leg — **unmeasured**; the earlier
  release-profile run took 30+ CPU-minutes.
- `test_maze_determinism` — asserts identical `RouteDb` + nonzero
  digest on 2× runs per design; untested at design time (cost
  follows the census number).

## Risks

- **Workspace-gate wall clock.** The `drt` test binary spends 3
  full picorv32 `route_nets` runs in dev profile (census ×1,
  determinism ×2 — sequential inside the test). If dev-profile
  picorv32 lands near or above the release figure, the gate takes
  on the order of hours. That is the honest cost — do **not**
  `#[ignore]` the legs (AGENTS §2, acceptance: "completes in a
  test, not a timeout"). Run it detached and record the real
  number.
- **Residual unrouted on picorv32.** The census test tolerates
  only the met3-bridge `NoPath` class (disconnected {li1,met1,met2}
  box union rejoined by met3+); any other class needs a named
  cause and a classifier extension — never a weakened assert.
- **`docs/e6-drt.md` §7 drift.** §7 is written; spot-check its
  claims against `maze.rs` while writing `results.md`. Code wins;
  note any delta in the commit body.

## Files

- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `crates/grus-core/src/drt/maze.rs` (contingency only — verify-driven fixes)
- `crates/grus-core/tests/drt.rs` (contingency only — classifier/fixture fixes)
