# drt-guide-escape — overview

## Problem (external review)

A pin whose metal lies outside its guide region has no legal attach
point: graph build records `RegionEscape`, the attach ladder finds
nothing, and the pin fails `Unattachable` — or its in-region attach
nodes are sealed by OBS and it fails the same way. The guide is
advisory; a pin a few track lines outside its box is recoverable
geometry, not a hard error.

## Re-baseline verdict (post-`e6-drt-e2e` dev, measured at this
worktree's HEAD `72dcc9c`)

The attach ladder already landed four rescue stages — the remaining
escape class is small but real:

- **Strict** `pin_nodes` (in-region node inside the pin rect).
- **`project_attach`** (`maze.rs` ~L753): in-region nodes within
  ±1 line-index of the pin rect's covered ranges, admitted on
  `mask.seg_allowed` + premark stub scan.
- **`access_vias`** (~L1383, A2/A3/A4 landed): pin-anchored mcon/mvia
  inside the pin-rect union on masked layers (li1/met1 only, io
  ports skipped), `b` landing within `B_EXT = 2` of the contact range.
- **Host-hull transit** (`transit_extend`, `graph.rs` ~L788): first-
  unmasked-layer hull interior + unbounded L-corridor to the region
  for every cell hosting a member pin. Plus the `+1` spare transit
  layer all maze legs already build with.

Measured outcome on gcd (`docs/results.md` 2026-11-26,
`test_maze_census` / `test_pin_attachment`):

- 17 `(net, pin)` pairs record `region_escape` at graph build
  (3 instance pins `_301_/B1`, `_307_/B1`, `_313_/X`; 14 io ports).
- 15 of 17 re-attach via projection/access arcs — only **2** fail
  lines carry ` region_escape`: `_301_/B1` on `_134_` and `_307_/B1`
  on `_138_`, both `Unattachable`.
- `unrouted=81` total: `NoPath=46 Unattachable=52` per-pin. The other
  ~50 `Unattachable` are the **walled** class — in-region attach
  candidates all blocked (`stub_blocked` → `stub_outside_cover`
  census, 142 records) — not graph escapes.
- picorv32 (v0.12.9 graph pin): 592 escapes (355 instance, 237 io);
  the final flagged-`Unattachable` count needs a full emit leg —
  deferred like every picorv32 leg (`steps.md` §6).

Remaining class decomposition:

- **C1 (in scope)** — strict-escape pins beyond every rescue's reach:
  nearest in-region node ≥2 lines on every pin layer *and* no
  access-arc landing, *and* no mask-legal node inside the pin's own
  rect to anchor the descent. For the gcd instance pins the likely
  gap is the li1→met1→met2 chain: transit gives met2 hull coverage,
  but no li1 pin-rect node and no met1 hull node exist for the
  via descent (confirm per-pin via `DRT_DEBUG_NET` before wiring —
  `steps.md` step 0).
- **C2 (named residual, not this mechanism)** — pins with strict or
  projected candidates all occupancy-blocked. They already fail
  named (`stub_outside_cover` + `fail` lines); a route-time escape
  retry is explicitly rejected below. The census delta records
  whether C1's halo incidentally rescues any.
- **C3 (out of scope)** — `NoPath` pockets incl. the `met3_bridge`
  guide-union disconnect (138 on picorv32) — different defect class.

## Mechanism decision

**Bounded halo + bounded corridor at graph build** — a new
`escape_extend` pass inside `build_route_graphs_ext`, between
`transit_extend` and node emission (`mechanism.md`):

- For each member pin with no strict attach on the post-transit
  region (exactly today's `region_escape` set — pre-halo semantics
  preserved, so the pinned 17/592 census and its names do not move):
- add **halo pairs** — track pairs covered by the pin's shape bbox
  inflated `ESCAPE_HALO` index steps per axis, on every layer from
  the pin's lowest shape layer up to `max(highest shape layer,
  first unmasked layer)` (li1 pin ⇒ {li1, met1, met2}) — each pair
  mask-filtered by `hull.point_allowed`;
- if no halo pair already merges with the region (index-adjacent),
  add a **bounded L-corridor** on the argmin-span layer, Manhattan
  index distance `≤ ESCAPE_CORRIDOR` (the `transit_extend` corridor
  pattern with a bound); refused corridors are recorded, not retried.
- Every strict-escape pin gets an `EscapeRecord`
  `{net, pin, kind, pairs_added, corridor_span}` on
  `RouteGraphs.escapes`; `EscapeKind` = `Halo | CorridorBound |
  CorridorMasked | HaloEmpty`.

Occupancy is **unchanged**: halo/corridor pairs are ordinary
`region` entries — nodes, edges, via edges, `edge_cost`, extraction,
`commit_route` rasterization and the collision censuses all apply
identically. "Escaped geometry is occupancy-checked like normal
metal" holds structurally, not by audit (`fixture.md` arm C proves
it red/green).

Rejected alternatives:

- **Region-expansion retry at route time** (the spec's other
  candidate): rebuild the net's `NetGraph`/`NetTopo` with inflated
  guide boxes when `route_one_net` fails escaped. Requires graph
  mutation mid-`route_nets` (topos are prebuilt per net; occupancy
  premarking already ran), doubles the build cost for escape nets,
  and can rescue the OBS-walled class this design defers anyway —
  invasive plumbing for unmeasured gain.
- **Halo for every pin** (not only strict-escapes): grows every
  graph for a defect measured at 17/269 nets; the walled class it
  would help can't even be detected at graph build.
- **Unbounded corridor** (`transit_extend`-style): a corridor is a
  silent guide rewrite — an arbitrarily long escape path turns a
  real guide miss into unbounded paint. The bound keeps the failure
  named: `corridor_bound(span)` instead of painted-through.

## Honesty contract

- `region_escape` keeps its pre-halo meaning ("no strict node in the
  guide+transit region") — pinned names/counts unchanged.
- Every strict-escape pin carries an `EscapeRecord` naming what the
  mechanism did; a pin still unrouted reports `FailKind` +
  `escape=<kind>` on its `fail` line (`reporting.md`).
- Successful escapes are ordinary routed metal — no emit-side
  special-casing, no census exemption.
- The fixture proves red (escape off ⇒ `Unattachable` +
  `region_escape`) before the mechanism lands, and proves occupancy
  applies to escaped metal (arm C, enabled-vs-disabled contrast).
- The OBS-walled class stays a named residual — measured in the
  census delta, not silently folded into this mechanism's claims.

## Files

- `crates/grus-core/src/drt/graph.rs` — `EscapeKind`/`EscapeRecord`,
  `RouteGraphs.escapes`, `escape_extend`, `GraphOpts`/`EscapeCfg`,
  region_escape push condition.
- `crates/grus-core/src/drt/maze.rs` — nothing in the wave; only if
  the fixture needs a seam (expected: none).
- `crates/grus-eda/src/main.rs` — `fail`-line `escape=` suffix join.
- `crates/grus-core/tests/drt.rs` — `test_maze_guide_escape` fixture,
  `test_pin_attachment` escape-arm update, graph census re-pins.
- `tests/designs/recount_unrouted.py` — `fail`-line parser for the
  new suffix.
- `tests/designs/gcd/gcd.drt.ours.routed.def` — regenerate if routes
  move (`steps.md` §6; host leg, `generate_drt_ours.sh gcd`).
- `docs/e6-drt.md` — attach-ladder stage + census paragraph.
- `docs/results.md` — dated entry.
- `docs/status.md` — only if a named number moves.
- `Cargo.toml` — `workspace.package` patch bump.
