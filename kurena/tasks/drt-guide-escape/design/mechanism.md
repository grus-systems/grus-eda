# Mechanism — `escape_extend`

## §1 Position in `build_route_graphs_ext`

Current order inside the per-net loop (`graph.rs` ~L1220–1370):

```
net_region            → region[layer] = covered pairs of guide boxes
hull mask retain      → point_allowed filter on masked layers
transit_extend        → host-hull interior + corridor on first unmasked
escape_extend         → strict-escape set + halo + corridor pairs
zero_coverage         → coverage counts (operates on `boxes`, not region)
emit nodes/edges/vias → pairs → RouteNode/RouteEdge/layer_via_edges
pin attach loop       → pin_nodes; pin ∈ escape set → region_escape
```

Why between `transit_extend` and emission:

- Corridor anchors must be *post-mask* pairs — anchoring on a pair the
  retain pass will drop yields a dead corridor.
- Transit pairs (host-hull met2 interior + corridor) are legitimate
  anchors and dedupe halo overlap for free — an escaped instance pin's
  met2 halo inside its host hull is usually already region-covered
  (`region.contains` skip), so the met2 span is often 0 and no corridor
  is needed at all; the missing links are the li1/met1 descent nodes.
- `zero_coverage` and `join_steiner`/`stub_medians` operate on `boxes`
  — escape pairs must not join that accounting.

## §2 The strict-escape set (pre-halo `region_escape` semantics)

`escape_extend` first computes, for every member pin of the net,
whether the *current* (guide + transit) region offers a strict node —
the same covered-range rule the attach loop applies: a pair `(x, y)`
on a pin-shape layer with the point inside the rect. The shared kernel
is `strict_hits(pairs, lt, rect)`, used by both the pre-pass and the
attach loop, so the two cannot drift.

The resulting set is pushed to `RouteGraphs.region_escape`
**regardless of post-halo attach** — the flag keeps meaning "pin
metal outside the guide region", the pinned 17/592 census and
`test_pin_attachment`'s name set do not move, and `fail` lines keep
their ` region_escape` provenance. With `cfg.escape == None` the set
is computed but no pairs are added and `escapes` stays empty —
bit-identical to the pre-escape build.

## §3 Halo construction

For each pin `p` in the escape set (iterated in `db.net_pins`
order — deterministic):

- **Layers**: `lo = min pin-shape layer`, `hi = max(pin-shape
  layers ∪ {first_unmasked})`, so `layers = lo..=hi`. An li1 pin gets
  {li1, met1, met2} — the pin layer plus the whole via-descent stack
  to the corridor-friendly unmasked surface; a met3 io port gets
  {met3}; a met1 fixture pin gets {met1, met2}.
