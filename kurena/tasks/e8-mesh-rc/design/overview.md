# e8-mesh-rc — Overview

E8 ticket 3 (after `e8-pdn-parse`, which landed the per-net `PdnNet`
model, conn expansion, pin attach, union-find connectivity, and the
`TechDb` resistance fields). Turns each POWER/GROUND net's extracted
geometry — `wire_rects`, `vias`, pin attachments, source `PIN` — into
the resistor network (`PdnMesh`) that `e8-solve` will stamp into
`G·V = I`.

## Measured input facts (this checkout, 2026-11-23)

- `gcd.placed.def` per net (VPWR, VGND): 47 `wire_rects` (15 met1
  FOLLOWPIN rails, 15 met2 DRCFILL segments, 15 met3 DRCFILL, 1 met4
  STRIPE, 1 met5 STRIPE), 46 `PdnVia`s: 15× `via2_3` (`+ LAYERS met1
  via met2`, `cut_count` 5 from `+ ROWCOL 1 5`), 15× `via3_4`
  (met2–met3, `cut_count` 4 from `+ ROWCOL 1 4`), 15× `via4_5`
  (met3–met4, 4 cuts), 1× `via5_6`
  (met4–met5, 1 cut). 384 attached instance terminals per net on
  met1 rails; the top-level `PIN` carries met5 + met4 io rects.
- `picorv32.placed.def` per net: 615 `wire_rects`, 792 `PdnVia`s
  (261× each `via2_3`/`via3_4`/`via4_5`, 9× `via5_6`), 10,205
  attached terminals.
- Resistance sources already in `TechDb` (all consumed by guide
  parasitics today — **no second table**):
  - `routing_layers[i].res_per_sq` — tech-LEF `RESISTANCE RPERSQ`
    per routing layer (`f32`, 0.0 when the layer omits it).
  - `TechDb::via_res_ohm(cut, cut_count) -> Option<f32>` — the cut
    layer's `via_res_ohm_above[cut.above] / cut_count`, `None` when
    the cut layer carries no `RESISTANCE`. `PdnVia::cut_count` is
    already normalized at parse (LEF cut-`RECT` count or `+ ROWCOL`
    product), so the mesh reads one field.
- Ticket-2 surface the mesh builds on (`pdn.rs`, same module):
  `try_build_graph` → `PdnGraph { nodes, attachments: Vec<(PinId,
  u32)>, source_nodes, conn_pins, unreachable_pins, … }`. `PdnNode`
  components are connectivity components — one node per whole
  same-layer-plus-vias island — **too coarse for a resistor mesh**
  (gcd: 1 node per net). The mesh is a *second, finer* node space
  over the same rects (`model.md`).
- `expand_conns`, `is_attachable`, `rects_intersect`,
  `rect_contains_point`, `UnionFind` are private items of `pdn.rs`;
  a child module `pdn::mesh` reuses them via `use super::…` — no
  visibility changes needed.
- No PDNSim mesh golden exists: `analyze_power_grid` reports only
  voltage scalars + the per-instance CSV. The mesh granularity is
  therefore ours; the census is **recorded, not gated** (acceptance
  says "record actual counts, no tuning"), and the `e8-e2e` ticket is
  the gate on the solve's output.

## Key decisions

- **Junction/run mesh model** (`model.md`): a mesh node is a
  *junction region* — a maximal merged set of same-layer rect
  intervals where a feature lands (rect end, same-layer overlap span,
  via landing, pin-attach intersection, source io-rect
  intersection). Between two junction nodes along one rect, the
  metal run is a wire resistor `R = res_per_sq × L/W`. This is the
  finest granularity consistent with the spec's "same-layer wire
  rects that touch/overlap merge into one node": the overlapped span
  itself becomes the shared node, and the unfeatured metal between
  nodes carries the resistance — which is what makes the
  acceptance's `wire R count` nonzero at all.
- **Center-to-center convention** for run length: `L = |center_b −
  center_a|` along the rect's major axis (spec's "between adjacent
  node centers"); `W` = the run's own rect minor extent. Junction
  regions are real metal; their half-lengths lump into the
  neighbouring runs.
- **No `DesignDb` schema change**: the mesh is derived data consumed
  in-memory by `e8-solve`; `build_mesh` recomputes it from
  `PdnNet` + `TechDb` on demand. `design_db.rs` is listed in the
  spec as "mesh storage or builder input" — it stays builder input
  only; no new field, no serde surface.
- **`pdn.rs` stays a file**, gains a child module
  `crates/grus-core/src/pdn/mesh.rs` (Rust-2018 `pdn.rs` + `pdn/`
  layout) and re-exports it. `e8-solve`'s spec anticipates
  `pdn/solve.rs` as the sibling.
- **`build_mesh` errors hard** only on structural impossibilities
  (conn-expansion `Err`, via resistance `None` — an undefined
  cut-layer `RESISTANCE` means the mesh cannot be priced). Soft
  findings (non-finite / non-positive R, nodes unreachable from any
  source, dangling via ends) are reported by `PdnMesh::validate()`
  — that is the fail-proof surface the acceptance's break→observe→
  restore ritual runs against.
- **Sinks = the ticket-2 attach set verbatim**: one `MeshSink` per
  *distinct* attached `PinId` (384 / 10,205 per net — matches the
  reference CSV row counts). A pin touching several disjoint nodes
  resolves to the lowest node index deterministically; the count of
  such pins is recorded in the census, not hidden.
- **Additive API → patch bump** `0.7.0 → 0.7.1` in the same commit.

## Files

Implementation is expected to touch:

- `crates/grus-core/src/pdn.rs` (`pub mod mesh;` + re-export, `RESISTANCE` added to the synthetic `PDN_LEF` fixture layers, new tests)
- `crates/grus-core/src/pdn/mesh.rs` (new: feature intervals, junction-piece union, run/via resistors, sink/source maps, census, `validate`)
- `Cargo.toml` (workspace version bump 0.7.0 → 0.7.1)
- `docs/results.md` (per-net census both designs, runtime, fail-proof observation)
- `docs/roadmap.md` (§E8 progress line)

`crates/grus-core/src/design_db.rs` is *not* expected to change —
builder input only (spec's "mesh storage or builder input": the
mesh is derived, rebuilt on demand).
