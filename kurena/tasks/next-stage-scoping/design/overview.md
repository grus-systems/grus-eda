# Next-Stage Scoping — Overview

Doc-only scoping ticket. Map the remaining pipeline from the current state
(E0–E3 + N7 power all PASS) to a usable flow: inventory the candidate stages,
give each a measurable done-when against the reference tool, order them by
dependency, and size them. No code changes.

**Status 2026-11-22**: all four probes ran (`docs/results.md`
"next-stage-scoping"); `done-when.md`, `inventory.md` and `order.md` carry the
measured numbers. No stage lost its reference — P-IRD found PDNSim in the
image, P-DRC found Magic reading foreign routed DEFs, and P-GR confirmed both
`estimate_parasitics -global_routing` and `read_guides`-driven parasitics +
detailed routing.

## Deliverables

1. `kurena/tasks/next-stage-scoping/design/` — this design set (finalized with
   probe results where a criterion could not be pinned from the desk).
2. `docs/roadmap.md` — new candidate-stage list appended in the existing style
   (metric + tolerance + fixture, or an explicitly deferred decision).
3. `docs/results.md` — one dated entry per probe actually run, with command and
   output (same discipline as the N7/activity probes).

## Candidate stages (goal's naming kept)

| Label | Stage | Flow position |
|---|---|---|
| E4 | Global routing → route guides + guide-based parasitics | after placement |
| E5 | Clock tree synthesis (CTS) | after placement, before routing in a real flow |
| E6 | Detailed routing | after global routing |
| E7 | DRC | after detailed routing |
| E8 | IR drop (power-grid analysis) | any time post-PDN |

Numbering is a roadmap label, not flow order: E5 (CTS) physically precedes E4 in
the real flow, but the verification paths below make E4 the first implementation
candidate — see `order.md`.

## What already exists that biases the answer

- `congestion/` is not just an estimator: `gcell.rs` builds the same grid and
  track supply OpenROAD's `global_route` uses (verified to the integer on
  met2..met5), `rsmt.rs` decomposes nets into rectilinear Steiner segments,
  `pdr.rs` is a verified port of OpenROAD's `pdr::primDijkstra`, and
  `route.rs` already commits every segment to a concrete path
  (`segment_paths`) — it just throws the paths away.
- `sta/` consumes SPEF and reports slack matching OpenSTA on routed
  extractions (`gcd.rcx.spef`, `picorv32.rcx.spef` modes PASS). A routed or
  guided result has a proven downstream consumer.
- `readers/spef.rs` writes SPEF (N4); `write_def_full` writes DEF (N5).
- `power.rs` (N7) reports per-instance power — the current source estimate an
  IR-drop stage needs.
- Committed routed artifacts: `gcd.routed.def`, `gcd.drc.rpt`,
  `gcd.rcx.spef` (picorv32 equivalents gitignored, reproducible via
  `generate_ref.sh` step 5b).

## What is missing (repo gaps found by reading, not assumed)

- `DesignDb` keeps no signal-net routing: the DEF `NETS` section skips `+`
  attributes and `ROUTED`/`NEW` wiring; `VIAS`/`NONDEFAULTRULES` are skipped
  (`readers/def.rs`).
- The LEF reader skips `VIA` and `SPACING` statements (counted in
  `LefSkipReport`): no via definitions or design rules exist in `TechDb`.
- `DesignDb` has no cell/net insertion API — CTS buffer insertion is netlist
  mutation, which nothing does today (`floorplan::generate_floorplan` only
  adds rows/pins).
- No sparse linear solver in the dependency tree (needed for IR drop).
- `SPECIALNETS` parsing captures wire rects into `special_wire_rects` but
  drops via connections between layers — PDN vertical connectivity is lost.

## Approach

1. Inventory each stage (`inventory.md`): inputs/outputs, reusable modules by
   name, the OpenROAD/OpenSTA/Magic command that produces its reference.
2. Done-when per stage (`done-when.md`): metric + tolerance + fixture. Where a
   number cannot be set from the desk (guide format, CTS skew magnitude,
   PDNSim availability), a small container probe is scripted first
   (`probes.md`); the probe result goes into `docs/results.md` and decides the
   tolerance or whether the stage has a reference at all.
3. Order + sizing (`order.md`): dependency reasoning and a rough ticket count
   per stage.
4. `docs/roadmap.md` updated in place; tests and goldens untouched.

## Non-goals

No implementation. No new test designs. No golden files generated beyond what
the probes print to console/`results.md` (a probe may write scratch files under
`runs/canonical/`, which is gitignored).
