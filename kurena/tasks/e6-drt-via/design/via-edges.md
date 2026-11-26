# e6-drt-via — Via edges in the route graph

## 1. The rule

A via edge exists between a node on layer `l` and a node on `l+1`
when both sit at the same `(x, y)`. The spec's condition — `(x,y)` is
a track crossing on **both** layers (`x ∈ tracks_x(l) ∧ y ∈
tracks_y(l)` and same for `l+1`) — is what node existence already
means on each side (a node is an x-track ∩ y-track pair on its own
layer, guide-covered). So the implementable form is:

> For each adjacent pair `(l, l+1)` and each lower-layer node
> `(l, ix, iy)`: `x = tracks[l].x[ix]`, `y = tracks[l].y[iy]`;
> `jx = tracks[l+1].x.binary_search(x)`, `jy =
> tracks[l+1].y.binary_search(y)`; when both hit and `(jx, jy)` is in
> region `l+1`, emit `ViaEdge { lower, upper }`.

The "track crossing on both axes" part matters because a via's
*destination* must be a node — tracks on non-preferred axes are still
grid lines the crossing (and the via) can sit on, which is why both
`tracks_x` and `tracks_y` of both layers participate. No candidate
needs choosing for the edge to exist (that is the policy's job, per
pair — `via-model.md` §3); edge existence is purely geometric.
Multi-hop transitions compose at the node: `(l,l+1)` and `(l+1,l+2)`
edges meet at the `l+1` node — no stacked-via primitive (non-goal).

- `ViaEdge { lower, upper }` are node ids; `lower < upper` always
  (layer-ascending node emission).
- ≤1 via edge per `(x,y)` per pair — pairs are deduped, track lists
  strictly increasing.
- Emission order: pair ascending → lower node order; the determinism
  gate double-runs.

## 2. Measured shared-crossing grid (the ground truth)

Shared `(x,y)` positions per adjacent pair, **before** guide
coverage, computed from the committed `TRACKS` in each placed DEF:

| pair | gcd shared crossings | picorv32 shared crossings |
|---|---|---|
| li1–met1 | 3,624 | 89,776 |
| met1–met2 | 156 | 3,906 |
| **met2–met3** | **0** | **0** |
| met3–met4 | 42 | 961 |
| met4–met5 | 1 | 36 |

met2 coords are `230 + 460k ≡ 10 (mod 20)`; met3 `340 + 680k ≡ 0
(mod 20)` — on both axes, on both designs, the grids share **no
coordinate at all**. met2–met3 therefore has **zero via edges by
construction** — not a bug to fix in this ticket: a **named tech gap
in the edge domain**, pinned `== 0` in the census test so any future
model change that produces edges there must update the pin and the
docs.

## 3. The divergence vs. the reference — measured

`gcd.routed.def` (the committed baseline routed output; same `TRACKS`
input) shows where TritonRoute actually drops vias:

| via name | instances | at strict shared crossing | at preferred anchor¹ | off-anchor |
|---|---|---|---|---|
| L1M1_PR_MR | 884 | 50 | 769 | 115 |
| M1M2_PR | 881 | 3 | 881 | 0 |
| M1M2_PR_MR | 2 | 0 | 2 | 0 |
| M2M3_PR | 28 | 0 | 28 | 0 |
| M3M4_PR | 2 | 0 | 2 | 0 |

¹ `x ∈` the vertical layer's `tracks_x` **and** `y ∈` the horizontal
layer's `tracks_y` — the preferred-track anchor of *each side*: e.g.
a met2–met3 via at `(met2_x, met3_y)`, where the met2 wire's x-track
and the met3 wire's y-track cross.

Every sampled `M2M3_PR` sits at `x ∈ met2_x ∧ y ∈ met3_y` — a
position that is a node on **neither** layer in our model (met3's
y-track is not a met2 y-track; met2's x-track is not a met3 x-track).
picorv32's routed DEF uses `M2M3_PR` 1,784 times — this is the
reference's second-busiest cut layer, not an edge case.

**Conclusion recorded for the maze ticket**: the reference's via
anchors live at preferred-track crossings across two *different*
layers; our same-layer-crossing nodes can host only the minority of
placements that happen to be shared crossings. Closing the gap needs
a node-model change — e.g. per-layer nodes at `(own preferred track,
union of perpendicular tracks)` the way TritonRoute's grid graphs
work — which revises `e6-drt-graph` semantics and is explicitly out
of this ticket's scope (the same class of deferred model work as the
already-recorded io-port attachment fallback at `docs/e6-drt.md` §5).
The 115 off-anchor `L1M1_PR_MR` placements are a second, smaller
relaxation (pin-access off-grid drops) — recorded, unmodeled.

## 4. Edge-domain findings

- **met2–met3: 0 via edges on both designs** — named, pinned,
  explained by grid arithmetic (`via-edges.md` §2) and the reference
  measurement (§3). Printed per pair in the census.
- met4–met5: ≤1 (gcd) / ≤36 (picorv32) possible positions before
  coverage — the reference uses zero `M4M5` instances anyway; our
  census prints whatever the guide regions admit (≥0, pinned after
  first run).
- The census asserts `gaps.is_empty()` and prints per-pair edge
  counts. Edge emission is geometric, so a hypothetical gap pair
  could still show positions — that is the data explaining the gap,
  not a contradiction; the maze router reads the `ViaGap`, not the
  raw edge list, for legality.

## Files

- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/drt/via.rs`
