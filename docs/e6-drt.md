# E6 — Detailed Routing Substrate (`docs/e6-drt.md`)

The on-grid model the maze router needs: which tracks exist per layer,
and where each routed pin can legally attach to them. The delegation
reference this substrate replaces is `e6-drt-ref`
(`tests/designs/generate_drt_ref.sh`, `<d>.drt.reference.json`).

Non-goals for this stage: maze routing, via insertion, DEF route
emission, track assignment (choosing among candidate access points),
and DRC-aware pruning of access points (spacing to macro OBS is the
router's concern, not the model's).

## 1. Track model

Track coordinates come from two provenances and merge per layer:

- **LEF `TRACKS`** records parse at the top level of any LEF file —
  `TRACKS {X|Y} start DO n STEP s [LAYER l]+ ;`, plus the bare form
  (no `LAYER`), which applies to every routing layer. Because a record
  may precede `UNITS` and the `LAYER` it names, raw µm values buffer on
  `TechDb::lef_track_records` (transient, `serde(skip)`) and expand at
  the end of `parse_lef_bytes` — the `cut_res_above` pattern. `start`
  and `step` round to DBU first (`(v * dbu).round()`, the
  `pitch_dbu`/`offset_dbu` convention), then `coord[k] = x0 + k·s`;
  per-layer lists sort and dedup into `RoutingLayer::tracks_{x,y}`.
  `X` means vertical lines (x-coordinates), `Y` horizontal — the same
  convention `TrackGrid::is_vertical_tracks` documents.
- **DEF `TRACKS`** statements were already parsed into
  `DesignDb::tracks` (`TrackGrid`, already DBU). `drt::tracks::
  layer_tracks` expands every grid (`start_dbu + k·step_dbu`) onto its
  named layers, unions with the LEF lists, and sorts/dedupes.

**The sky130 fact**: `sky130_fd_sc_hd__nom.tlef` declares zero `TRACKS`
records — `RoutingLayer::tracks_*` is empty on every layer, and the
effective grid on gcd/picorv32 is entirely DEF-sourced (12 `TRACKS`
statements per placed DEF: both axes on `li1`, `met1`–`met5`). The LEF
grammar support is exercised by synthetic fixtures; claiming the
sky130 LEF tracks "parse into lists" would be vacuous coverage.

**Preferred axis**: `RoutingLayer.dir` retains the LEF `DIRECTION`
statement (`Horizontal` ↔ `tracks_y`, `Vertical` ↔ `tracks_x` — a
horizontal layer's wires ride horizontal track lines; the
direction-to-axis flip is the one fact easiest to get backwards). A
routing layer whose preferred-axis list is empty is a `TrackGap`
(`drt::tracks::track_gaps`), named and never synthesized from
`PITCH`/`OFFSET`. On sky130: `li1`/`met2`/`met4` are vertical, `met1`/
`met3`/`met5` horizontal.

**Strictness asymmetry**: LEF `TRACKS` errors on an unknown layer name
and applies the bare form to all routing layers; DEF `TRACKS` drops
unknown layers silently and a bare form retains no layers at all
(pre-existing reader behavior, untouched).

## 2. Access-point model

An **access point** is a candidate attachment point: a grid location on
a routing layer where a track line crosses a routed pin's shape. For a
pin rect `R = [xl,xh]×[yl,yh]` on layer `l`:

- each `t ∈ tracks_x[l]` with `xl ≤ t ≤ xh` yields `(t, mid_y)`;
- each `t ∈ tracks_y[l]` with `yl ≤ t ≤ yh` yields `(mid_x, t)`;

with `mid_x = (xl+xh)/2`, `mid_y = (yl+yh)/2` in i32 — truncation
toward zero, which is floor on non-negative die coordinates. Boundaries
are inclusive: a track on the rect edge touches the shape. One point
per track line per rect — not the cross product.

This is deliberately not TritonRoute's access-point algorithm (which
places APs at preferred-track × secondary-track intersections under a
cost model). Our enumeration is a superset candidate list the later
maze router prunes.

**Shape enumeration** (`drt::access::pin_shapes`) mirrors `drc::geom`'s
five-source assembly without flattening the pin key:

