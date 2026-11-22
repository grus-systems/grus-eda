# e8-solve — Test plan

Tests live in `pdn/solve.rs`'s `#[cfg(test)]` module plus the per-instance
assertions in `power.rs`'s existing test module. They reuse
`pdn.rs`/`mesh.rs` fixtures and `test_support::{sky130_hd, design_dir,
require}`. Missing PDK/design/reference input remains a loud failure.

## Unit tests — no reference needed

1. **Analytic VPWR drop.** Build a minimal `PdnMesh` in the test:
   source node → `R1` → sink node → `R2` → leaf node, one sink at the
   leaf. With `source_voltage_v = 1.8` and sink current `I`, expect
   `V_leaf = 1.8 - I*(R1+R2)` within `1e-12` and the leaf drop
   `I*(R1+R2)`. This catches resistor stamping and POWER sign without
   relying on OpenROAD values.
2. **Analytic VGND bounce.** Same topology with `source_voltage_v =
   0.0` and `PdnUse::Ground`; expect `V_leaf = I*(R1+R2)`. This is the
   dedicated ground-sign check.
3. **Source clamp exact.** On the existing `pdn_design()` mesh, assert
   `node_voltages_v[s] == source_voltage_v` by ordinary equality for all
   `mesh.sources`, and `summary.max_source_error_v == 0.0`. Exactness is
   intentional because the values are assigned, not factored.
4. **Zero-power uniform map** (acceptance sanity): pass an all-zero
   `sink_currents` vector. Assert every node and every sink voltage is
   exactly `source_voltage_v`, `average_drop_v == 0`,
   `worst_drop_v == 0`, `total_current_a == 0`, and `residual_inf == 0`.
   Run once as POWER and once as GROUND.
5. **Parallel-resistor stamping.** A two-node source/free mesh with two
   parallel `R`s must produce the same voltage as one `R/2` resistor.
   This guards the `BTreeMap` accumulation rule.
6. **Input validation.** `sink_currents` wrong length, negative,
   NaN/infinite; `source_voltage_v` NaN; `current_scale_v <= 0`; and a
   mesh with no sources all return `Err`. A mesh with a dangling free
   node reports `validate()` violations before factorization.
7. **Deterministic rows.** Call `voltage_rows` twice on a fixture solve
   and assert equality of instance/terminal/layer/x/y/node sequence;
   write a CSV twice and compare bytes.

`power.rs` addition:

- In an existing low-cost power test (or a dedicated fixture test),
  assert `report.instances.len() == design.num_cells()`, names are in
  DEF cell order, each `total_w` equals its three components, and
  `sum(instances.total_w)` equals `report.total_w` within `1e-15 W`.
- No golden instance-power values are invented; the reference's
  `inst_power_feed` remains comparison evidence, not an assertion source
  for this ticket.

## Synthetic mesh solve

Using `pdn_design()` + `build_mesh` (7 nodes, 5 wire resistors, 1 via,
2 sinks, 1 source):

- Set distinct positive powers for `u1`/`u2`; solve POWER and GROUND.
- Assert no voltage is above VPWR's clamp or below VGND's clamp.
- Assert `worst_drop_v >= average_drop_v >= 0` and both are nonzero.
- Assert `total_current_a` equals the sum of sink currents, including
  the per-cell/per-net split calculation.
- Assert `residual_rel_inf <= 1e-10`.
- Solve through `solve_mesh` and assert `uninjected_power_w == 0`
  because both fixture cells attach.

## gcd real-design test

`test_gcd_solve_embedded_row_contract`:

1. Load sky130 Liberty/TLEF/LEF, `gcd.placed.def`, `gcd.sdc`.
2. Run `sta_pin_slews`, then `estimate_power` with default activity —
   the same input regime as the committed `generate_irdrop_ref.sh`
   (no VCD/SAIF).
3. Deserialize `gcd.irdrop.reference.json` directly; extract per-net
   embedded `supply_voltage_v`, `total_power_w`, `csv_rows`, and scalar
   drops.
4. Build and solve both meshes using `cell_power_w` from
   `report.instances`; `current_scale_v = abs(VPWR - VGND)` read from
   JSON.
5. For each net:
   - `summary.sinks == embedded.csv_rows`;
   - all source voltages exactly `source_voltage_v`;
   - `max_source_error_v == 0`;
   - `residual_rel_inf <= 1e-10`;
   - `total_current_a` finite and `uninjected_power_w == 0`;
   - generate `VoltageRow`s and compare header/order/first five fields
     against `gcd.irdrop.voltage.<net>.embedded.csv` verbatim;
   - print the full `SolveSummary`, N7-vs-reference power/current delta,
     and solve elapsed time under `--nocapture` for `docs/results.md`.

The test does **not** gate voltage values against PDNSim tolerance: this
ticket records deltas; `e8-e2e` owns the full comparison gate.

## picorv32 real-design test

`test_picorv32_solve_embedded_row_contract` repeats the same contract:

- both nets solve;
- `sinks == 10,205` from JSON;
- ordered first-five-column comparison;
- source clamp exact;
- `residual_rel_inf <= 1e-10`;
- print elapsed per-net solve time for `docs/results.md`.

Process-level runtime/RSS comes from the release CLI commands in
`cli.md`, not this unit test.

## CSV writer tests

- Write rows to a `Vec<u8>`; assert the first line is exactly
  `Instance,Terminal,Layer,X location,Y location,Voltage`.
- For the gcd emitted rows, compare the entire first five columns to the
  golden CSV line-by-line.
- Write twice and compare byte equality.
- Feed an instance name containing a comma in a synthetic row and assert
  RFC-style quoting — not exercised by committed fixtures, so the test
  claims only that narrow escaping behavior.

## Fail-proof ritual

Unit-level checks are permanent. During implementation, perform one
real-test break → observe → restore and record it:

- Break option A: flip the GROUND RHS sign, run the gcd VGND contract
  test, observe bounce/drop assertion or first-five-column test fail.
- Break option B: stop adding `g * V_s` to the RHS for source-connected
  resistors, run the gcd VPWR contract test, observe excessive residual
  or wrong analytic voltage.
- Restore the line and rerun the test green. `docs/results.md` records
  what was broken, the observed failure, and the restoring run.

A wrong-layer or wrong-order CSV regression is also caught by the exact
ordered first-five-column comparison; no additional break is needed.

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p grus-core --lib pdn::solve -- --nocapture
cargo build -p grus-eda --release
/usr/bin/time -l target/release/grus-eda irdrop ...   # cli.md, both designs
```

Record in `docs/results.md`:

- all commands verbatim;
- each net's N7 power/current and `ours - golden` delta;
- average/worst drop or bounce and delta vs embedded;
- `residual_inf` and `residual_rel_inf`;
- source clamp max error;
- picorv32 runtime and peak RSS from `/usr/bin/time`;
- the intentional break/restoration observation.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/solve.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/gcd/gcd.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/gcd/gcd.irdrop.summary.ours.json`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vpwr.csv`
- `tests/designs/picorv32/picorv32.irdrop.voltage.ours.vgnd.csv`
- `tests/designs/picorv32/picorv32.irdrop.summary.ours.json`
- `docs/results.md`
