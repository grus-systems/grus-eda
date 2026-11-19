#!/usr/bin/env bash
# Reference generation for the sta_edge/min_pulse_width fixture: OpenSTA
# `report_pulse_width_checks` rows on mpw.def under the three clock scenarios
# (ideal.sdc, propagated.sdc, viol.sdc) plus the ordinary endpoint envelope.
# Each scenario runs in its own OpenSTA process because read_sdc accumulates.
# The pulse rows are parsed from the report text (the MinPulseWidthCheck seq
# objects have no Tcl iterator) inside the same process — every committed
# value comes from the container run. Runs inside
# ghcr.io/librelane/librelane:3.0.16.
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
        -w "/work/tests/designs/sta_edge/min_pulse_width" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_edge/min_pulse_width/generate_ref.sh
    exit 0
fi

run_sta () {
    local sdc_file="$1"
    local out_file="$2"
    openroad -no_init -no_splash << EOF
proc count_violations {paths} {
    array set seen {}
    set n 0
    foreach pe \$paths {
        set name [get_full_name [\$pe pin]]
        if {[sta::time_sta_ui [\$pe slack]] < 0 && [info exists seen(\$name)] == 0} {
            set seen(\$name) 1
            incr n
        }
    }
    return \$n
}

proc collect_endpoints_json {} {
    array set max_arr {}
    array set max_req {}
    array set max_slack {}
    array set max_slew {}
    set pins {}

    set paths_max [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]
    foreach pe \$paths_max {
        set name [get_full_name [\$pe pin]]
        set v [\$pe vertex]
        lappend pins \$name
        set max_arr(\$name) [format "%.6f" [sta::time_sta_ui [\$pe data_arrival_time]]]
        set max_req(\$name) [format "%.6f" [sta::time_sta_ui [\$pe data_required_time]]]
        set max_slack(\$name) [format "%.6f" [sta::time_sta_ui [\$pe slack]]]
        set max_slew(\$name) [format "%.6f" [sta::time_sta_ui [\$v slew [\$pe end_transition] max]]]
    }

    array set min_arr {}
    array set min_req {}
    array set min_slack {}
    array set min_slew {}

    set paths_min [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]
    foreach pe \$paths_min {
        set name [get_full_name [\$pe pin]]
        set v [\$pe vertex]
        set min_arr(\$name) [format "%.6f" [sta::time_sta_ui [\$pe data_arrival_time]]]
        set min_req(\$name) [format "%.6f" [sta::time_sta_ui [\$pe data_required_time]]]
        set min_slack(\$name) [format "%.6f" [sta::time_sta_ui [\$pe slack]]]
        set min_slew(\$name) [format "%.6f" [sta::time_sta_ui [\$v slew [\$pe end_transition] min]]]
    }

    set sorted_pins [lsort -dictionary \$pins]
    set entries {}
    foreach name \$sorted_pins {
        lappend entries "      {\"pin\": \"\$name\", \"max_arr_ns\": \$max_arr(\$name), \"max_req_ns\": \$max_req(\$name), \"max_slack_ns\": \$max_slack(\$name), \"max_slew_ns\": \$max_slew(\$name), \"min_arr_ns\": \$min_arr(\$name), \"min_req_ns\": \$min_req(\$name), \"min_slack_ns\": \$min_slack(\$name), \"min_slew_ns\": \$min_slew(\$name)}"
    }
    return [join \$entries ",\n"]
}

# One row per report_pulse_width_checks table line, sorted by the row label
# (pin + edge) — the report itself is slack-ordered, the golden is name-ordered.
proc collect_pulse_width_json {path} {
    report_pulse_width_checks -digits 6 > \$path
    set f [open \$path r]
    set rows {}
    while {[gets \$f line] >= 0} {
        if {[regexp {^\s*(\S+) \((high|low)\)\s+(\S+)\s+(\S+)\s+(\S+)\s+\(\w+\)} \$line -> pin edge req actual slack]} {
            lappend rows [list "\$pin (\$edge)" \$pin \$edge \$req \$actual \$slack]
        }
    }
    close \$f
    set rows [lsort -dictionary -index 0 \$rows]
    set entries {}
    foreach r \$rows {
        lassign \$r label pin edge req actual slack
        lappend entries "      {\"pin\": \"\$pin\", \"edge\": \"\$edge\", \"required_ns\": \$req, \"actual_ns\": \$actual, \"slack_ns\": \$slack}"
    }
    return [join \$entries ",\n"]
}

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_edge/min_pulse_width/mpw.def
read_sdc /work/tests/designs/sta_edge/min_pulse_width/$sdc_file

set wns_max [format "%.6f" [sta::worst_slack -max]]
set tns_max [format "%.6f" [sta::total_negative_slack -max]]
set wns_min [format "%.6f" [sta::worst_slack -min]]
set tns_min [format "%.6f" [sta::total_negative_slack -min]]
set viol_max [count_violations [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]]
set viol_min [count_violations [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]]
set endpoints [collect_endpoints_json]
set pulse_width [collect_pulse_width_json "/tmp/pw_rows.txt"]

set fp [open "/work/tests/designs/sta_edge/min_pulse_width/$out_file" "w"]
puts \$fp "{"
puts \$fp "    \"wns_max_ns\": \$wns_max,"
puts \$fp "    \"tns_max_ns\": \$tns_max,"
puts \$fp "    \"wns_min_ns\": \$wns_min,"
puts \$fp "    \"tns_min_ns\": \$tns_min,"
puts \$fp "    \"setup_violations\": \$viol_max,"
puts \$fp "    \"hold_violations\": \$viol_min,"
puts \$fp "    \"endpoints\": \["
puts \$fp \$endpoints
puts \$fp "    \],"
puts \$fp "    \"pulse_width\": \["
puts \$fp \$pulse_width
puts \$fp "    \]"
puts \$fp "  }"
close \$fp
puts "Wrote $out_file"
exit
EOF
}

SCENARIOS="ideal propagated viol"

for name in $SCENARIOS; do
    run_sta "$name.sdc" "ref_$name.json"
done

{
    printf '{\n  "design": "sta_edge_min_pulse_width",\n'
    first=1
    for name in $SCENARIOS; do
        if [ $first -eq 0 ]; then printf ',\n'; fi
        first=0
        printf '  "%s": ' "$name"
        cat "ref_$name.json"
    done
    printf '\n}\n'
} > min_pulse_width.reference.json
rm -f ref_*.json
echo "Wrote min_pulse_width.reference.json"
