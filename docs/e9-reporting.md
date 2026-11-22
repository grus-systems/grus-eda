# E9 — Reporting & usability surface

Reporting features layered on the finished STA state (E3). This document is
written ticket by ticket; it currently covers the `critical_paths` block of
`grus-eda sta --paths N` (ticket `ux-sta-paths`).

## 1. `critical_paths` JSON schema

`StaReport` carries `critical_paths: Vec<CriticalPath>`, serialized with
`skip_serializing_if = "Vec::is_empty"`: **the key is absent** when `--paths`
is 0 (the default) or no trace was produced — never `[]` or `null`. `--paths
0` output is byte-identical to a build without the flag.

```jsonc
"critical_paths": [
  {
    "corner": "max",                    // "max" (setup/late) | "min" (hold/early)
    "startpoint": "_384_",              // cell name, port name, or pin name
    "startpoint_clock": "core_clk",     // omitted when no clock anchors the launch
    "endpoint": "resp_msg[15]",         // the EndpointReport.pin this trace explains
    "capture_clock": "core_clk",        // omitted for unclocked/delay-check leaves
    "data_arrival_ns": 2.3348222,       // Worst.arr — the reported check arrival
    "required_ns": 4.0,                 // Worst.req
    "slack_ns": 1.6651778,              // Worst.slack — equal to the endpoints[] row
    "stages": [
      {
        "pin": "_384_/CLK",             // inst/pin or bare port name
        "edge": "clock",                // see stage semantics below
        "transition": "rise",           // "rise" | "fall"
        "delay_ns": 0.0,                // this stage's own contribution
        "arrival_ns": 0.0,              // cumulative arrival at the pin
        "slew_ns": 0.0                  // recorded slew; null where unreached
      },
      {
        "pin": "_384_/Q",
        "edge": "cell_arc",
        "transition": "rise",
        "delay_ns": 0.31582928,
        "arrival_ns": 0.31582928,       // prev arrival + this delay, exactly
        "slew_ns": 0.06360613
      }
    ],
    "note": "..."                       // omitted unless something needs saying
  }
]
```

Library entry points: `run_sta_with_paths` / `run_sta_quiet_with_paths`
(`grus_core::sta`), each returning `StaReport` with `critical_paths` filled
when `path_count > 0`. `run_sta`/`run_sta_quiet` keep their signatures and
emit no traces.

## 2. Selection semantics

For each corner (`max` then `min`), the tracer ranks endpoints by their
**reported check slack** — `EndpointReport.max_slack_ns` /
`min_slack_ns` — ascending, ties keeping the report's name order, and traces
the first `N`. One trace per endpoint: `stages` is the single worst path into
that endpoint's winning polarity.

This is deliberately not OpenSTA's `-endpoint_path_count N` (N paths *per*
endpoint). To reproduce the OpenSTA golden's path *set*, `report_checks` is
run with `-group_path_count N -unique_paths_to_endpoint -endpoint_path_count 1`
(see `tests/designs/gcd/generate_paths_ref.sh`).

The ranking key is the reported slack, not `worst_*_slack_ns`: a multi-anchor
output port's `worst_*` folds every path-group check while the stored
`EndpointPolarities` only carries the reported (last-group) candidate —
ranking by `worst_*` could pick a check the trace cannot reconstruct. On such
ports the ranking and the `endpoints[]` row name the same check (the
`t2_clkcor2` last-group rule); the other anchors' traces are the named
approximation — not emitted.

## 3. Stage semantics

`stages` is ordered launch anchor -> endpoint (the `report_checks` order):
a single anchor stage, then alternating `cell_arc` (cell output) and `net`
(cell input) stages, ending on the `net` stage of the check pin / output
port.

| `edge` | meaning |
|---|---|
| `clock` | register clock pin — path start; delay = arrival = the seeded (propagated) clock arrival |
| `latch_open` | latch enable pin of an open-edge launch (a data-clock pin, not a flop CK) |
| `port` | top-level input port launch — one stage folding clock latency + edge offset + `set_input_delay` + driving-cell/wire delay |
| `pin` | `set_*_delay -from get_pins` segment-check startpoint (non-clock pin) |
| `cell_arc` | combinational or clk->Q arc (`driver_response` delay, derated) |
| `net` | net traversal driver -> sink (Elmore; `ln2 * Elmore` for a port driver; 0 under zero-wire) |

- `arrival_ns` is the **recorded physical pin arrival** in the engine's
  timing state — stage `i`'s `arrival_ns` equals stage `i-1`'s `arrival_ns`
  plus stage `i`'s `delay_ns` exactly (the borrow-arc stage carries
  `target - src_arrival`, so the chain stays exact even when the borrow
  window clamps the propagated arrival).
- `data_arrival_ns` is the **reported check arrival** (`Worst.arr`) — on a
  hold or cross-domain path it can sit a period off the last stage's
  `arrival_ns` (the launch-parity realign shift), and on a `set_*_delay`
  segment check it is the segment arrival, not the absolute one. The stage
  chain always shows physical times.
- The launch stage stops at the register clock pin / latch enable — the
  clock network itself is not traced (OpenSTA's `-format full` boundary).
- `slew_ns` is the recorded pin slew of the corner (`null` where the state
  marks the transition unreached).

## 4. How a trace is produced (and what that guarantees)

The engine stores merged per-pin extremes, not predecessor edges. The tracer
replays the predecessor each pin *would* have merged — `driver_response`,
`sink_state_with_wire`, `arc_legs`, derates, the same deterministic f32
arithmetic as `eval_sta` — and picks the candidate matching the recorded
extreme within `TRACE_EPS_NS` (1e-6 ns). On every modelled path the match is
bit-exact; the epsilon only catches a genuine bookkeeping bug.

Under timing exceptions the run is partitioned into signature classes with a
timing state each; the endpoint's winning `Worst` carries the class tag and
the trace replays against **that class's state** (plus the class's disabled
borrow arcs), never the merged union — a false-path-covered source cannot be
"re-won" by the trace.

## 5. Honesty rules (`note`)

Anything the walk cannot reconstruct is said in `CriticalPath.note`, not
silently dropped or invented:

- `untraced at <pin>: no predecessor reproduces the recorded arrival` —
  reached state with no candidate inside eps (also fired, defensively, on a
  revisited pin and on `pin sits on a clock network`).
- `launch on no declared clock (edge-0 launch)` / `latch enable on no
  declared clock` — `startpoint_clock` is `null`.
- `segment check: path starts at the set_*_delay -from pin` — the `pin`
  anchor.
- Ambiguous port/capture anchors keep the first surviving row and name it
  here.

A trace that stops early keeps the stages it has — read `note` first.

## 6. Corner model

A single-`--lib` run emits up to N `max` and up to N `min` traces from one
timing state (a corner with no checks contributes nothing). Under
`--lib-hold`, each corner's run traces its own state and
`merge_corner_reports` keeps the setup report's `max` paths plus the hold
report's `min` paths — the same source rule as the slack fields.

## 7. Human-readable form

The same traces print on stdout after the STA summary:

```text
paths (worst 3 per corner):
  [max] 1. _384_ -> resp_msg[15]   slack +1.665178  (core_clk -> core_clk)
           0.000000 +0.000000 clock     _384_/CLK ^
           0.315829 +0.315829 cell_arc  _384_/Q ^
           0.315902 +0.000073 net       _211_/A_N ^
           ...
           2.334822 +0.001312 net       resp_msg[15] ^
```

`arrival`, `+delay`, edge kind, pin, `^`/`v`; `note:` lines follow the path
they belong to.
