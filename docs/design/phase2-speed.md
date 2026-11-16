# Phase 2 continuation — STA and legalizer speed (pure Rust)

Design for the remaining "Further CPU / Data-Oriented Speedups" item in
`docs/status.md` (Remaining Follow-ups 1). The maintainer measured first; the
premises in `status.md` are corrected below where measurement disagreed.
Implementation tickets live in [`phase2/`](phase2/). This round is implemented
by the maintainer directly (no the delegation tool delegation).

## 1. Baseline (measured 2026-11-16, Apple Silicon, `cargo build --release`, `v0.1.52` + instrumentation)

`grus-eda sta` on `tests/designs/jpeg/jpeg.placed.def` (61,569 cells, 4,467 endpoints):

| mode | total | breakdown (inside `run_sta_detail`) |
|---|---|---|
| zero wire | ~34 ms | — |
| placement RC | ~4.1–4.7 s | parasitic extraction 149 ms, `pi_models` 4–8 ms, `wire_delays` 3–11 ms, `build_graph`+`topological_levels` 15–19 ms, **level loop 3.9–4.4 s** |
| SPEF | ~4.2–4.6 s | same shape |

Level loop internals: **448,726 `driver_response` calls** (~9.8 µs each), 33
levels, all `>= 32` pins so every level already runs on `rayon`. `sample` shows
the time under `solve_dmp_with` -> residual -> `fit_ramp_duration` -> `rc_slew`
-> `rc_crossing`, plus `driver_waveform_slew` -> `fit_ramp_duration` +
`pi_waveform_slew` (an 80-iteration bisection, two crossings per iteration, each
crossing evaluating `v1` with several `exp_m1`). `pi_charge` is already the
closed form from E3.6.

`grus-eda place` on `jpeg.floorplan.def`: `legalize_placement` ~4.0 s. Per
refinement round (4 rounds): **Pass 1 ~80 ms, Pass 2 ~355 ms, Pass 3
~510–700 ms**; steps 1–3 (Abacus) + verifies are small. The `status.md` premise
"speed up Pass 1 (`equal_width_swap_pass_fast`, 3.86 s)" is **wrong**: Pass 1 is
~2% of legalization. Pass 3 dominates, then Pass 2 — same lesson as E3.4
(measure before writing the premise).

## 2. Tickets

| Ticket | Scope | Expected effect |
|---|---|---|
| [phase2/01-dmp-inner-solvers.md](phase2/01-dmp-inner-solvers.md) | `sta/dmp.rs`: replace the three nested bisections (`rc_crossing_bisect`, `fit_ramp_duration`, `pi_waveform_slew` crossing) and the `solve_dmp` residual loop with bracketed Brent/secant iterations at the same stop tolerances | level loop 3.9 s -> ~0.5–1 s on jpeg (target: STA total ~1–2 s) |
| [phase2/02-sta-call-audit.md](phase2/02-sta-call-audit.md) | Measure `driver_response` call composition (arc type, polarity pair, slew variant); dedupe only what is provably identical; re-check the `< 32`-pin sequential threshold now that per-call cost is known | small; mostly a measurement ticket that may fold into 01 |
| [phase2/03-legalize-pass3.md](phase2/03-legalize-pass3.md) | `placement/legalize.rs` Pass 3: replace the per-cell `sort_unstable` of `other_xs` with `select_nth_unstable` (identical median), plus audit Pass 2/3 inner scans; exact incremental caches allowed per the Pass 1 `NetExtrema` precedent | legalize <= ~2.5 s on jpeg, `final_hpwl_dbu` bit-identical on all 5 designs |
| [phase2/04-timinggraph-csr.md](phase2/04-timinggraph-csr.md) | `sta/analysis.rs`: flatten `TimingGraph.edges_from`, `LevelizedGraph.edges_to`/`levels`, `checks: BTreeMap` into CSR / sorted arrays | structural (Goal 3); measured build+levels is only ~15 ms, so this is last and optional |

## 3. Rules for this round

- Same as `AGENTS.md` §2: fast path must give the same answer as the simple
  baseline. For ticket 01 the locked bound is the existing `dmp.rs` golden
  regression test (~1e-5) plus every STA endpoint test; for ticket 03 the
  placement must stay bit-identical (`final_hpwl_dbu` per design).
- Prove each change can fail (`AGENTS.md` §7 rule 7) and record before/after
  timings in `docs/results.md`.
- No OpenSTA source reading; OpenROAD source may be read but not copied.
- Version bump in the same commit as each functional change (`0.1.52` ->
  `0.1.53` patch for perf/internal refactors).
