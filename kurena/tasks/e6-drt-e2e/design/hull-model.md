# e6-drt-e2e — instance-hull occupancy model

The core fix: make the maze's occupancy model conservative w.r.t.
subcell-internal geometry it cannot see. Per the card's direction:
inside any instance bbox (including decap/filler/tap cells) block
all maze nodes on every layer for every net, except node geometry
fully overlapping a pin rect of that instance's own net.

## Why the current model is loose

`premark_obs` rasterizes `mac.obs_rects_raw` → `Obs(cell)` over each
placed instance. `hit_verdict` then makes `Obs(cell)` invisible to
any net that owns a member pin on `cell` ("host nets") — an
unconditional carve documented as recorded looseness in
`docs/e6-drt.md` §7. Two consequences measured by the red gate:

- Host nets route *through* their own cells' OBS/internals —
  consistent with error bboxes inside `_328_`-class instance bboxes.
- Two sources of subcell geometry are absent entirely:
  `mac.pin_obs_rects` (OBS declared inside `PIN` blocks — used by
  `drc::geom` source 4 but never by the maze) and everything Magic
  sees inside the master that LEF OBS does not declare
  (nwell/diff/poly and undeclared li1/met1 paint).

The delegated reference proves the real rule is finer than a bbox
ban: 234 li1/met1 segments in `gcd.routed.def` cross foreign
instance bboxes through genuine whitespace, Magic-clean. Our model
cannot see whitespace, so the bbox hull is the conservative
substitute — the cost is connectivity, not correctness.

## Model

For every cell `C` with `cell_status ∈ {Placed, Fixed, Cover}` and
every routing layer `l`, the **hull** is the placement bbox

```
H(C, l) = place_C(RectDbu(0, 0, mac.width_dbu, mac.height_dbu))
```

(`place_C` = `orient_rect` under `cell_orient[C]` + translate to
`cell_origin[C]` — the same transform `drc::geom` uses for macro
shapes.)

For each net `N`, the **own-pin cover** is

```
cover(N) : (CellId, LayerId) → [RectDbu]
```

built from `access::pin_shapes(db, tech, p)` over `N`'s member
instance pins `p`, grouped by `(pin_cells[p], lid)`. IO BTERMs are
not inside instance hulls; decap/filler/tap cells carry no member
pins, so they contribute no cover and are hulls for every net —
matching the card's parenthetical.

**Availability rule**: a maze node `(l, ix, iy)` at track crossing
`(x, y)` is emitted into `N`'s graph iff `(x, y)` lies in no hull,
or lies in `H(C, l)` where `C` hosts a member pin of `N` **and**
`(x, y)` is inside some rect of `cover(N)[(C, l)]`.

"Node geometry fully overlapping a pin rect" has two readings;
default to point-in-rect (consistent with the existing attach rule)
and keep the tightening knob: erode each cover rect by the layer's
half wire-width (`r ⊖ w/2`) so the node's implied ±w/2 crossing is
*contained* in the rect — apply if `Can't overlap those layers` /
partial-overlap residuals persist at pin edges after the first
measurement.

## Mechanics

Apply the mask at node emission, not in `Occupant`. Node-level
blocking is the only mechanically consistent choice: an occupancy
mark blocks *edges*, which would strand the approach edge into an
exempted pin node; removing nodes prunes the graph itself — foreign
transit dies because the chain of nodes through a hull is gone, and
host nets keep exactly the nodes inside their own pin rects.

In `build_route_graphs_ext` (after `net_region`, before node
emission):

1. Build once per design, per layer: a spatial index of hulls as
   track-index rects `[(ix0..ix1) × (iy0..iy1), CellId]` — instances
   snap to the grid, so DBU→index is exact (hull edges fall between
   crossings; use `partition_point` bounds like `Line` membership).
   Interval query via the same sorted-start + max-`iy1` skip pattern
   `Line::max_hi` uses; ~384 hulls on gcd, ~19k on picorv32.
2. Build once per net: `cover(N)` (member pins only — small).
3. Filter each `(net, layer)` region pair list: drop pairs whose
   node is masked. `pin_nodes`, `via_edges`, `relaxed_vias` then see
   only surviving nodes — a via edge survives iff *both* endpoints
   survive on their layers, so a via drop inside a pin rect is legal
   only where the pin carries port shapes on both layers.
