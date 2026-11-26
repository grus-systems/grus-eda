# drt-guide-escape — overview

## Problem (external review)

A pin whose metal lies outside its guide region has no legal attach
point: graph build records `RegionEscape`, the attach ladder finds
nothing, and the pin fails `Unattachable`/`NoPath` — or its
in-region attach nodes are sealed by OBS and it fails the same way.
The guide is advisory; a pin a few track lines outside its box is
recoverable geometry, not a hard error.

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

**Bounded halo + bounded corridor at graph build** — `escape_extend`
inside `build_route_graphs_ext`, between `transit_extend` and node
emission (`mechanism.md`):

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
  `RouteGraphs.escapes`; `EscapeKind` =
  `Halo | CorridorBound | CorridorMasked | NoAnchor | HaloEmpty`.

Occupancy is **unchanged**: halo/corridor pairs are ordinary
`region` entries — nodes, edges, via edges, `edge_cost`, extraction,
`commit_route` rasterization and the collision censuses all apply
identically. "Escaped geometry is occupancy-checked like normal
metal" holds structurally — and is now proven by the fixture's
`OccMode` enabled-vs-disabled arm (`fixture.md` arm F), not only by
audit.

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
  named: `corridor_bound(span=N)` instead of painted-through.

## Review revision (2026-11-26)

The first design pass diverged from the landed implementation in
four places; the contract is corrected here (same class as the
converge-stats revision — the design document is the contract):

1. **Escape-off `FailKind`: `NoPath`, not `Unattachable`.** The old
   fixture.md predicted `Unattachable`; the correct outcome is
   `NoPath`. With escape off, the strict-escape pin holds no strict
   node — but `transit_extend` plants the host-hull interior pairs
   on the first unmasked layer *for every member-pin host
   regardless of the escape switch*, and the route-time
   `access_vias` rung lands `b` on that transit-island node inside
   the pin's own hull → `attachable` holds → the wave dies on the
   one-node island → `NoPath`. `Unattachable` is only the outcome
   when *every* attach rung is empty (e.g. an io-port pin with no
   host hull, hence no transit island, and no projection/access
   landing). fixture.md, `docs/e6-drt.md`, and the `results.md`
   fixture line are amended to say so (`docs.md`).
2. **Occupancy arm restored (fixture arm F).** The old arm C —
   the `OccMode` enabled-vs-disabled contrast proving escaped metal
   is occupancy-checked — was dropped in implementation and the drop
   undocumented. It is feasible and specced in `fixture.md`: the
   band must be a foreign **`Pin`** or **`Net`** mark, because
   `hit_verdict` (`maze.rs` ~L850) returns `None` for `Obs` and
   foreign `ObsPin` *before* the `Disabled` check — those classes
   hard-block in both modes and give no contrast. A foreign io-port
   `Pin` rect on met2 covering exactly the escaped-pair column gives
   enabled `NoPath` vs disabled routed.
3. **`pairs_added` counts the union.** On the corridor-landed arm,
   `halo_total + extra.len()` double-counts the corridor layer's
   halo pairs (`extra = path ∪ halo[layer]`). The record's contract:
   `pairs_added` = the number of *distinct* pairs inserted into
   `region`; the fixture's landing arm pins the exact count (`== 3`
   for the fixture geometry; the old arithmetic reported 4).
4. **`escape_span` is a bounded enumeration, not a grid BFS.**
   Distance on an unobstructed 4-connected index grid *is*
   Manhattan distance — the multi-source BFS over `nx × ny` (with a
   `vec![u32::MAX; nx*ny]` allocation per escaped pin per layer)
   computed nothing more. The probe enumerates `region` pairs once,
   pruning each `v` in O(1) by its distance to the halo bbox, and
   takes a `cap`: corridor candidates probe with `cap = bound`
   (work bounded by `corridor_lines`, the early exit); the refusal
   path re-probes uncapped so `CorridorBound` records the *exact*
   measured gap (`mechanism.md` §4).
5. **`EscapeKind::NoAnchor` for the no-target case.** "No non-halo
   region pair on any escape layer" was folded into `CorridorBound`
   with `corridor_span = u32::MAX`, which rendered
   `corridor_bound(span=4294967295)` on fail lines. The explicit
   kind records `corridor_span = 0` and renders `escape=no_anchor`
   (`mechanism.md` §5, `reporting.md`).

## Honesty contract

- `region_escape` keeps its pre-halo meaning ("no strict node in the
  guide+transit region") — pinned names/counts unchanged.
- Every strict-escape pin carries an `EscapeRecord` naming what the
  mechanism did; a pin still unrouted reports `FailKind` +
  `escape=<kind>` on its `fail` line (`reporting.md`).
- Successful escapes are ordinary routed metal — no emit-side
  special-casing, no census exemption.
- The fixture proves red (escape off ⇒ `NoPath` + `region_escape`)
  before the mechanism lands, and proves occupancy applies to
  escaped metal (arm F, enabled-vs-disabled contrast on a band that
  geometrically can only intersect escaped pairs).
- The OBS-walled class stays a named residual — measured in the
  census delta, not silently folded into this mechanism's claims.

## Files

- `crates/grus-core/src/drt/graph.rs` — `EscapeKind::NoAnchor` +
  `token()` arm, `escape_span` bounded-enumeration rewrite,
  `pairs_added` union count, refusal-path uncapped probe.
- `crates/grus-core/tests/drt.rs` — `test_maze_guide_escape` arm F
  (`ESC_DEF_TAIL_OCC` + the io4/n2 met2 `Pin` band; enabled-vs-
  disabled contrast), landing-arm `pairs_added == 3`,
  `test_region_escape_fixture` empty-guide arm → `NoAnchor`,
  `test_pin_attachment` refused-kinds match arm.
- `docs/e6-drt.md` — §5 outcome list (`no_anchor` + the
  `Unattachable`/`NoPath` correction), §8 `fail`-line token list.
- `docs/results.md` — amend the 2026-11-26 fixture line
  (`Unattachable` → `NoPath`, with the transit-island reason) and
  append the revision paragraph (`docs.md`).
- `docs/status.md` — the v0.13.0 blurb's token list (`no_anchor`).

No `maze.rs`, `main.rs`, or `recount_unrouted.py` changes: arm F is
test-side (`MazeConfig.occ`), `token()` lives next to `EscapeKind`,
and the parser already accepts any `escape=` suffix. No version
bump: the workspace is at the unreleased `0.13.0` on this branch —
the added enum variant rides the same breaking bump.
