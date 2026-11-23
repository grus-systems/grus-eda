# e6-drt-maze — Test plan

All maze tests exist in `crates/grus-core/tests/drt.rs` at `2c3c713`
— the remaining work is making them *green*, not writing new ones.
Existing E6 gate conventions apply: `load_routed_inputs` helper,
fail-loud on missing inputs; `picorv32.placed.def` is present in the
worktree.

## Measured state at HEAD

- `test_maze_cases` — **green** (0.00 s).
- `test_maze_census` — **red**: gcd leg `routed=221/269`,
  `unrouted=48` (all `NoPath`; 47 not met3-bridge) → the
  `unclassified.is_empty()` assert fires; picorv32 leg never ran
  (assert aborts first). Runtime 203 s for gcd alone.
- `test_maze_occupancy`, `test_maze_determinism` — untested at HEAD
  (each runs `route_nets` ≥2× on gcd at 203 s → minutes; picorv32
  determinism needs 2 full runs).

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

Re-check after the routability fix — especially case 3 under any
carve widening (`blk`'s OBS must still block `n1`; the net owns no
pin on `blk`, so C1 does not open it).

## Real-design census — `test_maze_census` (fix target)

Per design: `route_nets` over the committed guide (spare layer = 1);
prints guided / routed / pins_connected / projected / unrouted
(named per pin + `FailKind`) / collisions / iterations / ms / digest
for `results.md`. Asserts:

- `nets.len() + unrouted.len() == join.nets.len()` (no silent drops).
- Every `unrouted` entry names ≥1 `(pin, FailKind)`.
- Nonzero evidence: nets nonempty, every routed net has wires.
- `NoPath` classification — **expected post-fix state**: unrouted =
  0 on both designs under the committed guides (acceptance). If a
  residual survives the `occupancy.md` §5 fixes, the classifier is
  *extended with the named cause* (met3-bridge today; add e.g.
  `obs_walled`/`foreign_pin_walled`/`region_split` with the probe's
  evidence) — the assert stays strict; classes are named, never
  excused. Whatever lands is recorded in `docs/results.md`.

## Occupancy fail-proof — `test_maze_occupancy`

gcd only: `Enabled` vs `Disabled`, identical inputs; asserts the
outcomes measurably differ (digest or `overlap_census` count).
Prints both digests + counts for `results.md`. Note: `Disabled`
still honors `Obs` blocks — the fail-proof measures foreign-`Net`
constraints, which is what the spec asks ("a fail-proof (skip
occupancy) measurably changes routes or collisions").

## Determinism — `test_maze_determinism`

Two full `route_nets` per design: `assert_eq!(r1, r2)` on the whole
`RouteDb`, nonzero `route_digest` equality. Runtime note: this is
2× the census cost per design — it passes the same perf budget as
the census (`perf.md` §3); if it does not, that is a perf finding,
not a reason to weaken the test.

## Diagnosis instrumentation (temporary)

One bounded probe (e.g. `debug_pin_block`) — same lifecycle as the
deleted `debug_*` fns: for each failed pin, enumerate its attach
nodes' incident-edge verdicts and classify the blocker
(`Obs(host)` / `Obs(other)` / `Net(foreign)` / none ⇒ component
split). Plus `OccMode::Disabled` control + per-phase ms + edge
counters (`perf.md` §0). Removed before the ticket lands.

## In-module unit tests (`drt/maze.rs`, written)

Band math (abut-at-`s` legal, strict `<`), perpendicular crossing,
projection fallback on an escaped pin, PQ tie order, square-graph
tree-extension path shape. If the routability fix adds a carve rule,
add a matching unit case (e.g. `Obs(host)` beyond the halo but on
the host cell) — keep it in-module.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/maze.rs`
