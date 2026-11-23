# e6-drt-maze — Search model

## 1. Per-net routing unit

Routing is strictly sequential, one `NetGraph` at a time in
`join.nets` (guide-file) order. A *route* for a net is a set of
committed in-plane edges and via edges — a subtree of the region
graph that reaches every member pin's attach set.

Terminology:

- **attach set** of a pin = its `pin_nodes` node ids, or the
  projection fallback below when that is empty.
- **tree** = nodes already part of the routed subtree (starts as the
  driver pin's attach set).
- **targets** = attach sets of still-unconnected member pins.

## 2. Attach sets — strict, then projection

**Strict** (unchanged from `e6-drt-graph`): `NetGraph::pin_nodes` —
in-region nodes lying inside the pin's candidate rects.

**Projection fallback**, computed lazily in `maze.rs` only when the
strict set is empty: for each candidate rect `(l, r)` of the pin
(`drt::access::pin_shapes`, the same census source), let
`X = covered_range(tracks_x[l], r)` and `Y = covered_range
(tracks_y[l], r)`; the attach set is every in-region node on layer
`l` with `ix ∈ X` **or** `iy ∈ Y` — the crossings on the track lines
that touch the pin shape. Rationale: a met2 io-port square holds an
x-track but no y-track; the wire that taps it runs *along* that
x-track, so any in-region node on the line is a legal connection
point in this model. The same rule covers the GCell-boundary
instance-pin escapes (the in-box sliver holds one axis).

- The set is computed on the same sorted `region` order node ids were
  assigned — deterministic, no re-sort.
- Pins connected through a projected attach set are recorded on
  `NetRoute::projected` — the loosening is visible in the report, not
  folded silently into `connected`.
- Still empty (e.g. the pin rect holds no track at all — the
  `off_track` class — or the region never enters the rect's lines):
  the pin is `Unattachable`, the net lands in the unrouted report
  with that pin named.

## 3. Tree extension (pathfinder-style)

`db.net_driver(net)` picks the seed — the driver pin's attach set,
or `pin_nodes[0]`'s pin's set when no pin is a driver (all-input io
nets do not occur in the guides, but do not rely on it). Then, while
unconnected pins remain:

1. Multi-source Dijkstra: push every tree node at cost 0.
2. Expand `(cost, node)` in `BinaryHeap<Reverse<(u32, u32)>>` order —
   total order, equal costs pop the lower node id; neighbor
   enumeration is edge-list order (in-plane `edges` then
   `via_edges`), each in-plane edge contributes both directions.
3. First popped node that lies in any target attach set wins:
   backtrack `parent` links to the tree, append the path (nodes +
   edge kinds) to the tree, mark that pin connected, repeat.
4. Dijkstra exhaustion with pins left = those pins are unreachable
   this round → the net is unrouted-candidate; the remaining pins are
   named (`NoPath`). One exhaustive pass costs one scan of the
   driver's component — no separate component labeling pass (a
   disconnected net pays exactly that).

Adjacency: build a CSR neighbor list per net per routing round
(`Vec` of `edge_idx` runs — edges and via edges share one index
space internally). `dist`/`parent` are `u32` arrays sized
`nodes.len()`, reset per round; `parent` stores the arriving edge
kind so backtracking distinguishes wire spans from via placements.

## 4. Costs — recorded constants

All integer; the constants live in `maze.rs` and are restated in
`docs/e6-drt.md` §7.

| constant | value | meaning |
|---|---|---|
| `PREF_COST` | 1 | in-plane edge along the layer's preferred axis |
| `OFF_AXIS_COST` | 2 | in-plane edge against it (stubs/jogs) |
| `VIA_COST` | 4 | one layer transition (≈ two preferred hops) |
| `OCC_PENALTY` | 8 | per distinct foreign net hit, soft iterations |
| `HIST_STEP` | 8 | added to `OCC_PENALTY` multiplier per re-queue |
| `MAX_ITERS` | 8 | iteration cap; the last iteration is hard mode |

**Preferred axis** (the direction-to-axis flip — an edge's `axis` is
the coordinate index it moves along): `Horizontal` layer → preferred
`Axis::X` (a horizontal wire rides a `tracks_y` line between
consecutive `ix`); `Vertical` → `Axis::Y`.

**Via legality**: a `ViaEdge` on pair `k` is traversable iff
`vias.pairs[k].default.is_some()` — a `ViaGap` pair forbids the
transition even though the geometry emitted edges (the
`e6-drt-via` contract: the router joins table + graph, so there is
no phantom routability). Cost is flat `VIA_COST`; the default
candidate's metal rects are what the commit rasterizes.

**Occupancy in the cost**: see `occupancy.md`. Per candidate edge,
foreign-net hits add `OCC_PENALTY × (1 + hist[net]) × hit_count` in
soft mode; `Obs` hits are always a hard block; foreign-net hits are
a hard block in the final iteration; own-net intervals are free.

No distance weighting: edges are unit index steps; hop count is the
cost, recorded as the simplification it is (merged LEF/DEF grids can
be non-uniform, sky130's are uniform per layer).

## 5. Rip-up and iteration rule

```
hist[n] = 0 for all nets;  bad = all guided nets (join order)
for it in 1..=MAX_ITERS:
    hard = (it == MAX_ITERS)
    for net in bad (join order):
        route the net (tree extension; soft unless hard)
        if fully connected: commit wires/vias into occupancy;
            in soft mode the foreign hits taken are recorded as
            provisional collisions on both parties
        else: it stays unrouted this round
    bad = unrouted nets ∪ nets named in any provisional collision
          (either party)
    if bad.is_empty(): break
    rip up every net in bad (free its occupancy + collision records)
    hist[bad] += 1
```

- Kept nets stay committed; re-routed nets see them as occupancy.
- `hist` escalation is per net — deterministic, and it breaks the
  ping-pong cycle where two nets keep colliding on the same line.
- After the cap, remaining unrouted nets go to the report with
  per-pin `FailKind`; a post-hoc `overlap_census` on the final
  routes produces the collision list that lands in `RouteDb` (exact
  on the committed result, not search-time bookkeeping).

## 6. Unrouted taxonomy

`UnroutedNet { net, connected, failed: Vec<(PinId, FailKind)> }` —
the test prints `cell/pin` or port names.

- `Unattachable` — strict + projection attach set empty (named
  carryover of `region_escape`/`off_track`; the pin is reported once,
  the net does not search).
- `NoPath` — attach set exists but no Dijkstra reached it this
  round: region-component split (the met2–met3 bridge class — the
  census test classifies it via the guide-box probe from
  `overview.md`) or hard-mode blocking at the cap.

Single-pin and empty-region nets can't occur (`join.nets` only holds
guided ≥2-pin signal nets with nonzero coverage — both are asserted
findings upstream); a defensive empty-graph case is still a
`NoPath`, never a panic.

## 7. Determinism contract

- Fixed net order (guide file), fixed pin order (`net_pins`), fixed
  neighbor order, `(cost, node)` total-order PQ, integer costs only.
- Route extraction canonicalizes: wires are merged per
  `(layer, line)` into sorted intervals; vias sorted
  `(layer, x, y)` — output is identical regardless of discovery
  order.
- Two full `route_nets` runs produce an identical `RouteDb`
  (`assert_eq` on the whole struct plus an `xxh3` digest helper —
  `xxhash-rust` is already a workspace dep).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
