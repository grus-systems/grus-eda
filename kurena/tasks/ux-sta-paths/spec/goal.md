# Goal

E9 ticket 1 (highest-value item from
`docs/history/run-2026-11-22-large-design.md` item 1):
critical-path detail in `sta` output — a `--paths N`
flag and `critical_paths` JSON block dumping the
worst-N startpoint→endpoint traces with cell/pin
names and per-arc delays, so a deep-slack outlier is
actionable without external netlist spelunking.

## Work

- `grus-eda sta --paths N` (default 0 = off): for each
  corner (max/setup, min/hold), take the N endpoints
  with the worst slack — OpenSTA
  `report_checks -endpoint_path_count N` semantics —
  and emit a path trace per endpoint.
- Each trace: `startpoint` (input port with its
  launch clock/edge, or the launching register's
  clock pin and Q pin), `endpoint`, the ordered stage
  list `launch → ... → endpoint` with per-stage
  `pin`, `edge` (`cell_arc`/`net`/`port`),
  `delay_ns`, `arrival_ns`, `slew_ns` where
  available, `required_ns`, `slack_ns`, and the
  launch/capture clock names when known.
- Extraction: walk the timing graph backward from
  the check pin through the incoming `ArcEdge`/net
  edge whose propagated arrival matches the recorded
  worst arrival (the engine's own propagation is the
  oracle — recomputing the winning predecessor per
  vertex at trace time is fine, it is deterministic).
  Stop at input-port arrival anchors and register
  clock pins. Handle the existing modelled kinds:
  combinational cell arcs, net wire delays, input
  port wire delay, latch open-through (`LatchOpen`),
  clock-gate enables, propagated-clock launch
  segments. Record honestly (in the stage list or a
  `note` field) when a stage kind cannot be traced
  rather than silently dropping it.
- JSON: `critical_paths: [{ corner, startpoint,
  startpoint_clock, endpoint, capture_clock,
  slack_ns, required_ns, stages: [...] }]` added to
  `StaReport` (only when `--paths > 0`; empty/absent
  otherwise — pick one and document it).
- Stdout: a compact human-readable path dump to
  stderr/stdout next to the existing summary (the
  review wanted at-a-glance); exact format is the
  implementer's choice — keep it small.
- Golden comparison: emit OpenSTA's worst-N for the
  same input in the pinned container
  (`report_checks -path_delay <min|max>
  -endpoint_path_count N` — probe the exact flag
  surface first; `sta`/`openroad` in
  `ghcr.io/librelane/librelane:3.0.16`) and record
  the per-stage pin sequence + delays. Compare our
  path *pin sequence* and per-stage delays against
  that golden on `gcd` (committed small golden
  fixture under `tests/designs/gcd/` or a
  `sta_edge`-style directory) — stage sequences
  should match where our arrivals match; per-stage
  delay diff bound `max(5 ps, 1%)` with any
  divergence named.
- Fail-proof: corrupt the trace (e.g. drop the
  winning-edge selection so it picks the first
  incoming edge) → path test must go red; restore.
- `docs/results.md` entry + `docs/status.md` E9 row
  update; version bump per convention (new
  user-facing surface → minor, matching the
  v0.4.0/v0.5.0 precedent).

## Done when

`sta --paths N` emits correct worst-N path traces
whose pin sequences and stage delays match OpenSTA's
`report_checks` golden on gcd within the stated
bound, the JSON block is schema-documented, and a
deliberate corruption makes the test red.

## Files

- `crates/grus-core/src/sta/analysis.rs` (trace
  extraction + report fields)
- `crates/grus-eda/src/main.rs` (`--paths` flag,
  stdout dump)
- `tests/designs/**` (golden fixture if a new one is
  needed — prefer existing `sta_clocks`/`gcd`
  inputs)
- `docs/results.md`, `docs/status.md`,
  `docs/e9-reporting.md` (new, or a section added to
  an existing doc)
