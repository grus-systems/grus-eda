# e8-mesh-rc — Mesh model

Two node spaces exist over the same `PdnNet` geometry; keep them
distinct:

- **`PdnGraph`** (ticket 2): connectivity components — same-layer
  rects unioned on strict overlap, then merged by via edges. Answers
  *"is everything connected"*; gcd gives `nodes == 1` per net.
- **`PdnMesh`** (this ticket): the resistor network — junction
  regions are nodes, unfeatured metal runs and via stacks are
  resistors. Answers *"where does V drop"*; gcd gives a few hundred
  nodes per net (recorded, `tests.md`).

## Junction / run model

The spec's "same-layer wire rects that touch/overlap merge into one
node" is applied at **junction granularity**, not component
granularity: two same-layer rects share one electrical node only
inside the span where they overlap; the metal on either side of that
span is a resistor. A component-level node would make `wire R count`
zero and the whole rail equipotential — physically wrong and
incompatible with the acceptance.

### Major axis

Each `wire_rect` gets a **major axis**: the axis of its longer
extent; ties (square stubs) use the layer's preferred direction
(`RoutingLayer.dir` — `Horizontal` → X, `Vertical` → Y). All
feature intervals, run lengths and widths below are taken along /
across this axis. `W` for `R = RPERSQ × L/W` is the rect's extent
perpendicular to the major axis.

### Feature intervals

Per rect, collect intervals along the major axis from five feature
kinds:

| kind | interval | source |
|---|---|---|
| rect end | `[min]` and `[max]` (points) | always — leaf nodes |
| same-layer overlap | overlap span projected on this rect's major axis | the same sorted-x sweep `build_graph` uses, per net layer |
| via landing | `[via.x]` or `[via.y]` (point) | every `PdnVia` whose point is contained (closed `rect_contains_point`) by the rect on `via.lower` or `via.upper` |
| pin attach | `intersection ∩ rect`, projected | each attachable conn pin's oriented absolute port rects (`MacroPin.rects` + `orient_rect` + cell offset) ∩ same-layer wire rects — the same intersection the graph's attach computes |
| source | `io_rect ∩ rect`, projected | each source `PinId`'s `pin_io_rects` (absolute) ∩ same-layer wire rects |

Feature sources outside a rect contribute nothing to it (a via
point on no rect is handled under *dangling ends* below).

### Junction pieces → nodes

1. Per rect, merge feature intervals by **closed union** (`[a,b]`
   and `[c,d]` merge iff `c <= b`) into sorted disjoint *junction
   pieces*. A via point on a pin interval's edge merges into it.
2. Each piece is a `RectDbu`: `[interval]` × the rect's full minor
   extent (a point interval gives a zero-width piece — allowed).
3. **Nodes**: union-find (reuse the ticket-2 `UnionFind`) over all
   pieces on the same layer under strict 2D overlap
   (`rects_intersect`) — the same geometric rule as the graph.
   Each merged set is one `MeshNode` carrying `layer`, the piece
   bboxes, and role flags.

### Wire resistors

Per rect, order its pieces along the major axis. Each consecutive
pair `(P, Q)` emits one resistor between `node(P)` and `node(Q)`:

```
L = |center_major(piece_Q) − center_major(piece_P)|   (DBU, i64 math)
W = rect minor extent                                  (DBU)
R = res_per_sq[layer] * L / W                          (f32)
```

Center-to-center per the spec's "between adjacent node centers":
junction pieces are real metal, so each piece's half-length belongs
to the adjacent run. Distinct pieces on one axis are disjoint
nonempty intervals → `L > 0` structurally whenever `res_per_sq > 0`.
Parallel resistors (the same node pair reached through two rects)
are kept — they are physically parallel paths.

### Via resistors

Each `PdnVia` emits one resistor:

```
R = tech_db.via_res_ohm(via.cut, via.cut_count)   // via_res_ohm_above / cut_count
```

endpoints = the node of the junction piece containing `(x,y)` on
`via.lower` and on `via.upper` (containment is by construction:
containing rects got the via point as a feature). If several pieces
on a side contain the point (possible only when the point sits on a
shared piece boundary — pieces are disjoint, so at most a
boundary-touch coincidence), pick the lowest node index and count
it in the census. `via_res_ohm` returning `None` (cut layer without
`RESISTANCE`) is a `build_mesh` `Err` naming net + via.

