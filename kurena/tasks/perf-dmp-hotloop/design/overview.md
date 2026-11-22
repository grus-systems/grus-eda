# perf-dmp-hotloop — DMP/NLDM inner-loop speedup: overview

The deepest remaining block after v0.3.6 (`docs/results.md`
2026-11-22 perf-par-slew / perf-slewonly-power / perf-parsers
entries): `Context::driver_response` — the NLDM table read behind the
DMP effective-capacitance solve — runs per arc-leg inside every
levelized walk. On jpeg it is ~75% of `power` wall
(`sta.slew.walk` ≈ 771 ms inside `power.slews`) and ~60% of `sta` wall
(`sta.eval.walk` ≈ 850 ms; `sta.placement.jpeg` 1,320 ms,
`power.placement.jpeg` 720 ms on the M4 mini at v0.3.6). `sample` puts
~80% of samples in `sta::dmp`/`sta::nldm`: `rc_crossing` 1,354,
`exp`/`expm1`/`log`/`log1p` ~450, `solve_dmp_with`/
`pi_waveform_slew`/`fit_ramp_duration` ~330.

## Cost model (code-read, one `driver_response` call with a pi model)

`driver_response` (`analysis.rs:765`) builds `table(c)` — two
`eval_2d` calls — then runs `solve_dmp` + `driver_waveform_slew`:

- `solve_dmp` (`dmp.rs:500`): `is_lumped_bailout` (3 `table` evals),
  `residual(lo)`/`residual(hi)` guards, then a Brent solve on `c`
  (~10–20 `residual` evals). Each `residual(c)` eval: `table(c)` ×2 +
  `rd_of` → `delay_slope` (2 more `table` evals) + `fit_ramp` →
  `fit_ramp_duration` (Brent over `rc_slew`, ~10–15 iterations × 2
  `rc_crossing` each) + `rc_crossing(0.5)` + `pi_charge`
  (closed form since E3.6, ~10 transcendentals).
- `driver_waveform_slew` (`dmp.rs:569`): `is_lumped_bailout` again
  (3 evals, provably false after the solve), `delay_slope`,
  `fit_ramp_duration` again, `pi_waveform_slew` (2 Brent crossings of
  `v1`, each ~15 evals of 2–4 `exp`).

Total ≈ 500–1,500 transcendental evaluations plus ~60–90 `eval_2d`
lookups per `driver_response`. Within one call the only *exact*
repeats are `table` evals at `total`, `total ± 5%`, and the final
`ceff` (see [`counts.md`](counts.md)); across calls, repeat
opportunities come from bitwise-identical argument tuples.

## Why measure first

The spec requires the counts to pick the mechanism. The plausible
repeat pools, each counted separately ([`counts.md`](counts.md)):

1. **Cross-call exact repeats of `driver_response`** keyed on
   `(epoch, net, pol, delay_table ptr, slew_table ptr,
   in_slew.to_bits())`:
   - `sta`: `propagate_slew` (`sta.prepare.clock.slew`) +
     `propagate_clock` (serial, clock nets only) + `eval_sta`
     (`sta.eval.walk`), plus one `eval_sta` **per exception class**
     in `run_sta_partitioned` — input slews are merged
     unconditionally, so every class re-issues bitwise-identical
     queries.
   - `power` (`sta_pin_slews`): `slew_eval` alone on ideal-clock
     SDCs (the committed set), plus the `propagate_slew` +
     `propagate_clock` block when any clock row is propagated.
   - `driver_input_slew` recursion (launch seeds, depth ≤ 16) shares
     upstream pins across launches.
   - Identical cell instances only hit when `pi` **and** `in_slew`
     are bitwise equal; the count measures whether that share is
     large.
2. **Intra-call `table(c)` repeats**: `is_lumped_bailout` runs in
   both `solve_dmp` and `driver_waveform_slew`; `residual(hi)` and
   `rd_of(hi)` re-evaluate `table(total)` and `delay_slope(total)`
   — ~6–12 redundant `eval_2d` per call.
3. **`rc_crossing` bisection share**: closed-form hits only when the
   crossing lands after the ramp (`v(dt) <= level`); during-ramp
   crossings pay Brent. **Any change to the Brent path (bracket,
   coverage) shifts the returned root by ≤ tol → different f64 →
   `cmp`-dirty `--json`. It cannot ship under this ticket's
   bit-identical bar — counted and named as residual only.**