4. Census: count masked nodes per design (`hull_masked` field on
   `RouteGraphs`) and print it in the emit census so the connectivity
   cost is visible.

Unchanged: `Obs(cell)` marks + the host carve stay — with foreign
nets barred from hulls, the marks matter only at hull boundaries
(spacing for wires skirting edges) and the carve still opens the
approach corridor into surviving pin-rect nodes. `project_attach`
keeps its semantics; a projection whose node lands inside a hull
outside own-pin cover is masked like any other.

Also in `premark_obs`: chain `mac.pin_obs_rects` alongside
`obs_rects_raw` — the OBS source `drc::geom` already uses and the
maze has been missing.

## Committed-geometry confinement

Node masking alone leaves one gap: a committed stub or min-area
patch endpoint inside a hull but outside own-pin cover (e.g. an
access stub that overruns the port, a patch walking pin-rect
nodes). `patch_component` is already occupancy-strict and walks
`g.edges`, so masked nodes bound it for free; access stubs emitted
by `project_attach`/`apply_routes` must be checked against the same
rule — a stub endpoint inside `H(C) \ cover(N)` is a named residual
(`stub_outside_cover`), not emitted silently.

## Connectivity consequence and the evaluation ladder

Strict all-layer masking makes interior pins graph islands: their
exempted nodes have no surviving neighbors, and via escape requires
port shapes stacked on adjacent layers (li1+met1 ports exist on
many sky130 pins; li1-only ports cannot escape upward). The card
anticipates this — "expect unrouted to rise" — and the design names
the residual classes (below). If measurement shows near-total
starvation (gcd's 269 nets collapsing to conns-only output), the
gate would technically pass on an empty route — not an acceptable
landing. Evaluate the ladder in order, re-running
`generate_drt_ours.sh <d>` + `check_drt_ours.sh <d>` per variant:

1. **L1 — painted-layer restriction.** Apply the hull mask only on
   layers the fired rules name as carrying master internals:
   {li1, met1} (li.*, li.c*, met1.*). Investigate the 8 met3.2 hits
   separately — sample their bboxes; if they are top-level
   boundary/port artifacts rather than interior crossings, they
   belong to a different residual class, not the hull. met2+ stays
   open → via-drop onto a port + upward escape restores most
   connectivity, and cells provably carry no met2+ internals.
2. **L2 — approach corridor.** For host nets, extend cover to the
   monotone corridor from each own pin rect to the nearest hull edge
   along the layer's preferred direction (the path an access stub
   takes). Still conservative: residual class `corridor_overlap`
   names corridors that cross invisible internals.
3. **L3 — port-cover emit.** Keep the mask; where the 143-name
   "abut or partially overlap" residuals concentrate at attach
   points, emit the attach as a `RECT` covering the whole touched
   pin rect (`NetGeom` already carries `RECT` stubs) — Magic
   requires full coverage of the port at the boundary.

L1 is the expected landing if strict masking starves; the doc
records which variant shipped and the measured residual counts.

## Residual classes (named, counted, printed)

- `hull_masked` — nodes removed per (design, layer), census print.
- `island_pin` — pins with no surviving attach node on any layer
  (pin rect holds no exempted node reachable from outside). Lands
  in `NoPath`/`Unattachable`; subclass the emit census by cause.
- `foreign_transit` — nets whose guides forced hull transit on
  masked layers and unrouted for it.
- `stub_outside_cover` — committed stubs violating the confinement
  rule (should be 0 — a nonzero count is a bug, not a residual).
- `boundary_overlap` / `boundary_abut` — the two allowed
  unclassified Magic names, per-input counts; `ours` target 0, any
  residual named in `results.md`.
- Any nonzero `lef_expressible` count post-fix is a gate failure to
  chase, **not** a residual.

## Falsification arms

- Occupancy mask disabled (`--occ` off path or a test cfg) must
  reproduce the red gate's nonzero counts — proves the gate's
  nonzero-failure path exists (E0 lesson 7).
- A gate arm asserting `hull_masked > 0` on gcd — proves the mask
  actually fires on the design, not vacuously.
