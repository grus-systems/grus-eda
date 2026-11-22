# Goal

Plan rank 3 (`kurena/tasks/perf-profile/design/plan.md`):
`place.legal.pass1/2/3` is the largest non-parser serial
block — jpeg `place` 4.2 s, legalize ≈ 44% (pass1 421 /
pass2 619 / pass3 783 ms, v0.3.2 baseline). **Not rayon**
(ordered-move passes); the work is algorithmic:
candidate-set pruning, incremental HPWL deltas, band
scan-order tuning in `crates/grus-core/src/placement/
legalize.rs`. Ceiling estimate ×1.3 on `place` wall
(4.2 s → ~3.2 s).

## Equivalence bar (different from bit-identical)

An algorithmic speedup may change move order. Required
instead of byte-identity:

- `verify_legality` must pass on every committed design's
  legalized output (existing tests already gate this);
- per-design legalized **HPWL not worse** than the v0.3.4
  serial baseline on gcd/picorv32/aes/ibex/jpeg — record
  the before/after per-design deltas; a regression >0.1%
  must be named and justified, a regression that pushes
  any design's HPWL-vs-OpenROAD golden out of its 5%
  tolerance is a hard fail;
- deterministic: identical output across repeated runs
  (already single-threaded; record the check);
- all committed E2 goldens green.

## Measurement

Re-run the `place` cells of
`kurena/tasks/perf-profile/measure.sh` (REPS=5, same
machine); `docs/results.md` entry: per-design before/after
`place` wall + per-pass phase table
(`place.legal.pass1/2/3`, `place.legal.abacus`,
`place.legal.verify`), achieved vs ceiling, named
residuals. Keep `GRUS_PHASE_TIMING` keys stable.

## Files

- `crates/grus-core/src/placement/legalize.rs` (the three
  passes + their trial evaluators)
- `docs/results.md`, `docs/roadmap.md`,
  `kurena/tasks/perf-legal-passes/`
- `Cargo.toml` (patch bump)
