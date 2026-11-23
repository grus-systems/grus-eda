# e6-drt-maze — Test plan

All maze tests already exist in `crates/grus-core/tests/drt.rs` at
`a52d771` — the remaining work is making them compile and pass, not
writing new ones. Existing E6 gate conventions apply: same
`load`/`load_guide`/`pin_label`/`require` helpers, fail-loud on
missing inputs; `picorv32.placed.def` is present in the worktree.

## Synthetic fixture — `test_maze_cases` (written)

`MAZE_LEF`/`MAZE_DEF_*`: met1 `HORIZONTAL` + met2 `VERTICAL` on a
6×6 shared-crossing grid (fixture grids coincide, so relaxed vias
degenerate onto strict positions), `mv12` between them, one OBS
macro (`blk`), a track-less pin (`u2/C`).

1. **Straight route** — `n1` met1-only guide; asserts exactly one
   merged `WireSeg` `(1000,1000)-(4000,1000)`, no vias, 2 connected,
   no collisions.
2. **Forced via** — `n2` two-layer guide; asserts exactly
   `ViaPlace { lower:0, x:4000, y:3000 }` — on this grid the relaxed
   edge's true position coincides with the strict crossing, and the
   CSR's strict-before-relaxed order wins the equal-cost tie.
3. **OBS detour** — same `n1` geometry with `blk`'s OBS on the
   direct line; asserts connected, `wires.len()` strictly greater
   than the case-1 control, and per emitted wire: no met1 segment
   enters the OBS rect inflated by `w/2 + s = 210`.
4. **Provably unroutable** — `n1`'s guide splits into two met1
   components (no met2 box ⇒ no upper nodes ⇒ relaxed edges cannot
   bridge it ⇒ `u0/A` = `NoPath`); `n3`'s `u2/C` covers no track
   (`Unattachable`). Asserts named pins/kinds, `nets` empty,
   bounded return (the harness completing is the no-hang evidence).

## Real-design census — `test_maze_census` (written)

Per design: `route_nets` over the committed guide; prints guided /
routed / pins_connected / projected / unrouted (named per pin +
`FailKind`) / collisions / iterations / ms / digest for
`results.md`. Asserts:

- `nets.len() + unrouted.len() == join.nets.len()` (no silent drops).
- Every `unrouted` entry names ≥1 `(pin, FailKind)`.
- Nonzero evidence: nets nonempty, every routed net has wires.
- `NoPath` classification: the `guide_union_connected` probe —
  `{li1,met1,met2}` union disconnected but the full union connected
  ⇒ met3-bridge. Every `NoPath` net must be met3-bridged or the
  assert lists the unclassified names. **Expected outcome after
  relaxed vias**: met2–met3 relaxed transitions may route this class
  outright (unrouted → 0 on both designs). If a residual remains,
  name the cause per net in the test comment (existing convention) —
  do not weaken the assert silently.

## Occupancy fail-proof — `test_maze_occupancy` (written)

gcd only: `Enabled` vs `Disabled`, identical inputs; asserts the
outcomes measurably differ (digest or `overlap_census` count).
Prints both digests + counts for `results.md`.

## Determinism — `test_maze_determinism` (written)

Two full `route_nets` per design: `assert_eq!(r1, r2)` on the whole
`RouteDb`, nonzero `route_digest` equality.

## Removals

- `test_maze_debug_gcd` — the NoPath-flood diagnosis harness; goes
  with the `debug_*` fns (`steps.md` §1.6).

## In-module unit tests (`drt/maze.rs`, written)

Band math (abut-at-`s` legal, strict `<`), perpendicular crossing,
projection fallback on an escaped pin, PQ tie order, square-graph
tree-extension path shape. If a relaxed-via fixture assertion is
added during bring-up (e.g. a sparse-crossing grid where only a
relaxed edge routes), keep it in-module — the synthetic fixture
above already covers the public contract.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/maze.rs`
