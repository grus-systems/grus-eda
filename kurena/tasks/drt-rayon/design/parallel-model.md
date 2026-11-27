# parallel-model — the route loop's scheme and its invariant

## 1. Named invariant (acceptance: "name its invariant")

**Snapshot-search / ordered-commit.** One rip-up iteration is two
disjoint phases:

1. **Search** — every net in `bad` runs `route_one_net` against the
   iteration-start occupancy, shared as `&Occupancy`. No search may
   write occupancy; the borrow checker enforces it (`route_one_net`
   and its whole call tree take `occ: &Occupancy` — verified: every
   `&mut Occupancy` site is `premark_obs`, `commit_route`, `unmark`,
   or the post-loop min-area patch).
2. **Commit** — outcomes are applied one at a time **in `bad` order**
   on the main thread: provisional-collision scan → `commit_route`
   rasterization → bookkeeping. Commits are the only writers.

There is no unsafe shared-occupancy mutation anywhere; the "snapshot"
is the ordinary `&Occupancy` borrow held for the duration of the
parallel section — no copy.

## 2. Where the provisional-collision scan moves

Today (`maze.rs` ~L2699–2710) `route_one_net`'s tail runs
`edge_collisions` over `run.used_edges` against `occ` — the occupancy
*at that moment*, which in a serial loop equals "everything committed
before this net". Under parallel search that same call would see only
the iteration-start marks and miss stale collisions between two
in-flight routes.

Move the call to the commit step: `NetOutcome::Routed` carries
`used_edges: Vec<u32>` (internal field — `NetRoute` is public and
unchanged), and the commit step runs `edge_collisions` per used edge
against live `occ` **before** `commit_route` rasterizes the route.
For a serial interleave this is identical to today's behavior (the
commit-time `occ` is exactly what the extraction-time `occ` was).
For parallel search it sees prior-iteration marks **plus** every
same-iteration commit in `bad` order — stale collisions are recorded
as ordinary provisional `Collision`s (canonical net×net via `prov`,
directed wire→pin via `prov_pins`) and both parties re-queue by the
existing `next` construction.

The scan is gated as today: only when `!hard && cfg.occ == OccMode::Enabled`.
In `hard` mode the results were already discarded; skipping the scan
there is identical semantics minus wasted work. In `Disabled` mode it
was already skipped.

## 3. Iteration structure (what changes, what does not)

```
let cap = cfg.max_iters.max(1);
while it < cap && !bad.is_empty() {
    it += 1;
    let hard = it == cap;
    if hard {
        // unchanged serial interleave: route_one_net → (drop prov
        // scan — its results are unused in hard mode) → commit_route,
        // per net in bad order. Foreign hits are hard blocks *during*
        // search; that semantics requires live occupancy, so this
        // iteration does not snapshot.
        for (bi, &i) in bad.iter().enumerate() { /* today's body */ }
    } else {
        // search phase — pure, order-preserved
        let outcomes: Vec<NetOutcome> = bad
            .par_iter()
            .map(|&i| route_one_net(db, tech, tracks, &graphs.nets[i],
                                    &topos[i], vias, &occ, &hull,
                                    hist[i], false, cfg.occ))
            .collect();
        // commit phase — serial, in bad order
        for (bi, (outcome, &i)) in outcomes.into_iter().zip(&bad).enumerate() {
            match outcome {
                Routed(route, used_edges, res) => {
                    // edge_collisions over used_edges → prov/prov_pins
                    // (soft+Enabled only), then commit_route, then
                    // res_of/routes/marks bookkeeping — same as today.
                }
                Failed { unrouted, stub_blocked } => { /* as today */ }
            }
            // progress sink fires here — see §5
        }
    }
    // iter_stats, next-bad construction, rip-up + hist escalation:
    // unchanged.
}
```

- `topos`: `graphs.nets.par_iter().map(NetTopo::new).collect()` —
  built over premark-only occupancy, a pure per-net function;
  order-preserved collect.
- Rip-up (`unmark` per recorded `Mark`), `hist` escalation, `next` /
  `bad` construction (`BTreeSet` dedup → sorted deterministic order),
  `iter_stats`, `stub_outside_cover` sort+dedup, min-area patch,
  censuses: all unchanged.
- `IterStat` semantics preserved: `attempted` = `bad.len()` at entry;
  `routed` counted at commit; `prov_collisions` = deduped prov sets —
  values legitimately differ from the serial baseline (different
  searches, stale pairs recorded at commit).

## 4. Determinism argument (the loop's gate)

- `route_one_net` is a pure function of
  `(db, tech, tracks, g, topo, vias, occ-snapshot, hull, hist, hard, mode)`:
  per-call `NetRun`/scratch, deterministic `BinaryHeap` tie-break on
  `(cost, node)`, BTree-ordered iteration, env reads are
  observational (`DRT_DEBUG_NET` eprintln only).
- `par_iter().map().collect()` on `Vec<usize>` is an indexed parallel
  iterator — `collect` preserves input order, so `outcomes` aligns
  with `bad` regardless of scheduling.
- Commits, marks logs, `routes[i]`/`res_of[i]`/`marks[i]` writes,
  `prov`/`prov_pins`, `failed`, `stub_outside_cover` all happen in
  `bad` order on one thread.
