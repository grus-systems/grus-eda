# perf-par-slew — algorithm

Transform `propagate_slew`'s level loop into `eval_sta`'s
compute-parallel / apply-serial shape, preserving the serial result
bit-for-bit in *both* levelization modes.

## Serial semantics being preserved

The pass (`analysis.rs` `propagate_slew`):

1. Levelizes with `topological_levels(cx, graph, true)`; on the
   combinational-loop `Err` retries with `false` (ibex hits this —
   `docs/results.md` 2026-11-20).
2. Seeds input `io_pins` (clock ports: `set_input_transition`; data
   inputs: the data pass's `slew_*` fields).
3. For each level `lvl ≥ 1`, each pin in `level_pins` order:
   - net-load pins (`!pin_is_driver` + `pin_driver[p]`): `next =
     slew_sink(driver_state, wire, pin_idx, is_port)` — a **replace**;
   - driver pins with a net (`pin_nets[p]`): merge every incoming arc's
     `arc_slew` — combinational arcs via `edges_to`, seq arcs (launch
     `clk->Q`, latch `enable->Q`) via `seq_in[p]` — via `merge_slew`
     (max/min, commutative);
   - write `slew[p] = next` iff anything changed.

All destination writes are `max`/`min` merges or a `slew_sink` replace of
an untouched pin — merge *order* never matters. What matters is **what a
source reads**.

### Read-time table (the whole correctness argument)

When pin `p` at level `L` reads `slew[src]`, the serial loop sees:

| `src` position vs. `p` | serial sees | parallel sees | handling |
|---|---|---|---|
| earlier level | post-level (final) value | same — applied before level `L` | exact |
| level 0 | seed or `UNREACHED` (level-0 pins are never processed) | same — never written | exact |
| same level, earlier in `level_pins` | **post-update value** | pre-level value | **diverges — fixup** |
| same level, later | pre-level value | pre-level value | exact |
| later level | pre-level value | pre-level value | exact |

**Lemma** (needed below): a pin at level ≥1 always enters its level with
`UNREACHED` slew. `slew` starts `UNREACHED`; the only pre-loop writes are
input-port seeds, and input ports are indegree-0 → level 0.

For combinational arcs and net loads the "same level, earlier" row can
never occur: both edge kinds are ordering edges in *both* levelization
modes, so sources are strictly shallower. Only `seq_in` edges can be
same-level, and only in the `include_seq=false` fallback — in the
`include_seq=true` levelization seq edges are ordering edges too. By the
lemma, a same-level seq source's pre-level slew is `UNREACHED`, so the
parallel body merges nothing for that edge while serial merges the
source's post-level contribution.

## Parallel transform

Mirror `eval_sta`'s walk (`analysis.rs ~L3040`): a `&[PinTimingState]` ->
`Option<(usize, PinTimingState)>` closure holding the *entire* serial pin
body verbatim — net load, combinational arcs, **and** the `seq_in` loop.

```text
let mut rank = vec![0u32; n];                      // position in level_pins
for (i, &p) in levelized.level_pins.iter().enumerate() { rank[p.0] = i; }

for lvl in 1..num_levels {
    let level = levelized.level(lvl);
    let lo = levelized.level_offsets[lvl] as usize;

    // 1. compute (parallel for level.len() >= 32, serial below)
    let updates: Vec<(usize, PinTimingState)> = /* same dual path as eval.walk */;

    // 2. apply, in collected (level) order — writes are disjoint
    for (p, next) in updates { slew[p] = next; }

    // 3. seq fixup: replay only same-level-earlier seq arcs, in level order
    for i in lo..lo + level.len() {
        let p = levelized.level_pins[i];
        let Some(out_net) = design.pin_nets[p.0 as usize] else { continue };
        for &(src_pin, lib_arc, src_pol) in &seq_in[p.0 as usize] {
            let r = rank[src_pin.0 as usize] as usize;
            if r >= lo && r < i {
                // src is in this level at an earlier position: serial read its
                // post-update slew; the parallel body read UNREACHED and merged
                // nothing. Merge it now from the applied value.
                let src = slew[src_pin.0 as usize];
                /* same s_max/s_min guard + arc_slew + merge_slew as the
                   serial seq loop, into slew[p] in place */
            }
        }
    }
}
```

Fixup condition `lo <= rank[src] < i` selects exactly "same level,
earlier position". In `include_seq=true` mode it is provably empty — the
scan still runs (cost: one `seq_in` length check per pin) so both modes
share one code path. Keep the `pin_nets[p]` guard — serial wraps the seq
loop in it (an unconnected `Q` gains no slew dependency, matching the
levelization's own edge filter).

### Why replay is exact (not approximate)

- Same-level-earlier `src`: serial merges `arc_slew(post-level src)`. The
  parallel body merged nothing (lemma → pre-level `UNREACHED` → `s_max ==
  NEG_INFINITY` skip). The fixup merges `arc_slew` from `slew[src]`,
  which at fixup time holds `src`'s full post-level value — the level is
  applied and the fixup runs in position order, so `src`'s own
  same-level-earlier seq merges are already in. Chains resolve exactly as
  serial.
- Same-level-later and later-level `src`: serial and parallel both read
  `UNREACHED`, both skip. No fixup — replaying them would *add* slew
  serial never saw.
- Fixup into `slew[p]` in place ≡ serial merging into `next`: the merges
  are `max`/`min`, commutable with the parallel update already applied;
  a pin untouched by the update keeps `UNREACHED` fields except the slew
  lanes, identical to serial `next = slew[p]` + merges.

`PinTimingState` is `Copy` — copy `src` out before mutating `slew[p]`
(avoids the borrow conflict and matches the serial body's `let src =
slew[src_pin.0 as usize]`).

## Determinism argument

- Parallel bodies read only `&slew`, `&levelized`, `design`, `cx`,
  `graph`, `seq_in` — all immutable borrows (`eval_sta`'s walk already
  calls `cx.driver_response` and `graph.latch_borrow.get` inside
  `par_iter`, so `Sync` holds). No interior mutability anywhere.
- Collect is over `level.par_iter()` on a `&[PinId]` slice — an indexed
  parallel iterator, so `collect()` preserves level order; apply order is
  deterministic *and* irrelevant (disjoint indices).
- The seq fixup is fully serial, in `level_pins` order.
- No `HashMap`/`BTreeMap` is *iterated* in the pass — `latch_borrow` is
  keyed lookup only. Merge results cannot depend on map order anyway
  (commutative max/min).

Result: output is a pure function of the inputs, independent of
`RAYON_NUM_THREADS` and scheduling. [`verification.md`](verification.md)
records the runtime evidence.

## Cost notes

- `rank`: one `O(n)` build per `propagate_slew` call — noise vs. the
  ~5 s of DMP table evals it parallelizes.
- Fixup scan per level: iterates the level's pins, dereferences
  `seq_in[p]` (empty for all but ~#launch+#latch_open pins). Serial work
  added: negligible — and zero extra `arc_slew` evals where no
  same-level-earlier seq edge exists.
- Threshold `32` matches `eval_sta`'s `level.len() < 32` split: per-level
  dispatch on shallow/tiny levels would dominate. Keep the same constant.
- Levels remain serial *between* levels — the dependency structure
  requires it. Serial residue: port seeding, the fixup scan/merges, the
  apply pass, levels < 32 pins, and (in fallback mode) the
  `topological_levels` retry itself. Expect `k` below the plan's `k = 3`
  assumption wherever DMP lookups are memory-bound; measurement names the
  residual.
