# grus-eda

A physical-design research toolkit for the open SkyWater sky130 PDK, written in Rust.
Each stage is checked against the established open-source tool that does the same
job (OpenSTA, OpenROAD, Magic, or PDNSim) on the same inputs, and a stage counts
as done only when its numbers agree with that tool's or its residual is named.

> The repository is self-contained: every test design is public (see
> [`tests/designs/README.md`](tests/designs/README.md)), and reference outputs are
> generated from those public designs.

> **Research prototype — read the skip summary.** `sta` covers a subset of
> SDC (stock LibreLane `write_sdc` output parses unchanged) and models clocks
> both ideally and via `set_propagated_clock`, including latch `D -> Q` and
> time borrowing for `sky130_fd_sc_hd` shapes.
>
> - Recognised check arcs the endpoint model does not evaluate — `non_seq_*`,
>   `preserve`, async `Clear`/`Preset`, and `min_pulse_width` arcs whose pin is
>   off the clock network — are counted and shown but exempt from `--strict`:
>   they add checks outside the reported set rather than altering an arrival.
>   (Clock-pin `min_pulse_width` arcs and `three_state_*` enable/disable arcs
>   *are* evaluated.)
> - Cells driven far outside their Liberty timing tables can diverge from
>   OpenSTA.
> - Every run prints what it left out as counts and shares of the design's arcs
>   and endpoints; `--strict` exits non-zero when a skip can change a reported
>   number, and `--json <path>` writes the same summary machine-readably.

## Why

The open-source flow (OpenROAD and the tools around it) is complete, but slow to
iterate with: a small block takes tens of minutes to place and route, and even a
quick estimate goes through a container and a Tcl session. Exploring a design asks
the same few questions over and over: does it meet timing, where is placement
dense, will routing be congested. This project shortens that exploration loop,
while keeping the established tools as the sign-off reference at standard file
boundaries.

## Goals

1. **Fast iteration**: answer timing, placement, congestion, power, and IR-drop
   questions in seconds for designs up to about 100k cells on a laptop.
   Implementation steps such as detailed routing report their runtime explicitly;
   they are not claimed to be seconds-scale.
2. **One Rust binary, decoupled stages**: no container and no Tcl. It reads the
   standard files (Liberty, LEF, DEF, SPEF, SDC, VCD/SAIF, and Yosys JSON or
   gate-level Verilog) and writes DEF, SPEF, route guides, and JSON reports. Every
   stage can also consume OpenROAD's intermediate files (such as a placed DEF)
   directly, so any stage can run standalone or hand over to OpenROAD at any
   boundary. Rust for memory safety without a garbage collector and a single
   static binary.
   - Current: CPU data parallelism through `rayon` across global placement (FFT Poisson & wirelength gradients), RSMT/parasitic extraction, levelized STA, and routing/search kernels.
3. **Data-oriented design**: flat arrays per attribute (structure of arrays),
   graphs as index arrays instead of pointers, and `f32` arithmetic.
   - Current: the netlist and floorplan are SoA with CSR lists (`design_db.rs`);
     the placer and congestion estimate use `f32` on the CPU; the timing graph is
     Kahn-levelized (`topological_levels` + per-pin incoming lists) so each combinational
     depth wave runs in parallel across threads.
   - Future: further flattening of remaining pointer-heavy structures, and an optional Metal GPU path.
4. **Known fast algorithms, not new ones**:
   - congestion: RSMT (Steiner tree) decomposition followed by pattern
     (L/Z-shape) and probabilistic routing on a GCell grid;
   - placement: analytical placement with an electrostatic density term solved by
     FFT (ePlace, DREAMPlace), plus an Abacus legalizer with local refinement;
   - timing: propagation in Kahn topological order with NLDM table lookup, as OpenSTA computes it.
5. **Apple Silicon first**: all cores through `rayon`, plus Accelerate / Metal or
   NEON intrinsics where profiling warrants them (the density FFT, the wirelength
   gradient).
   - Current: multi-core CPU parallel via `rayon` with reusable zero-alloc transforms (`PoissonWorkspace`, thread-local DP scratch) and precomputed trig tables; no handwritten NEON intrinsics or Accelerate/Metal GPU path yet.
   - Target: Accelerate / Metal for remaining kernels if profiling says they matter.
6. **Agreement before speed**: a stage gets faster only after it agrees with the
   reference tool, and the fast version must give the same answer as the simple one.

Not goals: sign-off or replacing the reference tools — see the scope note under Stages.

## Stages

The exact current tolerances and named residuals are in
[`docs/status.md`](docs/status.md); this table is the map of the implemented
surface.