- ⇒ `RouteDb` is a pure function of the inputs **for any thread
  count** — strictly stronger than the spec bar ("deterministic
  given a fixed thread count"). Tests pin the stronger property:
  pool-forced `1` vs `8` threads must give identical `route_digest`
  (`tests.md` §2).

What the scheme does **not** keep: serial-identity. Searches see the
iteration-start snapshot where serial order would have seen each
earlier net's commits, so routed geometry legitimately differs —
the spec names that non-goal. The serial baseline's truth is carried
forward by the recorded digest (`GCD_ROUTE_DIGEST` re-pinned,
`results.md` records old → new) and the emitted-DEF golden
(`tests.md` §3–4).

## 5. Progress sink and timing emission

`RouteProgress`'s wire contract: one event per attempt, `done` =
attempts finished so far this iteration in `bad` order, `unrouted` =
failures so far, `elapsed` observational. Under the split iteration
the per-attempt point is the **commit** (the only moment an attempt's
disposition exists in order): the sink moves into the commit loop,
emitted with the same fields — a hooked CLI sees the identical
sequence shape. `route_nets_with_progress`'s signature and
`test_route_nets_progress_hook`'s "hooked ≡ unhooked" contract are
unchanged.

`GRUS_DRT_TIMING` per-net lines (`net[bi/…] nodes=… edges=… pins=…
dur`) can no longer attribute search wall time per `bad` index — the
searches overlap. Emit them inside the parallel closure (lines
interleave, marked as such) carrying the in-closure `Instant`
elapsed; keep the per-iteration census line unchanged (commit phase
is serial, its `ti.elapsed()` covers search+commit — split into
`search=`/`commit=` segments so the phase split is honest).

## 6. `Sync` enablement — the only code change the compiler requires

`Occupancy` is `!Sync` solely through `ScanStats`'s `Cell<u64>`
fields. `Cell` → `AtomicU64` with `Ordering::Relaxed`:

- Totals stay exact: every query bumps once per event and the query
  stream is a pure function of the snapshot — counts are as
  deterministic as the route itself. Only `ScanStats::bump`
  (`fetch_add`) and the read sites (`.load`) change.
- `#[derive(Default)]` still applies (`AtomicU64::default()` = 0).
- Everything else crossing the boundary is plain data — verified by
  grep, no `Cell`/`RefCell`/`Rc`/`unsafe impl` in `DesignDb`,
  `TechDb`, `LayerTracks`, `ViaTable`, `RouteGraphs`/`NetGraph`,
  `NetTopo`, `HullMask`, `Occupancy`'s own fields.
- A `static_assertions`-free proof suffices: `par_iter` bounds
  (`T: Sync` on shared refs, `Send` on collected items) make the
  compiler the checker. Add a `const fn _assert_sync<T: Sync + Send>()`
  one-liner per shared type in `maze.rs`'s test module so a future
  `Cell` regression fails at compile time with a readable name.

## 7. Pool: forcing the thread count honestly

Plain `par_iter` runs on rayon's global pool, which reads
`RAYON_NUM_THREADS` **once** at first use — inside `cargo test` the
first rayon-touching test fixes it process-wide, so a test cannot
honestly "force `RAYON_NUM_THREADS=1`". All drt parallel sections
therefore run inside a private pool built per top-level call
(`drt/pool.rs`):

```rust
pub(crate) fn drt_pool() -> rayon::ThreadPool {
    let n = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .expect("drt thread pool")
}
// call sites: pool.install(|| ... par_iter sections ...)
```

- `num_threads(0)` = rayon's default (physical cores) — unchanged
  default behavior; `RAYON_NUM_THREADS` is honored at call time,
  which is what the acceptance's "forced `RAYON_NUM_THREADS=1`" test
  needs: `std::env::set_var` then run the leg, twice, with no
  process-wide pool state.
- Pool construction is once per `route_nets`/`build_route_graphs`/
  `apply_routes` call — µs-scale against seconds-to-minutes of work.
- Rejected: global-pool reliance (untestable env pinning),
  `std::thread::available_parallelism` manual shards (re-implements
  work stealing for no gain).

## 8. Rejected alternatives

- **Cloned-snapshot occupancy**: `Occupancy` would need `Clone`
  (deep `Vec<Line>` per layer × iteration) for zero benefit — the
  shared immutable borrow already *is* the snapshot.
- **Disjoint-region batching** (spec allows it "if coverage is
  measured honestly"): unnecessary — ordered commit already gives
  determinism; region partitioning adds a second policy surface and
  would starve small `bad` sets.
- **Serial fallback at `num_threads == 1`** (interleave search+commit
  to preserve the legacy byte stream): rejected — it forks the
  algorithm into two truth states; a single scheme that is
  deterministic at every thread count is the stronger, simpler
  contract. The legacy digest lives in `results.md` history.
- **Parallel hard iteration with commit-time conflict → fail the
  loser**: changes "hard = foreign hits block" into "block-or-lose",
  weakening the convergence mechanism the loop relies on. The serial
  last iteration is small (`bad` is minimal by then) and removes the
  question entirely.
- **rayon inside `overlap_census`/`pin_overlap_census`/`run_all`/
  min-area patch**: profiling scope for a later ticket; each needs
  its own determinism argument (BTreeSet output orders are already
  canonical, but the win is unproven — phase split first).
