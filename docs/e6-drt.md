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

Three named finding kinds, all gate-blocking on the test designs:

- `off_track` — the pin had candidate rects but zero access points
  (the shape sits entirely between tracks on every candidate layer).
- `no_shapes` — no candidate rects at all: unplaced cell, missing
  `pin_macro_pins` mapping, or io pin without `+ LAYER` rects.
- `TrackGap` — a routing layer whose preferred-axis list is empty.

On gcd/picorv32 all three are empty; any entry is a named finding with
its pin (`cell/pin` for instance pins, port name for io pins), not a
silent zero.

## 4. Claim boundary

The census gate proves **coverage and determinism on real inputs** —
every signal-net member pin has ≥1 candidate attachment on both
designs, and two runs emit identical point lists. Access points are
*not*:

- **DRC-clean points**: a point may collide with macro OBS or another
  net's shapes — spacing/OBS pruning is the maze router's problem.
- **Track-assigned points**: the report enumerates candidates; choosing
  among them is the TA stage, a later ticket.
- **A reproduction of TritonRoute's AP set**: different algorithm, a
  candidate superset — no comparison against TritonRoute's `pin_access`
  output is claimed.
