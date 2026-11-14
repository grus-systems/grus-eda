#!/usr/bin/env bash
# Reproduction script for picorv32 reference artifacts in grus-eda (E3 STA only).
# Generates picorv32.synth.v, picorv32.synth.json, picorv32.floorplan.def, picorv32.placed.def,
# picorv32.routed.spef (OpenROAD's placement estimate) and picorv32.sta.reference.json using LibreLane,
# OpenROAD, and OpenSTA inside the canonical container ghcr.io/librelane/librelane:3.0.16.

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
    # Prefer a native Docker daemon (Linux hosts); fall back to the
    # Colima route used on macOS.
    if docker info >/dev/null 2>&1; then
        docker_cmd="docker"
    else
        docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"
    fi

    ${docker_cmd} run --rm \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/picorv32" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/picorv32/generate_ref.sh
    exit 0
fi

echo "==> Running LibreLane flow for picorv32 on sky130_fd_sc_hd..."
PDK_LIB="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"

# 1. Run LibreLane flow up to OpenROAD.DetailedPlacement
RUN_DIR="/work/tests/designs/picorv32/runs/canonical"
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
cp "${SYNTH_DIR}/picorv32.nl.v" "${SCRIPT_DIR}/picorv32.synth.v"
cp "${SYNTH_DIR}/picorv32.nl.v.json" "${SCRIPT_DIR}/picorv32.synth.json"

# 3. Create canonical picorv32.floorplan.def from post-IO-placement DEF with logic cells marked UNPLACED
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/runs/canonical/21-openroad-ioplacement/picorv32.def
set block [ord::get_db_block]
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        $inst setPlacementStatus UNPLACED
    }
}
write_def /work/tests/designs/picorv32/picorv32.floorplan.def
exit
EOF

# 4. Create canonical picorv32.placed.def (global placement + detailed placement without filler cells)
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.floorplan.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
global_placement -density 0.55
detailed_placement
write_def /work/tests/designs/picorv32/picorv32.placed.def
exit
EOF

# 5. Extract SPEF parasitics
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_wire_rc -layer met2
estimate_parasitics -placement -spef_file /work/tests/designs/picorv32/picorv32.routed.spef
exit
EOF


# 9. Generate picorv32.sta.reference.json with per-endpoint OpenSTA values for E3 (zero_wire, placement_rc, placed_spef)
#    Units: PathEnd/Vertex methods return SI seconds, so they go through sta::time_sta_ui (-> ns).
#    sta::worst_slack / sta::total_negative_slack already return user units (ns); do not convert them.
#    Endpoints are collected max-first: calling find_timing_paths -min before iterating the -max
#    PathEnds invalidates them and segfaults OpenROAD 2026-02-17.
cat << 'EOF' > "${RUN_DIR}/dump_sta_ref.tcl"
proc collect_endpoints_json {} {
    array set max_arr {}
    array set max_req {}
    array set max_slack {}
    array set max_slew {}
    set pins {}

    set paths_max [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]
    foreach pe $paths_max {
        set name [get_full_name [$pe pin]]
        set v [$pe vertex]
        lappend pins $name
        set max_arr($name) [format "%.6f" [sta::time_sta_ui [$pe data_arrival_time]]]
        set max_req($name) [format "%.6f" [sta::time_sta_ui [$pe data_required_time]]]
        set max_slack($name) [format "%.6f" [sta::time_sta_ui [$pe slack]]]
        set max_slew($name) [format "%.6f" [sta::time_sta_ui [$v slew [$pe end_transition] max]]]
    }

    array set min_arr {}
    array set min_req {}
    array set min_slack {}
    array set min_slew {}

    set paths_min [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]
    foreach pe $paths_min {
        set name [get_full_name [$pe pin]]
        set v [$pe vertex]
        set min_arr($name) [format "%.6f" [sta::time_sta_ui [$pe data_arrival_time]]]
        set min_req($name) [format "%.6f" [sta::time_sta_ui [$pe data_required_time]]]
        set min_slack($name) [format "%.6f" [sta::time_sta_ui [$pe slack]]]
        set min_slew($name) [format "%.6f" [sta::time_sta_ui [$v slew [$pe end_transition] min]]]
    }

    set sorted_pins [lsort -dictionary $pins]
    set entries {}
    foreach name $sorted_pins {
        lappend entries "      {\"pin\": \"$name\", \"max_arr_ns\": $max_arr($name), \"max_req_ns\": $max_req($name), \"max_slack_ns\": $max_slack($name), \"max_slew_ns\": $max_slew($name), \"min_arr_ns\": $min_arr($name), \"min_req_ns\": $min_req($name), \"min_slack_ns\": $min_slack($name), \"min_slew_ns\": $min_slew($name)}"
    }
    return [join $entries ",\n"]
}

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc

set zw_wns_max [format "%.6f" [sta::worst_slack -max]]
set zw_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set zw_wns_min [format "%.6f" [sta::worst_slack -min]]
set zw_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set zw_endpoints [collect_endpoints_json]

set_wire_rc -layer met2
estimate_parasitics -placement
set prc_wns_max [format "%.6f" [sta::worst_slack -max]]
set prc_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set prc_wns_min [format "%.6f" [sta::worst_slack -min]]
set prc_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set prc_endpoints [collect_endpoints_json]

read_spef /work/tests/designs/picorv32/picorv32.routed.spef
set spef_wns_max [format "%.6f" [sta::worst_slack -max]]
set spef_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set spef_wns_min [format "%.6f" [sta::worst_slack -min]]
set spef_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set spef_endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/picorv32/picorv32.sta.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"picorv32\","
puts $fp "  \"zero_wire\": {"
puts $fp "    \"wns_max_ns\": $zw_wns_max,"
puts $fp "    \"tns_max_ns\": $zw_tns_max,"
puts $fp "    \"wns_min_ns\": $zw_wns_min,"
puts $fp "    \"tns_min_ns\": $zw_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $zw_endpoints
puts $fp "    \]"
puts $fp "  },"
puts $fp "  \"placement_rc\": {"
puts $fp "    \"wns_max_ns\": $prc_wns_max,"
puts $fp "    \"tns_max_ns\": $prc_tns_max,"
puts $fp "    \"wns_min_ns\": $prc_wns_min,"
puts $fp "    \"tns_min_ns\": $prc_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $prc_endpoints
puts $fp "    \]"
puts $fp "  },"
puts $fp "  \"placed_spef\": {"
puts $fp "    \"wns_max_ns\": $spef_wns_max,"
puts $fp "    \"tns_max_ns\": $spef_tns_max,"
puts $fp "    \"wns_min_ns\": $spef_wns_min,"
puts $fp "    \"tns_min_ns\": $spef_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $spef_endpoints
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Generated picorv32.sta.reference.json via live OpenROAD/OpenSTA queries"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_ref.tcl"

echo "==> Successfully generated all picorv32 reference artifacts!"
