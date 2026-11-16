# F1: honest convergence report for global placement

## Goal
Make `NesterovResult` and the `place` CLI tell the truth: a run that hit
the iteration limit is reported as not converged (DEF still written,
exit 0), `iterations` counts gradient steps executed, and
`final_overflow` describes the returned coordinates when the limit is
hit. Design: `docs/history/review-fixes.md` F1 and the Review resolution
section; review: the design review record (not included) Q1.

## Files
- crates/grus-core/src/placement/nesterov.rs
- crates/grus-eda/src/main.rs
- docs/results.md
- Cargo.toml
- Cargo.lock

## Interface
- `NesterovResult` gains `pub converged: bool`. No other field changes
  name or type.
- Semantics (exact):
  - Loop breaks because the overflow checked at that iteration is
    `<= config.stop_overflow`: `converged = true`,
    `iterations = <gradient steps executed>` (gcd stays 178; the
    initial-overflow case is `iterations = 0`).
  - Loop finishes all `config.max_iters` steps without breaking:
    re-evaluate the density overflow once on the returned
    `cell_x_um` / `cell_y_um` (via `DensitySolver::compute_density_and_forces`
    with scratch force buffers), set `final_overflow` to that value,
    `converged = (final_overflow <= config.stop_overflow)`, and
    `iterations = config.max_iters`.
- CLI (`place` handler in `main.rs`): when `converged`, keep a
  `converged in {} iters` line; when not, print
  `did not converge (iteration limit reached) in {} iters, final_overflow={:.4}, ...`
  on stdout and an `eprintln!` `warning:` line with the same fact.
  Legalization, DEF writing and exit code 0 are unchanged.
- The existing gcd test's diagnostic print follows `res.converged`
  instead of printing "converged" unconditionally, and the test also
  asserts `res.converged`.
- `docs/results.md`: the E2 row of the summary table changes to state
  PASS on gcd only (picorv32 does not converge / is not yet an E2 test),
  in the wording of `docs/history/review-2026-11-11.md` F1. No measured number
  in `docs/results.md` changes.
- Bump `[workspace.package] version` patch in `Cargo.toml` (AGENTS.md
  Section 5) and refresh `Cargo.lock`.

## Acceptance
- `cargo test --release -p grus-core --lib placement::nesterov::tests::test_convergence_accounting`
  (the new PDK-free regression tests described under Notes)
- `cargo test --release -p grus-core --lib placement::wirelength placement::density` still passes
  (these are PDK-free; the gcd PDK tests fail loudly without a PDK and are
  not part of acceptance on a machine without one — say exactly which
  tests you ran)
- `cargo clippy --release -p grus-core -p grus-eda -- -D warnings`
- `cargo build --release -p grus-eda`

## Notes
- Read first: `AGENTS.md`, `docs/history/review-fixes.md` (F1 + Review
  resolution), the design review record (not included) Q1 and Q5.
- New regression tests, PDK-free, in `nesterov.rs` tests, named with the
  prefix `test_convergence_accounting`: build a synthetic design in code
  following the fixture pattern of
  `crates/grus-core/src/placement/wirelength.rs` tests
  (`test_synthetic_wa_wirelength_and_gradient` constructs `TechDb` and
  `DesignDb` directly); extend it with whatever
  `DensitySolver::new` and `WaWirelengthPrecomp::new` require (read
  their constructors). Two cases:
  1. `max_iters = 3` and a `stop_overflow` the run cannot reach
     (a negative value is acceptable in a test config): assert
     `!converged` and `iterations == 3`.
  2. `stop_overflow` large enough that the initial overflow already
     satisfies it: assert `converged` and `iterations == 0`.
  If a synthetic fixture genuinely cannot drive
  `run_nesterov_placement`, report blocked with the exact missing
  structure instead of falling back to the gcd/PDK test.
- Prove each new test can fail: temporarily force `converged = true`
  (or the old `iterations` accounting), show red, then restore. Report
  this in your summary.
- Do not change the Nesterov algorithm, step size, or lambda schedule.
  Do not change any golden or reference file. Do not read anything
  outside this repository. Do not push.
