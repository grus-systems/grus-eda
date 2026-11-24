# e6-drt-emit — Post-route min-area patch

The fix for the measured picorv32 class: `li.6`×3 and `met3.6`×39 —
every finding a **connected component under the layer's `AREA`
bound** made of one via plus a short stub (the shape `relaxed_vias`
implied stubs and projected-pin access stubs produce). The checker's
bar is `check_area_report`: per routing layer with `AREA` declared,
merged plane polygons (`drc::union::components`, closed-touch merge,
net-agnostic) with ≥1 drawn member must hold `union_area ≥ area_dbu2`,
strict `<`.

The patch replicates that component model router-side and extends
each sub-bound component along the layer's **preferred axis** inside
the net's guide region until the bound is met — TritonRoute's
min-area patch analog.

## Placement in `route_nets` (decision 7)

```
while it < cap && !bad.is_empty() { …iterations… }
+ for (i, g) in graphs.nets.iter().enumerate() {
+     let Some(route) = &mut routes[i] else { continue };
+     area_patches += patch_net_min_area(db, tech, tracks, g, vias,
+                                      &mut occ, route, &mut residuals);
+ }
let nets: Vec<NetRoute> = routes.into_iter().flatten().collect();
RouteDb { collisions: overlap_census(&nets, …), … }
```

The pass runs *before* `routes` flattens so each `NetRoute` pairs
with its `NetGraph` by index — no net-id map needed.

`occ` at loop exit holds exactly the committed set — premarks plus
`Net(own)` marks of every `routes[i] = Some` net (re-queued nets were
unmarked before re-routing; hard-iteration failures never committed).
Running the pass here is strictly better than inside
`route_one_net`:

- patches see the **final** foreign picture — a soft-iteration patch
  could be needlessly blocked by a net that is ripped up moments
  later;
- each net is patched **once**, not once per rip-up attempt;
- accepted patch metal rasterizes straight into `occ` as
  `Net(own)`, so later nets' patches respect earlier patches;
- `overlap_census` rebuilds its own occupancy from `route.wires`,
  so patch wires join the collision census for free — the expected
  count stays 0 (patch steps are occupancy-strict; a nonzero census
  after this pass is a patch bug, not looseness).

The pass mutates only `route.wires` — `NetRoute` keeps its fields,
`commit_route`/rip-up bookkeeping is untouched (no marks log needed
post-loop), and emit reads patch wires like any other segment.

## Component model (mirrors the checker, conservatively)

For one committed `NetRoute` on layer `l`
(`tech.routing_layers[l].area_dbu2 > 0` only — layers without a
declared `AREA` skip, same convention as `check_area_report`):

- **Member rects** — every shape of *this net* on `l`:
  1. `route.wires` on `l` → `seg ± w/2` (the same expansion
     `commit_route` rasterizes and `seg_rect` emits);
  2. via metal: for each `route.vias` `vp`, the pair default's
     `cand.rects` filtered `DrcLayer::Routing(l)`, shifted by
     `(vp.x, vp.y)` — same source `commit_route` uses;
  3. member-pin metal: `pin_shapes(db, tech, pin)` for every pin in
     `g.pin_nodes`, filtered to `l` — instance-pin macro shapes and
     io-port rects alike, matching the checker's plane merge.
- **Merge**: feed the rects into `LayerShapes::from_rects` (src
  `SignalWire` throughout — `union::is_member` covers it) and call
  `union::components` — the *exact* closed-touch merge the checker
  uses, not a re-implementation.
- **Area**: `union::union_area(&member_rects)` per component —
  the exact integer the checker compares against `bound`.

Conservative delta vs the checker, recorded: the checker's merge is
net-agnostic — foreign metal touching ours inflates its component.
Our model holds only the net's own shapes (plus member pins), so
`area_ours ≤ area_checker` component-wise: a component that passes
our bound passes the checker's; a component we patch "unnecessarily"
(foreign metal would have carried it) costs extra metal only — never
a missed violation.

## Extension walk

For each sub-bound component (processed in deterministic order —
rect-order component ids, attempted one at a time):

