# e8-e2e — Test plan

New file: `crates/grus-core/tests/irdrop.rs`. It uses public
`grus_core` APIs and local test helpers; missing PDK or design inputs
panic with the expected path. There are no silent skips.

## Shared helpers

Implement these locally in the test file:

- `pdk_root()`, `sky130_hd()`, `design_dir(name)`, `require(path)` —
  the `test_support.rs` shape; integration tests cannot see that
  `#[cfg(test)]` module.
- `load_design(design)` — loads Liberty, tech LEF + cell LEF,
  `<d>.placed.def`, `<d>.sdc`, `<d>.irdrop.reference.json`, runs
  `sta_pin_slews` and `estimate_power` with no VCD/SAIF/parasitics.
- `solve_net(design, net_name, cell_power_w)` — finds the named
  `PdnNet`, builds the mesh, solves with embedded
  `supply_voltage_v` and the rail-to-rail `current_scale_v`, and
  returns `VoltageRow`s plus `PdnSolve`.
- `read_voltage_csv(path)`, `read_inst_power_tcl(path)`, and the
  comparison/statistics helpers from `harness.md`.

`load_design` must use the same default activity path as
`e8-solve`: `VcdActivity::default()`,
`ActivityOverrides::default()`, and no parasitics.

## Main gate

```rust
#[test]
fn test_irdrop_matches_reference() { ... }
```

Run both designs in one gate so a gcd failure does not prevent the
picorv32 measurement from printing. Collect per-design failures and
panic once at the end with the accumulated report.

For every design/net:

1. Recompute the N7 map.
2. Assert the emitted CSV bytes equal the committed
   `*.irdrop.voltage.ours.<net>.csv` (stale-artifact check).
3. Parse the embedded golden CSV and assert the identical
   `(Instance, Terminal)` set before any value comparison.
4. Print max/mean/median/P99 `|ΔV|`, scalar avg/worst drop or bounce
   deltas, total-power delta, and top-5 deviations.
5. Assert the scalar worst-deviation criterion:
   `|ours_worst − ref_worst| <= max(0.05 * |ref_worst|, 0.005)`.
6. Apply the design's per-pin policy below.

### gcd policy — strict PASS

Every joined row must satisfy:

```text
|V_ours − V_ref| <= max(0.05 * |V_ref|, 0.001)
```

and each net's scalar worst deviation must satisfy the `5% / 5 mV`
rule. The starting-point measurement passes all four assertions, so a
miss is a real implementation regression, not a pre-written floor.
If it misses, `test_irdrop_reference_power_axis` supplies the
decomposition; do not loosen gcd without naming the axis in
`docs/results.md`.

### picorv32 policy — measured floor with named cause

picorv32 always asserts:

- identical row sets;
- scalar worst-drop/bounce within `max(5%, 5 mV)`;
- VPWR per-pin voltages within the strict `max(5%, 1 mV)` rule;
- VGND per-pin voltages within the named floor:

```rust
const PICORV32_VGND_PIN_FLOOR_V: f64 = 0.0032;
```

The `3.2 mV` value comes from the committed starting-point max of
`3.164 mV` and the strict-rule count of 146 rows. After the
reference-power axis runs, update the constant's comment and
`docs/results.md` with the measured cause; until then it is only the
recorded floor, not a claim of correctness.

If the fresh reference-power measurement shows the mesh axis is the
residual, name that axis. If it shows the estimator axis, name the N7
feed and note the total/distribution split. If both exceed the floor,
increase only after the combined cause is named.

## Reference-power axis

```rust
#[test]
fn test_irdrop_reference_power_axis() { ... }
```

For each design:

1. Parse `<d>.irdrop.inst_power.tcl` and assert one finite row per
   `DesignDb` cell, no duplicates or unknown names, and a sum equal to
   `inst_power_feed.inst_feed_total_w` within `1e-12 W`.
2. Re-solve each net with that feed on the same mesh/options.
3. Assert the resulting row set is identical to the embedded golden.
4. Print three axes per net: `estimator`, `mesh`, `total`, each with
   `MapStats`, scalar deltas, and top-5 pins.

This test is diagnostic but still fail-loud on missing or malformed
inputs. It does not invent an axis tolerance before measuring it.

## Fail-proof ritual

After the gate passes, perform one deliberate break and restore:

- In the golden CSV lookup, substitute the `sta_per_instance` CSV for
  VGND while keeping everything else unchanged. The row set still
  matches, but gcd VGND must go red: the starting artifact comparison
  has ten rows above `max(5%, 1 mV)` and max `|ΔV|` about `1.06 mV`.
- Optionally also perturb one reference terminal name to verify the
  row-set assertion fires before any value comparison.

Restore and rerun green. `docs/results.md` records the mutation, the
red output, and the restore run.

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p grus-core --test irdrop -- --nocapture
```

For artifact provenance:

```bash
bash tests/designs/generate_irdrop_ref.sh gcd
bash tests/designs/generate_irdrop_ref.sh picorv32
git diff -- tests/designs/gcd tests/designs/picorv32 tests/designs/generate_irdrop_ref.sh
```

The picorv32 test requires the gitignored `picorv32.placed.def`; if it
is absent, run `tests/designs/picorv32/generate_ref.sh` first, exactly
as the other picorv32 tests require.

## Files

- `crates/grus-core/tests/irdrop.rs`
- `tests/designs/gcd/gcd.irdrop.inst_power.tcl`
- `tests/designs/picorv32/picorv32.irdrop.inst_power.tcl`
- `docs/results.md`
