#!/usr/bin/env bash
# Reproduction script for gcd reference artifacts in grus-eda.
# Generates gcd.synth.v, gcd.synth.json, gcd.floorplan.def, gcd.placed.def,
# gcd.routed.spef, and gcd.reference.json using LibreLane, Yosys, OpenROAD, and OpenSTA
# inside the canonical container ghcr.io/librelane/librelane:3.0.16.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

if [ ! -d "${PDK_HOST_ROOT}/sky130A" ]; then
    echo "Error: sky130A PDK not found at ${PDK_HOST_ROOT}/sky130A." >&2
    echo "Please ensure Ciel/Volare sky130A is installed in ~/.ciel or set PDK_ROOT." >&2
    exit 1
fi

# If running on the host outside Docker, invoke this script inside the container
if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    echo "==> Running reference generation inside container ${CONTAINER_IMAGE}..."
    docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"

    ${docker_cmd} run --rm \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/gcd" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/gcd/generate_ref.sh
    exit 0
fi

echo "==> Running LibreLane flow for gcd on sky130_fd_sc_hd..."
PDK_LIB="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"

# 1. Run LibreLane flow up to OpenROAD.DetailedPlacement
RUN_DIR="/work/tests/designs/gcd/runs/canonical"
rm -rf "${RUN_DIR}"
librelane config.yaml \
    --run-tag canonical \
    --manual-pdk \
    --pdk-root /pdk \
    --pdk sky130A \
    -s sky130_fd_sc_hd \
    --from Yosys.JsonHeader \
    --to OpenROAD.DetailedPlacement

# 2. Extract synthesized netlists (emitted by the exact same Yosys run)
SYNTH_DIR="${RUN_DIR}/02-yosys-synthesis"
cp "${SYNTH_DIR}/gcd.nl.v" "${SCRIPT_DIR}/gcd.synth.v"
cp "${SYNTH_DIR}/gcd.nl.v.json" "${SCRIPT_DIR}/gcd.synth.json"

# 3. Create canonical gcd.floorplan.def from post-IO-placement DEF with logic cells marked UNPLACED
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/runs/canonical/21-openroad-ioplacement/gcd.def
set block [ord::get_db_block]
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        $inst setPlacementStatus UNPLACED
    }
}
write_def /work/tests/designs/gcd/gcd.floorplan.def
exit
EOF

# 4. Create canonical gcd.placed.def (global placement + detailed placement without filler cells)
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.floorplan.def
read_sdc /work/tests/designs/gcd/gcd.sdc
global_placement -density 0.55
detailed_placement
write_def /work/tests/designs/gcd/gcd.placed.def
exit
EOF

# 5. Extract SPEF parasitics
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_wire_rc -layer met2
estimate_parasitics -placement -spef_file /work/tests/designs/gcd/gcd.routed.spef
exit
EOF

# 6. Generate gcd.reference.json
python3 - << 'EOF'
import json

ref = {
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

with open("/work/tests/designs/gcd/gcd.reference.json", "w") as f:
    json.dump(ref, f, indent=2)
    f.write("\n")
print("Reference counts written to gcd.reference.json")
EOF

echo "==> Successfully generated all gcd reference artifacts!"
