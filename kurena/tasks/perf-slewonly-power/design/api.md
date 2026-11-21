# perf-slewonly-power — code shape

All edits live in `crates/grus-core/src/sta/analysis.rs` (plus its
`#[cfg(test)]` module). Public surface unchanged: `sta_pin_slews(design,
lib, sdc, parasitics) -> Result<Vec<PinSlew>>` keeps signature and name
— `main.rs:872`, `power.rs`, and `sta/mod.rs` need no edit. The doc
comment is rewritten ("runs the full STA flow" is no longer true).

## 1. `prepare_sta` gains a mode

```rust
fn prepare_sta<'a>(
    design: &'a DesignDb, lib: &'a LibDb, sdc: &'a Sdc,
    parasitics: Option<&'a NetParasitics>,
    for_slew: bool,
) -> Result<StaPrepared<'a>>
```

One line changes: the clock block's guard

```rust
if any_propagated || !graph.pulse_width_checks.is_empty()
```

becomes

```rust
if any_propagated || (!for_slew && !graph.pulse_width_checks.is_empty())
```

Everything else in `prepare_sta` is identical for both callers —
context + net-load selectors, `pi_models`, `wire_delays_per_net`,
`build_graph`, `topological_levels(false)`, the `io_pins` seed loop
(`port_seeds`, `slew_seeds`, `state_ports`), `base_state`,
`ideal_clock_parity` → `clock_domain`, ideal-domain consumer seeding,
`eval_launch`/`launch_pairs`, launch/latch/crpr seed tables.
`run_sta_detail` passes `false`; `sta_pin_slews` passes `true`.

`StaPrepared` is returned as today — the slew path simply does not read
`clock_parity`, `clock_state`, `crpr_tags`, or the `par`/`tag`/`dom`
fields of the seeds. (A `for_slew` *parameter* was chosen over splitting
`prepare_sta` into stage functions: the shared body is one tightly
ordered sequence and a flag at the single divergent gate is the smaller,
more reviewable diff. If the implementer finds the gate naturally
wants the whole `if`-block hoisted into a helper, that is also
acceptable — the constraint is one shared code path, not the flag's
shape.)

## 2. `eval_seed_state` extraction

Move `eval_sta`'s `sta.eval.seeds` block (lines ~3017–3071) verbatim
into:

```rust
fn eval_seed_state(
    prepared: &StaPrepared,
    allowed: Option<&HashSet<u32>>,
) -> Vec<PinTimingState>
```

`eval_sta` calls it under the existing `sta.eval.seeds` phase; the slew
path calls `eval_seed_state(prepared, None)` under a new
`sta.slew.seeds` phase. `allowed = None` produces the unfiltered merge
— identical slew contributions to every filtered run, and real
arrivals so seed `reached` is exact (semantics §3).

## 3. `slew_eval` — the replay walk

```rust
fn slew_eval(prepared: &StaPrepared) -> Vec<PinTimingState>
```

`state = eval_seed_state(prepared, None)`, then a level loop over
`prepared.levelized` with the same dual-mode discipline as
`sta.eval.walk` (`level.len() < 32` serial, else
`par_iter().filter_map(..).collect()` + ordered apply). Per pin, the
legs mirror `eval_pin` exactly, minus non-slew work:

- **Net-sink leg**: `!pin_is_driver[p]`, `pin_driver[p]` set,
  `!graph.clock_net[net_idx]`. Driver `reached` on either pol →
  `next = slew_sink(driver_state, wire, pin_idx, is_port)` — a
  whole-state **assignment**, matching eval's
  `next = sink_state_with_wire(..)` which replaces (not merges) the
  vertex state; the assignment also carries the driver's `arrival_max`
  markers, propagating `reached` per pol exactly as the `+= late`
  arithmetic would preserve it. Driver `!reached && slew_reached` →
  `merge_slew` of the `slew_sink` fields per pol, as eval does (slew
  only, no `reached`).
