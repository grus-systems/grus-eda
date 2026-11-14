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
2. **One Rust binary, decoupled stages**: no container and no Tcl. It reads the
   standard files (Liberty, LEF, DEF, and Yosys JSON or gate-level Verilog) and
   writes DEF. Every stage can also consume OpenROAD's intermediate files (such
   as a placed DEF) directly, so any stage can run standalone or hand over to
   OpenROAD at any boundary. Rust for memory safety without a garbage collector
   and a single static binary.
   - Today: no parallelism; the code uses no threads.
   - Target: data parallelism through rayon.
3. **Data-oriented design**: flat arrays per attribute (structure of arrays),
   graphs as index arrays instead of pointers, and `f32` arithmetic.
   - Today: the netlist and floorplan are SoA with CSR lists (`design_db.rs`);
     the placer and congestion estimate use `f32` on the CPU.
   - Target: the STA timing graph in CSR with no allocation per object (today
     it is `Vec<Vec<u32>>`, and the checks sit in a `BTreeMap`); the timing graph
     levelized so that every level is one parallel pass (today: Kahn order, no
     levels stored); `f32` on the GPU.
4. **Known fast algorithms, not new ones**:
   - congestion: RSMT (Steiner tree) decomposition followed by pattern
     (L/Z-shape) and probabilistic routing on a GCell grid;
   - placement: analytical placement with an electrostatic density term solved by
     FFT (ePlace, DREAMPlace), plus an Abacus legalizer with local refinement;
   - timing: propagation in Kahn topological order with NLDM table lookup, as OpenSTA computes it.
5. **Apple Silicon first**: all cores through rayon and NEON SIMD, then
   Accelerate / Metal for the kernels that pay for it (the density FFT, the
   wirelength gradient).
   - Today: CPU only, single-threaded, no SIMD intrinsics, no Accelerate or Metal.
   - Target: the list above.
6. **Agreement before speed**: a stage gets faster only after it agrees with the
   reference tool, and the fast version must give the same answer as the simple one.

Not goals: sign-off, detailed routing, or beating OpenROAD on result quality.

## Stages

| stage | what | checked against | done when |
|---|---|---|---|
| E0 | readers: Liberty (NLDM tables), LEF (technology and cells), DEF, Yosys JSON (preferred) and gate-level Verilog | OpenROAD and OpenSTA reports on the same files | cell, net, pin and row counts match on every test design |
| E1 | global-routing congestion estimate (usage ratio per GCell edge, from placed DEF) | OpenROAD `global_route` GCell usage map | per-edge usage-ratio map vs OpenROAD: Spearman ρ ≥ 0.60 and ≥ negative-control null + 0.15, hot-set (ratio ≥ 0.9) overlap ≥ 0.50; the gcd rank-order test stays as a smoke test |
| E2 | global placement (wirelength plus a density penalty) and Abacus legalization with local refinement | OpenROAD `global_placement` | OpenROAD's legalizer accepts the result; HPWL within 5% |
| E3 | static timing (edge-triggered DFFs first): arrival, slew, setup and hold — first without wires, then with placement-based RC estimation (HPWL/Steiner) and SPEF | OpenSTA | worst slack and every endpoint's arrival within 1% or 5 ps |

Out of scope: detailed routing, GDS output, DRC, LVS, and (initially) transparent
latch time borrowing. These stay with OpenROAD, OpenSTA, Magic and KLayout.

A GPU path (Metal) may come after a stage's CPU version agrees with the reference
tool. The GPU result must then equal the CPU result.

## Documentation

- [`docs/status.md`](docs/status.md) — Current status per stage: designs covered, tolerances actually used, known gaps
- [`docs/architecture.md`](docs/architecture.md) — Data-oriented design (SoA/CSR), decoupled pipeline, `f32` numeric model, and Apple Silicon strategy
- [`docs/e0-readers.md`](docs/e0-readers.md) — E0: File formats, Yosys JSON vs. Verilog, and Liberty binary caching
- [`docs/e1-congestion.md`](docs/e1-congestion.md) — E1: GCell grid, RSMT (Steiner tree) decomposition, and pattern/probabilistic routing
- [`docs/e2-placement.md`](docs/e2-placement.md) — E2: ePlace electrostatic placement, 2D DCT/FFT, and Abacus legalization
- [`docs/e3-sta.md`](docs/e3-sta.md) — E3: STA in Kahn topological order, NLDM extrapolation, placement-based RC, SPEF, DMP driver model; status and open tasks E3.4–E3.6
- [`docs/results.md`](docs/results.md) — Verification log against OpenROAD and OpenSTA
- [`docs/review-2026-11-11.md`](docs/review-2026-11-11.md) — Project review: findings to fix, and what replacing the existing tools would take

## Dependencies

None on other Grus repositories. The reference tools come from the LibreLane
container image (`ghcr.io/librelane/librelane:3.0.16`). Test designs are public
(OpenROAD-flow-scripts sky130hd designs, picorv32).
