# Goal

Parallelize `propagate_slew` — the measured rank-1 hotspot
(`sta.prepare.clock.slew`: 41–81% of `sta` wall, ~79% of
`power` wall; jpeg `sta` 6.0 s baseline, measured ceiling
~×2.2 → ~2.7 s). Baseline and rationale:
`docs/results.md` 2026-11-22 "perf-profile baseline" and
`kurena/tasks/perf-profile/design/plan.md`.

Mechanism (already proven in-tree): per-Kahn-level
`par_iter` compute + apply-after-collect, the same shape
`eval_sta`'s `eval.walk` uses. The pass merges every
incoming arc's slew at each vertex (see `propagate_slew`
in `crates/grus-core/src/sta/analysis.rs`, added v0.1.72)
— within a level, vertex writes are disjoint, so compute
in parallel, collect per-vertex results, apply in a
deterministic (graph-order) sequence. No scope inside
`par_iter` bodies if a shared accumulator would serialize
— collect owned results instead.

## Hard requirement: bit-identical output

Per `AGENTS.md` §2, the fast path locks to the serial
baseline via regression tests:

- every committed STA golden
  (`tests/designs/**/ *.reference.json`) must stay
  **byte-identical** — the existing strict integration
  tests are the equivalence proof;
- determinism: the parallel merge must not depend on
  thread/HashMap order (collect + deterministic apply);
- `GRUS_PHASE_TIMING` output still names
  `sta.prepare.clock.slew` for the parallel pass;
- run the fail-proof check: temporarily serialize the new
  loop (or revert it) to confirm a representative test
  still exercises it, then restore — record that the test
  actually fails if results diverge (e.g. perturb a slew
  by epsilon in a debug build? no — a cleaner proof: gate
  the parallel path off via env var in a debug-only test
  or record an intentional-break run in the commit notes).

## Measurement

Re-run the committed `kurena/tasks/perf-profile/measure.sh`
matrix (or at minimum: `sta` placement-RC + SPEF cells and
`power` placement-RC on `picorv32`, `aes`, `ibex`, `jpeg`,
same machine/release build/REPS=5) and add a
`docs/results.md` entry: before/after wall-clock table,
achieved speedup vs the plan's Amdahl ceiling, and why the
residual remains (level imbalance / serial tails). Keep
the before numbers from the perf-profile baseline entry —
same binary flags.

## Files

- `crates/grus-core/src/sta/analysis.rs` (`propagate_slew`
  level loop)
- possibly `crates/grus-core/src/timing.rs` if the phase
  scope needs a small rename — keep the printed key stable
- `docs/results.md`, `docs/roadmap.md` (perf line)
- `kurena/tasks/perf-par-slew/` (design notes)
- `Cargo.toml` (patch version bump in the same commit)