- instance pin (`pin_cells[p] == Some(c)`): gated on
  `cell_status ∈ {Placed, Fixed, Cover}` — same gate as `drc::geom`
  source 4. `db.pin_macro_pins[p]` → `MacroPin.rects` (macro coords) →
  `DesignDb::orient_rect` under the cell's orientation → translate by
  `(cell_x_dbu, cell_y_dbu)`.
- io `BTERM` (`pin_cells[p] == None`): `db.pin_io_rects[p]`, already
  absolute (`drc::geom` source 5). `+ SPECIAL` ports (`VGND`/`VPWR`)
  share `pin_cells == None` but are never members of `net_names` nets,
  so they are never visited — no special case.

**Emission order** (determinism contract): nets `0..num_nets()`
ascending → member pins in `net_pins` order (driver first) → candidate
layers ascending by `LayerId` (rects in file order per layer) → per
rect: `tracks_x` hits ascending, then `tracks_y` hits ascending.
Per-pin `(layer, x, y)` duplicates collapse (two overlapping rects of
one pin can yield the same point). `per_layer` counts emitted points.

## 3. Findings taxonomy

Named finding kinds, all gate-blocking on the test designs:

- `off_track` — the pin had candidate rects but zero access points
  (the shape sits entirely between tracks on every candidate layer).
- `no_shapes` — no candidate rects at all: unplaced cell, missing
  `pin_macro_pins` mapping, or io pin without `+ LAYER` rects.
- `TrackGap` — a routing layer whose preferred-axis list is empty.
- `missing_signal` — a ≥2-pin signal net with no guide block. Empty on
  both designs (guide-name resolution failure — an "extra" guide net —
  is a hard error at join, not a census row).
- `absent_expected` — a `net_names` net absent from the guide for a
  known reason: <2 member pins or non-signal members. Named, not a
  failure — gcd has none (VGND/VPWR live in `SPECIALNETS`, never
  `net_names`); picorv32 has 67 single-pin io-port nets
  (`irq[0..31]`, `pcpi_rd[0..31]`, `pcpi_ready`, `pcpi_wait`,
  `pcpi_wr`). Signal-ness has no net-level flag in `DesignDb`, so it
  is classified per member pin: an instance pin is signal unless its
  macro pin is `is_power` (unmapped is conservative-signal); an io
  port is signal iff it is in `io_pins` (`+ SPECIAL` ports are never
  members).
- `zero_coverage` — a (net, layer) with guide boxes but no covered
  track crossing (boxes miss every track on at least one axis). Empty
  on both designs.
- `region_escape` — a member pin of a guided net whose candidate
  shapes contain no in-region node on any layer. The partition gate:
  every member pin attaches or is named, printed per design. Measured:
  gcd 17, picorv32 592 — see §5.

`off_track`/`no_shapes`/`TrackGap` are empty on gcd/picorv32; any
entry is a named finding with its pin (`cell/pin` for instance pins,
port name for io pins), not a silent zero.

## 4. Claim boundary

The census gate proves **coverage and determinism on real inputs** —
every signal-net member pin has ≥1 candidate attachment on both
designs, and two runs emit identical point lists; the graph gate adds
the same claims for the region/node/edge enumeration and the pin
attachment partition. Access points and route nodes are *not*:

- **DRC-clean points**: a point may collide with macro OBS or another
  net's shapes — spacing/OBS pruning is the maze router's problem.
- **Track-assigned points**: the report enumerates candidates; choosing
  among them is the TA stage, a later ticket.
- **A reproduction of TritonRoute's AP set**: different algorithm, a
  candidate superset — no comparison against TritonRoute's `pin_access`
  output is claimed.

The route graph is *not*:

- **Via edges / layer transitions** — the via model is the next
  ticket's; via selection needs LEF `VIA` geometry, which the readers
  do not yet surface.
- **Costs or direction legality** — both axes are offered on every
  layer (`RouteEdge.axis` records which); preferred-direction legality
  and cost assignment are the maze router's concern.
- **DRC/OBS-pruned** — the region is purely guide-defined; no node is
  dropped for spacing or blockage overlap.
