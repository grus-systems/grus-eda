# e6-drt-maze — Documentation updates

All English, matching existing style.

## `docs/e6-drt.md`

New section **"7. Maze search"** after §6 (keep §4/§5 in step — see
the last item):

- **Attach**: strict `pin_nodes` first; the projection fallback
  (same-track-line nodes of a rect's covered axes, only when strict
  is empty) — the recorded answer to the §5 router implication for
  io-port/GCell-boundary escapes; looseness stated (tap node may sit
  off the pin shape; connectivity-level only; `projected` names it).
- **Edge classes**: in-plane, strict via (shared `(x,y)` crossing),
  and **relaxed via** — the model change discovered mid-
  implementation: a real via needs a *preferred-track* crossing
  (vertical layer's x-track × horizontal layer's y-track), not a
  same-layer-node crossing. Strict crossings coincide every 7820 DBU
  on li1–met1/met1–met2 and never on met2–met3 (the §6 gap), so
  strict edges alone left pin areas disconnected. Per lower-layer
  node, one relaxed edge to the nearest upper preferred line +
  nearest in-region landing node; the true position, the implied
  stub wires (committed, not searched), the single-nearest rule,
  and the guide-box looseness all stated as simplifications.
- **Search**: pathfinder tree extension — seed at driver attach set,
  repeated multi-source Dijkstra to the nearest unattached pin,
  `(cost, node)` PQ order, edge-list neighbor order, stamped
  dist/parent.
- **Costs**: the constant table (`PREF 1`, `OFF_AXIS 2`, `VIA 4`,
  `OCC 8`, `HIST 8`, `MAX_ITERS 8`), the preferred-axis mapping,
  hop-count-not-distance.
- **Occupancy**: per-line rasterized-interval model — strict
  `w/2 + s` band, `Obs` always hard **with the pin-carve exemption**
  (a net may touch the OBS rect its own pin sits on — measured fix
  for the li1 wall-in), foreign nets soft then hard on the last
  iteration, cut-layer + io-port rects unmodeled.
- **Order/rip-up**: join-order nets, provisional collisions,
  `bad` = unrouted ∪ both parties, per-net `hist`, cap 8.
- **Unrouted taxonomy**: `Unattachable` vs `NoPath`, per pin.
- **Claim boundary additions**: routes are connectivity + painted
  intervals — not DEF text (emit ticket), not DRC-clean, not
  TritonRoute's FlexGridGraph. Update the §4 bullet list and the §5
  router-implication paragraph: projection attachment is the shipped
  answer for escapes, and the met2–met3 strict-crossing gap is now
  bridged by relaxed transitions — state whatever the census
  actually measured (the "dead capacity" claim dies or shrinks to a
  named residual).

## `docs/results.md`

Dated `### 2026-11-2x — e6-drt-maze: sequential maze router` entry:

- What landed (one paragraph, incl. the two mid-implementation
  fixes: pin-carve + relaxed vias).
- **Census table** per design: guided, routed, pins connected,
  projected count, unrouted (count + every net/pin named with
  `FailKind` and the cause class), collisions, iterations,
  **runtime ms** (measured — picorv32 completing in-test is the
  acceptance evidence).
- **Determinism**: double-run identical `RouteDb`, nonzero digest.
- **Occupancy fail-proof**: Enabled vs Disabled measured numbers.
- **Fixture fail-proofs**: the four synthetic cases incl. the named
  unroutable pin.
- **Checks run**: exact commands; version bump `0.12.3 → 0.12.4`.
- Residual unrouted nets stay in the log with the cause (same
  convention as earlier named residuals).

## `docs/status.md`

Replace the `Last updated:` head — dense one-paragraph style: E6
ticket `e6-drt-maze`, the sequential router (tree extension, costs,
occupancy + pin-carve, projection attachment, relaxed vias, rip-up
rule), the measured census numbers incl. runtime, fail-proof +
determinism results, what remains (`e6-drt-emit`).

## `docs/roadmap.md`

Progress line under the E6 block after `e6-drt-via`: what the router
does, census numbers, what remains (DEF emission / e2e).

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
