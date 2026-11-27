# drt-rayon — overview

## Problem (external review)

`route_nets` (`crates/grus-eda-core/src/drt/maze.rs` — the spec's
`crates/grus-core` path is stale, the crate is `grus-eda-core`, as the
`drt-occupancy-ds` design already noted) routes nets one at a time in
`join.nets` order: every `route_one_net` searches against live
`Occupancy` and commits before the next net searches. The measured
phase picture at HEAD (occupancy-ds landed, v0.14.2): gcd `route_ms`
median 11,960 ms, of which ~69 % of sampled frames sit in the
`scan_wire`/`scan_rect` occupancy-query class. Graph build and emit
are small by comparison — the split must be measured, not assumed
(`measurement.md` step 0 runs first).

## Stale spec assumptions (recorded, not silently followed)

- **`crates/grus-core/…` paths** → `crates/grus-eda-core/…` throughout
  (`src/drt/graph.rs`, `src/drt/maze.rs`, `src/drt/emit.rs`,
  `tests/drt.rs`, `Cargo.toml`).
- **"rayon becomes a real dependency — `cargo add`"**: rayon is
  already a workspace dependency (`rayon = "1"` in root `Cargo.toml`,
  used by `sta/analysis.rs`, `sta/parasitics.rs`, `placement/*`,
  `congestion/rsmt.rs`). No manifest dependency change is needed;
  `Cargo.lock` pins the resolved version and CI builds `--locked`.
  The workspace-version bump still applies per AGENTS §5.

## What's actually parallelizable (read of the code)

- **`build_route_graphs_ext`** (`graph.rs` L1243–1432): the per-net
  body reads `db`/`tech`/`tracks`/`join`/`hull` and writes (a) the
  per-net `NetGraph` and (b) shared `out` accumulators
  (`zero_coverage`, `region_escape`, `escapes`, `total_*`,
  `via_edges_per_pair`, `hull_masked`). Two shared mutable borrows:
  `hull_scratch` (a `Vec<u32>` reused per query — becomes per-worker
  state via `rayon::map_init`) and `out` (becomes a per-net partial
  folded serially in `join.nets` order). Determinism is by
  construction — see `build-emit.md` §1.
- **`route_nets`** (`maze.rs` L3877–4129): two separable phases.
  - `topos` construction (L3899–3903): per-net `NetTopo::new` over
    the premark-only occupancy — pure w.r.t. shared state; parallel
    map, order-preserved.
  - The rip-up loop (L3926–4043): searches take `occ: &Occupancy`
    (verified — `route_one_net` and everything it calls never mutate
    `occ`; all `&mut Occupancy` sites are `premark_obs`,
    `commit_route`, `unmark`, and the post-loop min-area patch). The
    scheme is **parallel search over the iteration-start occupancy +
    deterministic serial commit in `bad` order** — `parallel-model.md`.
  - Min-area patch (L4056–4070): `&mut occ` with cross-net
    visibility — stays serial in this ticket.
- **`apply_routes` / `nets_section`** (`emit.rs`): per-net `NetGeom`
  assembly and per-net DEF record strings are pure; writes into
  `db.net_geoms` and the final `String` concat happen serially in
  fixed order — byte-identical (`build-emit.md` §2).

## Decision (driver's pick, justified in `parallel-model.md`)

**Parallel soft-iteration search + ordered serial commit; the hard
(last) iteration stays fully serial.** Searches of a soft iteration
run `route_one_net` over the iteration-start occupancy via
`bad.par_iter()`; outcomes are committed one at a time in `bad`
order, and the provisional-collision scan (`edge_collisions` over
`used_edges`) moves from inside `route_one_net` to the commit step —
where it sees exactly the occupancy a serial run would have seen at
that point, plus the earlier same-iteration commits. Snapshot-stale
collisions (two parallel searches claiming the same resource) are
therefore recorded identically to serial ones and re-queue both
parties; the loop converges by the existing mechanism. The hard
iteration runs the unchanged serial interleave so "foreign hits are
blocks" keeps its exact meaning — a stale conflict there would have
no legal disposition.

Determinism: searches are pure functions of the snapshot; `collect`
on an indexed parallel iterator preserves `bad` order; commits are
in `bad` order. Output is deterministic **for any thread count** —
a stronger contract than the spec's "fixed thread count" bar. The
route loop's gate (`acceptance.md`): deterministic run-to-run +
collision-free end state — serial-identity is explicitly not
required, and the scheme indeed does not keep it (searches see a
different occupancy than serial order would; `GCD_ROUTE_DIGEST`
changes and the pin is re-recorded, `tests.md` §3).

`Occupancy` is `!Sync` only because `ScanStats` uses `Cell<u64>` →
swap to `AtomicU64` (Relaxed) — totals stay exact (the query stream
is a pure function of the snapshot). Everything else shared across
threads (`DesignDb`, `TechDb`, `LayerTracks`, `ViaTable`,
`RouteGraphs`, `NetGraph`, `NetTopo`, `HullMask`) is plain data —
verified by grep, no `Cell`/`RefCell`/`Rc`.

Rejected alternatives in `parallel-model.md` §4: cloned-snapshot
occupancy (the borrow checker already gives the snapshot for free —
`&occ` is immutably borrowed for the whole parallel section);
disjoint-region batching (unnecessary complexity — ordered commit
already gives determinism); serial-equivalence preservation
(degenerates to serial, named a non-goal by the spec).

## Ordering / dependency note

`drt-occupancy-ds` landed (merge `451664c`, v0.14.2): occupancy
lines are `Vec<Line>` dense-indexed, the ~70 % scan share is already
priced into the serial critical path this ticket multiplies.
`GRUS_DRT_TIMING` emission (premark / topos / per-iter / patch /
occ-scan totals) exists and is reused for the phase split —
`measurement.md` adds only the build/emit leg timers in `main.rs`.

## Non-goals

- No public-surface change: `route_nets`,
  `route_nets_with_progress`, `RouteDb`, `route_digest`,
  `MazeConfig`, `build_route_graphs(_ext)`, `apply_routes`,
  `preemit_check`, `write_routed_def` keep signatures.
  `Occupancy`/`NetOutcome`/`NetTopo` stay module-private.
- No literal serial-equivalence mode for the route loop (spec §3:
  non-goal — recorded).
- No `unsafe`, no shared mutable occupancy — searches share `&occ`;
  commits are the only writers, serialized in `bad` order.
- Min-area patch, `overlap_census`/`pin_overlap_census`,
  `preemit_check`'s `run_all` stay serial this ticket (named in
  results if the phase split shows them hot).
- No picorv32 golden regeneration requirement — gated
  (`tests.md` §4, honest fallback: named deferred).

## Files

- `crates/grus-eda-core/src/drt/graph.rs`
- `crates/grus-eda-core/src/drt/maze.rs`
- `crates/grus-eda-core/src/drt/emit.rs`
- `crates/grus-eda-core/src/drt/pool.rs`
- `crates/grus-eda-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-eda-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `Cargo.toml`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