- **Rect**: `pin_bbox` = bbox of all the pin's shape rects. For each
  layer, covered pair set of `pin_bbox` inflated `halo_lines` index
  steps on each axis (index steps in `tracks_x`/`tracks_y`, not DBU —
  grid-native like `transit_extend`'s nearest-pair search).
- **Filter**: each candidate pair not already in `region[l]` is kept
  iff `hull.point_allowed(l, x, y)` (masked layers) — own-cover keeps
  pin-rect nodes on strict layers, host-open keeps hull-interior
  met1, foreign-hull nodes drop. Unmasked layers pass through.
  `hull_masked` census is **not** charged for halo drops — the escape
  record carries `pairs_added`, keeping the existing pin.

If the union of kept halo pairs over all escape layers is empty →
`EscapeKind::HaloEmpty` (e.g. a sub-pitch pin rect holding no track —
the `u2/C` class — or a fully foreign-hull-sealed rect). No pairs
added, `pin_nodes` stays empty.

## §4 Span probe and corridor

### The probe — `escape_span(halo, halo_bbox, region, cap)`

Distance on an unobstructed 4-connected index grid is Manhattan
distance, so the probe is an enumeration, not a BFS:

```
best ← cap + 1; argmin ← none
for v ∈ region (sorted order), v ∉ halo:
    if dist(v, halo_bbox) ≥ best: continue      // O(1) prune
    for h ∈ halo (sorted order):
        if manhattan(h, v) < best: best, argmin ← manhattan, (h, v)
return argmin → (best, h, v)
```

- `cap` bounds both the reported span and the work: a `v` whose
  halo-bbox distance already exceeds `cap` can never produce a
  span ≤ `cap`, so it is skipped in O(1). Corridors only need
  `cap = corridor_lines` — the probe never evaluates the full
  `nx × ny` grid (the retired BFS's dominant cost was its
  `vec![u32::MAX; nx*ny]` allocation per escaped pin per layer,
  ~measurable at picorv32's 592 escapes).
- Determinism: `v` iterates in region order (sorted), `h` in
  `BTreeSet` order, strictly `<` improvement — the earliest
  `(v, h)` achieving each new minimum wins. Tie-breaks may pick
  different `(h, v)` endpoints than the retired BFS for equal
  spans — corridor *spans* are identical, but a different equal-cost
  L-path can shift painted pairs and the graph census; any drift is
  re-measured, never hand-derived (`steps.md`).
- `v ∉ halo`: kept halo pairs are never in `region` by construction,
  but a *previous pin's* escape may have merged halo-overlapping
  pairs — the guard keeps them from becoming span-0 self-anchors.

Per layer `l`, `cands` collects `EscapeCand{layer, span, h, v}` from
`escape_span(halo_l, halo_bbox_l, region[l], bound)`. A layer whose
region has no non-halo pair yields nothing.

### Merge or corridor

- Any `span ≤ 1` — a kept halo pair is index-adjacent to a region
  pair and emission induces the edge: merge every halo layer into
  `region`, record `Halo`, `corridor_span = 0`, `pairs_added =
  halo_total`.
- Otherwise sort `cands` by `(span asc, layer desc)` — the unmasked
  surface wins ties — and try each in order: build `l_path(h, v)`
  (horizontal leg then vertical leg, `transit_extend` shape,
  interior pairs only) and land the first whose every pair passes
  `point_allowed`. Landed corridor pairs + that layer's halo merge
  into `region[l]`; other halo layers merge as before. Record
  `Halo`, `corridor_span = landed span`, `pairs_added` = distinct
  inserted pairs (§5).
- `landed == None && cands` non-empty → `CorridorMasked`: every
  within-bound corridor died at the mask. The legal halo pairs still
  merge (a graph island the pin may attach to, or a route-time
  access-arc target); `corridor_span` = minimum attempted span
  (`cands[0]` after the sort).
- `cands` empty → the refusal probe: re-run `escape_span` per
  *halo* layer (the same layers the candidate probe visited — a
  layer with no halo pairs has nothing to span from) with `cap =
  u32::MAX` (refusal path only — zero cost on designs with no
  refused escapes; gcd/picorv32 currently record none). If every
  halo layer still yields nothing → `NoAnchor` (`corridor_span =
  0`); else `CorridorBound` with `corridor_span` = the minimum
  exact span over layers. The refusal probe is O(Σ |region_l|)
  per refused pin — bounded by region size with the same bbox
  prune, never by grid size.

Corridor pairs merge into `region[l]` like transit pairs. Halo pairs
on other layers stay — they share crossings and get via edges from
`layer_via_edges` plus topo-side `relaxed_vias`, and `access_vias`'s
`lines_h`/`lines_v` maps see them for `b`-landings — so the via
descent li1→met1→met2 needs no maze-side code.

Constants (recorded with measured deltas in `docs/results.md`):

```rust
pub struct EscapeCfg { pub halo_lines: i32, pub corridor_lines: i32 }
// EscapeCfg::DEFAULT = { halo_lines: 2, corridor_lines: 8 }
```

- `halo_lines = 2`: ≥ `B_EXT` so `access_vias` `b`-landings exist on
  the upper stack, and gives one slack line beyond `project_attach`'s
  ±1 reach for stub paint.
- `corridor_lines = 8`: ≈1–2 GCell pitches on the sky130 grids we run
  (met2 pitch 460 DBU ⇒ 3.7 µm) — the rescue window for "guide box
  stops just short", not an arbitrary rewrite. Bound is total
  Manhattan index steps.

## §5 Records

```rust
pub enum EscapeKind { Halo, CorridorBound, CorridorMasked, NoAnchor, HaloEmpty }

pub struct EscapeRecord {
    pub net: NetId,
    pub pin: PinId,
    pub kind: EscapeKind,
    pub pairs_added: u32,      // distinct pairs inserted into `region`
    pub corridor_span: u32,
}
```

Kind semantics:

| kind | meaning | `pairs_added` | `corridor_span` |
|---|---|---|---|
| `Halo` | halo merged — directly (span ≤ 1) or via a landed L-corridor | distinct inserted pairs | 0 if merged direct, else landed span |
| `CorridorBound` | ≥1 anchor exists on the escape layers; the nearest lay beyond `corridor_lines` | 0 | the measured gap — min over layers, *exact* |
| `CorridorMasked` | every within-bound corridor died at the mask; the legal halo stays merged | distinct inserted (halo only) | min within-bound attempted span |
| `NoAnchor` | no non-halo `region` pair on any layer the halo covers — nothing to aim at | 0 | 0 |
| `HaloEmpty` | no legal halo pair on any escape layer | 0 | 0 |

`pairs_added` contract: the count of *distinct* pairs inserted into
`region` — on the corridor-landed arm that is `|path ∪ halo[landed]|
+ Σ other-layer |halo|`, **not** `halo_total + extra.len()` (the
corridor layer's halo appears in both terms — the review's
double-count). The fixture's landing arm pins the exact value
(`fixture.md`).

`NoAnchor` is deliberately a kind, not a sentinel span: "nothing to
aim at" (the guide covers only layers outside the pin's stack, or a
fully empty region) is a different diagnosis from "target exists
beyond the bound", and a `u32::MAX` span rendered
`corridor_bound(span=4294967295)` on the fail line — nonsense text
the revision removes. No sentinel remains anywhere in the record.

`RouteGraphs.escapes: Vec<EscapeRecord>` — flat per-design census
next to `region_escape`/`transit_pairs`/`hull_masked`, deterministic
(net/`net_pins` iteration order), equal-counted in
`test_graph_determinism`.

`UnroutedNet`/`RouteDb` are **not** extended: `region_escape` already
joins graphs-side at print, and `escape=` joins the same way
(`reporting.md`). `route_digest` serializes `RouteDb` only —
structurally untouched.

## §6 `GraphOpts` — the escape switch (landed)

```rust
pub struct GraphOpts {
    pub spare_above: u8,
    pub escape: Option<EscapeCfg>,   // None → pre-escape behavior
}
```

- `build_route_graphs` ⇒ `GraphOpts { spare_above: 0, escape:
  Some(EscapeCfg::DEFAULT) }`.
- Maze legs ⇒ `GraphOpts { spare_above: 1, escape: Some(DEFAULT) }`.
- The fixture's red arm passes `escape: None`; the bound arm passes
  `Some(EscapeCfg { halo_lines: 0, corridor_lines: 0 })`.

## §7 Occupancy invariance (acceptance #2)

Halo/corridor pairs are indistinguishable `region` entries: emission
gives them ordinary nodes; `layer_edges`/`layer_via_edges` treat them
identically; every committed edge passes `edge_cost` → `scan_wire` →
`hit_verdict` (`Obs`/`ObsPin`/`Pin`/`Net` verdicts unchanged);
extraction merges them into `WireSeg`/`ViaPlace`; `commit_route`
rasterizes them as `Net(own)`; `overlap_census`/`pin_overlap_census`
see them. There is no code path by which escaped metal can skip a
check — the mechanism adds *nodes*, never exemptions. `fixture.md`
arm F turns that structural argument into a measured
enabled-vs-disabled contrast.

## §8 What the mechanism deliberately does not do

- **No route-time retry.** `stub_blocked`/`Unattachable`-without-flag
  pins keep `stub_outside_cover` naming (documented residual).
- **No via-only escape** into layers the halo rule wouldn't reach —
  the contiguous-layer rule already spans pin layer → first
  unmasked; higher layers are reachable through ordinary vias.
- **No `region_escape` semantic change** — pre-halo set, always.
- **No `FailKind` guarantees.** An escape pin's final classification
  follows the route-time attach ladder, not the escape record: a
  refused escape leaves `pin_nodes` empty, which yields
  `Unattachable` only when *every* route-time rung (projection,
  access arcs, relaxed vias) is also empty — a pin whose transit
  island still lands an access arc fails `NoPath` (the fixture's
  escape-off arm is exactly this case).
