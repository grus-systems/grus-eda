# perf-slewonly-power — the slew map `eval_sta` actually produces

This file derives, from the code, the exact `state.slew_*` map
`sta_pin_slews` returns today — the parity target — and enumerates every
place a naive fast path would diverge. Line numbers are v0.3.3.

## 1. The slew merge rule

`PinTimingState::merge` (`analysis.rs:154`) updates slew independently
of the arrival race: `slew_max[pol] = max(slew_max[pol], s_max)`,
`slew_min[pol] = min(slew_min[pol], s_min)` — unconditional, no winner
masking. `merge_all` (the partition reducer) documents the consequence:
*"slews are identical across runs because every source merges them
unfiltered."*

So the final slew at a pin is the max/min over a *set* of
`(out_max, out_min)` contributions. Enumerating the set exactly —
which legs fire, with which input slew — is the whole parity problem.
Order within a pin does not matter for the bits (f32 max/min are
commutative and associative on the finite/`±inf` values involved), but
the replay preserves eval's order anyway.

## 2. Contributions, enumerated

`eval_sta`'s state = seeds (`sta.eval.seeds`) + per-leg merges
(`sta.eval.walk`):

**Seeds** (`eval_sta:3017`–`3071`, to be extracted as `eval_seed_state`):

- `base_state` slew, built in `prepare_sta`:
  - unclocked input ports: `drive` slew merged via `merge_slew`
    (slew-only — the port stays *unreached*; `merge_slew` does not
    touch arrival fields);
  - propagated-domain clock consumers: whole `clk_state` copied,
    whose slew fields were overwritten with the **unified** slew —
    requires `propagate_slew` + `propagate_clock` to have run;
  - ideal-domain clock consumers: `row.transition_ns` merged as both
    bounds at both polarities (seeded arrival marks `reached` too).
- `port_seeds`: every anchored input port merges slew for both
  polarities — unconditionally (`allowed` flips only the arrival half
  of the merge).
