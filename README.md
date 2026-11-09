# grus-eda

A small physical-design toolkit for the open SkyWater sky130 PDK, written in Rust.
Each stage is checked against the established open-source tool that does the same
job (OpenSTA, OpenROAD) on the same inputs, and a stage counts as done only when
its numbers agree with that tool's.

> Private for now. It holds no design from other Grus repositories and must not:
> every test design here is public (see `AGENTS.md`).

## Stages

| stage | what | checked against | done when |
|---|---|---|---|
| E0 | readers: Liberty (NLDM tables), LEF (technology and cells), DEF, gate-level Verilog (yosys output) | OpenROAD's reports on the same files | cell, net, pin and row counts match on every test design |
| E1 | static timing: arrival, slew, setup and hold, first without wires, then with SPEF | OpenSTA | worst slack and every endpoint's arrival within 1% or 5 ps |
| E2 | global placement (wirelength plus a density penalty) | OpenROAD `global_placement` | OpenROAD's legalizer accepts the result; HPWL within 5% |
| E3 | global-routing congestion estimate (overflow per routing cell) | OpenROAD `global_route` | overflow ranks the test variants in the same order |

Out of scope: detailed routing, GDS output, DRC and LVS. These stay with
OpenROAD, Magic and KLayout.

A GPU path (Metal) may come after a stage's CPU version agrees with the reference
tool. The GPU result must then equal the CPU result.

## Dependencies

None on other Grus repositories. The reference tools come from the LibreLane
container image (`ghcr.io/librelane/librelane:3.0.16`). Test designs are public
(OpenROAD-flow-scripts sky130hd designs, picorv32).
