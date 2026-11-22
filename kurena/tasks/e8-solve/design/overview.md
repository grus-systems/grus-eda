# e8-solve — Overview

E8 ticket 4 turns each ticket-3 `PdnMesh` into a solved DC voltage map.
The pipeline is:

```text
Liberty + LEF/DEF + SDC + activity
  -> sta_pin_slews
  -> estimate_power (extended with per-instance totals)
  -> per-CellId power W
PdnNet -> PdnMesh -> per-sink current A
  -> clamped sparse G matrix
  -> reduced-node solve
  -> node voltages -> sink rows
  -> *.irdrop.voltage.ours.<net>.csv + summary JSON
```

One solve runs per `+ USE POWER`/`+ USE GROUND` `PdnNet`, in
`DesignDb::pdn_nets` order. `PdnGraph` is not a solver input: its nodes are
coarse connectivity components; `PdnMesh` supplies the resistor network and
the ticket-2 sink set.

## Measured input facts (planning checkout)

Existing mesh census from the committed tests:

| design/net | nodes | sources | wire R | via R | sinks | violations |
|---|---:|---:|---:|---:|---:|---:|
| gcd VPWR | 244 | 2 | 197 | 46 | 384 | 0 |
| gcd VGND | 244 | 2 | 197 | 46 | 384 | 0 |
| picorv32 VPWR | 5,468 | 6 | 4,853 | 792 | 10,205 | 0 |
| picorv32 VGND | 5,489 | 6 | 4,874 | 792 | 10,205 | 0 |

Reference facts read from the committed goldens:

- `convention_mirrored` is `embedded` on both designs.
- `supply_voltage_v`: VPWR `1.8`, VGND `0.0`.
- `total_power_w` reported for each net: gcd `1.04e-3`, picorv32
  `1.44e-2`.
- CSV row counts: gcd `384` per net, picorv32 `10,205` per net.
- Header: `Instance,Terminal,Layer,X location,Y location,Voltage`.
- Row order equals DEF component order on both designs. The mesh sink
  order already inherits that order through `PdnConn::AllInstances` →
  DEF-cell scan → `graph.attachments` first appearance.
- Every reference `Layer` field is `li1`, the first routing layer in the
  sky130 tech LEF, even though the power-pin macro geometry and mesh
  attachment nodes are on `met1`. This is a PDNSim voltage-map
  presentation convention; mirror it instead of printing the attach
  node's layer.

The current `power` path loads Liberty, LEF/DEF, SDC, `sta_pin_slews`,
activity, then calls `estimate_power`. `PowerReport` currently carries
only group and total fields; this ticket adds the same decomposition per
instance. Planning measurements of the existing estimator (default
activity, zero-wire parasitics) give:

| design | N7 total now | golden embedded `total_power_w` | delta |
|---|---:|---:|---:|
| gcd | `2.849576e-4 W` | `1.04e-3 W` | `-7.550424e-4 W` |
| picorv32 | `8.127580e-3 W` | `1.44e-2 W` | `-6.272420e-3 W` |

Those deltas are expected. The solve injects the measured N7 per-instance
power and records the delta rather than normalizing it away. The
`sta_per_instance` golden is evidence only: the reference ticket measured
`set_pdnsim_inst_power` as additive, so it is not a substitution model to
copy.

## Key decisions

- **New sibling module**: `crates/grus-core/src/pdn/solve.rs`, re-exported
  from `pdn.rs`. Mesh construction remains unchanged; solve input is a
  validated `PdnMesh`.
- **Dirichlet elimination**: source mesh nodes are removed from the
  unknown vector and clamped to the exact `source_voltage_v`. Their
  conductance contributions move to the RHS. This keeps the reduced graph
  Laplacian symmetric positive definite when every free node reaches a
  source.
- **Direct sparse solve**: `nalgebra-sparse 0.12` `CscCholesky`, f64.
  The matrix is SPD by construction; a direct solve gives a deterministic
  answer without CG tolerance tuning and supports the residual criterion.
  `sprs-ldl` was rejected for LGPL surface; `nalgebra-sparse-linalg` and a
  hand-rolled CG would add convergence knobs without a correctness need
  at 5.5k nodes.
- **Current scale vs clamp are separate**: the clamp voltage comes from
  each net's `conventions.embedded.supply_voltage_v` (VPWR 1.8, VGND 0).
  Current magnitude uses the rail-to-rail scale (`VPWR − VGND` when both
  are in the reference, otherwise `PowerReport.voltage_v`). This avoids
  dividing by VGND's 0 V clamp while preserving `I = P / V_supply`.
- **Per-instance extension in `power.rs`**: `PowerReport.instances`
  parallels `DesignDb` cell order; each entry carries internal,
  switching, leakage, total, and the instance name. A cell's total is
  divided only among its sink pins on the same net (one on sky130).
- **Sign is net-kind dependent**: `POWER` subtracts sink current from
  the RHS (drop below clamp); `GROUND` adds it (bounce above clamp).
  Both nets use the same positive per-instance current magnitude.
- **`irdrop` CLI**: a dedicated `grus-eda irdrop` subcommand, matching
  the existing `power` argument/loading style plus `--reference` and
  `--out-dir`. Reusing `power` would hide the IR-drop output contract;
  a generic `pdn` flag would make supply/source semantics less clear.
- **Persistent sidecar**: the CLI always writes
  `<design>.irdrop.summary.ours.json` beside the two voltage CSVs. It
  carries residuals, scalar summaries, golden deltas, row counts, and
  solver metadata for the later e2e ticket.
- **Version**: additive core API + new CLI surface → workspace `0.7.2`
  to `0.7.3` under the repository's 0.x patch-for-features convention.

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
