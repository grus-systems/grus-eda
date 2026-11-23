# e6-drt-maze — Overview

E6 ticket 5, the routing core: a sequential maze router that connects
each guided net's member pins inside its guide region on the track
graph built by `e6-drt-graph`, using the via model from `e6-drt-via`.
Non-goals stay as specced: no DEF emission, no full DRC during search,
no timing awareness, no track-assignment stage, no parallelism.

## Resume state (fifth revision, after card feedback #4)

**HEAD is WIP commit `cf34607` — a docs-only checkpoint on top of
the `c1832dc` router rewrite; the code at HEAD is byte-identical to
what was measured below.** Card feedback #4 re-scopes the remainder
to implementation finishing, docs, lint, the verify run, and the
commit — in the reviewer's order:

1. Implementation finishing — `maze.rs` compiles at HEAD
   (`cargo check -p grus-core --tests` clean); the remaining code
   task is only the clippy list. While writing `docs/e6-drt.md` §7,
   re-check each claim `docs.md` mandates against `maze.rs` (the
   constants, occupancy classes, rip-up rule, hop heuristic were
   re-verified against HEAD at design time — `steps.md` §0).
2. `docs/e6-drt.md` §7 (search order, cost constants, rip-up rule,
   occupancy) + `docs/results.md` dated entry with measured runtimes
   incl. the picorv32 census number + `docs/status.md` +
   `docs/roadmap.md` — all still untouched (`docs.md`).
3. `cargo clippy --workspace --all-targets -- -D warnings` — **6
   warnings at `cf34607`**, re-verified at the same lines mapped in
   `steps.md` §1.
4. `cargo test --workspace`; record the measured picorv32 maze
   runtime — earlier census runs took 25+ CPU-min; if that is the
   accepted number, say so explicitly in `results.md`.
5. Version bump `0.12.3 → 0.12.4` + one commit.

No further routing-model work is planned unless the verify runs
contradict the measured state below.

## Measured state (code identical at `cf34607`)

- `test_maze_cases` (4 synthetic cases) — **green** (0.00 s).
- `cargo test -p grus-core --lib maze` — **green**, 5/5.
- `test_maze_census` gcd leg — **green**: `guided=269 routed=269
  pins_connected=931 projected=17 unrouted=0 collisions=0
  iterations=8 ms=9704 digest=0xfbb3073487860974`. Down from 203 s
  and 48 `NoPath` unrouted at `2c3c713` — the C1/C2 occupancy fixes
  and the L1–L3+heuristic perf work landed together.
- `test_maze_occupancy` — **green** (17.6 s): Enabled digest
  `0xfbb3073487860974` census=0 vs Disabled digest
  `0x349f82f53a0c7143` census=6429 — occupancy measurably constrains.
- `test_maze_census` picorv32 leg — **still unmeasured at design
  time** (>15 min debug-profile on the abandoned earlier attempt;
  the 25+ CPU-min figure from that run is the bound the reviewer
  accepted if it recurs — record the measured number, whatever it
  is).
- `test_maze_determinism` — untested (2× full runs per design; its
  cost follows the census number).
- `docs/e6-drt.md` §7, `docs/results.md` entry, `docs/status.md`,
  `docs/roadmap.md` — not yet written.
- `cargo clippy` — 6 warnings (exact list in `steps.md` §1;
  re-verified at `cf34607`, same lines).

## What `c1832dc` landed over `2c3c713`

A rewrite of `drt/maze.rs` (1,204-line diff) that closed both the
routability gap and most of the runtime gap:

- **C1 — unconditional host-cell carve** (`occupancy.md`): `Obs(cell)`
  is invisible to a net that has a member pin hosted by `cell`
  (`host_cells`, sorted+deduped per net). The earlier access-halo
  carve left approach edges blocked before the halo was reached —
  measured root cause of 47/48 gcd `NoPath`s. `Obs(u32::MAX)`
  (net-less pin metal) is never carved.
- **C2 — `Occupant::Pin(u32)` class**: member pin metal is marked
  `Pin(own_net)`, not `Net` — free to its own net, soft-priced for
  foreign nets, never a hard block (rip-up cannot move a pin; a
  landing wire is an emit-time DRC finding).
- **C3 partially folded into C2's reasoning**: relaxed-via stayed
  single-nearest; the spare layer (`spare_above = 1`) stayed.
- **`NetTopo` cache (L2)**: attach sets, `relaxed_vias`, and the CSR
  adjacency over the unified edge index space built once per net in
  `route_nets`, reused across rip-up iterations — byte-identical arc
  order.
- **Chunked skip index (L3)**: per-line `Interval` lists are
  `lo`-sorted with a `SKIP_CHUNK = 16` `max_hi` index rebuilt from
  the dirty chunk; scans touch only the query window.
- **Goal-directed wave**: per round, a unit-weight BFS from every
  unconnected attachable pin's attach set fills `hop`; the PQ keys
  `(g + hop, node)` — a consistent heuristic (every edge ≥1 hop),
  so the wave stops at the first target pop without flooding the
  graph. `u32::MAX`-hop nodes sort last; `NoPath` semantics unchanged.
- **Bounded backtrack**: the parent-chain walk caps at
  `g.nodes.len()` steps — a bug-shaped invariant breach surfaces as
  named `NoPath`, never a hang.
- **`GRUS_DRT_TIMING` env-gated instrumentation**: premark/topo/
  per-iteration/per-net `eprintln!` — stays in the committed code
  (opt-in, zero cost when unset) and feeds the `results.md` runtime
  evidence. Recorded decision, not stray scaffolding.

## Decisions (detail in the sibling docs)

1. **Sequential single-net ordering** in `join.nets` (guide-file)
   order — deterministic for free; pin-count ordering rejected.
2. **Pathfinder-style tree extension**: multi-source goal-directed
   search (`g + hop` PQ) from the routed tree to the nearest
   unattached pin's node set.
3. **Integer hop costs** — `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 /
   HIST 8 / MAX_ITERS 8`, last iteration hard-conflict.
4. **Occupancy = per-line shape intervals** rasterized within the
   `w/2 + s` band on both axes, `lo`-sorted with the chunked skip
   index; `Obs` always hard (host-carved), foreign `Net` soft then
   hard, `Pin` soft-only, own net free.
5. **Projection attachment** only when strict `pin_nodes` is empty,
   with a one-line margin — loosening counted in `NetRoute::projected`
   (gcd: 17 pins, all the §5 escape classes).
6. **Relaxed via edges** per net: every lower-layer node gets one
   transition whose true position sits on both layers' preferred
   wires, plus committed stub wires at extraction.
7. **Spare-layer region** (`spare_above = 1`): each box's XY covered
   on `top_guided + 1` for OBS-sealed pocket escapes.
8. **Unmerged OBS marks** (`obs_rects_raw`): the merged bbox fills
   the gaps pins need.
9. **Rip-up rule**: re-queue unrouted nets plus both parties of every
   provisional collision; per-net `hist` escalation; cap 8.
10. **Named unrouted report** per pin (`Unattachable` vs `NoPath`) —
    never a panic, never an infinite loop (backtrack bound).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
