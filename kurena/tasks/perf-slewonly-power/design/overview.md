# perf-slewonly-power — slew-only `sta_pin_slews`: overview

`power` pays for STA work `sta_pin_slews` provably does not need.
`sta_pin_slews` (`crates/grus-core/src/sta/analysis.rs:2281`) runs
`run_sta_detail` — `prepare_sta` + `eval_sta` + endpoint collection +
`assemble_sta_report` — to read two `f32`s per pin (`slew_max` rise/fall).
Measured v0.3.3 baseline (`docs/results.md` 2026-11-22 perf-par-slew
entry, `kurena/tasks/perf-profile/design/plan.md` rank 2): jpeg `power`
placement RC 2170 ms, of which the `power.slews` envelope is 1824 ms —
nested `sta.prepare.clock.slew` 901 + `sta.eval.walk` 830 + ~90 rest.

Plan target: jpeg `power` → ~1.3 s. The removal candidates, in order:

1. **`sta.prepare.clock.slew` (~901 ms jpeg)**: `propagate_slew` +
   `propagate_clock` + consumer seeding run inside `prepare_sta` under
   `any_propagated || !pulse_width_checks.is_empty()`. Every committed
   design trips the second disjunct (sky130 flops carry clock-pin
   `min_pulse_width` arcs), yet nothing the block produces reaches
   `PinSlew` when no clock row is propagated: the propagated-consumer
   copy into `base_state` is itself gated on `any_propagated`, and
   `clock_state`/`clock_parity`/pulse checks feed arrivals and the
   report, never `slew_max`. Under a slew-only contract the block is
   needed iff `sdc.clocks.iter().any(|c| c.propagated)`.
2. **Endpoint collection + `sta.assemble` + `run_sta_partitioned`
   dispatch**: `PinSlew` is exception-invariant — `eval_sta` merges
   slew unconditionally for every source (`allowed` gates arrival
   fields only; `merge_all`'s doc comment already asserts the
   per-class slew maps are identical — the single arrival-coupled
   exception, the `latch_borrow` window, is fenced by the fallback in
   semantics §4). No `allowed` set, no endpoints, no report.
3. **Arrival bookkeeping inside the walk**: derate multiplies, dly/par/
   crpr/dom mask merges, borrow-window arithmetic. The slew legs
   themselves (`driver_response` ×2 per leg) are kept — they produce
   the output.

What is **not** available: the `propagate_slew` unified map as a drop-in
`PinSlew`. It diverges from `eval_sta`'s `state.slew_*` on every
committed design — see [`semantics.md`](semantics.md), which derives the
exact slew map `eval_sta` produces and why a *seeded slew replay* (not
the unified map) is the fast path.

## Why this shape

`eval_sta`'s `state.slew_*` is a pure max/min accumulation of
`(late_slew, early_slew)` contributions — `merge()` updates the slew
fields unconditionally, independent of which arrival wins. So the slew
map is reproducible without computing arrivals: seed the same state,
fire the same legs, merge only the slew pair. The one arrival-coupled
slew decision — the latch-borrow window — is fenced behind a cheap
fallback predicate rather than approximated (semantics §4).

The replay needs a `reached` bit per `(pin, pol)`, because `reached` is
*not* implied by `slew_reached` (a propagated clock pin can be
arrival-reached with `slew_max = -inf`, and it still fires eval's leg
gate `reached || slew_reached` with a `-inf` slew input). The design
carries `reached` as a finite `arrival_max` marker on the same
`PinTimingState` — cheap, and it lets `slew_sink`/`merge_slew`/
`eval_seed_state` be reused verbatim (semantics §3).

## What lands

1. `prepare_sta` gains a mode (`for_slew`) that re-gates the
   `propagate_slew`/`propagate_clock` block on `any_propagated` alone;
   everything else — context, net-load selectors, pi models, wire
   delays, graph, levels, port/launch/latch seed construction — is
   shared verbatim. No duplicated seed logic.
2. `eval_sta`'s seed block extracts to `eval_seed_state(prepared,
   allowed)`; `eval_sta` calls it under `sta.eval.seeds`, the slew path
   calls it with `allowed = None` under `sta.slew.seeds`.
3. New `slew_eval` pass (`sta.slew.walk`): the levelized walk's slew
   legs plus `reached` markers — no arrival/dly/mask merges, no
   endpoint collection.
4. `sta_pin_slews` = prepare(slew) + borrow fallback check +
   `slew_eval`; identical `Vec<PinSlew>` by construction, locked by a
   parity test on committed designs ([`verification.md`](verification.md)).
5. `docs/results.md` before/after entry from the committed `measure.sh`
   matrix; `docs/roadmap.md` N8 update; patch bump in the same commit
   ([`measurement.md`](measurement.md), [`steps.md`](steps.md)).

## Invariants

- **Bit-identical `PinSlew`**: `sta_pin_slews` output equals the
  `run_sta_detail` eval-state slew fields, pin by pin, on every
  committed design — proven by a test that runs both paths, not by
  argument alone.
- **Goldens untouched**: all `tests/designs/**` goldens and power
  outputs byte-identical (`git status` clean); `internal_power`
  lookups read the same slews so `mb.power.json`, the `*.openroad.txt`
  comparisons, and the activity diagnostic lines are unchanged.
- **Fallback, not approximation**: a `latch_borrow` edge with a
  non-clock output net sends `sta_pin_slews` through the existing eval
  tail — the borrow window's arrival-dependent `continue`s cannot be
  replayed without arrivals (semantics §4). No committed design hits
  it (ibex's gating latch borrows onto a clock net).
- **Phase keys honest**: slew work reports under `power.slews`; new
  keys `sta.slew.seeds`/`sta.slew.walk`; `sta.prepare.clock.*` shrinks
  to propagated-clock designs only — moves recorded in
  `docs/results.md`.
- Out of scope: `sta` command itself (still the full eval),
  `power.cells`/`activity.resolve`, further `eval.walk` speedup
  (rank 4, deferred), any golden/tolerance change.

## Documents

- [`semantics.md`](semantics.md) — what `eval_sta`'s slew map actually
  is; why the unified map fails parity; the replay's exact leg set;
  the borrow-window coupling and the fallback predicate.
- [`api.md`](api.md) — `prepare_sta` mode, `eval_seed_state`,
  `slew_eval`, `sta_pin_slews` body, phase keys.
- [`verification.md`](verification.md) — parity test matrix,
  fail-proof run, determinism, fallback coverage.
- [`measurement.md`](measurement.md) — measure protocol, ceiling,
  `docs/results.md` skeleton, moved phase keys.
- [`steps.md`](steps.md) — implementation order and gates.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-slewonly-power/design/`
- `Cargo.toml`
- `Cargo.lock`

(`main.rs`/`sta/mod.rs`/`power.rs` unchanged: the public
`sta_pin_slews` signature keeps `power`'s call site and the
`estimate_power` interface as-is; `mod.rs` already re-exports it.)
