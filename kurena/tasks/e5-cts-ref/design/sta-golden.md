# e5-cts-ref — propagated STA golden and `<d>.cts.sdc`

`<d>.cts.sta.reference.json` is the criterion-3 comparison target:
OpenSTA's per-endpoint values under `set_propagated_clock` on the
TritonCTS DEF — exactly what a later "our CTS" DEF is compared against.

## `<d>.cts.sdc` — new committed input, not `gcd.propagated.sdc`

`gcd.propagated.sdc`/`picorv32.propagated.sdc` were authored for the
routed-SPEF propagated comparison: they add `set_clock_latency 1.0`,
`set_clock_latency -source 0.2`, `set_clock_transition 0.1`,
`set_clock_uncertainty 0.1`. With a real clock tree those annotations
become noise the comparison has to explain (source latency shifts
slack, transition fights the propagated source slew).

`<d>.cts.sdc` is instead `<d>.sdc` verbatim plus one line:

```sdc
set_propagated_clock [get_clocks core_clk]
```

so the measured clock-pin arrival **is** the tree's own insertion
delay — nothing else is annotated. Both designs use `core_clk` on port
`clk`, so the file is uniform. ~250 B, committed.

## Golden schema

Identical block layout to `*.propagated.reference.json` (one named
mode), so a later `Rc::`-style arm consumes it unchanged:

```json
{
  "design": "gcd",
  "cts_placement_rc": {
    "wns_max_ns": ..., "tns_max_ns": ...,
    "wns_min_ns": ..., "tns_min_ns": ...,
    "endpoints": [
      {"pin": "...", "max_arr_ns": ..., "max_req_ns": ...,
       "max_slack_ns": ..., "max_slew_ns": ..., "min_arr_ns": ...,
       "min_req_ns": ..., "min_slack_ns": ..., "min_slew_ns": ...}
    ],
    "clock_pins": [
      {"pin": "...", "arr_rise_max_ns": ..., "arr_rise_min_ns": ...,
       "arr_fall_max_ns": ..., "arr_fall_min_ns": ...,
       "slew_rise_max_ns": ..., "slew_rise_min_ns": ...,
       "slew_fall_max_ns": ..., "slew_fall_min_ns": ...}
    ]
  }
}
```

- `endpoints` — `collect_endpoints_json` (8 fields/pin), same proc body
  as the STA goldens; the criterion-3 endpoint comparison.
- `clock_pins` — `collect_clock_pins_json` (`all_registers` + `is_clock`,
  the prop script's proc): per-sink propagated arrival/slew. This is
  also the criterion-1 data — skew is `max − min` over `arr_*` — so the
  golden carries the per-sink latencies, not just the aggregate.
- Parasitics: `set_wire_rc -layer met2` + `estimate_parasitics
  -placement` in-process (the `placement_rc` mode precedent) — no SPEF
  file involved. The mode is named `cts_placement_rc` for that reason.

## Why a separate process

`read_def`/`estimate_parasitics`/`find_timing_paths` state must not
inherit step A's parasitics — the step-9b precedent is one `openroad`
process per mode. Step C reads the **committed** `gcd.cts.def` from
disk, which is also the artifact's own proof: the DEF the golden was
made from is the DEF in the repo.
