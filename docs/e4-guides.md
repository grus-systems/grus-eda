# E4: Route-Guide Emission — Implementation Spec

E4 ticket 1: emit an OpenROAD `global_route -guide_file`-format guide from the
committed edge paths the E1 congestion router computes, and measure the emitted
topology against OpenROAD's own guide on `gcd` and `picorv32`. Guide parasitics
and the STA end-to-end path are later tickets.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E4.1** | Guide topology vs OpenROAD | `global_route -guide_file <d>.guide` on `<d>.placed.def` (step 5c of `generate_ref.sh`) | Per-net guide-box overlap share ≥ 0.70 on `gcd` and `picorv32` (box-weighted over reference boxes; provisional bound — see `docs/results.md`) |
| **E4.2** | Guide wirelength proxy | same | Total guide-box count within 15% of the reference on each design, with the per-net deltas examined (not only the total) |
| **E4.3** | OpenROAD round-trip | `read_guides <ours>` + `estimate_parasitics -global_routing` (`check_guide_readback.sh`) | `read_guides` accepts the emitted file in the pinned container; any error is a failure |
| **E4.4** | End-to-end STA | `sta --spef <d>.guide.ours.spef` vs OpenSTA `read_guides` + `estimate_parasitics -global_routing` golden (step 9c of `generate_ref.sh`) | Every endpoint's arrival/slack within `max(5 ps, 1%)` on gcd; picorv32 follows the routed-SPEF floors in `docs/status.md` — **measured, not met** (see `docs/results.md` `e4-sta-e2e`) |

The E4.1/E4.2 bounds are provisional per `kurena/tasks/next-stage-scoping/design/done-when.md`.

---

## 2. File Format (P-GR measured, 2026-11-22)

OpenROAD `global_route -guide_file` emits, per signal net, a block:

```
<net_name>
(
xlo ylo xhi yhi <layer_name>
xlo ylo xhi yhi <layer_name>
)
```

DBU rectangles, LEF layer names verbatim (`li1`, `met1`, …). `read_guides`
accepts the same file back (the `GRT-0008` "does not allow parasitics
estimation" warning is stale — read-back guides feed both
`estimate_parasitics -global_routing` and `detailed_route`).

**Net names are the raw DEF spelling.** OpenDB stores net names verbatim:
`ctrl.state.out\[1\]` keeps its backslashes and `req_msg[0]` stays bare, and
`read_guides` matches on exactly that token (`GRT-0234 Cannot find net`
otherwise — measured on both directions of a uniform escaping rule).
`DesignDb.net_def_spellings` therefore records the NETS `- name` token before
unescaping, and `write_guides` emits it. For non-DEF sources the field is empty
and `guide_net_name` falls back to `escape_guide_name` (the DEF-word escapes
plus `[`/`]`).

---

## 3. Grid → Box Geometry

`GCellGrid` gives `x_bounds(gx, die_xh)` / `y_bounds(gy, die_yh)`. Edge index
decoding (as in `route.rs`):

- `GridEdge::H(idx)`: `y = idx / (nx - 1)`, `x = idx % (nx - 1)`; connects
  GCells `(x, y)` and `(x+1, y)`.
- `GridEdge::V(idx)`: `y = idx / nx`, `x = idx % nx`; connects `(x, y)` and
  `(x, y+1)`.

---

## 4. Layer Assignment

Signal band `set_routing_layers -signal met1-met5`; the 2D router has no third
dimension, so the assignment is fixed:

- every committed **H edge → `met1`** (lowest horizontal-preferred layer);
- every committed **V edge → `met2`** (lowest vertical-preferred layer);
- a GCell incident to both an H edge and a V edge of the net is covered by both
  strips → it lands on `met1` **and** `met2` (the via coverage OpenROAD
  expresses the same way);
- **clock nets are not banded** — `route` loads no SDC, so every net uses the
  signal assignment (`met3-met5` clock band deferred to a later ticket; the
  approximation is recorded in `docs/results.md`).

## 5. Strips

Per net, per direction, per line (row `y` for H, column `x` for V): collect the
committed edges, deduplicate, and merge each maximal run of contiguous edge
indices into **one** strip rect spanning the union of the endpoint GCells:

- H run `x = a..=b` on row `y` → `[x_bounds(a).0, y_bounds(y).0, x_bounds(b+1).1,
  y_bounds(y).1]` on `met1`;
- V run `y = a..=b` on column `x` → `[x_bounds(x).0, y_bounds(a).0, x_bounds(x).1,
  y_bounds(b+1).1]` on `met2`.

