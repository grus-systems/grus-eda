# Goal

Deepest remaining block: the DMP/NLDM inner loop
(`crates/grus-core/src/sta/dmp.rs`, `nldm.rs`). After
v0.3.6 it is ~75% of `power` wall and ~60% of `sta` wall
on jpeg — `sample` put ~80% of samples in
`sta::dmp`/`sta::nldm` (`rc_crossing` 1354, `exp`/`expm1`/
`log`/`log1p` ~450, `solve_dmp_with`/`pi_waveform_slew`/
`fit_ramp_duration` ~330), effective k≈5.4/10 partly
memory-bound. Both `sta.slew.walk` (771 ms jpeg) and
`sta.eval.walk` (~850 ms) run it per arc-leg.

## Measure first inside the ticket

Count before optimizing (a debug/ephemeral counter run is
fine — record numbers, don't commit counters):

1. **Call multiplicity**: how many `driver_response` /
   `solve_dmp_with` / `rc_crossing` calls per walk, and
   how many are **exact-repeat** argument tuples
   (bitwise-equal f64 keys) vs unique. STA re-evaluates
   the same table point across fanout legs and across the
   Ceff/bisect iterations — if exact-repeat share is
   high, bitwise-exact memoization is the win (output
   stays bit-identical by construction: same key → same
   stored f64).
2. **Bisection share**: fraction of `rc_crossing` calls
   that miss the closed-form path and pay Brent — if
   large, widen closed-form coverage or tighten the
   initial bracket (count first; if small, skip).
3. **eval_2d** site cost: bounds-check/table-index
   overhead vs actual lerp math (only if it shows in the
   counts).

Then implement the top measured item(s). Candidates in
rough order: exact-key memoization of
`solve_dmp_with`/`driver_response` results keyed on the
bit pattern of inputs (deterministic, `Vec`-indexed or
`HashMap<u64,..>` with `to_bits`), closed-form bracket
improvement, table lookup hardening. **No approximate
caching, no f64→quantized keys** — tolerance-keyed caches
change results.

## Equivalence

- bit-identical: all committed goldens byte-identical;
  `--json` output `cmp`-clean pre/post on jpeg+ibex
  placement-RC cells (existing strict tests are the
  broad proof);
- `RAYON_NUM_THREADS` 1/4/10 `cmp`-clean on jpeg
  (memoization must be thread-local or per-level-scoped —
  no cross-thread shared cache that serializes);
- fail-proof recorded.

## Measurement

`measure.sh` `sta` + `power` placement-RC/SPEF cells,
REPS=5 same machine; `docs/results.md` entry: counts that
motivated the mechanism (repeat share, bisection share),
before/after wall + phase table, achieved vs residual.

## Files

- `crates/grus-core/src/sta/dmp.rs`,
  `crates/grus-core/src/sta/nldm.rs`,
  `crates/grus-core/src/sta/analysis.rs` (cache scoping)
- `docs/results.md`, `docs/roadmap.md`,
  `kurena/tasks/perf-dmp-hotloop/`
- `Cargo.toml` (patch bump)
