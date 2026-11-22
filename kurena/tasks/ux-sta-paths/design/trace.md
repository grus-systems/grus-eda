# ux-sta-paths — Trace extraction

How a `CriticalPath` is built from the finished STA state. Everything here lives
in `crates/grus-core/src/sta/analysis.rs`; the only public pieces are the report
types (`api.md`).

## 0. Inputs the tracer needs

The tracer runs inside `run_sta_detail` / `run_sta_partitioned`, where
`StaPrepared` is still alive:

- `prepared.cx` (`Context`) — `pin_name`, `port_name`, `driver_response`,
  `wire_delays`-cached nets, `sink_cap`, derates via `cx.sdc`.
- `prepared.graph` — `TimingGraph`: `edges`, `launches`, `latch_opens`,
  `latch_info`, `latch_borrow`, `gating_enables`, `clock_net`, `checks`.
- `prepared.levelized` — `incoming_to(pin)` (combinational reverse adjacency)
  and `pin_driver` (`(net_idx, pin_idx, driver)` per net sink).
- `prepared.wire_delays_per_net` — `[Vec<f32>; 2]` Elmore per sink per transition.
- `prepared.port_seeds` / `launch_seeds` / `latch_seeds` — the seeded
  per-transition `arr_max`/`arr_min` contributions (no recompute needed).
- `prepared.clock_domain` — `Sdc::clocks` row per pin (`u8::MAX` = none);
  names `capture_clock` / `startpoint_clock`.
