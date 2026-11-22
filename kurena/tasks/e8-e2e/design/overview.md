# e8-e2e — Overview

E8 ticket 5 is the stage gate. It compares the `e8-solve` voltage maps
against the committed PDNSim `embedded` goldens on `gcd` and
`picorv32`, on both `VPWR` and `VGND`, and records the result in the
same style as the E4/E5 end-to-end gates.

The gate is deliberately a comparison harness, not a second solver:
the implementation reloads Liberty/LEF/DEF/SDC, recomputes the N7
per-instance feed, rebuilds each `PdnMesh`, solves it, and compares the
resulting per-instance-terminal voltage rows with the golden CSVs.

## Starting point (measured on the committed artifacts)

`e8-solve` already committed the ours-side maps and summaries. A
planning-stage comparison of those files gives:

| design | net | max \|ΔV\| | median \|ΔV\| | P99 \|ΔV\| | rows above `max(5%, 1 mV)` |
|---|---:|---:|---:|---:|---:|
| gcd | VPWR | 0.541 mV | 0.141 mV | 0.541 mV | 0 / 384 |
| gcd | VGND | 0.501 mV | 0.128 mV | 0.501 mV | 0 / 384 |
| picorv32 | VPWR | 3.667 mV | 0.079 mV | 1.531 mV | 0 / 10,205 |
| picorv32 | VGND | 3.164 mV | 0.075 mV | 1.196 mV | **146 / 10,205** |

The scalar worst-drop/bounce deltas already satisfy `max(5%, 5 mV)` on
all four nets: gcd `−0.410 mV` (VPWR) and `−0.334 mV` (VGND); picorv32
`−3.498 mV` (VPWR) and `−2.889 mV` (VGND). The likely outcome is
therefore: gcd passes the full done-when gate; picorv32 passes the
scalar criterion but needs a named per-pin floor on VGND. The
implementation must still measure from code, not trust this table.

`docs/status.md` currently says every per-pin delta is inside the
roadmap tolerance. That is true for the two VPWR maps and both gcd maps
but not for picorv32 VGND under the literal `max(5% of V_ref, 1 mV)`
rule; this ticket corrects the record.

## Key decisions

- **Rust-side comparator in an integration test**, not
  `tests/designs/compare_irdrop.py`. Everything needed is already
  public on `grus-core`; keeping the comparator beside the test avoids
  a second language path and lets the test deserialize the committed
  JSON and CSVs directly. No production API is added for comparison.
- **New `crates/grus-core/tests/irdrop.rs`**. The repository normally
  keeps tests in `#[cfg(test)]` modules, but this ticket's spec asks
  for an integration test under `crates/grus-core/tests/`; the file
  carries its own small `design_dir`/`require` helpers because
  `test_support` is not exported to integration tests.
- **The comparison object is recomputed, then checked against the
  committed ours CSV too.** The gate uses fresh `voltage_rows` output,
  while a byte comparison of `write_voltage_csv` with each committed
  `*.irdrop.voltage.ours.<net>.csv` prevents a stale checked-in map
  from drifting away from the implementation.
- **Absolute-voltage join and sign rule.** Rows join on
  `(Instance, Terminal)`; row-set equality is asserted before any
  voltage comparison. VPWR rows are absolute voltages near `1.8 V`;
  VGND rows are absolute node potentials near `0 V`, so both use the
  same `|V_ours − V_ref|` path. For reporting, `VPWR` deviation is a
  drop below `1.8 V`, while `VGND` deviation is a bounce above `0 V`.
- **Reference-power feed becomes a committed generated artifact.** The
  existing script already creates `runs/irdrop/inst_power.tcl`, but
  `runs/` is gitignored. This ticket makes the script also publish
  `<design>.irdrop.inst_power.tcl` and its size/SHA/row metadata in
  `<design>.irdrop.reference.json`. Both feeds stay under the 1 MB
  rule and are produced by the same committed container run.
- **Decomposition is linear and diagnostic.** Re-solving the same mesh
  with the regenerated OpenSTA per-instance powers gives the
  mesh/solver axis. Comparing that map to the N7 map gives the
  estimator axis. These axes are always printed for both designs after
  the feed artifact lands, so a gcd miss is decomposable without a
  special rerun.
- **picorv32 gets a named floor, not a hidden pass.** The gate records
  and bounds its measured VGND residual (`3.2 mV` floor on the
  committed starting point) while naming the cause after the
  reference-power axis is measured. A floor is never widened silently.
- **No version bump planned.** The planned surface is test-only plus
  generated artifacts/docs: no `grus-core` API and no `grus-eda` CLI
  surface lands. Workspace version stays `0.7.4`. If implementation
  chooses a public comparator or CLI feed flag instead, this decision
  must be revisited and `0.7.5` is the patch-level consequence.

## Files

Implementation is expected to touch:

- `crates/grus-core/tests/irdrop.rs`
- `tests/designs/generate_irdrop_ref.sh`
- `tests/designs/gcd/gcd.irdrop.inst_power.tcl`
- `tests/designs/gcd/gcd.irdrop.reference.json`
- `tests/designs/picorv32/picorv32.irdrop.inst_power.tcl`
- `tests/designs/picorv32/picorv32.irdrop.reference.json`
- `tests/designs/README.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
