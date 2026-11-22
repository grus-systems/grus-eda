# perf-profile — measure-first performance baseline: overview

Measure-first performance ticket: establish where `grus-eda` wall-clock time
actually goes on the largest committed designs, then land a ranked optimization
plan. **No optimization implementation.** The deliverables are the phase
instrumentation, the measured tables in `docs/results.md`, and the plan in
[`plan.md`](plan.md).

## Why now

Every stage agrees with its reference tool, so `README.md`'s ordering ("a stage
gets faster only after it agrees") is unblocked. The last profiled baseline is
`v0.1.51`–`v0.1.55` (`docs/results.md` 2026-11-16, Phase 2): jpeg STA ~1.1–1.3 s,
jpeg legalization ~1.5 s on this same machine. Since then ~50 feature versions
landed (multi-clock, exception partitioning, latch borrowing, pulse-width
checks, activity propagation, `power`), each adding eval classes and walks the
old profile did not cover. No committed measurement says where v0.3.1 spends
its time.

## What lands

1. **Phase instrumentation** — a disabled-by-default, stderr-only phase timer
   at the natural boundaries of `sta`, `power`, `congestion`, `place`
   ([`instrumentation.md`](instrumentation.md) mechanism,
   [`phases.md`](phases.md) boundary inventory). Existing stdout lines —
   including the reader `elapsed=` lines — stay byte-identical.
2. **Measurement runs** — release build, ibex/aes/jpeg/picorv32/gcd, the
   command×mode matrix, reps and machine context of
   [`measurement.md`](measurement.md). `docs/results.md` gains the wall-clock
   table, per-phase breakdown, ranked hotspots, and a peak-RSS note.
3. **Optimization plan** — [`plan.md`](plan.md) commits the classification and
   the ticket skeleton; the implementation stage fills measured shares, Amdahl
   ceilings, and the recommended ticket order, and `docs/roadmap.md` gains the
   pointer.

## Invariants

- **No functional change to outputs.** Timing is additive instrumentation;
  every golden stays byte-identical. `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` pass; the phase lines
  are provably off when the gate is unset (instrumentation.md §Verification).
- **Honest measurement** (`AGENTS.md` §2): release build only; reps reported as
  min/median/max; any cold-vs-warm cache claim times a cold path only after
  deleting the Liberty cache; machine context recorded.
- **No heavyweight tooling install.** Symbol-level confirmation uses macOS
  `sample` (the tool the E3.6 and Phase 2 profiles already used);
  `samply`/`cargo-instruments` are absent and are not installed.
- Patch version bump (`0.3.1` → `0.3.2`): additive internal instrumentation,
  no behavior change.

## Documents

- [`instrumentation.md`](instrumentation.md) — the timing mechanism and why
  this shape.
- [`phases.md`](phases.md) — phase-name inventory per command with code anchors.
- [`measurement.md`](measurement.md) — run protocol and the `docs/results.md`
  entry skeleton.
- [`plan.md`](plan.md) — optimization-plan framework and follow-on ticket
  candidates.
- [`steps.md`](steps.md) — implementation order and gates.

## Files

- `crates/grus-core/src/timing.rs`
- `crates/grus-core/src/lib.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-core/src/activity.rs`
- `crates/grus-core/src/placement/nesterov.rs`
- `crates/grus-core/src/placement/legalize.rs`
- `crates/grus-core/src/congestion/route.rs`
- `crates/grus-eda/src/main.rs`
- `kurena/tasks/perf-profile/measure.sh`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
