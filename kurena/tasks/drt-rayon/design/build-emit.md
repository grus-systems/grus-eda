# build-emit — graph construction and DEF emission

Both phases are per-net maps over read-only inputs whose only shared
state is output accumulation. The pattern in both: compute per-net
partials in parallel (order-preserved `collect`), fold serially in
the baseline order. Byte-identity to serial is then a construction
argument, not a measurement — the tests (`tests.md`) pin it anyway.

## 1. `build_route_graphs_ext` (graph.rs L1243–1432)

The loop body is per-net except:

| Shared item | Parallel disposition |
|---|---|
| `hull_scratch: Vec<u32>` | `map_init(Vec::new, …)` — one scratch per worker, passed into `escape_extend`/the `point_allowed` probes as today |
| `out.nets` | indexed `collect` of per-net `NetGraph` — `join.nets` order preserved |
| `out.zero_coverage`, `out.region_escape`, `out.escapes` | per-net `Vec`s returned inside the partial; serial `extend` in `join.nets` order (each net's records stay in their emission order — `box_layers` order, `net_pins` order, escape-attempt order) |
| `out.total_nodes/edges/via_edges`, `out.via_edges_per_pair`, `out.hull_masked` | per-net partial counts, serial `+=` fold |

Per-net partial struct (private):

```rust
struct NetBuild {
    graph: NetGraph,
    zero_coverage: Vec<ZeroCoverage>,
    region_escape: Vec<RegionEscape>,
    escapes: Vec<EscapeRecord>,
    via_edges_per_pair: Vec<usize>, // len = pairs; zeros where untouched
    hull_masked: Vec<usize>,        // len = layers
}
```

`escape_extend` keeps its signature (`escapes: &mut Vec<EscapeRecord>`)
— it appends into the per-net partial's vec instead of `out.escapes`;
no other change inside it. `transit_extend`, `layer_edges`,
`layer_via_edges`, `strict_hits`, `pin_rects`, `HullMask` queries are
already read-only over shared inputs.

`HullMask::point_allowed` wants `&mut Vec<u32>` scratch — it goes in
the `map_init` state (one `Vec` reused per worker per net, matching
today's reuse-per-net semantics; the buffer is `clear`ed by
`hull_cells_at` on entry).

**Byte-identity argument**: the per-net computation is untouched and
deterministic (BTree-ordered `region` iteration, sorted node/pair
emission); the fold replays the same appends in the same order the
serial loop performed them. `RouteGraphs` equality (`PartialEq`
derived on every field type) at `RAYON_NUM_THREADS=1` vs `8` is the
pinned check; the serial-baseline digest is recorded before the
change and re-verified at 1 thread after (`measurement.md` §2).

## 2. `apply_routes` (emit.rs L81–138)

Per net in `route_db.nets` (its own `NetGeom` computation):
`seg_rect` expansion, `place_via` resolution (fallible — `Result`),
`vias_per_cut` and `wire_length_dbu` tallies. The only writes today
are `db.net_geoms[nr.net.0] = geom` and report accumulation.

Scheme: `route_db.nets.par_iter().map(|nr| -> Result<NetBuildOut>)`
where `NetBuildOut { net_idx, geom, wire_len, cut_counts: Vec<(String,u64)> }`;
collect order-preserved → serial fold writes `db.net_geoms` and
accumulates `EmitReport` in `route_db.nets` order. The unrouted-reset
loop and the guided-absent `bail` stay serial (trivial).

`place_via` already returns `Result` — errors propagate through the
`collect::<Result<Vec<_>>>()` unchanged; the first failure in
`route_db.nets` order is reported (collect preserves order, so the
error is deterministic).

**Byte-identity argument**: `db.net_geoms` receives the same
`NetGeom`s at the same indices; `EmitReport` accumulates the same
values (`BTreeMap` `vias_per_cut`, `wire_length_dbu` sums, `unrouted`
push order) — no ordering hazard exists.

## 3. `nets_section` (emit.rs L252–319)

Per-net `rec` strings are pure (reads `db`/`tech`/`geom`); today they
concatenate in `0..num_nets()` order. Scheme: `(0..num_nets())
.into_par_iter().map(build_rec).collect::<Result<Vec<String>>>()`
then `out.push_str` each in order — byte-identical text. The record
builder extracts today's per-net body verbatim (conns + stmts +
`+ USE` tail); the `NETS n ;` header/`END NETS` tail stay outside.

`preemit_check` (`assemble` + `run_all`) stays serial — phase-split
measurement decides whether it is ever worth a ticket; named in
`results.md` if hot.

## 4. Where the pool installs

Each public entry builds `drt_pool()` once and wraps its parallel
section in `pool.install(|| …)` (`parallel-model.md` §7). Small
inputs (a handful of guided nets — e.g., the synthetic test fixtures)
still work correctly at any pool size; no serial fast-path is added
(one code path, `num_threads` only changes scheduling).
