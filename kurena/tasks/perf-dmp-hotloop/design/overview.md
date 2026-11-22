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

- **M1 — run-scoped exact-key memo of `driver_response`**
  ([`memoization.md`](memoization.md)): `thread_local!`
  `HashMap<DrvKey, (f32, f32)>`, key on bitwise inputs plus a
  per-`Context` epoch. Bit-identical by construction (same key →
  same stored value); no shared-mutable state crosses rayon
  threads. Covers every call site at once, including partition
  classes.
- **M2 — per-call memo of the `table` closure**: `c.to_bits()`-keyed
  memo inside `driver_response`, single-threaded, dedupes the
  intra-call `eval_2d` repeats. Bit-identical; small absolute win —
  ships only if the counts justify it.
- **M4 — `eval_2d` site hardening** (`nldm.rs`): only
  arithmetic-preserving edits allowed (e.g. bounds-check elision —
  *not* reciprocal-multiply, which perturbs f32 bits). Ships only if
  `eval_2d` shows in the counts.

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