| stage | what | checked against | done when |
|---|---|---|---|
| E0 | core readers for Liberty (NLDM tables), LEF, DEF, Yosys JSON (preferred) and gate-level Verilog; shared writers for DEF, SPEF, and route guides | OpenROAD and OpenSTA reports on the same files | cell, net, pin and row counts match on every test design |
| E1 | global-routing congestion estimate (usage ratio per GCell edge, from placed DEF) | OpenROAD `global_route` GCell usage map | per-edge usage-ratio map vs OpenROAD: Spearman ρ ≥ 0.60 and ≥ negative-control null + 0.15, hot-set (ratio ≥ 0.9) overlap ≥ 0.45; the gcd rank-order test stays as a smoke test |
| E2 | global placement (wirelength plus a density penalty) and Abacus legalization with local refinement | OpenROAD `global_placement` | OpenROAD's legalizer accepts the result; HPWL within 5% |
| E3 | static timing: arrival, slew, setup and hold — zero-wire, placement-based RC estimation, guide parasitics, and SPEF | OpenSTA | worst slack and every endpoint's arrival within 1% or 5 ps, with documented per-design floors |
| N7 | power estimation from VCD/SAIF switching activity | OpenROAD/OpenSTA `report_power` | microbenchmark and gcd group totals match within documented tolerances; unsupported annotation/`when` cases are counted |
| E4 | global routing to OpenROAD-format route guides, plus guide parasitics | OpenROAD `global_route` and `estimate_parasitics -global_routing` | guide overlap and per-net capacitance meet the documented bounds; end-to-end STA residuals are named |
| E5 | clock-tree synthesis | TritonCTS plus OpenSTA propagated-clock STA | insertion delay, skew, endpoint timing, and buffer census meet the documented bounds or have named residuals |
| E6 | guide-bounded detailed routing and routed DEF emission | Magic `drc(full)`, OpenROAD detailed routing, and antenna reports | routed DEF re-parses and is deterministic; unrouted pins and violation residuals are named; gcd has zero LEF-expressible violations |
| E7 | LEF-expressible DRC subset checker | Magic `drc` rule taxonomy and mutation fixtures | zero violations on clean public routed DEFs across the implemented rules; mutations flag exactly the expected rules |
| E8 | IR drop on extracted PDN meshes | PDNSim `analyze_power_grid` and OpenSTA per-instance power feeds | worst drop/bounce and per-instance voltages meet the documented bounds or named floors |
| E9 | reporting and usability: critical paths, clock-domain summaries, slack histograms, `--top` ergonomics | OpenSTA `report_checks` and endpoint goldens | path sequences, timing fields, partitions, and histograms match the documented goldens |

The `floorplan` command is a utility that generates a starting DEF from a public
netlist; it is not a reference-checked stage by itself. Readers for SDC, SPEF,
VCD/SAIF, and route guides are verified as part of the stage that consumes them.

Out of scope: sign-off, GDS output, LVS, full Magic-deck DRC replacement, and
production-quality detailed routing — OpenROAD, OpenSTA, Magic, KLayout, and
PDNSim remain the reference and sign-off tools. The DRC command covers the
LEF-expressible subset; the detailed router is a research implementation whose
unrouted and residual cases are reported rather than hidden.

A GPU path (Metal) may come after a stage's CPU version agrees with the reference
tool. The GPU result must then equal the CPU result.

## Install

```
cargo install grus-eda
mise use -g github:grus-systems/grus-eda
```

Either one suffices: the first builds from source with a Rust toolchain, the
second fetches a prebuilt binary (`aarch64-apple-darwin`,
`x86_64-unknown-linux-gnu`) with [mise](https://mise.jdx.dev). Archives and
checksums are also on the
[releases page](https://github.com/grus-systems/grus-eda/releases) directly.

## Documentation

Start at [`docs/README.md`](docs/README.md) for the reading order.

- [`docs/status.md`](docs/status.md) — Current status per stage: designs covered, tolerances actually used, known gaps
- [`docs/roadmap.md`](docs/roadmap.md) — Remaining correctness and performance work, including named residuals
- [`docs/architecture.md`](docs/architecture.md) — Data-oriented design (SoA/CSR), decoupled pipeline, numeric model, and Apple Silicon strategy
- [`docs/e0-readers.md`](docs/e0-readers.md) — E0: File formats, Yosys JSON vs. Verilog, and Liberty binary caching
- [`docs/e1-congestion.md`](docs/e1-congestion.md) — E1: GCell grid, RSMT (Steiner tree) decomposition, and pattern/probabilistic routing
- [`docs/e2-placement.md`](docs/e2-placement.md) — E2: ePlace electrostatic placement, 2D DCT/FFT, and Abacus legalization
- [`docs/e3-sta.md`](docs/e3-sta.md) — E3: STA in Kahn topological order, NLDM extrapolation, placement-based RC, SPEF, DMP driver model
- [`docs/e4-guides.md`](docs/e4-guides.md) — E4: Global routing, route-guide emission, and guide parasitics
- [`docs/e5-cts.md`](docs/e5-cts.md) — E5: Clock-tree synthesis and propagated-clock verification
- [`docs/e6-drt.md`](docs/e6-drt.md) — E6: Detailed-routing substrate, maze routing, routed DEF emission, and the Magic end-to-end gate
- [`docs/e7-drc.md`](docs/e7-drc.md) — E7: LEF-expressible DRC subset and Magic taxonomy
- [`docs/e9-reporting.md`](docs/e9-reporting.md) — E9: Critical-path reporting, clock-domain summaries, slack histograms, and CLI ergonomics
- [`docs/results.md`](docs/results.md) — Verification log against OpenROAD, OpenSTA, Magic, and PDNSim
- [`docs/benchmarks.md`](docs/benchmarks.md) — Runtime measurements by stage, extracted from the verification log
- [`docs/history/`](docs/history/README.md) — Per-step design documents, implementation tickets, and the 2026-11-11 project review (records, not current guidance)

E8 IR drop and N7 power estimation do not yet have consolidated stage documents;
their current evidence and residuals are in `docs/status.md`, `docs/results.md`,
and `docs/roadmap.md`.

## Dependencies

None on other Grus repositories. The reference tools come from the LibreLane
container image (`ghcr.io/librelane/librelane:3.0.16`). Test designs are public
(OpenROAD-flow-scripts sky130hd designs, picorv32).

## License

grus-eda is licensed under the [Apache License 2.0](LICENSE). The test designs
under `tests/designs/` keep their own upstream licenses (BSD, ISC, Apache-2.0);
see [`tests/designs/README.md`](tests/designs/README.md) for the provenance and
license of each.