## 6. Pin-Access Stubs

Per pin of the net, one stub rect = the pin's GCell rect, emitted on:

1. the pin's **access layer**:
   - instance pin → `MacroPin.layer` (the first LEF PORT layer; `li1` for
     `sky130_fd_sc_hd` cells),
   - I/O pin → `pin_io_layer` (the DEF `+ LAYER`; `met2`/`met3` under
     `place_pins -hor_layers met3 -ver_layers met2`), falling back to `met1`
     when absent;
2. **`met1`** — the stack stub from the access layer into the routing band.

Stub boxes dedup across pins sharing a GCell. A net whose pins all sit in one
GCell produces no segments; its block still carries the stubs. Nets with
**fewer than 2 pins** emit no block — a lone port pin has nothing to connect
(measured: picorv32's 67 `irq`/`pcpi_*` port-only nets carry no reference
block; gcd's `_003_` — two pins in one GCell — does).

## 7. Determinism

- Nets emit in `net_names` order (DEF order).
- Boxes within a net keep `NetGuide.boxes` order — it is semantic, since
  `updateVias` pairs only consecutive same-point segments. `write_guides`
  dedups exact duplicates on first occurrence but never re-sorts.
- Builder order: `met1` H strips, then `met2` V strips, then **bend
  via-stacks**, then unbound pin-cell stub groups. Each pin GCell
  carries one contiguous **rung stack** of zero-length same-cell
  boxes (`updateVias` pairs a stack only when its rungs are
  consecutive and adjacent-layer — a non-contiguous group leaves
  every unpaired rung a dead self-loop). A pin cell **bound** to a
  covering strip emits a contiguous stack: when every access sits
  below the strip layer `S` the rungs are `min(accesses)..S` (the
  strip wire itself is the `S` rung); when an access sits at/above
  `S` the stack instead climbs `min(accesses below S, met1, S)..=
  max(accesses)` — the `met1` fill keeps the `met1`↔`met2` bridge at
  pin cells that are also H/V junctions. Runs split at every bound
  pin cell so a
  covered pin cell is always a strip start/interior/end cell:
  - pin cell at a strip **start** or **interior** split point: the
    rung stack goes *before* the following wire, ascending — its top
    rung lands adjacent to the wire's `L` segment and pairs;
  - pin cell at a strip **end**: stub group goes *after* the last
    wire, below-`S` rungs descending first — mirrors OpenROAD's own
    tail convention (`_000_`: `[met2-wire, met1, li1]` → the met1
    stub pairs, the access stub keeps its self-loop);
  - pin cell covered by **no** strip (unbound): the stack climbs from
    min(accesses, `met1`) through `max(accesses)+1` — OpenROAD covers
    a pin cell through `conn+1`, measured on picorv32's uncovered io
    cells (a `met3` port emits `li1..met4`). This is what keeps a
    `*P` port off a dead island even where no route strip covers its
    cell.
- **Bend via-stacks** (e4-topology): a non-pin GCell covered by both
  an H and a V run appends its GCell rect on `met1` then `met2`,
  consecutively — the same-point pair bridges the bend the way
  OpenROAD's own guide covers a turn cell. Without it the met1/met2
  subtrees of the same net stay disjoint and every downstream pin
  lands on a dead island (the dominant multi-node class). Bound pin
  cells are excluded — their rung stack already bridges.
- Two runs of `route` produce byte-identical files
  (`test_guide_emit_deterministic`).

## 8. API

See `crates/grus-core/src/congestion/guide.rs`: `GuideBox`, `NetGuide`,
`build_guides`, `write_guides`, `parse_guides`, `escape_guide_name`,
`guide_net_name`; `congestion/route.rs`: `RouteOutput`, `RoutedDesign`,
`route_design`; `congestion/rsmt.rs`: `decompose_design_nets_indexed`;
`design_db.rs`: `pin_io_layer`, `net_def_spellings`. CLI: `grus-eda route
--tlef --lef --def --guide-out [--gcell-tracks N] [--layer-adjust α]`.

## 9. Scope Notes

- `li1` appears only via stubs; `met3`-`met5` appear only when a convention
  above names them (I/O stubs on `met3` possible). The reference spreads wires
  across `met1`-`met4` because it assigns real layers — our convention is the
  documented approximation this ticket measures.
- Guide geometry is GCell-aligned only. No half-GCell rects, no track
  snapping — a detailed-router concern, out of scope here.
- Measured numbers, the criteria evaluation and the read-back record live in
  `docs/results.md`.

---

## 10. Guide-Based Parasitics (E4 ticket 2, probed 2026-11-22)

`estimate_parasitics -global_routing -spef_file` on a guide file: the model
measured off `gcd.guide` and confirmed against OpenROAD
`est/MakeWireParasitics.cpp` + `grt/GlobalRouter.cpp` (BSD source, pinned
revision — read to explain the measured numbers, not copied). Per-net
`*D_NET` totals on gcd are reproduced exactly by the rule below
(`_000_`-`_003_`, `req_msg[0]` verified to the last printed digit).

**Segments.** Each box maps to `GSegment`s in file order on its layer
(`boxToGlobalRouting`): first covered cell index `xl/tile`, last
`xh/tile - 1` (raw-DBU floor — a box ending mid-cell drops that cell,
tile-aligned or not; a box wholly inside one cell gives `x0 > x1` and is
dropped). Single covered cell → one zero-length same-layer segment; 1×N
strip → chain of center→center segments (`tile` apart); a box covering
multiple cells in *both* dims emits **no** segments. Segment endpoints
are tile centers `idx*tile + tile/2` (+die origin), never the
die-edge-clamped midpoint.

**updateVias** then rewrites consecutive pairs once: a zero-length seg on
layer `L` adjacent (same point) to a seg on `L+1` becomes an `L→L+1` via;
symmetrically a trailing via below the previous seg's layer is raised.
This is what turns `li1`/`met1` stub pairs into real `mcon`/`via1` vias;
a stub box not adjacent to an `L+1` segment stays a same-layer
zero-length segment → self-loop resistor (still charged the cut-layer R
above it, electrically dead).

**Nodes** are `(x, y, layer)` RoutePts = covered (cell, layer) pairs at
tile centers. Wire segs: `R = r_layer · len`, cap split half/half on the
endpoints (pi model). Via segs: `R` = the LEF `RESISTANCE` of the cut
layer above `min(init, final)` (mcon 9.3 Ω, via1 4.5 Ω, via2/via3 3.41 Ω,
via4 0.38 Ω on sky130). `mergeSegments` (colinear degree-2 merges) only
changes node granularity, not totals.

**Pin attach** (`makeParasiticsToPin` + `Pin::findOnGridPositions`):
pin `pt` = ll-corner of the pin's last routing-layer geometry box
(every routing-layer LEF/DEF box, transformed by the instance's
placement and orientation — all eight orientations corner-wise).
`conn_layer` = the pin's highest routing layer, or for pad/macro
die-edge pins the highest layer matching the edge direction (test
designs have none). `grid_pt` = majority vote over the on-grid
positions of the pin's *connection-layer box middles* — each box's
middle maps to its covered GCell's center (`Grid::getPositionOnGrid`,
index clamped; first candidate wins ties) — and a pin overlapping a
single track on its layer re-snaps `grid_pt` onto that track
(`pinOverlapsWithSingleTrack`: span ≤ 3 pitches and exactly one covered
track). This per-box vote — not "the cell containing `pt`" — is what
the picorv32 comparison required: it is the difference between a pin
attaching to a nearby route node and a far island hop (root-caused on
`_02629_` and `_00007_` in `docs/results.md`). Look up the node at
`(grid_pt, conn+1)` first: if present, attach with `R =
via_above(conn) + r_{conn+1} · Manhattan(pt, grid_pt)` on layer
`conn+1`; else `(grid_pt, conn)` with `R = r_conn · Manhattan`, no via;
else the pin node is left dangling (`EST-0026`, no resistor and no
cap). The pin-wire cap splits half to the pin node, half to the grid
node.

