#!/usr/bin/env bash
# Reproduction script for picorv32 reference artifacts in grus-eda (E3 STA only).
# Generates picorv32.synth.v, picorv32.synth.json, picorv32.floorplan.def, picorv32.placed.def,
# picorv32.routed.spef (OpenROAD's placement estimate), picorv32.routed.def, picorv32.drc.rpt,
# picorv32.rcx.spef (routed extraction) and picorv32.sta.reference.json using LibreLane,
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

    # Run as the invoking user so generated files are owned by them, not root:
    # CI checkouts (and any later cleanup) run as the runner user and cannot
    # remove root-owned files.
    ${docker_cmd} run --rm \
        -u "$(id -u):$(id -g)" \
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

# 5. Placement-estimate SPEF (estimate_parasitics -placement). Despite the file name, this is
#    not a routed extraction; the routed extraction is step 5b.
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

# 5b. Routed SPEF: global + detailed routing of picorv32.placed.def, then RC extraction on the routed
#     shapes. Writes picorv32.routed.def, picorv32.drc.rpt and picorv32.rcx.spef. The routing flow is
#     the one LibreLane's own OpenROAD steps run in this image.
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
global_route -guide_file /work/tests/designs/picorv32/runs/canonical/picorv32.guide -congestion_iterations 100
detailed_route -output_drc /work/tests/designs/picorv32/picorv32.drc.rpt -verbose 0
write_def /work/tests/designs/picorv32/picorv32.routed.def
define_process_corner -ext_model_index 0 X
extract_parasitics -ext_model_file /pdk/sky130A/libs.tech/openlane/rules.openrcx.sky130A.nom.calibre -lef_res
write_spef /work/tests/designs/picorv32/picorv32.rcx.spef
exit
EOF

# write_spef puts the wall-clock time in the *DATE line; pin it so regeneration is bit-identical.
# sed is not installed in the pinned container image, so python3 does the edit.
python3 - "${SCRIPT_DIR}/picorv32.rcx.spef" <<'PYEOF'
import sys
path = sys.argv[1]
lines = open(path).read().splitlines(keepends=True)
dated = [i for i, line in enumerate(lines) if line.startswith("*DATE ")]
if len(dated) != 1:
    sys.exit(f"expected exactly one *DATE line in {path}, found {len(dated)}")
lines[dated[0]] = '*DATE "normalized"\n'
open(path, "w").writelines(lines)
PYEOF
if [ "$(grep -c '^\*DATE "normalized"$' "${SCRIPT_DIR}/picorv32.rcx.spef")" -ne 1 ]; then
    echo "Error: expected exactly one *DATE line in picorv32.rcx.spef to normalize." >&2
    exit 1
fi

# A golden taken from a dirty route is worthless. The DRC report is empty when the route is clean.
if [ -s "${SCRIPT_DIR}/picorv32.drc.rpt" ]; then
    drc_count="$(wc -l < "${SCRIPT_DIR}/picorv32.drc.rpt")"
    echo "Error: detailed routing reported ${drc_count} DRC line(s) in picorv32.drc.rpt; refusing to continue." >&2
    exit 1
fi
echo "DRC violations: 0 (picorv32.drc.rpt is empty)"

# 9. Generate picorv32.sta.reference.json with per-endpoint OpenSTA values for E3 (zero_wire, placement_rc, placed_spef)
#    Units: PathEnd/Vertex methods return SI seconds, so they go through sta::time_sta_ui (-> ns).
#    sta::worst_slack / sta::total_negative_slack already return user units (ns); do not convert them.
#    Endpoints are collected max-first: calling find_timing_paths -min before iterating the -max
#    PathEnds invalidates them and segfaults OpenROAD 2026-02-17.
cat << 'EOF' > "${RUN_DIR}/collect_endpoints.tcl"
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
EOF

# The routed mode runs in its own OpenROAD process (below) so that its read_spef starts from a
# clean parasitics state. This process writes the first three modes and leaves the JSON open;
# the routed process appends the fourth mode and closes it.
cat << 'EOF' > "${RUN_DIR}/dump_sta_ref.tcl"
source /work/tests/designs/picorv32/runs/canonical/collect_endpoints.tcl

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
puts $fp "  },"
close $fp
puts "Wrote the first three modes of picorv32.sta.reference.json"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_ref.tcl"

# 9b. Routed SPEF mode (picorv32.rcx.spef from step 5b). Same DEF and SDC; only the parasitics differ.
cat << 'EOF' > "${RUN_DIR}/dump_sta_routed_ref.tcl"
source /work/tests/designs/picorv32/runs/canonical/collect_endpoints.tcl

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc

read_spef /work/tests/designs/picorv32/picorv32.rcx.spef
set routed_wns_max [format "%.6f" [sta::worst_slack -max]]
set routed_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set routed_wns_min [format "%.6f" [sta::worst_slack -min]]
set routed_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set routed_endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/picorv32/picorv32.sta.reference.json" "a"]
puts $fp "  \"routed_spef\": {"
puts $fp "    \"wns_max_ns\": $routed_wns_max,"
puts $fp "    \"tns_max_ns\": $routed_tns_max,"
puts $fp "    \"wns_min_ns\": $routed_wns_min,"
puts $fp "    \"tns_min_ns\": $routed_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $routed_endpoints
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Appended routed_spef mode to picorv32.sta.reference.json"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_routed_ref.tcl"

echo "==> Successfully generated all picorv32 reference artifacts!"