- **A reproduction of TritonRoute's `FlexGridGraph`** — this is a
  substrate the later search walks, checked by census, not by graph
  diff against the reference implementation.

## 5. Guide-bounded track graph

`drt::graph` turns the committed `<d>.guide` files (the OpenROAD
`global_route -guide_file` reference, already round-tripped by
`congestion::guide`) into the on-grid graph the maze search walks.

**Join** (`join_guides`): `resolve_guide_nets` maps each block to a
`NetId` by `guide_net_name` — an unmatched name is a hard error, never
a skip. `net_names` nets with no block classify into `missing_signal`
/ `absent_expected` (§3) in net order.

**Region**: per (net, layer), the union of the net's guide boxes on
that layer clipped to the track grid — a set of covered `(ix, iy)`
track-index pairs. Per box, two `partition_point`s on `tracks_x`/
`tracks_y` give contiguous index ranges and the cross product is
inserted; per-layer sets are kept sorted and deduped. Because coverage
is enumerated per box, diagonally disjoint boxes never spawn corner
nodes (a bounding `covered_x × covered_y` would). A `(net,layer)` with
≥1 box but no covered pair is a `zero_coverage` finding. Work is
`O(Σ box × tracks-per-box)`; the gate asserts the union bound
`nodes ≤ Σ_box |covered ix|·|covered iy|` per net — never `O(die)`.

**Nodes/edges**: one `RouteNode (layer, ix, iy)` per covered pair —
an x-track ∩ y-track crossing by construction. Edges join
consecutive-index nodes on the same layer on both axes
(`(l,ix,iy)–(l,ix+1,iy)` and `(l,ix,iy)–(l,ix,iy+1)`, each unordered
pair once) — the induced subgraph of the grid restricted to in-region
nodes, so an edge never wires across an uncovered corridor gap.
Emission order is fixed (layers ascending → pairs ascending → per
node the +x then +y probe, in node order): two runs are byte-identical
and the gate double-runs with nonzero totals.

*Recorded simplification*: an edge trusts that the unit segment
between adjacent-index in-region nodes stays inside the union — two
boxes can cover adjacent x-tracks at one y-track while leaving a
sub-pitch sliver between them. GCell-aligned guide boxes abut, and
sub-track coverage is not modeled.

**Access hookup**: attachment is evaluated per pin over its
`drt::access::pin_shapes` candidate rects: a node `(l, ix, iy)`
attaches when it lies inside a pin rect — `xl ≤ x_ix ≤ xh ∧
yl ≤ y_iy ≤ yh`. The spec's literal reading ("an access point `(x,y)`
is attached when `(x,y)` is an in-region track crossing") degenerates:
APs carry `(track, rect_midline)` coordinates and a pin-rect midline
is almost never a track coordinate, so the literal check would escape
~89% of gcd's member pins (measured). The implemented rule is "the
crossing the AP designates" — every in-region node inside a pin shape
corresponds to an x-track AP and a y-track AP of that pin. A member
pin with zero attached nodes on every layer is a `region_escape`,
named in the census.

Measured escapes are two genuine classes: (a) the pin shape straddles
a GCell boundary and the in-box sliver holds tracks on one axis only —
gcd `_313_/X`, `_301_/B1`, `_307_/B1` and picorv32's 355 instance-pin
escapes (e.g. `_09474_/S`: FS-placed `mux2_1`, the li1 box stops at
the GCell edge mid-rect and each in-box fragment sees a single axis);
(b) io ports whose ±140 DBU `met2` square holds an x-track but no
y-track (gcd's 14 `req_msg`/`resp_msg` ports, picorv32's 237 io
escapes) — no complete crossing exists inside the pin shape. The
pinned gcd set is asserted by name in `tests/drt.rs`.

Router implication: class-(b) escapes are *unattachable* under the
pure track-crossing model — the io port square holds no node at all,
so the maze search cannot reach them. The via-model ticket must add
an attachment fallback for these ports (an off-crossing tap on the
port's own met2 rect, or honoring the reference DEF's port via), or
every io-port net stays unroutable — recorded, not worked around.
