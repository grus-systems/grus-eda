# e4-guides-emit — Guide emission semantics

The rules `build_guides` implements. This document is the design-time
statement of the rule; the implementation ticket must land the same rule in
`docs/e4-guides.md` in the style of `docs/e1-congestion.md` (AGENTS.md
§7.6: the rule goes in `docs/` first, then the code).

## File format (P-GR measured, `docs/results.md` 2026-11-22)

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

Net names are written with the **raw DEF spelling** (measured 2026-11-22:
OpenDB stores net names verbatim — `ctrl.state.out\[1\]` keeps its
backslashes, `req_msg[0]` stays bare — and `read_guides` matches on
exactly that token; over- or under-escaping fails `GRT-0234 Cannot find
net`). `DesignDb` therefore keeps `net_def_spellings`, the raw NETS token
per net, and emission writes it verbatim. For non-DEF sources the field
is empty and emission falls back to `escape_guide_name(net_names[n])`
(DEF-word escapes plus `[`/`]`, which are bus delimiters in OpenDB).

## Grid → box geometry

`GCellGrid` gives `x_bounds(gx, die_xh)` / `y_bounds(gy, die_yh)`. A GCell
`(x, y)` rect is `[x_bounds(x).0, y_bounds(y).0, x_bounds(x).1,
y_bounds(y).1]`. Edge index decoding (as in `route.rs`):

- `GridEdge::H(idx)`: `y = idx / (nx - 1)`, `x = idx % (nx - 1)`; connects
  GCells `(x, y)` and `(x+1, y)`.
- `GridEdge::V(idx)`: `y = idx / nx`, `x = idx % nx`; connects `(x, y)` and
  `(x, y+1)`.

## Layer assignment (documented convention)

Signal band `set_routing_layers -signal met1-met5`; preferred directions
from `RoutingLayer.dir` (sky130: met1 H, met2 V, met3 H, met4 V, met5 H).
The 2D router has no third dimension, so layer assignment is fixed:

- every committed **H edge → `met1`** (lowest horizontal-preferred layer);
- every committed **V edge → `met2`** (lowest vertical-preferred layer);
- a GCell incident to both an H edge and a V edge of the net is covered by
  both strips → it lands on `met1` **and** `met2`. That pair is the via
  coverage OpenROAD expresses the same way (per-layer boxes over the same
  rect);
- **clock nets are not banded** — `route` loads no SDC, so every net uses
  the signal assignment. Recorded as an approximation in `docs/results.md`;
  the `met3-met5` clock band belongs to a later ticket that wires SDC in.

## Strips

Per net, per direction, per line (row `y` for H, column `x` for V):
collect the committed edges, deduplicate, and merge each maximal run of
contiguous edge indices into **one** strip rect spanning the union of the
endpoint GCells:

- H run `x = a..=b` on row `y` → `[x_bounds(a).0, y_bounds(y).0,
  x_bounds(b+1).1, y_bounds(y).1]` on `met1`;
- V run `y = a..=b` on column `x` → `[x_bounds(x).0, y_bounds(a).0,
  x_bounds(x).1, y_bounds(b+1).1]` on `met2`.

Merging matches the reference's convention of one box per straight run and
keeps the total box count comparable (done-when metric 2). Edges
re-traversed by overlapping Steiner segments dedup to one strip.

## Pin-access stubs

Per pin of the net, one stub rect = the pin's GCell rect, emitted on:

1. the pin's **access layer**:
   - instance pin → `MacroPin.layer` (the first LEF PORT layer;
     `li1` for `sky130_fd_sc_hd` cells),
   - I/O pin → `pin_io_layer` (the DEF `+ LAYER`; `met2`/`met3` for
     `place_pins -hor_layers met3 -ver_layers met2`), falling back to
     `met1` when absent;
2. **`met1`** — the stack stub from the access layer into the routing band
   (mirrors the li1/met1 stub pairs the reference emits).

Stub boxes dedup across pins sharing a GCell. A net whose pins all sit in
one GCell produces no segments; its block still carries the stubs. Nets
with **fewer than 2 pins** emit no block — a lone port pin has nothing to
connect (measured: picorv32's 67 `irq`/`pcpi_*` port-only nets carry no
reference block; gcd's `_003_` — two pins in one GCell — does).

## Determinism

- Nets emit in `net_names` order (DEF order).
- Boxes within a net emit sorted by `(layer index, yl, xl, xh, yh)`.
- The pipeline is deterministic end-to-end (rayon collects per net before
  flattening; the router is sequential), so two runs must produce
  byte-identical files — the tests assert exactly that.

## Scope notes

- `li1` appears only via stubs; `met3`-`met5` appear only when a
  convention above names them (I/O stubs on `met3` possible). The reference
  spreads wires across `met1`-`met4` because it assigns real layers —
  our convention is the documented approximation this ticket measures.
- Guide geometry is GCell-aligned only. No half-GCell rects, no track
  snapping — that is a detailed-router concern, out of scope here.
