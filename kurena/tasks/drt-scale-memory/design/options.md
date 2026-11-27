# options — candidate reductions and the pick rule

The spec leaves the fix to the driver "after seeing what dominates".
Each option below names the lever, the code surface, the determinism
argument, and the pick rule keyed to the `measurement.md` split rows.
Options are composable but land as separate commits — one logical
change per commit (AGENTS §4).

## O1 — flatten `NetTopo`'s maps-of-vecs to CSR + pack `RelaxedVia`

**Lever**: `net_topos` row. `members: BTreeMap<u32, Vec<u32>>` and
`cover: PinCover` (`BTreeMap<u32, Vec<RectDbu>>`) pay ~80 B/entry node
overhead over a few bytes of payload; `attach`/`access` pay a `Vec`
header (24 B) per pin.

- `members` → CSR keyed by node id: `member_start: Vec<u32>` (len
  `n_nodes + 1`), `member_pin: Vec<u32>`. `members.get(&v)` at
  `extend_once` L2162 becomes a range read. Same iteration order by
  construction → identical search.
- `cover` → compact `PinCover`: `cells: Vec<u32>` (sorted) + CSR
  (`rect_start`, `rects`). `pin_rects` produces it directly; the
  `HullMask::{point_allowed, seg_allowed, rect_allowed}` /
  `exempt_obs` signatures take `&PinCover` unchanged — only the repr
  moves. The same repr then serves the graph build's `cover`.
- `attach: Vec<Vec<u32>>` → `attach_start`/`attach_node` CSR;
  `access: Vec<Vec<AccessVia>>` → `access_start`/`access_via` CSR.
  `attach` is mutated during build (`push` of access `b`s) — build
  into a `Vec<Vec>` then freeze to CSR at the end of `NetTopo::new`.
- `RelaxedVia` 20 B → 16 B: drop `lower` (always `nodes[a].layer`).
  Bigger cut if `relax` dominates: it holds ≤ `RELAX_K=3` entries per
  node and is the likeliest single biggest `Vec` in the topo.

**Determinism**: pure representation change; `route_digest` and the
emitted DEF must not drift (they pin it — `tests.md`).
**Cost**: internal-only churn; `PinCover` is `pub` so the type change
is visible outside `drt` — check `tests/drt.rs` uses.
**Pick if**: `net_topos` is ≥ ~50 % of the per-net resident set or
`members`/`cover`/`relax` sub-rows are individually large. Cheap and
always non-negative → the default first move unless the split says
topos are already small.

## O2 — drop `NetGraph`s after the topo pass, rebuild per attempt

**Lever**: the `route_graphs` row — the only way to remove the
*whole* structure from the steady-state footprint.

- `NetGraph` is a pure function of `(db, tech, tracks, join boxes,
  HullMask)` — occupancy never enters its build, so a rebuild is
  bit-identical. (`NetTopo` is the opposite — it reads `occ`; it can
  never be rebuilt. See `overview.md` constraints.)
- Lift the `map_init` body of `build_route_graphs_ext` into
  `build_net_graph(db, tech, tracks, hull, net, boxes, opts, scratch)
  -> (NetGraph, NetCensusPartial)`. The bulk builder becomes a
  `par_iter` over it, so the census fields (`zero_coverage`,
  `region_escape`, `escapes`, `total_*`, `via_edges_per_pair`,
  `hull_masked`) still come out in `join.nets` order — unchanged.
- `route_nets*` keeps `join` + census + `topos`; `graphs.nets` is
  dropped after the topo pass. Each `route_one_net` callsite rebuilds
  `g` first: in the parallel arm inside the `map` (≤ threads graphs
  in flight), in the serial hard arm per net. `route_one_net`'s
  signature is unchanged.
- `apply_routes` takes `guided: &[NetId]` (from `join.nets`) instead
  of `&RouteGraphs` for its membership check.
- Iter-1 optimisation (optional): keep the freshly built graphs
  through iteration 1 and drop at first rip-up — avoids one rebuild
  per net at the price of holding both structures through iter 1.
  Only take this variant if the `post_topos` checkpoint — not the
  iter-1 window — is the peak.

**Determinism**: rebuild = identical pure function on identical
inputs; `test_net_graph_rebuild_identity` asserts `PartialEq` per
net (`tests.md`). CPU cost: one graph build per re-attempt
(iter 2+ nets only, or every attempt under the always-rebuild
variant) — bounded, and recorded with `route_ms` deltas.
**Pick if**: `route_graphs` is ≥ ~20–25 % of the resident peak —
below that the refactor isn't buying enough. This is the spec's
"share immutable structure / stream-and-drop" realised; note the
literal review alternative (share structure across nets) is rejected:
regions are per-net, nothing is shareable.

