# e6-drt-maze — Documentation updates (remaining)

All English, matching existing style. `docs/e6-drt.md` §7 is
already landed at `4ff7ade` — this file covers only the three
remaining documents. Every number written into `results.md` comes
from the `measurement.md` captures, verbatim.

## `docs/results.md`

Append a dated entry at the end:

`### 2026-11-23 — e6-drt-maze: sequential maze router (v0.12.4)`

Content, in the established entry style:

- **What landed** (one dense paragraph): sequential maze routing
  over the guide-bounded track graph — pathfinder tree extension
  with the `g + hop` goal-directed PQ, the unified
  `[in-plane | strict via | relaxed via]` edge space with `NetTopo`
  cached across iterations, cost constants
  `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 / HIST 8 / MAX_ITERS 8`,
  per-line interval occupancy (`Obs` host-carved hard, `Net`
  soft→hard, `Pin` soft-only), projection attachment fallback,
  spare-layer region, the `bad`-set rip-up rule, and the named
  per-pin `Unattachable`/`NoPath` report. `GRUS_DRT_TIMING` is the
  opt-in stderr instrumentation.
- **Diagnosis record**: the 48-net gcd `NoPath` residual at
  `2c3c713` → 0 after the host-cell carve + `Pin` class (see
  `measurement.md`).
- **Census table** per design: guided, routed, pins_connected,
  projected, unrouted (count; every residual net named with its
  per-pin `FailKind` and the `met3_bridge` class), collisions,
  iterations, `ms=`, `digest=` — with the test profile stated.
  gcd row is already measured (`269/269, 931 pins, 17 projected,
  0 unrouted, 0 collisions, 8 iterations, 9,704 ms,
  0xfbb3073487860974`, dev profile). picorv32's row is the
  acceptance number — verbatim `ms=`; if it lands near the
  earlier 30+ CPU-min release bound, **say so explicitly**.
- **Occupancy fail-proof**: Enabled census=0 vs Disabled
  census=6,429 on gcd, digests differ — occupancy measurably
  constrains.
- **Determinism**: two full runs produce identical `RouteDb`s,
  nonzero digests on both designs (picorv32 digest from the
  measured run).
- **Fixture fail-proofs**: the four synthetic cases, incl. the
  named unroutable pins (`u0/A` `NoPath`, `u2/C` `Unattachable`).
- **Runtime work record**: the before/after table (gcd 203 s →
  9.7 s; picorv32 >21 CPU-min killed → measured), the levers
  (NetTopo cache, chunked skip index, `hop` heuristic, scratch
  reuse), and the `GRUS_DRT_TIMING` phase breakdown naming
  picorv32's dominant phase.
- **Checks run**: the exact commands incl. `--release` where
  used, the suite wall-clock time, and `0.12.3 → 0.12.4`.

## `docs/status.md`

Replace the `Last updated:` head paragraph — same dense
single-paragraph style, demoting the current `v0.12.3 e6-drt-via`
head to the next `Previous:` in the chain. Content: E6 ticket
`e6-drt-maze` (`v0.12.4`) — the sequential router (tree extension
+ `hop` heuristic, cost constants, interval occupancy with
host-cell carve + `Pin` class, projection attach, relaxed vias +
stubs, spare layer, rip-up rule); measured census numbers incl.
the picorv32 runtime; fail-proof + determinism evidence; named
residuals (met3-bridge class if picorv32 unrouted nets exist,
the met2–met3 node-model gap standing); remaining = DEF emission
(`e6-drt-emit`) + e2e.

## `docs/roadmap.md`

One `- Progress 2026-11-23 (`v0.12.4`, ticket `e6-drt-maze`):`
bullet under the E6 block, immediately after the `e6-drt-via`
progress line and before the `**E7 DRC**` paragraph. Same style:
what the router does in one phrase, the census numbers for both
designs, the measured runtime with profile, named residuals,
remaining (DEF emission + e2e).

## `docs/e6-drt.md` — spot-check only

§7 is written and committed. While composing `results.md`,
re-verify its factual anchors against `maze.rs` at HEAD (the
constant table, the `hit_verdict` occupancy table, the rip-up
rule, `GRUS_DRT_TIMING`). Any drift: fix the doc line in the same
change and note it in the commit body — code wins.

## Files

- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `docs/e6-drt.md` (drift fixes only)
