# e6-drt-maze — Documentation updates

All English, matching existing style. §7 content is fully determined
by the committed code — write it from `maze.rs`, `search.md`, and
`occupancy.md` as they stand at `cf34607` (code identical to the
measured `c1832dc`); only `results.md` waits on the picorv32
measurement (AGENTS §2: measured, never estimated). Per card
feedback #4, every bullet below must be re-read against the code
before it is written into `e6-drt.md` — the verified anchors are in
`steps.md` §0; a drift means the code wins and the delta is noted in
the commit body.

## `docs/e6-drt.md`

New section **"7. Maze search"** after §6, plus §4/§5 amendments:

- **Region input**: `build_route_graphs_ext(spare_above = 1)` — each
  box's XY also covered on `top_guided + 1`; the recorded answer for
  OBS-sealed pin pockets (gcd `_038_` precedent). Amends the §5
  "guide layers bind" implication — guide boxes bound XY, the layer
  choice does not bind detailed routing.
- **Attach**: strict `pin_nodes` first; the projection fallback
  (in-region nodes on the rect's covered lines ± one line, only when
  strict is empty) — the recorded answer to the §5 router
  implication for io-port/GCell-boundary escapes; looseness stated
  (tap node may sit off the pin shape; connectivity-level only;
  `projected` names it — gcd: 17 pins).
- **Edge classes**: in-plane, strict via (shared `(x,y)` crossing),
  and **relaxed via** — a real via needs a *preferred*-track
  crossing (vertical layer's x-track × horizontal layer's y-track),
  not a same-layer-node crossing. Strict crossings coincide every
  7820 DBU on li1–met1/met1–met2 and never on met2–met3 (the §6
  gap), so strict edges alone leave pin areas disconnected. Per
  lower-layer node, one relaxed edge to the nearest upper preferred
  line + nearest in-region landing node; the true position, the
  committed stub wires (occupancy-scanned via `scan_stub`, emitted
  as real `WireSeg`s), the single-nearest rule, and the guide-box
  looseness all stated as simplifications.
- **Search**: pathfinder tree extension — `NetTopo` built once per
  net (attach sets + relaxed vias + CSR over the unified
  `[0,E)|[E,E+V)|[E+V,E+V+R)` index space); seed at driver attach
  set; per round a unit-weight target BFS fills `hop` and a
  `(g + hop, node)` PQ pops the same minimum-`g` target Dijkstra
  would; `absorb_taps`; bounded backtrack; `NoPath` on exhaustion.
- **Costs**: the constant table (`PREF 1`, `OFF_AXIS 2`, `VIA 4`,
  `OCC 8`, `HIST 8`, `MAX_ITERS 8`), the preferred-axis mapping,
  hop-count-not-distance.
- **Occupancy**: per-line rasterized-interval model — strict
  `w/2 + s` band, `lo`-sorted `ivs` + `SKIP_CHUNK = 16` `max_hi`
  skip index; **unmerged** `obs_rects_raw` marks; the three occupant
  classes — `Obs(cell)` hard except the unconditional **host-cell
  carve** (member pin hosted by `cell` ⇒ invisible; the rasterized
  "access shapes ignore own-instance OBS" rule, recorded looseness:
  whole-cell crossing allowed), `Net` own-free/foreign soft→hard on
  the last iteration, `Pin` own-free/foreign soft-only (**never**
  hard — a pin cannot be ripped up; a landing wire is an emit-time
  finding); `Obs(u32::MAX)` net-less pin metal never carved; stub
  scanning; cut-layer + io-port rects unmodeled.
- **Order/rip-up**: join-order nets, provisional collisions,
  `bad` = unrouted ∪ both parties, per-net `hist`, cap 8, hard last
  iteration.
- **Unrouted taxonomy**: `Unattachable` vs `NoPath`, per pin.
- **`GRUS_DRT_TIMING`**: the opt-in stderr knob stays — one line.
- **Claim boundary additions**: routes are connectivity + painted
  intervals — not DEF text (emit ticket), not DRC-clean, not
  TritonRoute's FlexGridGraph. Update §4 and the §5
  router-implication paragraph: projection attachment + the spare
  layer are the shipped answers for escapes/pockets, and relaxed
  transitions bridge the strict-crossing gap *within* a pair whose
  upper preferred lines exist — the met2–met3 pair itself still has
  no transition path (disjoint grids ⇒ the §6 "dead capacity" note
  survives as the met3-bridge residual class; state what the
  picorv32 census measured).

## `docs/results.md`

Dated `### 2026-11-23 — e6-drt-maze: sequential maze router (v0.12.4)`
entry:

- What landed (one paragraph): sequential maze routing over the
  track graph — tree extension, relaxed vias + stubs, spare layer,
  projection attach, interval occupancy with host carve + `Pin`
  class, rip-up rule.
- **Diagnosis record**: the 48-net gcd `NoPath` residual at
  `2c3c713` resolved as C1 (unconditional host carve — approach
  edges blocked outside the old halo) + C2 (`Pin` class — pin metal
  must never hard-block); evidence = post-fix `unrouted=0`.
- **Census table** per design: guided, routed, pins connected,
  projected, unrouted (count + every net/pin named with `FailKind`
  and cause class), collisions, iterations, **runtime ms** —
  gcd `ms=9704` measured; picorv32's measured `ms=` is the
  acceptance number: record it verbatim, and if it lands near the
  earlier 25+ CPU-min bound, **say so explicitly** (reviewer
  instruction) — the debug-profile number, not an estimate.
- **Determinism**: double-run identical `RouteDb`, nonzero digest
  (gcd `0xfbb3073487860974`; picorv32's measured digest).
- **Occupancy fail-proof**: Enabled `census=0` vs Disabled
  `census=6429`, digests differ — occupancy measurably constrains.
- **Fixture fail-proofs**: the four synthetic cases incl. the named
  unroutable pins (`NoPath` + `Unattachable`).
- **Runtime work record**: levers landed (NetTopo cache, skip
  index, hop heuristic, scratch reuse) and the before/after —
  gcd 203 s → 9.7 s; picorv32 >21 CPU-min killed → measured.
- **Checks run**: exact commands; version bump `0.12.3 → 0.12.4`.
- Residual unrouted nets stay in the log with the cause (named-
  residual convention).

## `docs/status.md`

Replace the `Last updated:` head — dense one-paragraph style per the
existing entries: E6 ticket `e6-drt-maze` (`v0.12.4`), the sequential
router (tree extension + `hop` heuristic, cost constants, interval
occupancy + host-cell carve + `Pin` class, projection attachment,
relaxed vias + stubs, spare layer, rip-up rule), the measured census
numbers incl. picorv32 runtime, the C1/C2 blocker resolution,
fail-proof + determinism evidence, what remains (`e6-drt-emit`).

## `docs/roadmap.md`

Progress line under the E6 block after the `e6-drt-via` entry: what
the router does, census numbers (269/269 + picorv32's), runtime,
what remains (DEF emission / e2e).

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
