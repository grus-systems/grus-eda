#!/usr/bin/env bash
# Reference generation for the sta_edge/clock_uncertainty fixture: OpenSTA endpoint
# values on base.def + edge.sdc (with set_clock_uncertainty). Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    if docker info >/dev/null 2>&1; then
        docker_cmd="docker"
    else
        docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"
    fi
    ${docker_cmd} run --rm \
        -u "$(id -u):$(id -g)" \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/sta_edge/clock_uncertainty" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_edge/clock_uncertainty/generate_ref.sh
    exit 0
fi

openroad -no_init -no_splash << 'EOF'
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
read_def /work/tests/designs/sta_edge/base.def
read_sdc /work/tests/designs/sta_edge/clock_uncertainty/edge.sdc


set wns_max [format "%.6f" [sta::worst_slack -max]]
set tns_max [format "%.6f" [sta::total_negative_slack -max]]
set wns_min [format "%.6f" [sta::worst_slack -min]]
set tns_min [format "%.6f" [sta::total_negative_slack -min]]
set endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/sta_edge/clock_uncertainty/clock_uncertainty.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"sta_edge_clock_uncertainty\","
puts $fp "  \"zero_wire\": {"
puts $fp "    \"wns_max_ns\": $wns_max,"
puts $fp "    \"tns_max_ns\": $tns_max,"
puts $fp "    \"wns_min_ns\": $wns_min,"
puts $fp "    \"tns_min_ns\": $tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $endpoints
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Wrote clock_uncertainty.reference.json"
exit
EOF