- `prepared.clock_state` — propagated-clock arrivals (for launch-stage notes
  only; the launch clock pin's *seeded* arrival already sits in `state`).
- `state: &[PinTimingState]` — the timing state the endpoint's candidate was
  evaluated on: the single eval state (unpartitioned), `class_states[class]`
  (partitioned, tagged), or the merged union (`class == u16::MAX`).

Per endpoint the tracer gets `(pin: PinId, pols: &EndpointPolarities)` — see
`api.md` for the plumbing that keeps `PinId` attached.

## 1. Candidate selection (per corner)

For each corner in `{max, min}`:

1. Rank endpoints by `report.endpoints[i].max_slack_ns` (max corner) /
   `min_slack_ns` (min corner), ascending; `None` does not compete. Ties keep
   the existing name-sorted order.
2. Take the first `path_count`.
3. For each, pick the winning polarity `pol` with
   `pick_worst_polarity(&pols.{max|min})` — the same `Worst` whose `slack` is
   the ranking key.
4. `trace_path(prepared, state, ep, corner, pol, worst)`.

A `u16::MAX` `worst.class` traces against the state handed in; any other value
indexes `class_states` (partitioned path only). `worst.from_pin` (`u32::MAX` =
none) truncates the walk at a `-from get_pins` startpoint (`overview.md` §6).

## 2. The backward walk

`corner` selects the extreme: `arr_field = arrival_max`, `slew_field = slew_max`,
late derates (`sdc.derate(true, ...)`) for `max`; `arrival_min`, `slew_min`,
early derates for `min`.

Iterate from the endpoint pin, collecting stages in reverse, until a startpoint
fires. Per visited `(pin, pol)` the target is `state[pin].arr_field[pol]` and the
predecessor is the argmax (`argmin` on the min side) over the candidates below —
the same quantity `PinTimingState::merge` kept. Candidates within
`TRACE_EPS_NS` (`1e-6` ns — bit-exact is expected, the epsilon only catches a
genuine bookkeeping bug) of the target are legal; pick the best, first in
`incoming_to` scan order on exact ties.

**Sink pin** — `!pin_is_driver[p]` and `pin_driver[p] = Some((net, idx, drv))`,
guarded by `!clock_net[net]` exactly as `eval_pin`:
- one candidate: `state[drv].arr_field[pol] + wire_delay`, where `wire_delay`
  replays `sink_state_with_wire`: `wire_delays_per_net[net][pol][idx]` (0 when
  `None`), `* derate(late, DERATE_NET, false, pol)` — or `* LN_2` first when the
  driver is a top-level port (`pin_cells[drv].is_none()`), the measured
  input-port wire rule. Emit stage `{pin, edge: "net", delay_ns: wire_delay,
  arrival_ns: target, slew_ns: state[pin].slew_field[pol]}`, continue at
  `(drv, pol)`.

**Driver pin** — any of:
- *Combinational arcs*: for `(src, lib_arc)` in `levelized.incoming_to(pin)`,
  for each `(src_pol, dst_pol)` leg of `arc_legs(arc)` with `dst_pol == pol`,
  honouring the `CombinationalRise`/`CombinationalFall` filters and requiring
  `delay_tables(arc, dst_pol)` non-`None`: candidate =
  `state[src].arr_field[src_pol] + delay * derate(late, DERATE_CELL, false, src_pol)`
  where `delay` is `driver_response(out_net, dst_pol, delay_table, slew_table,
  state[src].slew_field[src_pol])`. The `out_net`/`!clock_net` gate is the same
  `pin_nets[p]` check eval applies.
- *Latch-borrow arcs* (`graph.latch_borrow` contains `(src, pin)`): max corner
  only — the min side contributes nothing (`INFINITY` in eval). Replay the eval
  formula verbatim: read `latch_info`, skip when the enable is unreached,
  unclocked, or `src.arr <= open - unc`, else the candidate is
  `cap(state[src].arrival_max[src_pol]) + late_delay * derate` with
  `cap(arr) = min(arr, open + pulse - unc - setup)` — same accessors
  (`constraint_table` on `latch_info.setup_arc`, `unc_worst`,
  `clock_domain[enable]`). Under a partitioned run also consult the class's
  `disabled_borrow`: a disabled `(src, pin)` is not a candidate.
- *Launch seeds*: every `prepared.launch_seeds`/`latch_seeds` with
  `q == pin` contributes `seed.pairs[pol].arr_field` — `LaunchSeed` already
  stores the merged pair, so no `eval_launch` replay. A winning launch seed
  ends the walk (§3).
- *Port seeds*: every `prepared.port_seeds` with `pin == p` contributes
  `seed.per_pol[pol].arr_field`. A winning port seed ends the walk (§3).
- On an arc win: emit `{pin, edge: "cell_arc", delay_ns: delay * derate,
  arrival_ns: target, slew_ns: state[pin].slew_field[pol]}` and continue at
  `(src, src_pol)` — `src` is a sink, so the next iteration takes the net
  branch.

**Dead end**: `arr_field` reached but no candidate within `TRACE_EPS_NS` —
stop, set `CriticalPath.note = "untraced at <pin>: no predecessor within
tolerance"` and keep the stages collected. Also stop (same note) if the walk
revisits a pin — defensive; the graph is a DAG by construction.

## 3. Startpoints (stages are emitted, then the whole list is reversed)

- **Register launch** (`launch_seeds` win): emit `{q, edge: "cell_arc",
  delay_ns: arr_field(q) - state[clk].arr_field[edge], ...}` then
  `{clk, edge: "clock", delay_ns: clk_arrival, arrival_ns: clk_arrival,
  slew_ns: state[clk].slew_field[edge]}` where `edge = LaunchArc.edge`
  (`LaunchArc` lookup via the seed's `q`). `startpoint` = the cell name
  (`pin_name(clk)` minus `/PIN`), `startpoint_clock` =
  `sdc.clocks[clock_domain[clk]].name` — `None` when `clock_domain == u8::MAX`
  (the `t2_u2` unclocked-launch shape; `note` records it).
- **Latch open-through** (`latch_seeds` win): identical, but `clk` is the
  `LatchOpen.enable` pin, `edge = open_pol`, and the enable stage's
  `edge` is `"latch_open"` — it is a data-clock pin, not a flop clock pin.
- **Input port** (`port_seeds` win): emit `{pin, edge: "port",
  delay_ns: arrival, arrival_ns: arrival, slew_ns}` — one stage folding the
  whole external launch (clock latency + edge offset + `set_input_delay` +
  driving-cell/wire delay); the doc states the composition rather than faking
  a split. `startpoint` = port name. `startpoint_clock`: the port's
  `set_input_delay` anchor rows for this corner — resolve the winning anchor
  (the row whose `clock_row_latency + edge_offset(0) + delay + drv` equals the
  seeded arrival within eps); `launch_domain` maps virtual rows to the design
  name; several anchors still tying → first row + `note`.
- **`-from get_pins` pin start** (`worst.from_pin != u32::MAX`): emit
  `{from_pin, edge: "pin", delay_ns: 0, arrival_ns: state.arr_field}` as the
  terminal stage; `startpoint` = that pin's name; `startpoint_clock` = `None`;
  `note` = `"set_*_delay -from pin segment"`.

## 4. Endpoint & path-level fields

- `endpoint` = `pin_name(ep)`; `corner` = `"max"`/`"min"`.
- `data_arrival_ns` = `worst.arr`, `required_ns` = `worst.req`,
  `slack_ns` = `worst.slack` — verbatim the reported check values (overview
  §4 on why `arr` can sit one period off the last stage's arrival).
- `capture_clock` = `sdc.clocks[clock_domain[check.clk]].name` for register
  checks and `clock_domain[gating_enable.clk_pin]` for gate enables (the
  check's own capture row, `u8::MAX` → `None`). Output ports: the
  `set_output_delay` anchor row whose composed required time equals
  `worst.req` within eps; first surviving anchor + `note` when ambiguous.
  Delay-check leaves (`req = N`, no clock) → `None` + `note`.
- `slack`/`required`/`data_arrival` are *not* recomputed — the endpoint
  machinery's numbers are the report; the trace only explains the arrival.

## 5. Stage ordering & counts

Stages are collected endpoint→launch, then reversed so `stages[0]` is the
clock/port/pin anchor and `stages.last()` is the check pin — the OpenSTA
`report_checks` order (`clock`, `clk->Q`, alternating `net`/`cell_arc`,
final `net` into `D`/port). A reg→reg path with K combinational cells yields
`2 + 2K + 1` stages.

## 6. Partitioned-run specifics

- `Worst.class` tags the signature class whose `class_states[ci]` produced the
  candidate. Stamped where winners fold: the `best_max`/`best_min` fold
  (~line 5018), the leaf-delay fold (~5347), `u16::MAX` in
  `collect_endpoints` (single state) and in the deferred-endpoint union
  compose (~5095, candidates built on the union by construction).
- `class_states` is already materialized; the tracer runs before it drops.
  The per-class `disabled_borrow` set (computed from `sigma` at ~4500) must be
  kept in a `Vec` parallel to `class_states` for the borrow replay gate.
- A launch/port seed filtered out of the winning class still appears in the
  candidate set (seeds are stored unfiltered); if such a seed coincidentally
  argmax-ties the target the trace follows it — physically identical arrival,
  accepted as a named residual, not re-filtered.

## 7. Cost

`path_count` × path length × `incoming_to` fan-in `driver_response` calls —
NLDM/DMP lookups identical to eval's per-arc cost, a few dozen per path.
Negligible; no extra eval pass, no new tables.

## Files

- `crates/grus-core/src/sta/analysis.rs`
