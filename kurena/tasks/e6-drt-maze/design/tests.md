# e6-drt-maze — Test plan

All maze tests exist in `crates/grus-core/tests/drt.rs` and pass
where measured — HEAD `cf34607` is a docs-only checkpoint on the
measured `c1832dc` code, so the state below carries over. The
remaining work is *running* the rest and recording the numbers, not
writing new tests. `picorv32.placed.def` is present in the worktree;
`load_routed_inputs` fails loud on missing inputs.

## Measured state (code identical at `cf34607`)

- `cargo test -p grus-core --lib maze` — **green**, 5/5
  (`test_band_math`, `test_perpendicular_crossing`,
  `test_projection_fallback`, `test_pq_tie_order`,
  `test_tree_extension_simple_path`).
- `test_maze_cases` — **green** (0.00 s): the four synthetic
  acceptance cases incl. the OBS detour under the *unconditional*
  host carve (`blk`'s OBS still blocks `n1` — the net owns no pin on
  `blk`, exactly the boundary case C1 must not open).
- `test_maze_census` gcd leg — **green**: `guided=269 routed=269
  pins_connected=931 projected=17 unrouted=0 collisions=0
  iterations=8 ms=9704 digest=0xfbb3073487860974`.
- `test_maze_census` picorv32 leg — **still unmeasured at design
  time**; the assert to watch is the met3-bridge classifier (below).
- `test_maze_occupancy` — **green** (17.6 s): Enabled
  `digest=0xfbb3073487860974 census=0` vs Disabled
  `digest=0x349f82f53a0c7143 census=6429`.
- `test_maze_determinism` — untested (2× `route_nets` per design).
- `cargo test --workspace` — pending the picorv32 leg.

## Synthetic fixture — `test_maze_cases` (green)

`MAZE_LEF`/`MAZE_DEF_*`: met1 `HORIZONTAL` + met2 `VERTICAL` on a
6×6 shared-crossing grid (fixture grids coincide, so relaxed vias
degenerate onto strict positions), `mv12` between them, one OBS
macro (`blk`), a track-less pin (`u2/C`).

1. **Straight route** — `n1` met1-only guide; exactly one merged
   `WireSeg` `(1000,1000)-(4000,1000)`, no vias, 2 connected.
2. **Forced via** — `n2` two-layer guide; exactly
   `ViaPlace { lower:0, x:4000, y:3000 }` (strict position wins the
   equal-`VIA_COST` tie — CSR enumerates strict before relaxed).
3. **OBS detour** — `n1` geometry with `blk`'s OBS on the direct
   line; connected, `wires.len()` > control, and no emitted met1
   wire enters `OBS + w/2 + s = 210`.
4. **Provably unroutable** — `n1`'s guide splits into two met1
   components (no met2 box ⇒ no upper nodes ⇒ relaxed edges cannot
   bridge ⇒ `u0/A` = `NoPath`); `n3`'s `u2/C` covers no track
   (`Unattachable`). Named pins/kinds, `nets` empty, bounded return.

## Real-design census — `test_maze_census`

Per design: `route_nets` over the committed guide (spare layer = 1);
prints guided / routed / pins_connected / projected / unrouted
(named per pin + `FailKind`) / collisions / iterations / ms / digest
for `results.md`. Asserts:

- `nets.len() + unrouted.len() == join.nets.len()` (no silent drops).
- Every `unrouted` entry names ≥1 `(pin, FailKind)`.
- Nonzero evidence: nets nonempty, every routed net has wires.
- `NoPath` classification — every unrouted `NoPath` net must be the
  met3-bridge class (guide boxes rejoin only on met3+, which the
  relaxed-via model still cannot transition to). **Expected post-fix
  state**: unrouted = 0 on both designs (acceptance). If a residual
  survives on picorv32, the classifier is *extended with the named
  cause* (e.g. `obs_walled`/`foreign_pin_walled`/`region_split` with
  probe evidence) — the assert stays strict; classes are named,
  never excused. Whatever lands is recorded in `docs/results.md`.

## Occupancy fail-proof — `test_maze_occupancy` (green)

gcd only: `Enabled` vs `Disabled`, identical inputs; asserts the
outcomes measurably differ (digest or `overlap_census` count) —
measured: digest differs **and** census 0 → 6,429. `Disabled` still
honors `Obs` blocks; the fail-proof measures foreign-`Net`/`Pin`
constraints, which is what the spec asks.

## Determinism — `test_maze_determinism`

Two full `route_nets` per design: `assert_eq!(r1, r2)` on the whole
`RouteDb`, nonzero `route_digest` equality. Cost = 2× the census per
design — its runtime follows the picorv32 census number; a timeout
there is a perf finding, not a reason to weaken the test.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/maze.rs`
