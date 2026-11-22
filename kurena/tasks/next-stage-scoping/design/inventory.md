# Candidate-Stage Inventory

Per stage: what it consumes and emits, the repo modules it reuses (by name),
what is missing, and the reference-tool command that produces its numbers.

All reference commands run inside `ghcr.io/librelane/librelane:3.0.16`
(`openroad` / `sta`; `magic` availability is probed — see `probes.md`).

---

## E4 — Global routing → route guides + guide-based parasitics

**What it is**: the E1 congestion machinery taken one step further — keep the
committed per-segment paths, assign layers, and emit an OpenROAD-format route
guide per net, then feed the guides into an RC model so `sta` answers with
guide-level parasitics instead of the placement PD-tree estimate.

- **Inputs**: `tlef` + `lef` + placed `def` (+ optional `sdc` to identify clock
  nets for the clock-layer band).
- **Outputs**: guide file (OpenROAD `global_route -guide_file` text format),
  guide-based `NetParasitics` → `--spef-out` SPEF; the existing congestion
  report comes free.
- **Reuses**:
  - `congestion/gcell.rs::GCellGrid::build` — grid + per-edge derated track
    capacity, verified against `dbGCellGrid`/`GRT-0053` to the integer.
  - `congestion/rsmt.rs::decompose_net` / `decompose_design_nets` /
    `build_net_rsmt` — per-net Steiner segment decomposition.
  - `congestion/route.rs::route_segments` — 3-pass router; already computes
    committed `segment_paths: Vec<Vec<GridEdge>>` internally but returns only
    aggregate usage. First sub-step: return the paths and the net→segments
    mapping (`decompose_design_nets` currently flattens).
  - `sta/parasitics.rs::WireRc::from_layer`, `NetRcTree`, `NetParasitics` —
    RC tree machinery the guide parasitics drop into.
  - `readers/spef.rs` writer (N4) and `sta` CLI plumbing (`--parasitics`,
    `--spef-out`).
- **Missing**: guide→layer assignment (committed 2D edge paths must become
  layer-tagged guide boxes under `set_routing_layers -signal met1-met5` /
  `-clock met3-met5` conventions), guide-file writer (+ reader if we want to
  round-trip OpenROAD guides into our RC model), pin-access stubs at guide
  granularity, via allowance between layers for parasitics.
- **Reference commands**: `global_route -guide_file <file> [-allow_congestion]`
  (already used in `generate_ref.sh` step 5b), `estimate_parasitics
  -global_routing -spef_file <file>` (guide-based RC — availability flagged
  for probe P-GR), the `dbGCellGrid` usage queries already exercised by E1.

## E5 — Clock tree synthesis

**What it is**: insert buffers under the clock net to bound skew and insertion
delay, emit a DEF with the tree, and let the existing propagated-clock STA
score the result.

- **Inputs**: placed `def` + `sdc` (clock source/period), `tlef`/`lef`,
  `lib` (buffer candidates).
- **Outputs**: DEF with inserted clock cells and split clock nets; a
  skew/insertion-delay report; feeds E4 (clock nets get routed first in a real
  flow).
- **Reuses**:
  - `placement/legalize.rs` — Abacus legalizer for the inserted cells.
  - `readers/def.rs::write_def_full` — DEF emission.
  - `sta/analysis.rs` propagated-clock mode — already matches OpenSTA on
    propagated clocks; this is both the scoring path and the done-when anchor.
  - `congestion/pdr.rs` PD tree / `rsmt.rs` — candidate topologies for the
    tree backbone (CTS optimizes latency-balance, not wirelength, so the
    reuse is a starting point, not the algorithm).
- **Missing**: DesignDb cell/net insertion (nothing mutates a netlist today),
  the clock-net decomposition into buffer-driven segments, a balance-driven
  tree constructor, post-insertion placement of new cells.
