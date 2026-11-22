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

Both bounds of E4.1/E4.2 are provisional per `kurena/tasks/next-stage-scoping/design/done-when.md`.

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
- Boxes within a net emit sorted by `(layer, yl, xl, xh, yh)`; `write_guides`
  sorts + dedups so the order holds regardless of caller order.
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