- `launch_seeds`/`latch_seeds`: `pair.slew_max/slew_min` merged at
  `pair.q`/`dst_pol` whenever `pair.arr_max > NEG_INFINITY`. The pair's
  slew is `eval_launch`'s `driver_response(out_net, dst_pol, tables,
  clk_state.slew_{max,min}[edge_pol])` — the input slew is the
  **seeded** clock-pin slew (ideal `transition_ns`, propagated unified
  slew, or `driver_input_slew` for an unclocked launch), *not* the
  unified tree slew.

**Walk legs** (`eval_sta:3077`–`3320`), over `prepared.levelized`
(the `include_seq=false` data-edge levelization):

- *Net-sink leg* (sink pins): requires `!pin_is_driver[p]`,
  `pin_driver[p]` set, `!clock_net[net]`. Driver `reached` →
  `sink_state_with_wire` (slew = `slew_sink` formula + arrival);
  driver `!reached && slew_reached` → `slew_sink` slew merge only,
  sink stays unreached.
- *Arc legs* (driver pins): requires `out_net` set and
  `!clock_net[out_net]`. Per `(src, arc, src_pol→dst_pol)` leg: legs
  are `arc_legs(arc)` — or `[(RISE,RISE),(FALL,FALL)]` on a
  `latch_borrow` edge — gated by `src.reached(src_pol) ||
  slew_reached(src, src_pol)` plus the `CombinationalRise/Fall`
  timing-type filter; `driver_response` is called at `s_max` and at
  `s_min` (skipped when equal) and the pair merges as
  `(late_slew, early_slew)`. A `reached` leg additionally merges the
  arrival (marks dst `reached`); a slew-only leg merges slew only.
- *Sequential arcs* are **not** walk legs — `clk→Q` and latch
  `enable→Q` contribute slew only through the launch/latch seeds
  above.

**Two consequences of the `graph.clock_net` guards:** on a clock net,
*no* wire slew reaches sink pins and *no* arc slew merges at driver
pins — a clock-net pin keeps exactly its seed slew (consumer
transition / copied `clk_state` / launch-pair slew at a `q` whose net
is a clock net / `UNREACHED`). And every committed SDC is ideal-clock,
so `transition_ns` is 0 and `set_input_transition 0.15` never reaches
a clock pin's eval slew at all.

## 3. Why the unified `propagate_slew` map is not the parity answer

`propagate_slew` (`analysis.rs:1754`) walks *all* arcs — clock and
data — with no `clock_net` guards, seeds clock ports with
`set_input_transition`, and data ports from `state_ports`. Relative
to eval's map it:

- fills real tree slews at clock-net pins where eval keeps the seed
  (`transition_ns` = 0 on committed SDCs) or `UNREACHED`;
- sources seq arcs from the unified clock-pin slew where eval uses the
  seeded basis — on ideal domains that is `transition_ns`, a different
  number that shifts every downstream `driver_response` on the data
  cone;
- merges arc slew into clock-net driver pins eval never touches.

So `unified ≡ eval` fails on every committed design; the parity gate
the plan named exists precisely because of this. The fix is not to
patch the unified map post-hoc — max/min merges are not invertible
downstream of a changed source slew. The fix is a **seeded replay**:
eval's exact seeds + eval's exact legs, with arrival bookkeeping
replaced by a `reached` bit.

`reached` cannot be dropped or derived from `slew_reached`: eval's leg
gate is `reached || slew_reached`, and `reached ⇒ slew_reached` does
not hold — a propagated clock pin can be arrival-reached with
`slew_max = -inf` (e.g. its path crossed an arc with no slew table),
and it still fires the gate, feeding `-inf` into `driver_response`.
The replay therefore carries the bit on the same `PinTimingState`:
the net-sink reached leg assigns `slew_sink(driver, ..)` wholesale, so
the sink inherits the driver's marker arrivals; an arc leg that fires
off a `reached` source sets `arrival_max[dst_pol] =
max(arrival_max[dst_pol], 0.0)` — a finite marker standing in for the
arrival eval would have merged. Seeds arrive with real arrival values
from `eval_seed_state`, so seed `reached` is exact.

## 4. The one arrival-coupled slew decision: `latch_borrow`

On a borrow edge, a *reached* leg does not merge unconditionally: eval
computes the borrow window (`en` arrivals, `en_dom`, setup table,
`unc_worst(src.dom_max, …)`, `src.arrival_max`) and `continue`s —
dropping the slew merge — when the data arrival precedes the open edge
or the enable is untimed. That decision needs `src.arrival_max` and
`src.dom_max` at the D pin — real propagated arrivals the replay does
not have.

A replay that merged borrow-leg slew unconditionally would *overstate*
slew at the latch Q and downstream exactly where eval drops it. This
is real for data latches; it is vacuous for clock-gating latches,
whose Q net is a clock net — the `!clock_net[out_net]` guard means the
borrow leg never fires in either path (ibex's `prim_clock_gating` is
exactly this case).

Resolution — a **fallback predicate**, not an approximation: if any
`graph.latch_borrow` key `(d, q)` has `pin_nets[q]` on a non-clock
net, `sta_pin_slews` runs the existing eval tail (`run_sta_partitioned`
dispatch for exception SDCs, else `eval_sta`) on the same `prepared`
and reads its `state.slew_*`. The slew-mode prepare is still
sufficient: the clock block's only slew-visible effect (the
propagated-consumer `base_state` copy) is already gated on
`any_propagated` in both modes. No committed design trips the
predicate; the parity test covers the fallback on the
`sta_edge/latch_borrow` fixture.

(Note for the parity record: a *partitioned* eval unions per-class
slew maps — a `disabled_borrow` leg merges slew while the enabled
reached leg may drop it — so the fallback must dispatch through
`run_sta_partitioned`, not a single `eval_sta` run, to keep today's
`sta_pin_slews` output on exception SDCs.)

## 5. What the slew-only path can drop, provably

- `propagate_slew` + `propagate_clock` + propagated-consumer copy +
  `cell_driven_clock_nets` scan: needed iff `any_propagated` — their
  only slew-visible product is the `base_state` consumer copy, itself
  `any_propagated`-gated. `pulse_width_checks` never touch slew.
- `eval_sta`'s endpoint collection, `pulse_rows`, `assemble_sta_report`,
  `emit_sta_warnings`, `run_sta_partitioned`'s class machinery: slew is
  exception-invariant (§1), except through `latch_borrow` legs — fenced
  by the fallback (§4).
- Per-leg: arrival/dly/par/crpr/dom merges, `derate` multiplies,
  borrow-window math — slew output does not read them.
- `clock_parity`, `crpr_tags`, `LaunchSeed.{par,tag,dom}`,
  `clock_state`: arrival/report bookkeeping — kept *computed* inside
  the shared seed/prep code (cheap; removing them would fork the seed
  logic), unread by the replay.
