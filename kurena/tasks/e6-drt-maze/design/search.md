# e6-drt-maze — Search model

Matches the implementation at `2c3c713`, plus the bounded-backtrack
hardening (`steps.md` §1).

## 1. Per-net routing unit

Routing is strictly sequential, one `NetGraph` at a time in
`join.nets` (guide-file) order. A *route* is a subtree of the
region graph reaching every member pin's attach set. The maze tests
build graphs via `build_route_graphs_ext(.., spare_above = 1)`: the
guide boxes' XY is covered on `top_guided + 1` too, because
OBS-sealed pin pockets are reached one layer above the global
route's layer choice (recorded in `docs/e6-drt.md` §7 — gcd `_038_`
in the reference routed DEF escapes its li1+met1 guide on met2).

- **attach set** = the pin's `pin_nodes`, or the projection fallback
  (§2) when that is empty.
- **tree** = nodes already in the routed subtree (seeded with the
  driver's attach set).
- **targets** = attach sets of still-unconnected member pins.

## 2. Attach sets — strict, then projection

**Strict**: `NetGraph::pin_nodes` (in-region nodes inside the pin's
candidate rects), unchanged from `e6-drt-graph`.

**Projection fallback** (`project_attach`), only when strict is
empty: for each candidate rect `(l, r)` from `pin_shapes`, the
in-region nodes on `l` whose `ix`/`iy` sit inside the covered ranges
**expanded by one line on each side**, on at least one covered line.
A met2 io-port square holds an x-track but no y-track; the tapping
wire runs *along* that x-track, so adjacent-line nodes are the
connection points. The one-line margin matters: far nodes on a
covered line can coincide with *another* pin's attach node, which
`absorb_taps` would then claim as a zero-edge connection. Computed
on the sorted node order — deterministic. Pins connected through it
are recorded on `NetRoute::projected`. Still empty ⇒
`FailKind::Unattachable`.

## 3. Edges — in-plane, strict via, relaxed via

One unified `u32` edge index space per net:

```
[0, E)        in-plane `g.edges`
[E, E+V)      strict `g.via_edges` (shared (x,y) crossings)
[E+V, E+V+R)  relaxed via edges (`relaxed_vias()` output)
```

`other_end(g, relax, eidx, v)` resolves the far endpoint across all
three classes.

**Relaxed via edges** (`struct RelaxedVia { a, b, x, y, lower }`,
private): the strict rule — a via only at a node shared by both
layers — is stronger than physics. A real via needs a *preferred*-
track crossing: the vertical layer's x-track and the horizontal
layer's y-track. Strict crossings coincide every 7820 DBU on
li1–met1/met1–met2 and never on met2–met3, leaving pin areas with no
via edge. So for every node `v` on routing layer `k` (with `k+1`
still a routing layer and `pairs[k].default.is_some()` — a `ViaGap`
pair emits none) the maze synthesizes one edge to `k+1`:

- Upper `Horizontal` ⇒ the via sits at `(v.x, up.tracks_y[jy])`,
  `jy` the upper y-track nearest `v.y`, and `b` is the in-region
  upper node on that line whose ix is nearest `v.x` (the two
  bracketing entries of the line's sorted ix list, lower index on
  ties).
- Upper `Vertical` ⇒ mirrors it: `(up.tracks_x[jx], v.y)`, `b` on
  that x-line nearest `v.y`.
- `a` = `v`. The true position `(x, y)` is on both layers'
  preferred wires; the hang node is the same wire a track step away
  at most. Recorded looseness: the implied stubs may step outside
  the guide boxes (guide boxes are GCell-boundary, not exact) and
  are not searched — the edge's occupancy check scans the via
  footprint only.
- Emitted in node order — deterministic. Parallel strict/relaxed
  edges are left alone: equal `VIA_COST` breaks toward the strict
  arc (CSR order), extraction dedupes the placement by `(lower,x,y)`.
- **Single-nearest rule (recorded gap)**: if the nearest upper line
  holds no in-region node, that node gets no relaxed edge — a
  candidate cause for stranded pockets (H3); widening to the
  next-nearest lines is the first knob (`occupancy.md` §5 C3).

The implied geometry is committed, not just traversed: extraction
emits the two stub wires — lower-layer wire `a.xy → (x,y)` and
upper-layer wire `(x,y) → b.xy` (each along a preferred track line
of its layer, degenerate when the endpoints coincide) — so
`NetRoute.wires`, occupancy marks, and the collision census cover
the real metal. `docs/e6-drt.md` §7 records this model change.

## 4. Tree extension (pathfinder-style)

`db.net_driver(net)` picks the seed — driver attach set, else the
first attachable member in `pin_nodes` order. While unconnected
attachable pins remain:

1. Multi-source Dijkstra: every tree node at cost 0.
2. `BinaryHeap<Reverse<(u32,u32)>>` — total order, equal costs pop
   the lower node id; CSR neighbor order is in-plane edges, then
   strict via edges, then relaxed via edges, each both directions.
3. First popped node carrying an unconnected attachable pin wins;
   backtrack `parent` edge ids to the tree, append the simple path
   (strictly decreasing `dist` chain — always simple), mark the pin,
   `absorb_taps` for pins whose attach set already touches the tree.
4. Wave exhaustion with targets left ⇒ net fails this round;
   remaining pins named `NoPath`.

`dist`/`parent` use per-round stamps (`dstamp`/`pstamp`) — no
O(nodes) reset per round.

**Backtrack bound (hardening)**: the `while !in_tree[v]` walk is the
only unbounded loop in the router. A parent chain is provably simple
(strictly decreasing dist to a dist-0 tree node), but if a bug ever
breaks that invariant the loop would hang silently — exactly the
reviewer's "stuck vs slow" ambiguity. Bound the walk at
`g.nodes.len()` steps; exceeding it returns the net as `NoPath`
named failure (a bug-shaped result, still bounded, still reported).
This is a cheap invariant guard, not a routing heuristic.

## 5. Costs — recorded constants

| constant | value | meaning |
|---|---|---|
| `PREF_COST` | 1 | in-plane edge along the layer's preferred axis |
| `OFF_AXIS_COST` | 2 | in-plane edge against it (stubs/jogs) |
| `VIA_COST` | 4 | one layer transition (strict or relaxed) |
| `OCC_PENALTY` | 8 | per distinct foreign net hit, soft iterations |
| `HIST_STEP` | 8 | added per re-queue to the penalty multiplier |
| `MAX_ITERS` | 8 | iteration cap; the last iteration is hard mode |

Preferred axis (the direction-to-axis flip): `Horizontal` layer →
`Axis::X` edges; `Vertical` → `Axis::Y`. Hop count, not distance —
recorded simplification. Via legality: traversable iff
`pairs[k].default.is_some()`; relaxed edges are pre-gated by
`relaxed_vias`, strict edges gated in the CSR build. Edge cost =
`base + soft occupancy extra` or blocked (`occupancy.md` §2).

## 6. Rip-up and iteration rule

```
hist[n] = 0;  bad = all guided nets (join order)
for it in 1..=max_iters:
    hard = (it == max_iters)
    for net in bad (join order):
        route the net (soft unless hard)
        connected → commit wires/vias/stubs into occupancy;
            soft+Enabled: the foreign hits taken are recorded as
            provisional collisions on both parties
        else → stays unrouted this round
    if hard: unrouted = failures; break
    bad = unrouted ∪ both parties of every provisional collision
    if bad.empty: break
    rip up each bad net's logged Marks (free occupancy), hist[i] += 1
```

- Kept nets stay committed; re-routed nets see them as occupancy.
- Per-net `hist` escalation breaks the two-net ping-pong cycle.
- After the cap, `overlap_census` on the committed routes produces
  the exact collision list in `RouteDb` (skipped under
  `OccMode::Disabled`).

## 7. Unrouted taxonomy

`UnroutedNet { net, connected, failed: Vec<(PinId, FailKind)> }`.

- `Unattachable` — strict + projection attach empty.
- `NoPath` — attachable but unreachable this round: pocket walls
  (`occupancy.md` §5 classes), region-component splits
  (met3-bridge class — see `tests.md`), or hard-mode blocking.
- A leftover unattachable member after the loop lands the net in the
  report; empty graph is `NoPath`, never a panic.

## 8. Determinism contract

Fixed net/pin/neighbor order, `(cost,node)` total-order PQ, integer
costs, canonical extraction (merged sorted wire intervals, sorted
via placements), `BTree` containers throughout, `relaxed_vias`
emitted in node order. The `NetTopo` cache (perf lever L2) reuses
byte-identical topology across iterations — construction order is
unchanged. Two full runs produce identical `RouteDb` (full
`assert_eq` plus `xxh3` digest — nonzero evidence).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt/graph.rs`