1. **Candidate starts** — every `g.nodes` node on `l` whose absolute
   `(x, y)` lies inside (closed containment) any member rect, in
   node order. Every router-produced component holds ≥1 node:
   in-plane wires end at nodes, relaxed-via stubs hang on `rv.a`,
   projected stubs end at the attach node, and a strict-via island
   contains its own node point. A component with no interior node
   is un-patchable by construction → residual (defensive arm).
2. **Step domain** — per layer, per perpendicular line, the sorted
   node coordinates (built once per patch call:
   `BTreeMap<iy, BTreeSet<ix>>` for axis-X walks, mirrored for
   axis-Y). A step from node `n` to the adjacent covered node on the
   same line is exactly one existing in-plane `g.edges` span —
   inside the guide region to the router's own standard.
3. **Per candidate node `n0`**, walk `+pref` then `−pref`:
   - each step proposes span `[prev, next]` on the line;
     `occ.scan_wire(axis, l, line, prev, next)` must return only
     `Net(own)`/`Pin(own)` hits — any `Obs` (including host-cell;
     the carve is not inherited — patch metal is optional) or
     foreign `Net`/`Pin` hit stops that direction;
   - after each accepted step, `union_area(members ∪ patch_rects)`
     — exact, so overlap with existing metal or a second component
     is accounted; `≥ bound` → commit and stop;
   - a span may accumulate steps in both directions from `n0`
     (one `WireSeg` covering `[lo, hi]` through `n0` — a single
     emitted record, not two).
4. **Commit**: append the `WireSeg` to `route.wires`, rasterize its
   `seg ± w/2` rect into `occ` as `Net(own)` (no `marks` log —
   post-loop has no rip-up), then **re-derive components** — an
   accepted patch can merge two sub-bound components into one that
   already clears, so component identity is recomputed after every
   commit.
5. **Exhausted** (region edge or occupancy block on every
   candidate): the component is skipped this sweep — a later patch
   on a neighbor can still lift it. Residuals are **not** recorded
   per attempt.

Loop bound and residual finalization: sweep until a full pass
accepts no patch or every component clears (each patch strictly
adds metal → structural termination). When the loop ends, derive
components one final time and emit one `AreaResidual { net, layer,
region: comp bbox, area_dbu2, bound_dbu2 }` per still-sub-bound
component — residuals describe the *final* geometry, so a component
a later patch rescued is never stale-recorded.

After the pass, `route.wires.sort_unstable()` restores the canonical
`WireSeg` order emit and `route_digest` rely on.

Signature (private to `maze.rs`):

```rust
/// Patch one committed net's sub-bound components toward its
/// layers' `AREA` bounds; returns components fixed.
fn patch_net_min_area(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    g: &NetGraph,
    vias: &ViaTable,
    occ: &mut Occupancy,              // final committed state
    route: &mut NetRoute,
    residuals: &mut Vec<AreaResidual>,
) -> usize
```

`NetTopo` is not consulted — the host-cell OBS carve stays a router
convention; the patch is occupancy-strict (decision 8).

## What the patch guarantees — and what it does not

- **Guaranteed**: every patch wire is on-track, preferred-axis,
  in-region (graph-edge spans), occupancy-clear of foreign metal and
  OBS at spacing distance, and joins an existing own-net component —
  it cannot create a new component, a spacing finding, or a width
  finding (it is a full-width on-track wire).
- **Not claimed**: identical geometry to TritonRoute's patch (its
  algorithm is its own — ours is the guide-region/occupancy analog);
  perpendicular or non-preferred extensions (a component blocked on
  the preferred axis in every direction is a residual, not a
  fallback); antenna/timing awareness.

## Residual → gate path

`RouteDb.area_residuals` names un-patchable components up front; the
self-DRC gate is the final arbiter. If the picorv32 re-run still
shows `*.6` findings, each must map to a recorded `AreaResidual` —
a finding *without* a residual means the component model diverged
from the checker's (a bug to fix, not a tolerance).

## Determinism

Every choice point is sorted: component order (rect insertion →
`union::components` id order), candidate nodes (g order), direction
(`+` before `−`), steps (adjacent covered node). Two runs produce
identical `RouteDb`s — `PartialEq` covers `area_patches`/
`area_residuals`; `route_digest` already covers `wires`, so patched
nets shift the digest by construction (that is the point — the
pre-patch picorv32 digest cannot be reproduced and the maze entry's
pinned digests are not back-edited).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs` (re-export `AreaResidual`)
