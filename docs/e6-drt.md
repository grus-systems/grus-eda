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

- **Costs or direction legality** — both axes are offered on every
  layer (`RouteEdge.axis` records which); preferred-direction legality
  and cost assignment are the maze router's concern.
- **DRC/OBS-pruned** — the region is purely guide-defined; no node is
  dropped for spacing or blockage overlap. Via edges are likewise
  geometric only (§6): a shared crossing plus guide coverage, not a
  DRC-clean transition.
- **A reproduction of TritonRoute's `FlexGridGraph`** — this is a
  substrate the later search walks, checked by census, not by graph
  diff against the reference implementation.

The via model (§6) is *not*:

- **DRC-aware selection** — `ViaPair::default` is a declared policy
  pick (preferred-direction alignment → area → file order), not a
  per-site legality check; every candidate is retained for the later
  cost model.
- **Stacked/multi-hop transitions** — only one-cut adjacent edges are
  emitted; stacked vias are the search composing them.
- **A reference-via reproduction** — the reference join names the
  majority via per cut layer as evidence for the policy, not a target
  to match (the `mcon` pick is a documented name-only divergence on
  rect-identical candidates).
- **A preferred-anchor node model** — the reference anchors most vias
  at preferred-track crossings across two *different* layers
  (`via-edges.md` §3), positions our same-layer-crossing nodes cannot
  host. §7's relaxed via edges closed the transition gap without
  revising node semantics — each carries its true preferred-crossing
  position separately from its endpoint nodes.
- **FIXED-viarule parameterization** — a DEF `+ VIA <lef>` record is
  matched to its LEF def by the cut-layer triple only; any FIXED
  parameterization beyond that triple is unmodeled.

The routed result (§7) is *not*:

- **DRC-clean** — the occupancy model is spacing-lite: `w/2 + s`
  bands on track lines, no enclosure/cut rules, unmerged
  OBS marks with an unconditional host-cell carve, and `Pin` marks
  that never hard-block. A foreign wire landing on pin metal is an
  emit/e2e finding, not a search-time error. Min-area is the one
  rule family with a post-pass: §7's occupancy-strict patch lifts
  sub-bound components along in-plane edges and names what it cannot
  fix in `RouteDb.area_residuals` — residuals the emit gate surfaces
  as `*.6` findings, not a claim of min-area cleanliness.
- **A TritonRoute reproduction** — the search is a pathfinder-style
  tree extension over our own region graph with relaxed (non-strict-
  crossing) via transitions, not a port of `FlexGridGraph`/`drWorker`;
  the census checks connectivity, not route equality.

The emitted DEF (§8) is *not*:

- **Magic-DRC-verified** — the self-DRC gate runs our 50-rule
  `lef_expressible` subset on our own re-parsed output; the full
  Magic deck check on our routed DEF is `e6-drt-e2e`'s container
  gate, not this one's.
- **Connectivity-verified** — the subset checks geometry, not
  netlist continuity; opens beyond the named `unrouted` set and
  antenna checks are e2e scope.
- **Reference-geometry-identical** — the wire-length/via census vs
  `variants.delegated`/`baseline` is informational; topology
  differences are expected and the signed deltas are recorded, not
  gated.

## 5. Guide-bounded track graph

`drt::graph` turns the committed `<d>.guide` files (the OpenROAD
`global_route -guide_file` reference, already round-tripped by
`congestion::guide`) into the on-grid graph the maze search walks.

**Join** (`join_guides`): `resolve_guide_nets` maps each block to a
`NetId` by `guide_net_name` — an unmatched name is a hard error, never
a skip. `net_names` nets with no block classify into `missing_signal`
/ `absent_expected` (§3) in net order.

*Flow contract — clock nets*: the CTS-inserted tree (`clkbuf_*` cells
driving `clknet_*` nets plus the clock root net) lives in `NETS` as
ordinary signal nets and **is routed by `drt`** — the same flow
contract as TritonRoute (detail routing does not alter the CTS-fixed
tree topology). There is no clock-net exclusion: dropping the clock
tree from routing would leave it unmetaled, and the clock is usually
the highest-fanout net in the design — removing it would understate
measured congestion. Clock-specific quality treatment (route-first
ordering, non-default width/spacing rules) is deliberately deferred:
it binds against skew/EM concerns that only become measurable once
STA closure exists.

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

