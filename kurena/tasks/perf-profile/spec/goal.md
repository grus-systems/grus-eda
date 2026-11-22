# Goal

Measure-first performance ticket: establish where
`grus-eda` wall-clock time actually goes on the largest
committed designs, then land a ranked optimization plan.
**No optimization implementation** — the deliverable is the
numbers table and the plan, matching this repo's
measure-first discipline.

Context: every stage now agrees with its reference tool, so
README's ordering ("a stage gets faster only after it
agrees") is unblocked. Readers already print `elapsed_ms`
per file (main.rs); what is missing is per-phase timing
*inside* the heavy passes (STA walk, parasitics, slew
propagation, power resolution, placement iterations) and a
measured profile on the big designs.

## What lands

1. **Phase instrumentation** (small, flag-gated or stderr —
   not in the hot path of normal output): coarse
   `Instant` timers at the natural phase boundaries of the
   heaviest commands — `sta` (parasitics build, slew
   propagation, arrival/required walk, check evaluation,
   endpoint collection), `power` (activity resolution,
   per-cell internal, switching), `place` if it is a CLI
   stage (per-iteration or per-subphase aggregate, not
   per-iteration lines). Record what instrumentation shape
   landed.

2. **Measurement runs** on the biggest committed fixtures —
   `ibex` (largest STA design, ~3.7k endpoints), `aes`,
   `jpeg`, `picorv32`, `gcd` — for the commands that matter
   (`sta` SPEF + placement-RC modes, `power`, `congestion`,
   `place` if exposed). Multiple reps; report median or
   min-median-max honestly. Machine context recorded (the
   mini's cores/RAM, release vs debug — release only).
   If a real profiler is feasible on the mini
   (`samply`/`cargo instruments`/`xctrace` availability —
   check first, do not install heavyweight tooling without
   noting it), use it for the top hotspot's symbol-level
   confirmation; otherwise the phase timers are the record.

3. **`docs/results.md` entry**: per-design wall-clock table,
   per-phase breakdown, the top hotspots ranked by share,
   and a note on memory if observed (peak RSS from
   `/usr/bin/time -l` or equivalent — cheap to record).

4. **Optimization plan** committed in the design docs +
   roadmap pointer: which phases dominate, which are
   embarrassingly parallel (per-net loads, per-cell
   internal power, endpoint checks, per-level fanin —
   Kahn levels parallelize within a level), which are
   serial/memory-bound, and the recommended ticket order
   with expected ceilings (Amdahl math per phase share).
   `rayon` is already a workspace dependency pattern —
   name where it plugs in.

## Files

- `crates/grus-eda/src/main.rs` and/or
  `crates/grus-core/src/...` (phase timers only)
- `docs/results.md`, `docs/roadmap.md` (perf note)
- `kurena/tasks/perf-profile/` (design docs + plan)
- `Cargo.toml` / `Cargo.lock` (only if a dependency or
  instrumentation lands)