## O3 — chunk the `outcomes` collect

**Lever**: `outcomes` hi-water row — the iter-1 transient where every
net's `NetOutcome` (`NetRoute` + `used_edges` + residuals) is live at
`collect` end.

- `bad.chunks(CH)` (`CH ≈ 4 × nthreads`): `collect` per chunk, run the
  existing ordered commit pass per chunk, then the next chunk. The
  commit stream is chunk-wise identical to the whole-batch stream →
  byte-identical by construction. `hits` scratch and the `progress`
  sink move inside the chunk loop unchanged.
- Bounds in-flight outcomes to `CH` without touching `bad` order.

**Determinism**: identical commit stream — the progress sink already
fires per commit, so event order is preserved.
**Pick if**: the iter-1 `outcomes` row is material (> a few % of
peak). Trivial change; do it opportunistically even as a rider.

## O4 — pool the per-attempt `NetRun` scratch

**Lever**: allocation churn — `NetRun::new` allocates ~8 Vecs of
`n_nodes` scale per attempt, plus `pq`/`bfs`/`hits` per
`extend_once` round.

- A `Scratch` struct owns the Vecs; `NetRun` borrows it
  (`&'t mut Scratch`). Parallel arm: `map_init` per worker; serial
  arm: one scratch across the loop. `extend_once`'s `pq`/`bfs`/`hits`
  move into the same scratch (cleared per round, capacity retained).

**Determinism**: reuse only — contents are re-derived per attempt.
**Honesty note**: RSS benefit is allocator-dependent (freed Vecs may
stay in bins); measure, and claim only what the after-run shows.
**Pick if**: `route_ms` pressure is acceptable and the split shows
fragmentation/`rest` growth, or as a rider on O2 (rebuild already
makes transient graphs per-worker).

## O5 — slim `Occupancy` / `marks` (conditional)

**Lever**: `occupancy` + `marks` rows, **only if they dominate** —
unlikely on the code read (`ivs` intervals are already dense 24 B
records on dense `Vec<Line>` lines) but the split decides.

- `Interval` 24 B: `perp_lo`/`perp_hi` could share a packed
  representation — only if `occ` is the dominant row.
- `marks`: instead of logging every `rasterize`, re-derive the mark
  list at rip-up by re-running `commit_route`'s rasterization
  (deterministic → identical intervals) then `unmark`. Trades Σ marks
  bytes (whole-run retention) for a re-rasterize per rip-up.

**Pick if**: `occupancy` + `marks` together exceed the per-net pair —
the expected case is they don't, and this stays deferred with the
recorded row.

## O6 — post-route drops (tail trim, not the peak)

Drop `graphs.nets` (if O2 didn't already), `topos`, `marks` before
the emit leg; `apply_routes`'s `built: Vec<NetEmit>` is itself an
all-nets transient — chunk-apply if the emit leg ever peaks higher
than the route leg.

**Pick if**: the measured peak sits at `post_emit`, not `post_topos`
— expected false on this code, stated for completeness.

## Rejected options (with reasons)

- **Lazy/streamed `NetTopo`**: *illegal*, not merely costly —
  `NetTopo::new` reads live occupancy (`project_attach` L922,
  `access_vias` L1684). Any rebuild past `premark_obs` sees committed
  marks → different attach sets → different output. Byte-identity
  forbids it outright.
- **Sort `bad` to bound big graphs**: `bad` order *is* the commit
  order — any reordering changes the emitted DEF. Pool width or
  in-order chunking are the only levers.
- **Share immutable graph structure across nets**: regions are the
  per-net guide union — disjoint by construction; nothing exists to
  share.
- **Cap thread count on the route**: bounds per-worker scratch (O4's
  terms) but buys nothing structural; `RAYON_NUM_THREADS` already
  gives the user this knob. Documented, not coded.

## Pick rule

1. `rest` (db/tech/misc) dominates → stop after instrumentation;
   record, don't touch router structures.
2. `net_topos` dominates → O1 first; add O2 if `route_graphs` ≥ 20–25 %.
3. `route_graphs` dominates → O2 (with the iter-1 variant decided by
   where `post_topos` vs iter-1 rss sits).
4. `outcomes`/`occ`/`marks` material → O3 / O5 riders.
5. Whatever ships must show a lower measured `/usr/bin/time -l` peak
   on **both** designs — if the picked change doesn't move the
   picorv32 number, the wrong row was cut; re-open the split.
