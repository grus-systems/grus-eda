# e8-solve — Implementation order

One focused implementation sequence. The additive `PowerReport` field,
solve module, dependency, CLI, artifacts, and workspace `0.7.2 → 0.7.3`
bump belong in the same feature commit under the repository's 0.x
versioning rule.

## Steps

1. **Expose per-instance N7 power.** Add `InstancePower` and
   `PowerReport.instances` in `power.rs`; accumulate leakage/internal in
   the existing cell loop and switching in the driver-cell loop. Add the
   length/order/sum test. Run the existing power tests to confirm the
   aggregate report did not move.
2. **Solver skeleton + dependency.** Add `nalgebra-sparse = "0.12"` to
   workspace dependencies and `grus-core`; create `pdn/solve.rs`;
   re-export from `pdn.rs`. Implement `SolveOptions`, `PdnSolve`,
   `SolveSummary`, `VoltageRow`, and report structs from `api.md`.
   Workspace compiles before solving logic lands.
3. **Current injection.** Implement `sink_currents_a`: power length and
   finite/nonnegative validation, per-cell sink counts, even splitting,
   `I = P / current_scale_v`. Add unit tests for split, invalid input,
   and `uninjected_power_w`.
4. **Reduced matrix assembly.** Implement node→free-index map, resistor
   stamping into `BTreeMap`, source contribution to RHS, POWER/GROUND
   sink sign, and `mesh.validate()` gate. Unit tests: analytic VPWR,
   analytic VGND, parallel resistors, dangling/no-source errors.
5. **Direct solve + summary.** Convert to `CscMatrix`, run
   `CscCholesky`, expand free voltages into dense node order, assign
   exact source clamps, compute sink voltages, drops, currents,
   residual absolute/normalized, `max_source_error_v`. Add zero-power,
   exact-clamp, and residual tests.
6. **Rows and CSV.** Implement `voltage_rows` (mesh order, first
   routing layer, `pin_pos_dbu`) and `write_voltage_csv` (`{:.4}`,
   `{:.6}`, fixed header/escaping). Add deterministic-bytes and
   first-five-column tests against the committed golden CSVs.
7. **CLI.** Add `IrdropArgs` and `Commands::Irdrop`; reuse the `power`
   loading flow for Liberty/LEF/DEF/SDC/slews/activity/parasitics;
   parse the reference JSON; compute `current_scale_v`; build/solve all
   POWER/GROUND nets; write CSVs and
   `<design>.irdrop.summary.ours.json`; print per-net summaries.
8. **Real-design tests.** Add the gcd and picorv32 contract tests from
   `tests.md`. Iterate until row order and first five fields match the
   embedded goldens exactly and residuals are in range.
9. **Run CLI + artifacts.** `cargo build -p grus-eda --release`; run
   the two `/usr/bin/time -l` commands in `cli.md`. Commit the four
   voltage CSVs and two summary JSONs (each is below the 1 MB rule).
10. **Fail-proof + docs.** Perform the chosen break→observe→restore
    ritual. Update `docs/results.md` with commands, metrics, deltas,
    residual, runtime, peak RSS, and the observation; update
    `docs/roadmap.md` so E8 says the solve landed and e2e comparison is
    next. Bump `Cargo.toml` to `0.7.3` and include `Cargo.lock`.
11. **Final verify.** Run:

    ```bash
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    ```

## Acceptance → step map

- gcd both nets, exact golden row set and deterministic order → steps
  4–8.
- Scalar summary and N7-vs-golden total/current delta → steps 1, 3, 5,
  7, 10.
- `‖GV−I‖∞` and ≥6 significant digits + method/crate justification →
  steps 2, 4, 5, 10.
- Exact source clamp → steps 4–5, 8.
- picorv32 both nets, runtime + peak RSS → steps 8–10.
- Zero-power uniform map unit test → step 5.
- workspace test/clippy and version bump → steps 10–11.
- `docs/results.md` commands/deltas/residual/runtime → step 10.

## Risks and named fallbacks

- **`PowerReport.instances` total drift**: the switching attribution is
  the only cross-cell term; keep it in the same driver-cell rule as the
  group report and test the sum. Do not rebalance to match OpenSTA.
- **Cholesky fill**: picorv32 is measured. If `CscCholesky` memory or
  runtime is unreasonable, retain the reduced assembly/API and swap the
  backend to Jacobi-preconditioned CG; the commit must then record the
  measured reason and the new convergence criterion. This is a fallback,
  not the planned path.
- **Layer field surprise**: all committed goldens use `li1`. The test
  compares that exact field and will immediately show if a different PDK
  violates the first-routing-layer assumption.
- **Reference shape drift**: the CLI fails loudly on a missing
  `conventions.embedded` block or scalar field; it never substitutes a
  hard-coded supply.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/solve.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-core/Cargo.toml`
- `crates/grus-eda/src/main.rs`
- `Cargo.toml`
- `Cargo.lock`
- `docs/results.md`
- `docs/roadmap.md`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/gcd/gcd.irdrop.summary.ours.json`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/picorv32/picorv32.irdrop.summary.ours.json`