**`*D_NET` total = Σ segment caps + Σ pin-wire caps** — the pin-node
halves are counted in the total but not printed under `*CAP` (measured:
gcd `*D_NET` Σ 0.7397 pF vs `*CAP` Σ 0.5978 pF; the difference is exactly
the pin-wire caps). `set_wire_rc -layer` is ignored for guide
parasitics — met2 vs met4 SPEFs are byte-identical; every box charges
its own layer's LEF RC (`R = RPERSQ/width`, `C = CPERSQDIST·width +
2·EDGECAPACITANCE`, the `WireRc::from_layer` formula).

Our `estimate_guide_parasitics` reproduces this on `NetRcTree`: one leaf
node per pin (driver's pin node is the root), covered (cell, layer)
nodes with wire links between adjacent covered cells on the same layer
(pitch length), via links per the updateVias rule, pin edges as above.
Pin geometry comes from `MacroPin.rects` (LEF PORT boxes) and
`DesignDb.pin_io_rects` (DEF PINS `+ LAYER` rects absolutized with the
`+ PLACED` origin) so `pt`/`grid_pt`/`conn` match OpenROAD per box.
Pins in the partial die-edge strip resolve to the last full tile. A pin
whose cell has no node on `conn`/`conn+1` mirrors `EST-0026`: it
attaches to the net's nearest node in the driver's component
(Manhattan), carrying no cap — our tree needs a path to the driver
where OpenROAD leaves the pin dangling.

**Disconnected islands (dead components).** OpenROAD's segment graph is
not always connected — a stub box that never pairs into an `L+1`
segment stays a self-loop island, and whole detour branches can end up
in a second component (gcd `_132_`: 118 route nodes, two components).
OpenROAD's SPEF leaves such components electrically dead: island `*CAP`
entries stay, unpaired stubs become `n n r` self-loop resistors, and no
resistor joins an island to the driver's component — island pins see no
wire delay and island caps never load the driver's path.

`NetRcTree` mirrors this instead of bridging: after the driver-rooted
BFS, each remaining component is appended as a **dead subtree** rooted
at its lowest-index node as a self-parented sentinel. Island caps stay
in `wire_cap_pf`/`total_wire_cap_pf` (so `Σ*CAP == *D_NET` round-trips),
self-loops are recorded per node and re-emitted verbatim by
`write_spef`, and `elmore_ns` returns 0 for pins on dead nodes. The
same rules apply to `spef_parasitics` on the read-back side (a dead
`*RES` component is kept, not rejected).

**Lumped fallback.** OpenSTA does not always reduce a partially
connected net to pi/elmore. Probed with `report_dcalc` on the
gcd/picorv32 guide SPEFs (no `Pi model` line → the driver is looked up
at `total_output_net_capacitance`, and every pin — live or dead — gets
zero wire delay):

- a dead `*P` **port** always forces the lumped fallback (the elmore
  reduction cannot produce a port delay); the lumped load is the
  pin-capacitance sum only — wire capacitance is excluded entirely
  (measured on `gcd.guide.ours.spef` `_332_`/`_284_`, `picorv32`
  `mem_la_wdata[14]`);
- with only dead `*I` pins, the net goes lumped once the island carries
  roughly a third of the net's wire capacitance — the probed band
  (~0.29–0.34) is itself inconsistent between structurally identical
  nets, so `NetRcTree::lumped` implements `dead_cap >= 0.32 * total`
  as an approximation, documented as such.

The `lumped` flag then suppresses the pi model (`pi_models[..] = None`,
so `driver_response` evaluates the tables at the pin-cap load) and
makes `wire_delays` return zero for every pin.

## 11. End-to-End Guide STA Golden (E4 ticket 3, measured 2026-11-22)

Step 9c of each `generate_ref.sh` emits `<d>.guide.sta.reference.json`
with three OpenSTA endpoint blocks on `<d>.placed.def` + `<d>.sdc` —
one `openroad` process each so every `read_spef`/`estimate_parasitics`
starts clean:

- `guide_spef` — `read_spef <d>.guide.spef` (OpenROAD's guide → its
  `-global_routing` RC). The criterion-4 **gate** reference:
  `test_<d>_guide_sta_matches_opensta` compares our
  `sta --spef <d>.guide.ours.spef` consumer path against it.
- `guide_ours_spef` — OpenSTA `read_spef` on our
  `<d>.guide.ours.spef`. Ours-vs-this is the pure STA-engine residual
  (`test_<d>_guide_spef_engine_residual`, asserted), and the block's
  existence is the `write_spef` interop check.
- `guide_ours_readback` — `read_guides runs/guide-check/<d>.ours.guide`
  + `estimate_parasitics -global_routing`. Diagnostic only: vs
  `guide_spef` it isolates guide topology (e4-1), vs `guide_ours_spef`
  the RC model (e4-2). `test_<d>_guide_sta_decomposition` prints the
  four-row axis table (`topology` / `parasitics` / `engine` / `total`);
  only the two asserted tests gate, the decomposition rows and the
  named-cause `*_GUIDE` floors are regression guards.

Our side regenerates via `tests/designs/generate_guide_ours_spef.sh`
(release binary; `*VERSION` stamped from the workspace version).
Verdict and named-cause numbers: `docs/results.md` `e4-sta-e2e` and
`e4-guide-islands` (the island-semantics fix — parasitics-axis median
collapsed to ~0; the gate residual is now the e4-1 topology axis).