## Mechanisms (chosen by the counts — see [`counts.md`](counts.md))

**Counts landed 2026-11-22** (ephemeral counters, release build,
jpeg `sta`/`power` and ibex `sta` placement RC, patch kept at
`/tmp/dmp-counters.patch`, reverted):

- `driver_response` pi-path exact-repeat share `R = 0.0%` — every
  one of the 897,452 calls on jpeg `sta` is a bitwise-unique key
  (per-walk: seeds 8,750; clock.slew 448,726; eval.walk 439,976 —
  all unique; jpeg `power` slew.walk 448,726 all unique; ibex
  `sta` 156,617 all unique). **M1 is dead: a run-scoped memo would
  score literally zero hits.**
- Intra-call `table(c)` repeat share **21.6%** of `table` evals
  (jpeg `sta` 5,384,658/24,921,651; jpeg `power` 21.6%; ibex 21.8%)
  — and `table(c)` is ~all of `eval_2d` (49.9M of 49.9M), so the
  dedupe removes ~21.6% of `eval_2d` calls. Clears the M2 gate
  (≥ ~15%).
- `rc_crossing` brent share **43.7%** (jpeg `sta`: 49.36M calls,
  56.3% closed-form, 43.7% brent — all brent-accepted, zero
  40-iteration fallbacks). Unfixable under bit-identity → residual.
- `eval_2d` leaf share of `sample` on jpeg `sta.placement` ~6%
  (125/1979 samples). Below the M4 gate (~20%) → M4 not shipped.
- `fit_ramp_duration` 71.5% degenerate early-return; `pi_charge`
  100% discrete; `pi_waveform_slew` 100% two-pole Brent;
  `driver_waveform_slew` `is_lumped_bailout` early-return 27/897,452
  (the solve already bailed).

- **M1 — run-scoped exact-key memo of `driver_response`**:
  ~~selected~~ **rejected by the counts** — `R = 0.0%`, zero hits.
- **M2 — per-call memo of the `table` closure** — **chosen**:
  `c.to_bits()`-keyed memo inside `driver_response`, single-threaded
  `RefCell<Vec>` linear scan (~22 distinct `c` per call), dedupes the
  intra-call `eval_2d` repeats shared between `solve_dmp`'s
  `is_lumped_bailout`/`residual(hi)`/`rd_of` and
  `driver_waveform_slew`'s `is_lumped_bailout`/`delay_slope`.
  Bit-identical (pure function, exact key). Expected ceiling ≈
  21.6% of `eval_2d` ≈ ~1–2% of the walk — small, but it is the
  only redundancy the counts found.
- **M4 — `eval_2d` site hardening** (`nldm.rs`): rejected by the
  counts — ~6% leaf share, below the ~20% gate.

## Equivalence bar

- All committed goldens byte-identical; `sta --parasitics placement
  --json` `cmp`-clean pre/post on jpeg and ibex.
- `RAYON_NUM_THREADS` 1/4/10 `cmp`-clean on jpeg — thread-local
  caches make results thread-count-independent by construction;
  verified, not argued ([`verification.md`](verification.md)).
- Fail-proof recorded: a deliberately wrong memo (e.g. dropped key
  field) must take a test or the `cmp` red.
- **No approximate or quantized caching**: keys are `to_bits()`
  exact; a tolerance-keyed cache changes results and is out of
  bounds by spec.

## Documents

- [`counts.md`](counts.md) — ephemeral instrumentation plan, what
  to count, decision rule.
- [`memoization.md`](memoization.md) — M1 key/scope/threading
  design, M2 shape, rejected alternatives.
- [`verification.md`](verification.md) — `cmp` protocol, rayon
  matrix, fail-proof, committed test hook.
- [`measurement.md`](measurement.md) — `measure.sh` protocol,
  ceiling, `docs/results.md` skeleton.
- [`steps.md`](steps.md) — implementation order and gates.

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/dmp.rs`
- `crates/grus-core/src/sta/nldm.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `kurena/tasks/perf-dmp-hotloop/design/`
- `Cargo.toml`
- `Cargo.lock`
