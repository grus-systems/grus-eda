# e6-drt-graph — Region, node, edge, and attachment model

## 1. Region mapping

Per `(net, layer)` the region is the net's guide boxes on that layer
clipped to the track grid — a set of covered **track-index pairs**:

```text
for box b = (layer, [xl,xh] × [yl,yh]):
    ix range = [ tracks_x.partition_point(t < xl),  tracks_x.partition_point(t <= xh) )
    iy range = [ tracks_y.partition_point(t < yl),  tracks_y.partition_point(t <= yh) )
    region[layer] |= ix_range × iy_range
```

A box contributes only crossings that lie inside it — two diagonally
disjoint boxes do *not* create phantom corner nodes (that was the
rejected `covered_x × covered_y` reading: the "set of track indices
(x and y) covered" is the index representation of the union, not a
bounding cross product). Per-box coverage is always
index-contiguous because a box is a contiguous interval.

- "Preferred-axis tracks with no coverage are excluded" is automatic:
  uncovered track indices simply produce no pairs. A `(net, layer)`
  with ≥1 box but an empty pair set — boxes miss every track on at
  least one axis — is a named **`ZeroCoverage { net, layer }`** record.
  Measured on gcd: zero such pairs.
- Work is `O(Σ box × tracks-per-box)`, never `O(die area)`.

## 2. Nodes

One node per covered pair: `(layer, ix, iy)` indexing into the
layer's `tracks_x`/`tracks_y`. Coordinates are real tracks on both
axes by construction — a node is an x-track ∩ y-track crossing that
lies inside the box union.

**Enumeration order** (the determinism contract): layers ascending by
`LayerId`, then the region's pair set in `BTreeSet` order `(ix, iy)`.
`nets` themselves iterate in guide-file order. Two runs produce byte-
identical `nodes`/`edges` — the gate double-runs with nonzero counts.

## 3. Edges

An edge connects two nodes on the same layer at **consecutive track
indices**: `(l, ix, iy)–(l, ix+1, iy)` (axis X) and
`(l, ix, iy)–(l, ix, iy+1)` (axis Y), each unordered pair emitted
once in node order. This is the induced subgraph of the full grid
restricted to in-region nodes.

- **Both axes on every layer** — the maze router decides which axis
  is legal where (the edge carries its `axis`); cost assignment and
  direction legality are explicitly later tickets.
- **Rejected alternative**: connecting consecutive *covered* indices
  regardless of index distance would jump the corridor gaps where the
  region is genuinely absent — an edge must not wire across an
  uncovered span.
- **Known simplification, recorded**: an edge trusts that the unit
  segment between adjacent-index in-region nodes stays inside the
  union. Two boxes can cover adjacent x-tracks at one y-track while
  leaving a sub-pitch sliver between them (`A.xh < B.xl` strictly);
  GCell-aligned guide boxes abut instead, and the census does not
  model sub-track coverage.
- **No via edges** — layer transitions are the next ticket's model
  (via selection needs LEF `VIA` geometry, not yet read).

## 4. Access hookup — measured decision

Spec text: "an access point `(layer, x, y)` is attached when `(x,y)`
is an in-region track crossing (`x ∈ tracks_x ∧ y ∈ tracks_y` ∧ node
covered)". Applied literally to `drt::access`'s output it degenerates:
APs are `(track, rect_midline)` points, and a pin-rect midline is
almost never a track coordinate — on gcd only 99/933 member pins own
a literal-crossing AP (~11%). That cannot be the spec's intent, which
predicts escapes "only where the guide genuinely misses a pin".

**Rule as designed**: an AP stands for the crossing its track line
makes inside the generating shape, so attachment is evaluated per
pin over its candidate rects (the `drt::access::pin_shapes`
enumeration, widened to `pub(crate)`):

```text
attached_nodes(pin) =
    ⋃ over candidate rects (l, [xl,xh]×[yl,yh]):
        { node (l, ix, iy) ∈ region(net, l) :
          xl <= x_ix <= xh  and  yl <= y_iy <= yh }
```

Every in-region node inside a pin shape corresponds to an x-track AP
and a y-track AP of that pin, so this is exactly "the APs map onto
in-region nodes" — the node, not the AP's own midpoint coordinates,
must be a track crossing. A member pin with zero attached nodes on
every layer is a named **`RegionEscape { pin, net }`** — the pin, its
net, printed per design, never silently counted.

**Measured on gcd** (independent Python model): `attach = 839`,
`region_escape = 3` — `_313_/X` (net `_007_`), `_301_/B1` (net
`_134_`), `_307_/B1` (net `_138_`). All three are the same genuine
class: the pin's li1 shape straddles a GCell boundary and the guide
stub box covers the neighboring GCell — the sliver of shape inside
the box holds no track crossing (e.g. `_313_/X`: rect y-extent
[54655,56865] vs li1 box top 55200; the only y-track in the overlap
is covered by a rect fragment with no x-track inside it). This is the
spec's expected class — the guide genuinely misses the pin's on-grid
crossing. io-port escapes: 0.

## 5. Findings taxonomy (additions to `docs/e6-drt.md` §3)

| finding | meaning | gate |
|---|---|---|
| `missing_signal` | ≥2-pin signal net with no guide block | empty on both designs |
| `absent_expected` | <2-pin or non-signal net absent | named count only |
| `zero_coverage` | (net,layer) has boxes but no covered crossing | empty on both designs |
| `region_escape` | member pin of a guided net with no in-region node inside its shapes | partition gate; measured set pinned by name |

Guide-name resolution failure ("extra net") is a hard error at join,
not a census row.

## 6. What the graph does NOT do (for `docs/e6-drt.md`)

- No via edges / layer transitions — next ticket, needs LEF `VIA`.
- No costs, no preferred-direction legality, no maze search.
- No DRC/OBS pruning of nodes; the region is purely guide-defined.
- Not a reproduction of TritonRoute's `FlexGridGraph` — a substrate
  the later search walks, checked by census not by graph diff.
