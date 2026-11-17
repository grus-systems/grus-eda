#!/usr/bin/env bash
# Reference generation for propagated-clock STA checks on the real designs: OpenSTA endpoint
# values on <design>.placed.def + <design>.propagated.sdc (set_propagated_clock) +
# <design>.rcx.spef (routed extraction). No LibreLane flow is re-run; only the design's own
# DEF/SPEF/SDC and the public Sky130 PDK are read. For picorv32 the placed DEF and rcx SPEF
# are git-ignored — run tests/designs/picorv32/generate_ref.sh first to produce them (gcd's
# are committed). Usage: generate_prop_ref.sh <design>   (designs with a routed-extraction
# SPEF: gcd, picorv32).
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

DESIGN="${1:?usage: generate_prop_ref.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

for f in "${DESIGN}.placed.def" "${DESIGN}.propagated.sdc" "${DESIGN}.rcx.spef"; do
    if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${f}" ]; then
        echo "Error: missing ${SCRIPT_DIR}/${DESIGN}/${f}" >&2
        exit 1
    fi
done

if [ ! -d "${PDK_HOST_ROOT}/sky130A" ]; then
    echo "Error: sky130A PDK not found at ${PDK_HOST_ROOT}/sky130A." >&2
    exit 1
fi

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
        -e PROP_DESIGN="${DESIGN}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_prop_ref.sh "${DESIGN}"
    exit 0
fi

export PROP_DESIGN="${PROP_DESIGN:-${DESIGN}}"

openroad -no_init -no_splash << 'EOF'
set DESIGN $env(PROP_DESIGN)

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

proc fmt_prop {pin prop} {
    if {[catch {format %.6f [get_property $pin $prop]} r]} { return "null" }
    return $r
}

# Per-clock-pin propagated arrival/slew, independent of any data path: this is what
# `set_propagated_clock` actually computes, so it is asserted separately from the
# endpoint aggregates. Covers `is_clock` pins on `all_registers` (sequential cells —
# the only clock consumers in gcd and picorv32, which have no clock-gate cells). A
# design with clock gating would need this extended; OpenSTA's arrival queries
# segfault on pins outside the timing graph, so gate pins are not a
# `get_pins -hierarchical` scan. (`all_latches`/`all_cells` do not exist in this
# OpenSTA build; `all_registers` is the sequential-cell enumeration.)
proc collect_clock_pins_json {} {
    set names {}
    array set v {}
    foreach cell [all_registers] {
        foreach p [get_pins -of $cell] {
            if {[get_property $p is_clock] == 1} {
                set name [get_full_name $p]
                lappend names $name
                set v($name,arm) [fmt_prop $p arrival_max_rise]
                set v($name,armn) [fmt_prop $p arrival_min_rise]
                set v($name,afm) [fmt_prop $p arrival_max_fall]
                set v($name,afmn) [fmt_prop $p arrival_min_fall]
                set v($name,srm) [fmt_prop $p slew_max_rise]
                set v($name,srmn) [fmt_prop $p slew_min_rise]
                set v($name,sfm) [fmt_prop $p slew_max_fall]
                set v($name,sfmn) [fmt_prop $p slew_min_fall]
            }
        }
    }
    set entries {}
    foreach name [lsort -dictionary $names] {
        lappend entries "      {\"pin\": \"$name\", \"arr_rise_max_ns\": $v($name,arm), \"arr_rise_min_ns\": $v($name,armn), \"arr_fall_max_ns\": $v($name,afm), \"arr_fall_min_ns\": $v($name,afmn), \"slew_rise_max_ns\": $v($name,srm), \"slew_rise_min_ns\": $v($name,srmn), \"slew_fall_max_ns\": $v($name,sfm), \"slew_fall_min_ns\": $v($name,sfmn)}"
    }
    return [join $entries ",\n"]
}

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$DESIGN/$DESIGN.placed.def
read_sdc /work/tests/designs/$DESIGN/$DESIGN.propagated.sdc

read_spef /work/tests/designs/$DESIGN/$DESIGN.rcx.spef
set prop_wns_max [format "%.6f" [sta::worst_slack -max]]
set prop_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set prop_wns_min [format "%.6f" [sta::worst_slack -min]]
set prop_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set prop_endpoints [collect_endpoints_json]
set prop_clock_pins [collect_clock_pins_json]

set fp [open "/work/tests/designs/$DESIGN/$DESIGN.propagated.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"$DESIGN\","
puts $fp "  \"propagated_routed_spef\": {"
puts $fp "    \"wns_max_ns\": $prop_wns_max,"
puts $fp "    \"tns_max_ns\": $prop_tns_max,"
puts $fp "    \"wns_min_ns\": $prop_wns_min,"
puts $fp "    \"tns_min_ns\": $prop_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $prop_endpoints
puts $fp "    \],"
puts $fp "    \"clock_pins\": \["
puts $fp $prop_clock_pins
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Wrote $DESIGN.propagated.reference.json"
exit
EOF