- **Arc legs**: `out_net` set, `!graph.clock_net[out_net]`; per
  `(src_pin, lib_arc)` in `incoming_to`: legs =
  `arc_legs(arc)` — keep the `graph.latch_borrow` lookup for leg
  selection (`[(RISE,RISE),(FALL,FALL)]` on borrow edges) so the leg
  set cannot drift from eval's even though uncovered borrow falls back
  before this point. Gate `src.reached(src_pol) ||
  slew_reached(&src, src_pol)`; `CombinationalRise/Fall` filter;
  `delay_tables` required. `driver_response` at `s_max` and `s_min`
  (skip second when equal) → `merge_slew(next, dst_pol, late_slew,
  early_slew)`; when `src.reached(src_pol)`, also set the
  `arrival_max[dst_pol]` reached marker.
- No seq legs (they are seeds, as in eval), no endpoint collection,
  no `pulse_rows`, no `SkipReport` merge.

Reuse `PinTimingState` as the vertex type: it lets `merge_slew`,
`slew_sink`, `eval_seed_state`, and the `reached`/`slew_reached`
predicates be shared unchanged; the marker lives in `arrival_max`,
which the replay never reads for value. Comment at the marker: *"the
replay keeps `reached` only — `arrival_max` holds a finite marker, not
a real arrival."*

## 4. `sta_pin_slews` body

```rust
pub fn sta_pin_slews(
    design: &DesignDb, lib: &LibDb, sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<Vec<PinSlew>> {
    let prepared = prepare_sta(design, lib, sdc, parasitics, true)?;
    // Arrival-coupled slew legs (semantics §4): replay cannot evaluate
    // the borrow window, so borrow edges landing off a clock net take
    // the full eval tail.
    let uncovered_borrow = prepared.graph.latch_borrow.keys().any(|&(_d, q)| {
        design.pin_nets[q as usize]
            .is_some_and(|n| !prepared.graph.clock_net[n.0 as usize])
    });
    let state = if uncovered_borrow {
        eval_state_full(&prepared)?
    } else {
        slew_eval(&prepared)
    };
    Ok(state.iter().map(|s| PinSlew {
        rise_ns: s.slew_max[RISE],
        fall_ns: s.slew_max[FALL],
    }).collect())
}
```

`eval_state_full(prepared) -> Result<Vec<PinTimingState>>` is a small
dispatcher used by the fallback, replicating `run_sta_detail`'s branch:
`run_sta_partitioned(prepared, false)` → `.1` when the SDC partitions
(`!exceptions.is_empty() || !path_delays.is_empty() ||
!clock_groups.is_empty()` — same condition), else
`eval_sta(prepared, None, &HashSet::new(), prepared.skipped.clone())`
→ `.state`. `run_sta_detail` keeps its own tail (`run_sta_partitioned`
assembles internally; the non-partitioned path assembles the outcome);
the fallback discards the report but keeps the partitioned union
semantics for slew.

## 5. Phase keys

| key | when it fires under `power.slews` now | under the slew path |
|---|---|---|
| `sta.prepare.{loads,pi_models,wire_delays,graph,levels,ports,launch}` | always | unchanged |
| `sta.prepare.clock.slew` / `.walk` | whenever `any_propagated \|\| pulse checks` — i.e. always on committed designs | only when `any_propagated` |
| `sta.prepare.clock.seed` | twice (propagated copy + ideal seed) | ideal seed only, unless propagated |
| `sta.eval.seeds` / `sta.eval.walk` / `sta.eval.endpoints`, `sta.assemble`, `sta.partition.*` | always / on exception SDCs | never (fallback excepted) |
| `sta.slew.seeds` / `sta.slew.walk` | — | new: seed replay + level walk |
| `power.slews` | envelope | unchanged name, smaller |

`timing.rs` needs no edit — `crate::timing::phase` takes the string.

## 6. Diff budget

~150–220 net lines in `analysis.rs`: `prepare_sta` signature + one-line
guard, `eval_seed_state` move (~55 lines relocated), `slew_eval` (~80),
`eval_state_full` split (~20), `sta_pin_slews` rewrite (~25), doc
comments. Plus the parity tests (~120). `run_sta`/`run_sta_quiet`/
`run_sta_inner`/`run_sta_with_state` are untouched call sites of
`run_sta_detail`.