- **Reference commands**: `clock_tree_synthesis` (TritonCTS — probe the image
  for flag surface: `-buf_list`, `-root_buf`, `-wire_unit`,
  `-sink_clustering_*`, `-post_cts_disable`), `repair_clock_nets`,
  `estimate_parasitics -placement`, `report_clock_skew -setup/-hold`,
  `report_clock_tree`-family reports — all inside the pinned `openroad`/`sta`.

## E6 — Detailed routing

**What it is**: track-level, rule-aware routing producing real shapes (wires +
vias) in a routed DEF. TritonRoute-class work.

- **Inputs**: placed DEF + guides (E4) or standalone, track grids (have), LEF
  `VIA`/`SPACING` rules (not parsed today).
- **Outputs**: routed DEF with `ROUTED`/`NEW` wire geometry and via
  instances on signal nets.
- **Reuses**: `tech_db` layer/track data, `design_db` pin/obstacle rects
  (`obs_rects`, `pin_obs_rects`), `write_def_full` (needs a routing section),
  `special_wire_rects` handling.
- **Missing** (the load-bearing gaps): LEF `VIA` + `SPACING` parse (skipped
  today), DEF `VIAS`/`NONDEFAULTRULES` parse (skipped), signal-net `ROUTED`
  wire storage (NETS section drops it), via-stack model, DRC-correct maze
  router, pin-access planning. This is the biggest delta in the list.
- **Reference commands**: `detailed_route -output_drc <rpt>` + `write_def`
  (used in step 5b), then `extract_parasitics` / `write_spef` for the
  parasitic consequence. Metrics: total wirelength, via count, DRC violation
  count (0 on gcd/picorv32), routed-SPEF STA agreement.

## E7 — DRC

**What it is**: geometric rule checking on routed shapes.

- **Inputs**: routed `def` (+ LEF rules; the sky130 rule deck lives in
  Magic's tech file, only partially expressible in LEF).
- **Outputs**: violation list/count per rule.
- **Reuses**: `tech_db` + `design_db` geometry once routed wires are stored
  (depends on E6's reader extensions — or on E6 itself for our own routes).
- **Missing**: everything E6 is missing, plus rule evaluation machinery
  (spacing tables, min area, enclosure) whose canonical expression is Magic's
  tech file, not LEF.
- **Reference commands**: `detailed_route -output_drc` (DRT's internal check —
  only validates DRT's own routes), Magic `drc` (probe whether the image's
  magic can `def read` a routed DEF with the sky130A magicrc; see P-DRC),
  `check_antennas` for the antenna subset.
- **Note**: meaningless as a standalone stage until something emits shapes —
  either it pairs with E6 (check our own routes) or it shrinks to a LEF-rule
  subset checker on committed `gcd.routed.def`/`picorv32.routed.def`.

## E8 — IR drop (power-grid analysis)

**What it is**: static (DC) voltage-drop analysis on the power/ground network.

- **Inputs**: `def` carrying `SPECIALNETS` PDN geometry (`gcd.placed.def` has
  rails + straps), per-instance current from `power.rs::estimate_power`
  (N7 — power/voltage gives average current; leakage-only fallback is a probe
  decision), layer/via resistance.
- **Outputs**: per-node/per-instance voltage map, worst drop.
- **Reuses**: `special_wire_rects` (PDN wire geometry — horizontal),
  `power.rs` per-instance estimates, `RoutingLayer::res_per_sq` (have),
  `pin_obs_rects`/`MacroPin` power-pin positions.
- **Missing**: SPECIALNETS via extraction (only rect wires captured today —
  the vertical stacks connecting met1 rails to met4/met5 straps are dropped),
  LEF `VIA` resistance parse, resistor-mesh construction, a sparse DC solve
  (new dependency — e.g. `sprs`, or a small Jacobi/CG kernel), current-source
  attachment of instances to grid nodes.
- **Reference commands**: PDNSim inside OpenROAD — `analyze_power_grid` /
  `check_power_grid` (probe P-IRD: whether the pinned image ships `psm`; if
  not, this stage has no usable reference in the container and stays deferred
  by discipline).
