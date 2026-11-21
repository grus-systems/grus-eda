# perf-par-slew — parallel `propagate_slew`: overview

Parallelize `propagate_slew` (`crates/grus-core/src/sta/analysis.rs`), the
measured rank-1 hotspot: `sta.prepare.clock.slew` is 41–81% of `sta` wall
(4.84 s of jpeg's 6.0 s placement-RC run) and ~79% of `power` wall via
`sta_pin_slews` (`docs/results.md` 2026-11-22 "perf-profile baseline",
`kurena/tasks/perf-profile/design/plan.md` rank 1). Mechanism: the
per-Kahn-level `par_iter` compute + apply-after-collect shape
`eval_sta`'s `sta.eval.walk` already uses.

## Why this shape

Within one Kahn level, vertex writes are disjoint — each pin's merged slew
is written only to `slew[p]`. All reads inside a pin's body are of *other*
pins' states, which the levelization places at earlier levels. So compute
per pin in parallel against an immutable `&slew`, collect owned
`(index, state)` results, apply in level order. No shared mutable state
inside parallel bodies; no `timing::phase` scope inside `par_iter`.

## The one non-obvious correctness constraint

`propagate_slew` levelizes with `topological_levels(cx, graph, true)` —
register `clk -> Q` and latch `enable -> Q` arcs are ordering edges. On a
cycle (ibex: a clock-gate latch's Q feeds the gated clock net, closing a
~42k-pin SCC; `docs/results.md` 2026-11-20 entry) it retries with
`include_seq=false`. In that fallback order a seq arc's source can sit in
the **same level** as its destination (latch `Q` pins carry combinational
`D -> Q` in-edges, so they levelize ≥1 while their enable may land in the
same level). The serial loop reads `slew[src]` *as of when dst is
processed*; a naive collect-then-apply reads the pre-level value.
[`algorithm.md`](algorithm.md) derives the exact serial semantics and the
per-level sequential fixup that preserves them bit-for-bit in both modes.

## What lands

1. `propagate_slew` level loop → `eval_sta`-style dual path:
   `level.len() < 32` serial, else `par_iter().filter_map(..).collect()`
   + ordered apply, plus a per-level serial replay of same-level-earlier
   seq arcs.
2. Equivalence + determinism evidence, and a recorded intentional-break
   run ([`verification.md`](verification.md)).
3. `docs/results.md` before/after entry from the committed `measure.sh`
   matrix; `docs/roadmap.md` N8 update; patch version bump in the same
   commit ([`measurement.md`](measurement.md), [`steps.md`](steps.md)).

## Invariants

- **Bit-identical output**: every committed `tests/designs/**` golden and
  `--json` report byte-identical to the serial baseline. The strict
  integration tests are the proof; `RAYON_NUM_THREADS` sweep + pre/post
  `cmp` are the recorded evidence.
- **Deterministic**: result independent of thread count and HashMap
  iteration (the pass uses a `BTreeMap` lookup only; no map-order
  dependence anywhere).
- **Phase key stable**: `GRUS_PHASE_TIMING` still reports
  `sta.prepare.clock.slew`; the scope already wraps the
  `propagate_slew` call, so `crates/grus-core/src/timing.rs` is untouched.
- Out of scope: `propagate_clock` (`sta.prepare.clock.walk`, measured
  0.3% — plan folds it in only if free; it is not), the slew-only
  `sta_pin_slews` path (`perf-slewonly-power` ticket), and any
  levelization-cost work inside the phase.

## Documents

- [`algorithm.md`](algorithm.md) — serial read semantics, the parallel
  transform, the same-level seq fixup, determinism argument.
- [`verification.md`](verification.md) — equivalence layers, fail-proof
  plan, determinism runs, fixup coverage probe.
- [`measurement.md`](measurement.md) — measure protocol, Amdahl ceiling,
  `docs/results.md` entry skeleton.
- [`steps.md`](steps.md) — implementation order and gates.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-par-slew/design/`
- `Cargo.toml`
- `Cargo.lock`
