# Test Designs

This directory contains public benchmark designs used for verification against reference EDA tools (Yosys, OpenROAD, OpenSTA).

---

## 1. Design Catalog & Provenance

### `gcd` (Greatest Common Divisor)
- **Directory**: `tests/designs/gcd/`
- **Origin**: [The-OpenROAD-Project/OpenROAD-flow-scripts](https://github.com/The-OpenROAD-Project/OpenROAD-flow-scripts) (`flow/designs/src/gcd/gcd.v`)
- **License**: BSD 3-Clause License (The Regents of the University of California). See `tests/designs/gcd/gcd.v`.
- **Target PDK**: SkyWater `sky130_fd_sc_hd` (1.8 V nominal corner `tt_025C_1v80`).
- **Description**: Standard 16-bit synchronous iterative GCD accelerator with valid/ready handshake interface (`req_msg`, `req_val`, `req_rdy`, `resp_msg`, `resp_val`, `resp_rdy`). Used across E0–E3 for golden equivalence checking.

---

## 2. Reference Artifacts (`tests/designs/gcd/`)

All golden files are strictly under 1 MB and generated from a single canonical run inside `ghcr.io/librelane/librelane:3.0.16`:

| File | Size | Description |
|---|---|---|
| `gcd.v` | 18 KB | Original Verilog RTL source. |
| `gcd.sdc` | 234 B | Minimal benchmark timing constraints (5.0 ns period, 1.0 ns I/O delays). |
| `config.yaml` | 177 B | LibreLane design configuration. |
| `generate_ref.sh` | 4.8 KB | Turnkey reproduction script running inside `ghcr.io/librelane/librelane:3.0.16`. |
| `gcd.synth.v` | 30 KB | Gate-level post-synthesis Verilog (Yosys + ABC, 233 logic instances). |
| `gcd.synth.json` | 693 KB | Yosys JSON representation (`write_json`) of the exact same synthesis netlist. |
| `gcd.floorplan.def` | 71 KB | Initialized floorplan DEF: placed I/O pins, fixed tap/endcaps (151 instances), PDN stripes, logic cells unplaced. |
| `gcd.placed.def` | 75 KB | Legalized placement DEF (OpenROAD global placement + detailed placement without filler cells, 384 components). |
| `gcd.routed.spef` | 164 KB | IEEE 1481 SPEF parasitics extracted from `gcd.placed.def`. |
| `gcd.reference.json` | 666 B | Golden counts and baseline metrics dumped by OpenSTA and OpenROAD. |

---

## 3. Golden Reference Metrics (`gcd.reference.json`)

```json
{
  "design": "gcd",
  "pdk": "sky130_fd_sc_hd",
  "corner": "tt_025C_1v80",
  "lib_cells": 428,
  "lib_pins": 1897,
  "lib_sequential_cells": 87,
  "tech_dbu_per_micron": 1000,
  "tech_routing_layers": 6,
  "tech_macros": 437,
  "site_width_dbu": 460,
  "site_height_dbu": 2720,
  "die_area_dbu": [0, 0, 91950, 102670],
  "instances": 384,
  "logic_instances": 233,
  "physical_instances": 151,
  "signal_nets": 269,
  "total_nets": 271,
  "bterms": 56,
  "rows": 29,
  "placed_hpwl_dbu": 4529200,
  "placed_hpwl_um": 4529.2,
  "zero_wire_wns_ns": 1.7578,
  "zero_wire_tns_ns": 0.0,
  "placed_spef_wns_ns": 1.6642,
  "placed_spef_tns_ns": 0.0
}
```

---

## 4. How to Reproduce Reference Artifacts

1. Ensure Docker is running via Colima:
   ```bash
   mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8
   ```

2. Ensure the SkyWater `sky130A` PDK is cached in `~/.ciel/sky130A` (or set `PDK_ROOT`):
   - `.lib`: `~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib` (12 MB, excluded from git)
   - `.tlef`: `~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef` (18 KB)
   - `.lef`: `~/.ciel/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef` (1.3 MB, excluded from git)

3. Run the reproduction script:
   ```bash
   ./tests/designs/gcd/generate_ref.sh
   ```
   The script runs inside `ghcr.io/librelane/librelane:3.0.16` and regenerates all golden files in `tests/designs/gcd/`.
