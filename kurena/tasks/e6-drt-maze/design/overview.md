# e6-drt-maze — Overview

E6 ticket 5, the routing core: a sequential maze router that connects
each guided net's member pins inside its guide region on the track
graph built by `e6-drt-graph`, using the via model from `e6-drt-via`.
Non-goals stay as specced: no DEF emission, no full DRC during search,
no timing awareness, no track-assignment stage, no parallelism.

## Resume state (third revision, after card feedback #2)

**HEAD is WIP commit `2c3c713`; it compiles** (2 dead-code warnings:
`Hit::x_line`, `hit_rect`) and the synthetic cases pass. Two real
gaps were *measured* while preparing this revision:

1. **Routability gap.** `test_maze_census` on gcd: `guided=269
   routed=221 pins_connected=662 projected=12 unrouted=48
   collisions=189 iterations=8 ms=203058` — then the
   `unclassified.is_empty()` assert goes red: **47 of 48 `NoPath`
   nets are not met3-bridge**. Acceptance expects unrouted = 0; this
   is the primary remaining engineering task. The failure pattern is
   systematic, not congestion: single inner pins (`A1`/`A2`/`B1`/`B2`)
   on multi-input AOI/OAI cells (`a21oi`, `a22o`, `a221o`, `a32o`,
   `o2bb2a`, `a21boi`), whole pin groups on `_131_`/`_132_`. All are
   `NoPath` (attach sets exist but are unreachable), meaning the
   pockets are *walled*, not detached.
2. **Runtime gap.** gcd alone takes 203 s in the debug test profile;
   picorv32 (28.4 M nodes / 54.6 M edges, ~37× gcd's graph) did not
   finish inside the ~21+ CPU-minutes observed on the review run —
   far beyond linear scaling. Whether it is stuck or merely slow is
   unresolved until instrumented (`perf.md`); the only unbounded loop
   in the code is the `extend_once` backtrack, which gets a bound.

What `2c3c713` landed on top of `a52d771` (the earlier checkpoint):

- `RelaxedVia` threading finished: `relaxed_vias()` per call,
  unified edge index space `[0,E) | [E,E+V) | [E+V,E+V+R)`, CSR
  carries all three arc classes (strict before relaxed so
  equal-`VIA_COST` ties break to strict), extraction emits the
  placement **and** the two implied stub wires.
- `drt::graph::net_region`/`build_route_graphs_ext` gained
  `spare_above` (maze calls it with 1): every guide box's XY is also
  covered on `top_guided + 1` — OBS-sealed pin pockets are reached
  one layer above the global route's layer choice.
- `MacroDef::obs_rects_raw` (pre-`merge_connected_obs`) added in
  `tech_db.rs` + populated in `readers/lef.rs` — the merged bbox
  seals the gaps decap/edge cells leave, so `premark_obs` marks the
  unmerged set.
- `Occupant::Obs(u32)` now carries the cell index; pin shapes are
  premarked `Net(own_net)` (or `Obs(u32::MAX)` for net-less pins).
  Pin-carve rule changed to host-cell + access-halo: an `Obs(cell)`
  hit is invisible when the candidate rect overlaps the halo
  (pin rect + `w/2` + widest track gap) of an own pin *hosted by that
  same cell*.
- Projection attach now takes a one-line margin (`project_attach`).
- The diagnosis scaffolding (`debug_*` fns, `test_maze_debug_gcd`)
  is already gone; `tests/drt.rs` has `load_routed_inputs` +
  `guide_union_connected` and the four acceptance test groups.

## Working diagnosis for the 48 gcd failures

`NoPath` means attachable-but-unreached. In soft iterations foreign
`Net` marks only cost `OCC_PENALTY`; a net that still fails in *every*
iteration is blocked by `Obs` marks or by a node-level component
split. Ranked hypotheses (instrumentation in `steps.md` §2 decides):

- **H1 — carve radius too small.** The carve engages only when the
  candidate rect already overlaps the pin halo. A pin pocket deeper
  than `w/2 + max_gap` from the pocket mouth is unreachable: the
  approach edge hits host-cell OBS *before* it overlaps the halo and
  stays blocked. Matches the deep-inner-pin pattern exactly.
  Recorded fix: carve `Obs(host)` for the net unconditionally once
  the net has a pin on `host` (OpenROAD's "access shapes ignore
  own-instance OBS"), not only inside the halo.
- **H2 — non-host walls.** Neighbor cells' OBS (abutted placement)
  and foreign pin metal (`Net(m ≠ own)`, hard-blocked on the last
  iteration) can seal a pocket the host-carve never opens. Recorded
  fix candidates: a third occupant class `Pin(net)` that never
  hard-blocks (or: stop marking pin shapes — the post-DRC tickets
  own pin-access correctness), and/or pocket-mouth escape via the
  spare layer.
- **H3 — node-level disconnect inside a guide-connected region**:
  `guide_union_connected` is box-level; the only intra-level check.
  Relaxed-via's *single-nearest rule* (nearest upper line holds no
  in-region node ⇒ no edge at all) can strand nodes. Fix: widen to
  the next-nearest lines — already the recorded first knob.
- **H4 — stuck-loop masquerading as NoPath**: ruled out by the
  bounded backtrack guard + per-phase instrumentation.

If H1/H2 hold, an `OccMode::Disabled` control run should still fail
(Obs stays blocking under Disabled) — which is itself diagnostic:
the probe (`steps.md` §2) reports, per failed pin, which occupant
class blocks the attach neighborhood's incident edges.

## Decisions (detail in the sibling docs)

1. **Sequential single-net ordering** in `join.nets` (guide-file)
   order — deterministic for free; pin-count ordering rejected.
2. **Pathfinder-style tree extension**: multi-source Dijkstra from
   the routed tree to the nearest unattached pin's node set.
3. **Integer hop costs** — `PREF 1 / OFF_AXIS 2 / VIA 4 / OCC 8 /
   HIST 8 / MAX_ITERS 8`, last iteration hard-conflict.
4. **Occupancy = per-line shape intervals** rasterized within the
   `w/2 + s` band on both axes; `Obs` always hard, foreign `Net`
   soft then hard, own net free, host-cell pin-carve exemption.
5. **Projection attachment** only when strict `pin_nodes` is empty,
   with a one-line margin — loosening counted in `NetRoute::projected`.
6. **Relaxed via edges** per net: every lower-layer node gets one
   transition whose true position sits on both layers' preferred
   wires (strict crossings coincide every 7820 DBU on li1–met1/
   met1–met2 and never on met2–met3).
7. **Spare-layer region** (`spare_above = 1`): each box's XY covered
   on `top_guided + 1` for OBS-sealed pocket escapes.
8. **Unmerged OBS marks** (`obs_rects_raw`): the merged bbox fills
   the gaps pins need.
9. **Rip-up rule**: re-queue unrouted nets plus both parties of every
   provisional collision; per-net `hist` escalation; cap 8.
10. **Named unrouted report** per pin (`Unattachable` vs `NoPath`) —
    never a panic, never an infinite loop (backtrack bound added).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
