# grus-eda

A small physical-design toolkit for the open SkyWater sky130 PDK, written in Rust.
Each stage is checked against the established open-source tool that does the same
job (OpenSTA, OpenROAD) on the same inputs, and a stage counts as done only when
its numbers agree with that tool's.

> Private for now. It holds no design from other Grus repositories and must not:
> every test design here is public (see `AGENTS.md`).

## Why

The open-source flow (OpenROAD and the tools around it) is complete, but slow to
iterate with: a small block takes tens of minutes to place and route, and even a
quick estimate goes through a container and a Tcl session. Exploring a design asks
the same few questions over and over: does it meet timing, where is placement
dense, will routing be congested. This project answers those in seconds, and
hands over to the established tools for everything after.

## Goals

1. **Seconds, not minutes**: timing, placement and congestion estimates in seconds
   for designs up to about 100k cells on a laptop.
2. **One Rust binary**: no container and no Tcl. It reads the standard files
   (Liberty, LEF, DEF, gate-level Verilog) and writes DEF, so OpenROAD can take
   over from any stage. Rust for memory safety without a garbage collector, easy
   data parallelism (rayon) and a single static binary.
3. **Data-oriented design**: flat arrays per attribute (structure of arrays),
   graphs as index arrays (CSR) instead of pointers, no allocation per object.
   The timing graph is levelized so that every level is one parallel pass.
4. **Known fast algorithms, not new ones**:
   - timing: levelized propagation with NLDM table lookup, as OpenSTA computes it;
   - placement: analytical placement with an electrostatic density term solved by
     FFT (ePlace, DREAMPlace);
   - congestion: pattern and probabilistic routing on a grid of routing cells.
5. **Apple Silicon first**: all cores through rayon, then Metal for the kernels
   that pay for it (the density FFT, the wirelength gradient).
6. **Agreement before speed**: a stage gets faster only after it agrees with the
   reference tool, and the fast version must give the same answer as the simple one.

Not goals: sign-off, detailed routing, or beating OpenROAD on result quality.

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
