# Mechanism — `escape_extend`

## §1 Position in `build_route_graphs_ext`

Current order inside the per-net loop (`graph.rs` ~L780–940):

```
net_region            → region[layer] = covered pairs of guide boxes
hull mask retain      → point_allowed filter on masked layers
transit_extend        → host-hull interior + corridor on first unmasked
zero_coverage         → coverage counts (operates on `boxes`, not region)
emit nodes/edges/vias → pairs → RouteNode/RouteEdge/layer_via_edges
pin attach loop       → pin_nodes; attached.is_empty() → region_escape
```

New order:

```
net_region
hull mask retain
transit_extend
escape_extend         ← NEW (gated on cfg.escape; always computes the
                        strict-escape set, mutates region only when on)
zero_coverage
emit nodes/edges/vias — halo/corridor pairs flow through identically
pin attach loop       — region_escape pushed iff pin ∈ escape set
```

Why between `transit_extend` and emission:

- Corridor anchors must be *post-mask* pairs — anchoring on a pair the
  retain pass will drop yields a dead corridor.
- Transit pairs (host-hull met2 interior + corridor) are legitimate
  anchors and dedupe halo overlap for free — an escaped instance pin's
  met2 halo inside its host hull is usually already region-covered
  (`pairs.contains` skip), so the met2 span is often 0 and no corridor
  is needed at all; the missing links are the li1/met1 descent nodes.
- `zero_coverage` and `join_steiner`/`stub_medians` operate on `boxes`
  — escape pairs must not join that accounting.

## §2 The strict-escape set (pre-halo `region_escape` semantics)

`escape_extend` first computes, for every member pin of the net,
whether the *current* (guide + transit) region offers a strict node —
the same covered-range rule the attach loop applies (`graph.rs`
~L975–1010): a pair `(x, y)` on a pin-shape layer with the point
inside the rect and `x` on a covered line within `[y0, y1]` or `y`
within `[x0, x1]`. Factor that test into a shared helper (e.g.
`strict_attached(pairs, lt, rect) -> bool`) used by both the pre-pass
and the attach loop, so the two cannot drift.

The resulting set is pushed to `RouteGraphs.region_escape`
**regardless of post-halo attach** — the flag keeps meaning "pin
metal outside the guide region", the pinned 17/592 census and
`test_pin_attachment`'s name set do not move, and `fail` lines keep
their ` region_escape` provenance. With `cfg.escape == None` the set
is computed but no pairs are added and `escapes` stays empty —
bit-identical to today's behavior (the pin still fails
`Unattachable`).

## §3 Halo construction

For each pin `p` in the escape set (iterated in `db.net_pins`
order — deterministic):

- **Layers**: `lo = min pin-shape layer`, `hi = max(pin-shape
  layers ∪ {first_unmasked})`, so `layers = lo..=hi`. An li1 pin gets
  {li1, met1, met2} — the pin layer plus the whole via-descent stack
  to the corridor-friendly unmasked surface; a met3 io port gets
  {met3}; a met1 fixture pin gets {met1, met2}.
- **Rect**: `pin_bbox` = bbox of all the pin's shape rects. For each
  layer, covered pair set of `pin_bbox` inflated `ESCAPE_HALO` index
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
added, `pin_nodes` stays empty → `Unattachable` + `region_escape`.

## §4 Corridor

Let `H_l` be the kept halo pairs on layer `l`. Per layer, the span

```
span_l = min over h ∈ H_l, v ∈ region[l] \ H_l of |ix_h − ix_v| + |iy_h − iy_v|
```

(pairs already in region are excluded; a layer with no pre-existing
pairs has `span_l = ∞`). If any `span_l ≤ 1` the halo already merges
(adjacent pairs induce an edge at emission) — no corridor.

Otherwise pick the argmin-span layer — iterate layers **descending**
with strict `<` so the unmasked surface wins ties — and add the L-path
(horizontal leg then vertical leg, `transit_extend` shape) from the
nearest halo pair to the nearest region pair, subject to:

- `span ≤ ESCAPE_CORRIDOR` (total index steps, both legs) — else
  `CorridorBound`, `corridor_span` records the measured span;
- every corridor pair passes `point_allowed` on that layer — a
  dropped pair breaks the corridor; try the next argmin layer; if
  all within-bound corridors die at the mask → `CorridorMasked`.

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
pub enum EscapeKind { Halo, CorridorBound, CorridorMasked, HaloEmpty }

pub struct EscapeRecord {
    pub net: NetId,
    pub pin: PinId,
    pub kind: EscapeKind,
    pub pairs_added: u32,      // halo + corridor pairs that entered region
    pub corridor_span: u32,    // 0 when the halo merged without a corridor
}
```

`RouteGraphs.escapes: Vec<EscapeRecord>` — flat per-design census next
to `region_escape`/`transit_pairs`/`hull_masked`, deterministic
(net/pin iteration order), equal-counted in `test_graph_determinism`.

`UnroutedNet`/`RouteDb` are **not** extended: `region_escape` already
joins graphs-side at print (main.rs ~L2188), and `escape=` joins the
same way (`reporting.md`). `route_digest` serializes `RouteDb` only —
structurally untouched (route drift may still move its value; that's
a results entry, not a schema change).

## §6 `GraphOpts` — the escape switch

`build_route_graphs_ext` gains an opts parameter replacing the bare
`spare_above`:

```rust
pub struct GraphOpts {
    pub spare_above: u8,
    pub escape: Option<EscapeCfg>,   // None → today's behavior
}
```

- `build_route_graphs(db, tech, tracks, join)` ⇒
  `GraphOpts { spare_above: 0, escape: Some(EscapeCfg::DEFAULT) }`.
- Maze legs currently passing `+1` become
  `GraphOpts { spare_above: 1, escape: Some(EscapeCfg::DEFAULT) }`.
- The fixture's red arm passes `escape: None`; the bound arm passes
  `Some(EscapeCfg { corridor_lines: 1, .. })`.

Call sites: `load_routed_inputs`/test helpers + `main.rs`'s emit leg —
a handful of lines; no `MazeConfig` involvement (graph build already
predates it).

## §7 Occupancy invariance (acceptance #2)

Halo/corridor pairs are indistinguishable `region` entries: emission
gives them ordinary nodes; `layer_edges`/`layer_via_edges` treat them
identically; every committed edge passes `edge_cost` → `scan_wire` →
`hit_verdict` (`Obs`/`ObsPin`/`Pin`/`Net` verdicts unchanged);
extraction merges them into `WireSeg`/`ViaPlace`; `commit_route`
rasterizes them as `Net(own)`; `overlap_census`/`pin_overlap_census`
see them. There is no code path by which escaped metal can skip a
check — the mechanism adds *nodes*, never exemptions. `fixture.md`
arm C turns that structural argument into a test.

## §8 What the mechanism deliberately does not do

- **No route-time retry.** `stub_blocked`/`Unattachable`-without-flag
  pins keep `stub_outside_cover` naming (documented residual).
- **No via-only escape** into layers the halo rule wouldn't reach —
  the contiguous-layer rule already spans pin layer → first
  unmasked; higher layers are reachable through ordinary vias.
- **No `region_escape` semantic change** — pre-halo set, always.
