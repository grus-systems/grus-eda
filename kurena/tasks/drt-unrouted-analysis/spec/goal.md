# drt-unrouted-analysis: root-cause decomposition of the unrouted population

picorv32 strands 2016 of its ~6744 guided nets (gcd: 80 of 269). The
pinned census already gives per-pin kinds (`NoPath` 2225 /
`Unattachable` 1480 on picorv32) and wave-death postmortem blockers
(`l1Net` 1194, `l1Obs` 444, `l2Net` 286 dominant) — but these do not
answer the question every fix ticket needs: **would this pin route if
the other nets were not in the way?** The fix for a yes answer is
negotiation/ordering/rip-up work; the fix for a no answer is access
geometry, attach-ladder, or relaxed-via work. Picking wrong wastes a
multi-hour measurement cycle per attempt.

This ticket is analysis only — no routing behavior change. It
instruments the router to decompose the final unrouted population into
mutually-exclusive root-cause classes, runs the decomposition on gcd
and picorv32, and records the prioritized result in docs.

## Required decomposition

For every pin in the final (hard-iteration) unrouted report, classify
exactly one root cause:

1. **geometric** — the pin fails even when foreign `Net`/`Pin` marks
   are absent (OBS/`ObsPin`/off-track/guide-coverage only). Subclasses
   the census must separate: OBS-sealed attach, no in-region node
   (guide coverage), off-track pin, and the already-named
   single-nearest stranded-pocket class (the met3-bridge residual).
2. **negotiation** — the pin attaches and routes when foreign
   `Net`/`Pin` marks are absent, but fails in the real hard iteration.
   Subclass by what blocks in the real occupancy: foreign `Net` on the
   attach stub vs foreign `Net` in the wave path (the postmortem
   blocker layer/kind already exists — reuse it).
3. **residual named classes** — anything the above misses, named and
   counted, never silently bucketed.

The natural probe: re-attempt each finally-unrouted net's attach and
search against an occupancy rebuilt without foreign `Net`/`Pin` marks
(OBS/`ObsPin` retained — the `OccMode::Disabled` blocking semantics,
but per-net as a post-pass rather than a global mode). The driver
picks the implementation shape, subject to:

- routed output unchanged: gcd/picorv32 route digests and DEF goldens
  byte-identical; the decomposition is additive census/analysis output
  only;
- the probe's cost is proportional to the unrouted set, never a full
  re-route — state the measured added time in the results entry;
- if the probe is opt-in (config/flag) rather than default-on, the
  results entry says how it was invoked.

## Validation anchors

The decomposition must independently reproduce the already-named
counts as subclasses — picorv32's met3-bridge residual (43 nets), the
`region_escape` handling population, and the unguided io-port nets —
or explain any delta in the results entry. A classifier that cannot
reproduce known counts is not done.

## Deliverables

1. The instrumented census/classifier with test pins (including a
   prove-it-red arm: inject a known-class failure and check it lands
   in the right class).
2. The decomposition table for gcd and picorv32 in a dated
   `docs/results.md` entry: per-class counts, measured probe cost, and
   the resulting prioritized target list (which class, how many pins,
   which fix direction).
3. `docs/e6-drt.md` updated where the unrouted census is documented;
   `docs/status.md` last-updated line.

## Files

- `crates/grus-eda-core/src/drt/` — analysis implementation (driver
  picks the module)
- `crates/grus-eda-core/tests/drt.rs` — census/classifier pins
- `docs/results.md`, `docs/e6-drt.md`, `docs/status.md`

Out of scope: any change to routing decisions, cost models, iteration
structure, or emit; aes/ibex legs; the fix tickets themselves.