Router implication (updated by §7 — this paragraph's prediction is
now historical): class-(b) escapes are *unattachable* under the pure
track-crossing model — the io port square holds no node at all, so
the maze search cannot reach them. The shipped answer is the §7
projection fallback (in-region nodes in the rect's track
neighborhood, one line of margin), plus `spare_above = 1` region
layers for OBS-sealed pockets: guide boxes bound the search in XY,
but the guide's layer choice does not bind which layer detailed
routing taps on. The remaining recorded gap is the met2–met3
transition itself (§6's disjoint-grids gap) — relaxed edges bridge
strict-crossing sparsity *within* a pair whose upper preferred lines
exist, and met2–met3 has no transition path at all.

## 6. Via model

`drt::via` builds the per-design `ViaTable` and `drt::graph` emits
`ViaEdge`s — the layer-transition half of the substrate.

**Candidate table** (`via_table`): every LEF `VIA` in
`tech.via_defs` (file order) then every resolvable DEF `VIAS` record
in `db.def_vias` (record order) is expanded at the origin by
`drc::geom::expand_via` into `ViaCandidate.rects` — the same geometry
the DRC walkers price — plus its total painted `area_dbu2`. The DEF
reader now retains those records: `+ VIARULE` generated records keep
their `GeneratedViaGeom`; `+ VIA <lef>` records resolve through
`tech.via_by_name` and keep `ViaGeom::Lef` under their DEF name;
`+ RECT` inline records stay unresolvable and are not retained (same
lazy-error semantics as wire resolution). A def whose `(lower,
upper)` is an adjacent pair joins `pairs[lower]`; any other span is a
named `UnmatchedVia`, and a pair with zero candidates is a named
`ViaGap` — sky130 has neither on real inputs (all 25 LEF defs land on
adjacent pairs; the placed DEFs' four generated records add one each
to met1–met2 … met4–met5).

**Default policy** (`ViaPair::default`): `min` by
`(nonpref_metal, area_dbu2, declaration order)` — first the count of
metal rects elongated *against* their layer's preferred direction
(the techlef's `PR`/`PR_R`/`PR_M`/`PR_MR` naming encodes which metals
ride the preferred direction), then total painted area, then file
order. This is a declared pick, not DRC-driven selection; every
candidate stays in the table for the later cost model.

**Reference join** (the census test deserializes
`<d>.drt.reference.json` and groups `vias_per_cut_layer` by the
file's `via_names_meaning` mapping): sky130 picks `M1M2_PR`,
`M2M3_PR`, `M3M4_PR` on `via`/`via2`/`via3` — the policy's pick on
all three. On `mcon` the reference majority is `L1M1_PR_MR` while the
policy picks `L1M1_PR`: the two candidates are **rect-for-rect
identical** (asserted in `test_via_candidate_table`), so the
divergence is name-only — the reference's rotated-name twin is the
same painted geometry. `via4` has candidates but zero reference
instances on both designs; its `M4M5_PR` pick is policy-only,
unobserved.

| pair | cut | candidates | default | ref majority | ref total |
|---|---|---|---|---|---|
| li1–met1 | mcon | 5 | `L1M1_PR` | `L1M1_PR_MR` (name-only) | 884 / 26,365 |
| met1–met2 | via | 6 | `M1M2_PR` | `M1M2_PR` | 883 / 31,789 |
| met2–met3 | via2 | 6 | `M2M3_PR` | `M2M3_PR` | 28 / 1,784 |
| met3–met4 | via3 | 6 | `M3M4_PR` | `M3M4_PR` | 2 / 90 |
| met4–met5 | via4 | 6 | `M4M5_PR` | unobserved | 0 / 0 |

(ref totals: gcd / picorv32; the met1–met2 row groups `M1M2_PR` and
`M1M2_PR_MR`, both present in the reference.)

**Via edges** (`NetGraph::via_edges`): a `ViaEdge{lower, upper}`
joins node `(l, ix, iy)` to node `(l+1, jx, jy)` iff the lower node's
physical `(x, y)` binary-search-hits *both* upper track lists and the
resulting `(jx, jy)` is a covered upper pair — the same coordinate a
track crossing on both layers *and* guide-covered on both.
Non-preferred-axis tracks count: every coordinate in a layer's
`tracks_x`/`tracks_y` is a real grid line. Each lower node emits ≤1
edge and distinct coordinates map to distinct upper nodes, so ≤1 via
edge exists per coordinate per pair. Emission is deterministic
(adjacent pair ascending → lower node order) and geometric only — no
`ViaTable` is consulted: edges are the shared-crossing evidence,
candidate legality/pricing is the router's join. Per-pair totals land
in `RouteGraphs::via_edges_per_pair`.

**Census** (`test_via_edge_census`, pinned per design):

| pair | gcd | picorv32 |
|---|---|---|
| li1–met1 | 13,292 | 395,479 |
| met1–met2 | 375 | 13,961 |
| met2–met3 | 0 | 0 |
| met3–met4 | 2 | 47 |
| met4–met5 | 0 | 0 |
| **total** | **13,669** | **409,487** |

Two named zeros. **met2–met3 is an edge-domain tech gap**: met2's
DEF grids are `230 + 460k` (≡10 mod 20) on both axes while met3's are
`340 + 680k` (≡0 mod 20) — the coordinate sets are disjoint, so *no*
shared crossing exists anywhere on the die; `M2M3_PR` vias are
legal on track-adjacent geometry the strict crossing rule cannot see.
The router will need an offset-snap rule (TritonRoute places `via2`
freely — the reference has 28/1,784 instances) before this pair can
route; recorded, not worked around. **met4–met5** has shared
crossings (both ≡20 mod 40; e.g. `x = y = 39100` on gcd's grid) but
zero guide-covered node pairs on both designs — a coverage gap, not
a grid gap.

**Fixture** (`test_via_edge_fixture`, synthetic LEF/DEF): a node
present on the lower layer only — same `(x,y)` fails the upper
track-list search — gets no via edge, and a coordinate shared by both
grids but outside the guide gets none either; the gate asserts both.

## 7. Maze search

`drt::maze` connects each guided net's member pins inside its region
graph — sequential, deterministic, one `NetGraph` at a time in
`join.nets` (guide-file) order. `route_nets` returns a `RouteDb`:
per-net `NetRoute`s (merged wire segments, via placements, connected
pins), a named unrouted report, the post-hoc collision census, and
the iteration count.

**Region input**: the tests build graphs with
`build_route_graphs_ext(.., spare_above = 1)` — every guide box's XY
is also covered on `top_guided + 1`. Guide boxes bound the search in
XY, but their layer choice does not bind detailed routing: OBS-sealed
pin pockets are reached one layer above the global route's pick
(OpenROAD's gcd `_038_` escapes its li1+met1 guide on met2 inside the
same guide XY — the precedent this spare layer answers).

**Attach sets**: strict `pin_nodes` first — the §5 in-region
node-inside-pin-rect rule. When strict is empty the pin falls back
to **projection** (`project_attach`): for each candidate rect
`(l, r)`, the in-region nodes on `l` whose `ix`/`iy` sit inside the
rect's covered track ranges expanded by one line on each side, on at
least one covered line. A met2 io-port square holds an x-track but no
y-track, so the tapping wire runs *along* that x-track and the
adjacent-line nodes are the connection points; the one-line margin
matters because a far node on a covered line can coincide with
another pin's attach node, which `absorb_taps` would claim as a
zero-edge connection. A projected tap node may sit off the pin
shape — `route_one_net` paints the single-axis **access stub**
closing the gap at extraction (the node's x or y lies inside the
rect's covered range by `project_attach` construction, so the stub
is always axis-aligned and reaches the pin metal); `NetRoute::
projected` names every pin connected this way (gcd: 17). A pin with
empty strict *and* projection sets is `Unattachable`.

**Edge classes** — one unified `u32` index space per net:
`[0, E)` in-plane edges, `[E, E+V)` strict via edges (§6's shared
`(x,y)` crossings, gated on `pairs[k].default.is_some()`), `[E+V,
E+V+R)` **relaxed via edges**. A real via needs a *preferred*-track
crossing — the vertical layer's x-track and the horizontal layer's
y-track — not a same-layer-node crossing; strict crossings coincide
only every 7820 DBU on li1–met1/met1–met2 and never on met2–met3
(the §6 gap), so strict edges alone leave pin areas disconnected.
For every node `v` on routing layer `k` (with `k+1` a routing layer
and `pairs[k].default` set) the maze synthesizes one relaxed edge:
upper `Horizontal` ⇒ via at `(v.x, nearest upper y-track)`, `b` the
in-region upper node on that line nearest `v.x` (bracketing entries
of the sorted ix list, lower index on ties); upper `Vertical`
mirrors it. The true position `(x, y)` sits on both layers'
preferred wires; the hang node is the same wire a track step away at
most. Recorded looseness: the implied stubs may step outside the
guide boxes (boxes are GCell-boundary, not exact) and are not
searched — but they *are* committed metal, occupancy-scanned via
`scan_stub` before the via footprint check and emitted as real
`WireSeg`s at extraction. **Single-nearest rule (recorded gap)**: if
the nearest upper preferred line holds no in-region node, `v` gets
no relaxed edge — a named stranded-pocket class; on gcd it stays
theoretical (`unrouted=0`), while picorv32 measures 65 `NoPath`
nets inside already-`met2`-connected guide unions (pockets, OBS
seals, or final-pass occupancy — `docs/results.md`). This includes
met2–met3: `M2M3_PR` is a default pair, so relaxed edges do give
it a transition path wherever a nearby met3 preferred line carries
in-region nodes — the §6 "dead capacity" note survives as the
met3-bridge residual class (43 picorv32 nets whose ≤met2 guide
union only met3+ rejoins; the census test's classifier names both
classes and rejects anything else). Emitted in node
order — deterministic; parallel strict/relaxed edges coexist and the
equal-`VIA_COST` tie breaks toward the strict arc (CSR order), with
extraction deduping the placement on `(lower, x, y)`.

**Search** (pathfinder-style tree extension): `NetTopo` is built
once per net — attach sets, `host_cells`, relaxed vias, and the CSR
adjacency over the unified index space — and reused across rip-up
iterations (byte-identical arc order). The tree seeds at the driver
pin's attach set (`net_driver`, else the first attachable member in
`net_pins` order). Each extension round: a unit-weight BFS over all
arc classes from every unconnected attachable pin's attach set fills
`hop[v]` (occupancy-blind, so it undercounts — a consistent
heuristic since every edge costs ≥1 hop); a `BinaryHeap` keyed
`(g + hop, node)` then pops the same minimum-`g` target a plain
Dijkstra would without flooding the region. `u32::MAX`-hop nodes
sort last; the wave still exhausts before `None`, so the `NoPath`
verdict is unchanged. On a hit the `parent` chain backtracks to the
tree — provably simple (strictly decreasing `dist`), but bounded at
`nodes.len()` steps so a broken invariant surfaces as `NoPath`,
never a hang. `absorb_taps` then marks unconnected pins whose attach
set already touches the tree (shared attach nodes connect for free).
Wave exhaustion with targets left ⇒ the remaining pins are `NoPath`.

**Costs** — hop count, not distance (recorded simplification):

| constant | value | meaning |
|---|---|---|
| `PREF_COST` | 1 | in-plane edge along the layer's preferred axis |
| `OFF_AXIS_COST` | 2 | in-plane edge against it (stubs/jogs) |
| `VIA_COST` | 4 | one layer transition, strict or relaxed |
| `OCC_PENALTY` | 8 | per distinct foreign net hit, soft iterations |
| `HIST_STEP` | 8 | added per re-queue to the penalty multiplier |
| `MAX_ITERS` | 8 | iteration cap; the last iteration is hard mode |

The preferred axis follows §1's direction-to-axis flip: a
`Horizontal` layer's `Axis::X` edges cost `PREF_COST`; `Vertical`'s
`Axis::Y` edges.

**Occupancy** (`Occupancy`): committed geometry — macro OBS, pin
shapes, committed wires/stubs, via footprints — rasterized onto
track lines so an edge's conflict check is a per-line interval
query. Per layer, `lines_y`/`lines_x` map a line coordinate to a
`Line`: `lo`-sorted `ivs` plus a `max_hi` skip index per
`SKIP_CHUNK = 16` chunk (rebuilt eagerly from the dirty chunk; a
scan skips `max_hi ≤ qlo` chunks and stops at `lo ≥ qhi` — identical
hit *set*, both consumers order-insensitive). A rect `r` marks
`[r.yl, r.yh]` on every vertical line inside the strict
`w/2 + s` band (`w/2 = width_dbu/2`, `s = min_spacing_dbu()`) and
`[r.xl, r.xh]` likewise on horizontal lines; `perp_*` stores the
perpendicular extent so `scan_rect` runs the true 2-D test on via
footprints. Candidate `scan_wire` hits are intervals overlapping
`[c − w/2 − s, d + w/2 + s]` — edge-distance `< s` along the line;
perpendicular `< s` is guaranteed by the band. A collision sliver
containing no track line on either axis is the recorded miss class.
Cut-layer occupancy stays unmodeled (spacing-lite); io-port rects
now mark `Pin(own_net)` (pre-mark below) — the unmarked class
produced real emit-time `met3.2` findings the first gate run
surfaced.

Three occupant classes in `hit_verdict`:

| mark | soft iteration | last iteration (hard) |
|---|---|---|
| `Obs(cell)`, `cell ∈ host_cells` | invisible | invisible |
| `Obs(cell)`, `cell ∉ host_cells` | hard block | hard block |
| `Obs(u32::MAX)` (net-less pin metal) | hard block | hard block |
| `Net(own)` | free | free |
| `Net(foreign)` | `+(OCC_PENALTY + hist·HIST_STEP)` per distinct foreign net | hard block |
| `Pin(own)` | free | free |
| `Pin(foreign)` | same penalty, pooled into the distinct set | **soft — never a block** |

The **host-cell carve** is unconditional: `host_cells` is the sorted,
deduped set of cells hosting the net's member pins, and an
`Obs(cell)` hit is invisible whenever `cell ∈ host_cells` — the
rasterized "access shapes ignore own-instance OBS" rule (the earlier
pin-halo carve left approach edges blocked before the halo was
reached — measured root cause of 47/48 gcd `NoPath`s). Recorded
looseness: the net may cross its host cell's OBS anywhere, not just
near the pin — connectivity-level acceptable; the emit/e2e DRC owns
the real check. `Pin` never hard-blocks because rip-up cannot move a
pin — charging it as congestion churns the queue without clearing
the mark; a foreign wire landing on pin metal is an emit-time
finding. Macro OBS marks use the **unmerged** `obs_rects_raw` set —
the `merge_connected_obs` bbox fills the decap/edge gaps pins need.

**Pre-mark** (`premark_obs`, once at `route_nets` start): every
`{Placed, Fixed, Cover}` cell's `obs_rects_raw` on routing layers →
`Obs(cell)` (the `drc::geom` source-4 convention — `orient_rect` +
translate); every member pin's shapes → `Pin(own_net)` — instance
pins and io-port rects alike, a priced band that never hard-blocks
(or `Obs(u32::MAX)` when the pin has no net); and every PDN
`SPECIALNETS` wire rect and via-stack routing-layer rect
(`expand_via`-expanded) → `Obs(u32::MAX)` — signal routes must hold
the `w/2 + s` band off the power grid, the shapes the emit-time
self-DRC measures spacing against.

**Commit** (`commit_route`): merged wire rects (`± w/2`), relaxed-via
stub wires, projected-pin access stubs, and via footprints (the
default candidate's routing rects shifted to the placement) →
`Net(net)`, logged as `Mark`s — the rip-up journal (`unmark` frees
them).

**Order and rip-up**: nets route in `join.nets` order. In soft
iterations the foreign hits a committed edge actually took are
recorded as provisional `Collision`s deduped on `(a, b, layer, line,
lo, hi)` with `a < b`. After each round: `bad` = unrouted nets ∪ both
parties of every provisional collision; each bad net's marks are
ripped up and its `hist` escalates — per-net escalation breaks the
two-net ping-pong cycle. Kept nets stay committed and price later
re-routes. Cap `MAX_ITERS = 8`; the last iteration runs hard mode and
its failures become the unrouted report. `OccMode::Disabled` (the
fail-proof knob) skips foreign-`Net`/`Pin` checks and collision
recording entirely; `Obs` still blocks — a fixed blockage is not
"occupancy". After the cap, `overlap_census` rebuilds every route's
wire rects and via footprints into a fresh occupancy (no OBS) and
reports each cross-net pair whose `R + s`-intersect rule violates —
the exact `RouteDb.collisions` list (`Enabled` only).

**Min-area patch** (`patch_net_min_area`, once per committed net
after the iteration loop — occupancy then holds the final foreign
picture, and patches rasterize straight into it so later nets'
patches respect earlier ones): the picorv32 self-DRC gate surfaced
`*.6` findings the search's spacing-lite model never priced — TritonRoute's
equivalent is a post-route min-area patch pass, and this is ours.
Per routing layer with `AREA` declared the net's metal is rebuilt
exactly the way `check_area_report` sees it — wires `± w/2` as
`SignalWire`, pair-default via routing-layer rects as `Via`,
member-pin shapes as `MacroPin`/`IoPort` — merged by
`union::components`, evaluated only when a component holds a drawn
member (a `MacroPin`-only island is never evaluated, mirroring the
checker's `is_drawn` gate). A sub-bound component tries
`patch_component`: from each in-rect node it walks `+pref` then
`−pref` along adjacent covered nodes on its line — every step is one
in-plane `g.edges` span, so patch metal stays inside the guide
region and on-track by construction. A step commits only when
`scan_wire` reports own-net `Net`/`Pin` hits exclusively —
**occupancy-strict**: any `Obs` hit (the host-cell carve is not
inherited; patch metal is optional, not access) or foreign hit stops
that direction, and the strictness holds even under
`OccMode::Disabled`. On reaching `union_area ≥ bound` the
accumulated span commits as one `WireSeg` marked `Net(own)`; the
layer re-derives because one patch can merge two sub-bound
components into a cleared union. A component no walk lifts lands in
`RouteDb.area_residuals` — `AreaResidual{net, layer, region,
area_dbu2, bound_dbu2}` — the named decline arm: every residual
maps to a `*.6` self-DRC finding if the gate fires, and a `*.6`
finding with no matching residual is a model divergence (a bug, not
a tolerance). Recorded step-domain gap: the walk follows in-plane
edges only — a component needing off-axis metal or a detour through
an index gap is residual-bound even when legal metal could reach.

**Unrouted taxonomy**: `UnroutedNet` names `connected` pins plus
`failed` per-pin `(PinId, FailKind)` — `Unattachable` (strict +
projection attach empty) vs `NoPath` (attachable but unreachable:
pocket walls, region-component splits, or hard-mode blocking). An
empty region is `NoPath`, never a panic; a leftover `Unattachable`
member after the loop still lands the net in the report.

**Determinism**: fixed net/pin/neighbor order, the `(g + hop, node)`
total-order PQ (equal keys pop the lower node id), integer costs,
`BTree` containers, canonical extraction (merged sorted wires,
deduped sorted vias), `relaxed_vias` in node order, and the `NetTopo`
cache's byte-identical arcs. Two full runs produce identical
`RouteDb`s — asserted on the full structure plus the nonzero `xxh3`
`route_digest`.

**Instrumentation**: `GRUS_DRT_TIMING` (any value) makes `route_nets`
print premark/topo/per-iteration/per-slow-net timings to stderr —
opt-in, zero cost unset; it feeds `docs/results.md` runtime evidence.

## 8. Routed DEF emission

`drt::emit` turns a `RouteDb` into routed DEF in two steps:
`apply_routes` writes each net's geometry into `db.net_geoms`, and
`write_routed_def` splices a canonical `NETS` section into the placed
DEF template — everything outside `NETS n ;` … `END NETS` passes
through verbatim (`VIAS`, the `SPECIALNETS` PDN, `TRACKS`, `PINS`,
`GCELLGRID` all survive; that survival is why `write_def_full` was
rejected — it regenerates only `COMPONENTS`+`NETS` and drops the PDN
shapes the self-DRC picture needs).

**Geometry mapping**: every `WireSeg` expands to the square-cap rect
`seg ± w/2` — identical to `parse_net_wire`'s two-point expansion and
to `commit_route`'s occupancy rasterization, so the emitted DEF
describes exactly the shapes the router priced. Every `ViaPlace`
becomes a `PdnVia` naming its pair's `ViaTable` **default** verbatim
— the emitter never reselects, because the placed shape must be the
via the router priced. A `default == None` pair reaching emit is a
graphs/table bug and bails; `vp.lower != cand.lower` likewise. Every
`route_db.unrouted` net resets to `NetGeom::default()` — a guided net
with no route emits a conns-only record, never stale geometry, and
its DEF spelling is named in `EmitReport.unrouted`.

**Record grammar** (TritonRoute's statement order):

```
 - <def_spelling> ( <comp> <pin> ) …
   + ROUTED <layer> ( x1 y1 ) ( x2 y2 )
   NEW <layer> ( x1 y1 ) ( x2 y2 )
   NEW <lower_layer> ( x y ) <via_name>
   + USE SIGNAL ;
```

Exactly one `+ ROUTED` — the first emitted statement — then `NEW`
per remaining wire and per via. **No width token**: a bare integer
after the layer parses as a point coordinate in regular `NETS`
records — width is a `SPECIALNETS`-only construct, and TritonRoute's
committed routed DEFs confirm the zero-width form. A standalone via
record's layer token is the via's **lower** metal. Statements order
deterministically: wires in `WireSeg` order (`layer, x1, y1, x2,
y2` — canonical endpoint order keeps `(x1,y1) ≤ (x2,y2)`), vias in
`ViaPlace` order (`lower, x, y`), nets in `db` order (= template
`NETS` order on DEF input).

**Byte stability** — `write(parse(write(db))) == write(db)`: the
non-NETS bytes pass through verbatim both passes; `NETS` regenerates
from `net_geoms` identically (net order preserved by parse's
record-order append, conn order by `net_pin_ids`, and each wire rect
recovers its centerline as `(xl+w2, yl+w2)–(xh−w2, yh−w2)` on the
horizontal arm, symmetric on vertical — the exact fixed point of the
`± w/2` expansion). A `wire_rect` that is not square-cap-expressible
— foreign geometry like a `RECT` stub in a re-routed template —
bails by name rather than distorting.

**Self-DRC gate**: the emitted file re-parses through
`parse_net_wire`/`resolve_def_via` (`db2`, not `db` — broken grammar
or an unresolvable via name surfaces as a parse error, not wrong
geometry), `drc::geom::assemble` builds the full picture (our signal
geometry + the template's PDN stripes + macro pins/OBS + io rects),
and `drc::report::run_all` applies the same 50-rule `lef_expressible`
subset `tests/drc.rs` puts on TritonRoute's routed DEFs at 0
findings. The claim boundary of §4 stands: this is our subset on our
DEF; the full Magic deck check on our routed DEF is `e6-drt-e2e`.

**CLI** — the delegation path's native replacement:

```
grus-eda drt --tlef <t> --lef <l> --def <placed.def> \
    --guide <d>.guide --out-def <routed.def> \
    [--reference <d>.drt.reference.json] [--self-drc] \
    [--occ enabled|disabled] [--max-iters N]
```

prints the emit census (`guided/routed/unrouted/projected/
collisions/area_patches/area_residuals/iterations/route_ms`, one
`unrouted <name>` line per unrouted net and one `area_residual
<net> <layer> area=… bound=… region=…` line per unfixable §7
component), the informational `wirelength_um` + `via.<cut>`
deltas vs `variants.delegated`/`baseline`, and — under `--self-drc`
— the re-parse→assemble→`run_all` verdict. The CLI reports and
never exits nonzero on findings; the gate is `tests/drt.rs`.
