# Step 4 ticket 07 — ibex: reference generation script

## Background

The ibex source spike passed (maintainer, 2026-11-14, Intel, pinned LibreLane
container): the container's yosys has no slang plugin on its default path, but the
plugin ships in the image (`/nix/store/*-yosys-slang/share/yosys/plugins/slang.so`);
loaded explicitly, `read_slang` elaborates `ibex_core` from the committed sources
with 0 errors and 0 warnings. Plain `read_verilog -sv` fails at
`ibex_alu.sv:23` (unpacked array ports, e.g. `imd_val_q_i[2]` — a hard yosys
frontend limitation), which is why ORFS uses `SYNTH_HDL_FRONTEND = slang` for this
design.

The maintainer has already prepared:
- `tests/designs/ibex/config.yaml` now sets `SYNTH_HDL_FRONTEND: slang`.
- `tests/designs/ibex/ibex.sdc` is now ORFS `sky130hd/ibex/constraint.sdc`
  verbatim (clock `core_clock` on `clk_i`, period 8.0, `set_clock_latency 1.095`,
  virtual IO clock). The earlier hand-simplified SDC is replaced.

## Files

- `tests/designs/ibex/generate_ref.sh` (new file, mode 100755)
- `tests/designs/ibex/config.yaml` (only if a key below needs adding; do not
  change the file list, the SDC pointers, or the values already present)

## Work

Write `tests/designs/ibex/generate_ref.sh` by mirroring
`tests/designs/jpeg/generate_ref.sh` step for step (same host wrapper with the
Docker-preference route and the `-u "$(id -u):$(id -g)"` flag, same step
structure, same Tcl helpers), with these design parameters:

- Design dir `tests/designs/ibex`, top `ibex_core`, clock port `clk_i`.
- The LibreLane run uses the committed `config.yaml` as-is (slang frontend,
  `FP_CORE_UTIL: 50`, `PL_TARGET_DENSITY: 0.6`).
- The global-routing layer adjustment for the utilisation variant and the
  gcell usage dump is **0.2** (ORFS `sky130hd/ibex/fastroute.tcl`:
  `set_global_routing_layer_adjustment <min>-<max> 0.2`).
- The STA reference dump keeps the jpeg fix: `find_timing_paths` capped at
  `-group_path_count 100000`, and the null guard that emits JSON `null` for an
  endpoint with no min-corner path and prints a `NOTE:` count.
- The SDC has a virtual clock and clock latency; do not simplify or regenerate
  it — the flow and the STA dump consume `ibex.sdc` as committed.
- Output artifact names follow the other designs: `ibex.synth.v`,
  `ibex.synth.json`, `ibex.floorplan.def`, `ibex.placed.def`, `ibex.util65.def`,
  `ibex.gcell_usage.txt`, `ibex.orient.def`, `ibex.orient.pins.txt`,
  `ibex.routed.spef`, `ibex.reference.json`, `ibex.sta.reference.json`.
- `ibex.reference.json` carries the same fields the jpeg script emits
  (design, instances, rows, tracks, signal nets, bterms, pin counts, HPWL
  values, verilog counts, `grt_total_edge_demand`).

## Acceptance

- `bash -n tests/designs/ibex/generate_ref.sh` passes; the script is executable.
- If LibreLane rejects `SYNTH_HDL_FRONTEND` (unknown key or wrong value), stop
  and report the exact error instead of working around it — the maintainer
  verifies the key against the pinned image.
- The maintainer runs the script (Intel, native Docker) and checks the
  artifacts; the script itself is the deliverable of this ticket.
- Final message: any deviation from the jpeg script's structure, and why.
