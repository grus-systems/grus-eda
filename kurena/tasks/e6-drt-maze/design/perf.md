# e6-drt-maze — Runtime plan

The reviewer's open question — *slow or stuck* — plus the measured
fact that gcd alone takes **203 s** in the debug test profile make
performance a required work item, not a cleanup. picorv32's graphs
total 28.4 M nodes / 54.6 M edges (~37× gcd's 756 k / 1.45 M); at
the observed scaling it does not finish inside the workspace gate.

## 0. Answer "slow or stuck" first

Temporary instrumentation patch (uncommitted or committed-then-
removed — same convention as the deleted `debug_*` scaffolding):

- `eprintln!` per iteration: `it`, `bad` count, routed/failed this
  round, cumulative ms — proves forward progress (or pinpoints the
  stall site).
- Per-net top-5 by ms with `nodes / edges / pins / waves` — finds
  whether a few monster nets dominate (high-fanout nets like
  `_00012_`, fanout 273, exist) or the cost is spread evenly.
- Counters: `edge_cost` calls, `scan_wire`/`scan_rect` interval
  compares — separates occupancy-scan cost from raw graph work.
- The bounded backtrack (`search.md` §4) removes the one theoretical
  infinite loop; if the counters still climb steadily, the answer is
  *slow*, and the levers below apply.

Run order: gcd first (already 203 s — plenty of signal), then
picorv32 under an external timeout.

## 1. Complexity accounting (why it is superlinear today)

Per iteration, for every `bad` net: `relaxed_vias` rebuilds two
`BTreeMap` line indexes over all nodes (`O(V log V)`), `NetRun::new`
rebuilds the CSR over all edges (`O(V+E)`), attach sets recompute
`pin_shapes`/`project_attach`, then each pin extension runs a wave
whose every relaxation calls `edge_cost` → a fresh `Vec` alloc for
`cand` + `scan_wire`/`scan_rect` → **linear scan over the whole
per-line interval list**. Per-line lists grow as nets commit, so the
same edge costs more later in the run — the superlinear term. With
`bad` staying ≈ all nets through all 8 iterations (gcd: 189
provisional collisions + 48 unrouted kept it dense), the whole
pipeline repeats ~8×.

Secondary suspects the counters will rank: `overlap_census`
pairwise-per-line sweep (O(k²) worst case on dense lines), and the
extraction-time `edge_collisions` re-scan per committed edge.

## 2. Levers, applied in order until the budget is met

All are semantics-preserving unless stated; apply only what the
measurements justify, record what landed in `docs/results.md`.

- **L0 — delete dead code** (step 0 anyway): `Hit::x_line`,
  `hit_rect` — the two warnings.
- **L1 — scratch reuse.** `edge_cost` takes a `&mut Vec` for `cand`
  like `hits` already does — kills a heap alloc per edge relaxation.
- **L2 — `NetTopo` cache.** Split the occupancy-independent part of
  `NetRun` (attach sets + projected/attachable + `relaxed_vias` +
  CSR arcs) into a per-net `NetTopo` built once in `route_nets` and
  reused across iterations; `NetRun` keeps only per-attempt search
  state. Removes the 8× rebuild of ~55 M arcs and ~28 M
  `BTreeMap` insertions on picorv32. Byte-identical edge order —
  determinism untouched.
- **L3 — bounded interval scans.** Per-line `Vec<Interval>` →
  `lo`-sorted with a chunked `max_hi` skip index (fixed-size groups,
  e.g. 16 entries, each storing `max(hi)`; rebuilt lazily per line
  when dirty). `scan_wire`/`scan_rect` then skip whole groups whose
  `max_hi ≤ qlo` and break at `lo ≥ qhi` — touching only the query
  window. Insert/remove stay ordered (`rasterize` binary-inserts,
  `unmark` removes in place). Hit *set* unchanged; hit *order* may
  differ — both consumers are order-insensitive (`hit_verdict` is
  any-match + a set, `edge_collisions` emits into a `BTreeSet`), so
  `RouteDb` bytes are preserved.
- **L4 — only if still over budget**: wave-level work is already
  Dijkstra-bounded by the first target pop; the remaining honest
  lever is profile-level — e.g. `[profile.test]` `opt-level = 1`
  workspace-wide — which changes the whole suite's build/test
  tradeoff; take it only with the tradeoff recorded in
  `docs/results.md`, never silently. Do *not* shrink `max_iters`,
  gate the picorv32 leg, or skip — acceptance requires it to run and
  complete.

## 3. Budget

- picorv32 `test_maze_census` leg **completes** under the default
  (debug) test profile — acceptance: "completes in a test, not a
  timeout — state the measured number". Working target: single-digit
  minutes for the leg; the whole `drt` test binary (16 tests; the
  occupancy + determinism tests add 3 more gcd runs and 2 more
  picorv32 runs) should stay inside the CI envelope demonstrated by
  earlier E6 tickets.
- The measured per-design `ms=` from the census print goes into
  `docs/results.md` verbatim — no estimates (AGENTS §2).

## 4. Interaction with the routability fix

Order matters: diagnose/fix the `NoPath` walls (`occupancy.md` §5)
*before* finalizing perf numbers — the fix changes both the work
(unrouted nets stop dominating `bad`) and the measured baseline.
Instrument first anyway: the same counters drive both the wall
diagnosis (which `Occupant` class blocks) and the perf breakdown.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/results.md`
- `Cargo.toml`
