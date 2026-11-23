# e6-drt-maze — Documentation updates

All English, matching existing style.

## `docs/e6-drt.md`

New section **"7. Maze search"** after §6 (keep §4 claim boundary in
step — see last item):

- **Attach**: strict `pin_nodes` first; the **projection fallback**
  (same-track-line nodes of a rect's covered axes, only when the
  strict set is empty) — name it as the recorded answer to the §5
  router implication (io-port/class-(a) escapes), with the looseness
  stated: the tap node may sit off the pin shape, connectivity-level
  attachment only, emit decides the stub geometry.
- **Search**: pathfinder-style tree extension — seed = driver attach
  set (`net_driver`, else first member), repeated multi-source
  Dijkstra to the nearest unattached pin's set, `(cost, node)` PQ
  order, edge-list neighbor order.
- **Costs**: the constant table (`PREF 1`, `OFF_AXIS 2`, `VIA 4`,
  `OCC 8`, `HIST 8`, `MAX_ITERS 8`), the preferred-axis mapping
  (`Horizontal` → `Axis::X` edges — the direction-to-axis flip),
  hop-count not distance weighting (recorded simplification).
- **Occupancy**: the per-line rasterized-interval model — the
  strict-band rule, `Obs` always hard, foreign nets soft then hard
  on the last iteration, cut-layer and other nets' pin shapes
  unmodeled (named simplifications).
- **Order/rip-up**: join-order nets, provisional collisions from
  soft-mode hits, `bad` = unrouted ∪ both parties, per-net `hist`
  escalation, cap 8.
- **Unrouted taxonomy**: `Unattachable` vs `NoPath`, reported per
  pin.
- **Claim boundary additions**: routes are connectivity + painted
  intervals — not DEF text (emit ticket), not DRC-clean (enclosure/
  min-area post-checks), not TritonRoute's FlexGridGraph; met3+
  regions are dead capacity while met2–met3 has zero shared
  crossings (the deferred node-model gap — update the §4 bullet
  list and the §5 router-implication paragraph to point at §7's
  projection attachment as the shipped answer for escapes and to
  name the met3-bridge residual class).

## `docs/results.md`

Dated `### 2026-11-2x — e6-drt-maze: sequential maze router` entry
in the established shape:

- What landed (one paragraph).
- **Census table** per design: guided nets, routed, member pins
  connected / projected-attachment count, unrouted (count + every
  net/pin named with `FailKind` and the met3-bridge vs
  other-cause classification), collisions, iterations, **runtime
  ms** (the acceptance's required measured number — picorv32's
  completion in-test is itself the evidence).
- **Determinism** line: double-run identical `RouteDb`, nonzero
  digest.
- **Occupancy fail-proof**: Enabled vs Disabled — what differed
  (digests and/or post-hoc collisions) with the measured numbers.
- **Fixture fail-proofs**: the four synthetic cases, including the
  unroutable pin returning a named failure (bounded, no hang).
- **Checks run**: exact commands; whether `picorv32.placed.def` was
  present or regenerated; version bump `0.12.3 → 0.12.4` (feature,
  patch on 0.x); commit policy per the run card.
- If a residual unrouted set exists, keep it in the log with the
  cause — same convention as earlier named residuals.

## `docs/status.md`

Replace the `Last updated:` head — dense one-paragraph style: E6
ticket `e6-drt-maze`, the sequential router (tree extension, cost
constants, occupancy model, projection attachment, rip-up rule),
per-design census numbers incl. unrouted names and runtime,
fail-proof + determinism results, what remains (`e6-drt-emit`).

## `docs/roadmap.md`

Progress line under the E6 block after the `e6-drt-via` entry:
ticket `e6-drt-maze`, what the router does, census numbers, what
remains (DEF emission / e2e).

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
