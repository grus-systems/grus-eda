#!/usr/bin/env bash
# Reference generation + async-arc probe for the sta_edge/async_arcs fixture:
# `dfrtp_1`/`dfstp_1`/`dfbbp_1`/`dfbbn_1`/`dfrbp_1`/`dfxtp_1` cells under an
# ideal clock (ideal.sdc) and a propagated clock (propagated.sdc). Three flops
# (`u_ar`/`u_ap`/`u_aq`) sit on port `aclk`, which has no `create_clock`, so
# their `Q`/`Q_N` (and the `u_d*/D` capture endpoints behind them) can only
# carry an arrival if OpenSTA propagates `clear`/`preset` async arcs — that is
# the R6 re-measurement. `u_fbn` (`dfbbn_1`, `CLK_N`) covers falling
# recovery/removal; `dfbbp`/`dfbbn` carry the `non_seq_*` SET_B/RESET_B pair.
# Emits the golden JSON plus verbatim probe text per mode. Each scenario runs
# in its own OpenSTA process because read_sdc accumulates. Runs inside
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
        -w "/work/tests/designs/sta_edge/async_arcs" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_edge/async_arcs/generate_ref.sh
    exit 0
fi

# Per-scenario intermediates live in a scratch dir so a mid-run failure
# leaves no stale untracked files in the fixture dir.
SCRATCH_DIR="$(mktemp -d)"
trap 'rm -rf "$SCRATCH_DIR"' EXIT

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
# (pin + edge) — same collector as sta_edge/min_pulse_width.
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
read_def /work/tests/designs/sta_edge/async_arcs/async_arcs.def
read_sdc /work/tests/designs/sta_edge/async_arcs/$sdc_file

set wns_max [format "%.6f" [sta::worst_slack -max]]
set tns_max [format "%.6f" [sta::total_negative_slack -max]]
set wns_min [format "%.6f" [sta::worst_slack -min]]
set tns_min [format "%.6f" [sta::total_negative_slack -min]]
set viol_max [count_violations [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]]
set viol_min [count_violations [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]]
set endpoints [collect_endpoints_json]
set pulse_width [collect_pulse_width_json "/tmp/pw_rows.txt"]

set fp [open "$SCRATCH_DIR/$out_file" "w"]
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

# Verbatim probe transcript per SDC mode: command-surface discovery (Q0),
# the -unconstrained listing (Q1), async-arc arrival discrimination (Q2),
# falling recovery/removal on dfbbn (Q3), non_seq_* reporting (Q4) and the
# preset-side check pairing (Q5). Tcl on stdin: no `!` (history expansion).
run_probe () {
    local sdc_file="$1"
    local out_file="$2"
    openroad -no_init -no_splash << EOF > "/work/tests/designs/sta_edge/async_arcs/$out_file" 2>&1
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_edge/async_arcs/async_arcs.def

puts "=== scenario $sdc_file ==="
read_sdc /work/tests/designs/sta_edge/async_arcs/$sdc_file

puts "=== Q0 command surface ==="
puts "sta::*arrival*   : [lsort [info commands sta::*arrival*]]"
puts "sta::*vertex*    : [lsort [info commands sta::*vertex*]]"
puts "sta::*pin_*      : [lsort [info commands sta::*pin_*]]"
puts "*unconstrained*  : [lsort [info commands *unconstrained*]]"
puts "*check_types*    : [lsort [info commands *check_types*]]"
puts "report_arrival   : [info commands report_arrival]"

puts "=== Q1 report_checks -unconstrained ==="
report_checks -unconstrained

puts "=== Q2a find_timing_paths -from rst_n ==="
if {[catch {set pe_list [find_timing_paths -from [get_ports rst_n]]} err]} {
    puts "error: \$err"
} else {
    puts "path count: [llength \$pe_list]"
    foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
}
puts "=== Q2a find_timing_paths -from set_n ==="
if {[catch {set pe_list [find_timing_paths -from [get_ports set_n]]} err]} {
    puts "error: \$err"
} else {
    puts "path count: [llength \$pe_list]"
    foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
}

puts "=== Q2b find_timing_paths -to u_d*/D ==="
foreach p {u_ds/D u_dp/D u_dq/D} {
    if {[catch {set pe_list [find_timing_paths -to [get_pins \$p]]} err]} {
        puts "\$p error: \$err"
    } else {
        puts "\$p path count: [llength \$pe_list]"
        foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
    }
}

puts "=== Q2d raw arrivals at async-driven outputs ==="
foreach p {u_ar/Q u_ap/Q u_aq/Q_N u_ff0/Q} {
    puts "--- \$p ---"
    if {[catch {report_arrival [get_pins \$p]} err]} { puts "report_arrival: \$err" }
    foreach cmd {sta::find_vertex sta::pin_vertex} {
        if {[catch {set v [\$cmd [get_pins \$p] rise max]} err]} {
            puts "\$cmd rise/max: \$err"
        } elseif {[catch {puts "\$cmd rise/max arrival: [sta::time_sta_ui [\$v arrival]]"} err]} {
            puts "\$cmd rise/max vertex=\$v arrival: \$err"
        }
    }
}
puts "=== Q2d report_checks -from rst_n -through u_ar/Q -format full ==="
if {[catch {report_checks -from [get_ports rst_n] -through [get_pins u_ar/Q] -format full} err]} { puts "error: \$err" }

puts "=== Q2c clocked control ==="
if {[catch {set pe_list [find_timing_paths -from [get_ports rst_n] -through [get_pins u_ff0/Q]]} err]} {
    puts "find -from rst_n -through u_ff0/Q error: \$err"
} else {
    puts "u_ff0/Q through-path count: [llength \$pe_list]"
}
report_checks -to [get_pins u_ff2/D] -format full

puts "=== Q2e transient set_disable_timing u_ff0 CLK->Q ==="
if {[catch {set_disable_timing [get_cells u_ff0] -from CLK -to Q} err]} {
    puts "set_disable_timing error: \$err"
} else {
    if {[catch {set pe_list [find_timing_paths -to [get_pins u_ff2/D]]} err]} {
        puts "find -to u_ff2/D error: \$err"
    } else {
        puts "u_ff2/D path count after disable: [llength \$pe_list]"
    }
    report_checks -to [get_pins u_ff2/D] -format full
}

puts "=== Q3 falling recovery/removal on u_fbn (dfbbn_1, CLK_N) ==="
report_checks -to [get_pins u_fbn/RESET_B] -format full
report_checks -to [get_pins u_fbn/SET_B] -format full

puts "=== Q4 non_seq_* on u_fbp/u_fbn (dfbbp_1/dfbbn_1) ==="
if {[catch {report_check_types} err]} { puts "report_check_types: \$err" }
report_checks -to [get_pins u_fbp/RESET_B] -format full
report_checks -to [get_pins u_fbp/SET_B] -format full

puts "=== Q5 preset-side check pairing ==="
report_checks -to [get_pins u_fp/SET_B] -format full
exit
EOF
}

run_sta ideal.sdc ref_ideal.json
run_sta propagated.sdc ref_propagated.json
run_probe ideal.sdc probe_async_ideal.txt
run_probe propagated.sdc probe_async_propagated.txt

MODE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
{
    printf '{\n  "design": "sta_edge_async_arcs",\n'
    printf '  "ideal": '
    cat "${SCRATCH_DIR}/ref_ideal.json"
    printf ',\n  "propagated": '
    cat "${SCRATCH_DIR}/ref_propagated.json"
    printf '\n}\n'
} > "${MODE_DIR}/async_arcs.reference.json"
echo "Wrote async_arcs.reference.json"