**Dangling ends**: a via endpoint on *no* rect creates a degenerate
node — `layer` = the via's declared `lower`/`upper`, empty `rects`,
`dangling: true` — and the resistor still connects to it (mirroring
the graph's `via_end` handling: a dangling via is modelled, then
flagged by `validate`, not silently dropped).

## Sinks

One `MeshSink { pin, node }` per **distinct** `PinId` in
`graph.attachments` — the set is taken from the graph verbatim, so
`sink set == ticket-2 attach set` holds by construction (384 /
10,205 per net). Node resolution: recompute the pin's port-rect ∩
wire-rect intersections (guaranteed non-empty — the pin is in the
attach set), collect the junction nodes under those intersections,
take the **lowest node index**. The census counts pins that offered
>1 candidate node.

## Sources

Each source `PinId`'s `pin_io_rects` ∩ same-layer wire rects
creates source features on the intersected rects (they are feature
kind *source* above); every junction node under those
intersections is a source node. A PIN rect touching several
disjoint nodes marks them all — spec item 5 verbatim. The `TopPin`
pin itself is in `conn_pins` but is not attachable (no cell), so it
never produces a sink.

## Roles & output shape

`MeshNode` role flags: `is_source`, `is_sink` (both are flags, not
an exclusive enum — a source node can coincide with a sink in
degenerate geometry). Everything else is `role: wire`.

`PdnMesh` is self-contained per net: `net` name, `nodes`,
`resistors` (each `(a, b, r_ohms, kind)` where `kind ∈ {Wire{rect},
Via{via}}` — the index into `PdnNet::wire_rects`/`vias` is kept for
diagnostics), `sinks`, `sources`.

## `validate()` — the fail-proof surface

`PdnMesh::validate() -> Vec<MeshViolation>` checks, in order:

1. **Every resistor `r_ohms` is finite and `> 0`** —
   `NonFiniteResistance` / `NonPositiveResistance`. This is what a
   zeroed `res_per_sq` trips: wire `R` collapses to `0.0`.
2. **Sink/source resolution** — every `sinks[].node` and
   `sources[]` entry is a valid node index (`DanglingSink`,
   `DanglingSource`); `sources` empty → `NoSources`.
3. **Source reachability** — BFS over the undirected resistor
   adjacency from all source nodes; every node not reached is
   `UnreachableNode { node }`. Removing via edges splits the mesh so
   met1-rail sinks lose the source path → red (the acceptance's
   second break variant). Dangling via ends are unreachable by
   construction and surface here.

Real designs (gcd, picorv32) must produce an empty violation list;
the census records counts alongside.

## Census

```rust
pub struct MeshCensus {
    pub net: SymbolId,
    pub nodes: u32,
    pub source_nodes: u32,
    pub wire_resistors: u32,
    pub via_resistors: u32,
    pub sinks: u32,
    /// Sinks whose pin offered >1 candidate node.
    pub multi_node_sinks: u32,
    /// Via endpoints that landed on no rect.
    pub dangling_via_ends: u32,
    /// Nodes with no resistor at all.
    pub isolated_nodes: u32,
    pub min_r_ohms: f32,
    pub max_r_ohms: f32,
    pub violations: u32,
}
```

## Arithmetic & units

All geometry stays in DBU `i32`; `L/W` is dimensionless so unit
conversion cancels. `r_ohms` is `f32` (matches `res_per_sq`,
`via_res_ohm_above`, and the codebase's `f32` numeric model).
`i32` minor extents: `W == 0` cannot occur — wire rects come from
`width > 0` DEF segments; a degenerate rect would make `L/W`
non-finite and lands as `NonFiniteResistance`.

## Complexity

Per net: same-layer overlap sweep ~linear (ticket-2 code path
reused); pin/via/source features are the same per-layer rect scans
the graph already does (O(pins × rects_on_met1) ≈ 10k × ~200 for
picorv32); per-rect interval merge `O(F log F)`; piece union is a
second sweep. No spatial index is added at this scale — measure,
don't pre-tune.
