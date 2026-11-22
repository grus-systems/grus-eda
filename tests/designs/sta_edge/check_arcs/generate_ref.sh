#!/usr/bin/env bash
# Reference generation + deferred-check-arc probe for the
# sta_edge/check_arcs fixture: `dfbbp_1`/`dfbbn_1` flops carrying the
# `non_seq_*` SET_B/RESET_B pair plus `clear`/`preset` arcs, and
# `ebufn_1`/`einvn_1`/`einvp_1` tristate drivers feeding capture flops.
# Measures whether OpenSTA 3.0.16 evaluates `non_seq_*`/`three_state_*`/
# `preserve` arcs or ignores them (probe.md Q0-Q5). `te_slow.sdc` delays
# the enable port by 2.5 ns so a propagating `TE_B -> Z`/`TE -> Z` arc
# shifts the `u_cap*/D` arrivals versus `ideal.sdc` — the propagation
# discriminator. Emits the golden JSON plus verbatim probe text per mode.
# Each scenario runs in its own OpenSTA process because read_sdc
# accumulates. Runs inside ghcr.io/librelane/librelane:3.0.16.
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
        -w "/work/tests/designs/sta_edge/check_arcs" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_edge/check_arcs/generate_ref.sh
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
read_def /work/tests/designs/sta_edge/check_arcs/check_arcs.def
read_sdc /work/tests/designs/sta_edge/check_arcs/$sdc_file

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

# Verbatim probe transcript per SDC mode: command/flag surface (Q0),
# non_seq_* data-check surfaces (Q1), three_state_* propagation (Q2),
# Clear/Preset re-confirmation (Q4). Tcl on stdin: no `!` (history
# expansion); every catch error is recorded verbatim as evidence.
run_probe () {
    local sdc_file="$1"
    local out_file="$2"
    openroad -no_init -no_splash << EOF > "/work/tests/designs/sta_edge/check_arcs/$out_file" 2>&1
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_edge/check_arcs/check_arcs.def

puts "=== scenario $sdc_file ==="
read_sdc /work/tests/designs/sta_edge/check_arcs/$sdc_file

puts "=== Q0 command surface ==="
puts "*check*          : [lsort [info commands *check*]]"
puts "sta::*check*     : [lsort [info commands sta::*check*]]"
puts "*data*           : [lsort [info commands *data*]]"
puts "*three*          : [lsort [info commands *three*]]"
puts "*non_seq*        : [lsort [info commands *non_seq*]]"
puts "*preserve*       : [lsort [info commands *preserve*]]"

foreach flag {-data_setup -data_hold -recovery -removal -clock_gating_setup -clock_gating_hold -max_delay -min_delay -min_period -min_pulse_width -max_slew -max_capacitance -max_fanout -violators} {
    puts "--- report_check_types \$flag ---"
    if {[catch {report_check_types \$flag} err]} {
        puts "error: \$err"
    }
}

foreach pd {data_setup data_hold non_seq_setup non_seq_hold three_state recovery removal} {
    puts "--- find_timing_paths -path_delay \$pd ---"
    if {[catch {set pe_list [find_timing_paths -path_delay \$pd]} err]} {
        puts "error: \$err"
    } else {
        puts "path count: [llength \$pe_list]"
        foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
    }
}

puts "=== Q1 non_seq_* on u_fbp/u_fbn (dfbbp_1/dfbbn_1) ==="
foreach p {u_fbp/RESET_B u_fbp/SET_B u_fbn/RESET_B u_fbn/SET_B} {
    if {[catch {set pe_list [find_timing_paths -to [get_pins \$p] -endpoint_path_count 10 -group_path_count 100000]} err]} {
        puts "\$p error: \$err"
    } else {
        puts "\$p path count: [llength \$pe_list]"
        foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
    }
}
report_checks -to [get_pins u_fbp/RESET_B] -format full -endpoint_path_count 10
report_checks -to [get_pins u_fbp/SET_B] -format full -endpoint_path_count 10
foreach p {u_fbp/RESET_B u_fbp/SET_B} {
    puts "--- report_arrival \$p ---"
    if {[catch {report_arrival [get_pins \$p]} err]} { puts "report_arrival: \$err" }
}

puts "=== Q2 three_state_* on u_ebuf/u_einv/u_evp ==="
if {[catch {set pe_list [find_timing_paths -from [get_ports te] -endpoint_path_count 10 -group_path_count 100000]} err]} {
    puts "find -from te error: \$err"
} else {
    puts "find -from te path count: [llength \$pe_list]"
    foreach pe \$pe_list { puts "  end: [get_full_name [\$pe pin]]" }
}
report_checks -to [get_pins u_cap1/D] -format full
foreach p {u_ebuf/Z u_einv/Z u_evp/Z u_fbp/Q u_fbn/Q_N} {
    puts "--- report_arrival \$p ---"
    if {[catch {report_arrival [get_pins \$p]} err]} { puts "report_arrival: \$err" }
}

puts "=== Q3 three_state edge model (report_edges/report_slews) ==="
report_edges -from [get_pins u_ebuf/TE_B] -to [get_pins u_ebuf/Z]
report_edges -from [get_pins u_ebuf/A] -to [get_pins u_ebuf/Z]
report_edges -from [get_pins u_einv/TE_B] -to [get_pins u_einv/Z]
report_edges -from [get_pins u_einv/A] -to [get_pins u_einv/Z]
report_edges -from [get_pins u_evp/TE] -to [get_pins u_evp/Z]
report_edges -from [get_pins u_evp/A] -to [get_pins u_evp/Z]
foreach p {u_ebuf/Z u_einv/Z u_evp/Z} {
    puts "--- report_slews \$p ---"
    if {[catch {report_slews [get_pins \$p]} err]} { puts "report_slews: \$err" }
}
foreach n {z1 z2 z3} {
    puts "--- report_net \$n ---"
    if {[catch {report_net \$n -digits 6} err]} { puts "report_net: \$err" }
}

puts "=== Q2 transient set_disable_timing u_ebuf A->Z ==="
if {[catch {set_disable_timing [get_cells u_ebuf] -from A -to Z} err]} {
    puts "set_disable_timing error: \$err"
} else {
    if {[catch {set pe_list [find_timing_paths -to [get_pins u_cap1/D]]} err]} {
        puts "find -to u_cap1/D error: \$err"
    } else {
        puts "u_cap1/D path count after disable A->Z: [llength \$pe_list]"
    }
    report_checks -to [get_pins u_cap1/D] -format full
}

puts "=== Q2 transient set_disable_timing u_evp TE->Z ==="
if {[catch {set_disable_timing [get_cells u_evp] -from TE -to Z} err]} {
    puts "set_disable_timing error: \$err"
} else {
    if {[catch {set pe_list [find_timing_paths -to [get_pins u_cap3/D]]} err]} {
        puts "find -to u_cap3/D error: \$err"
    } else {
        puts "u_cap3/D path count after disable TE->Z: [llength \$pe_list]"
    }
    report_checks -to [get_pins u_cap3/D] -format full
}

puts "=== Q4 Clear/Preset stay count-only ==="
if {[catch {set pe_list [find_timing_paths -through [get_pins u_fbp/Q]]} err]} {
    puts "find -through u_fbp/Q error: \$err"
} else {
    puts "paths through u_fbp/Q: [llength \$pe_list]"
}
if {[catch {set pe_list [find_timing_paths -through [get_pins u_fbn/Q_N]]} err]} {
    puts "find -through u_fbn/Q_N error: \$err"
} else {
    puts "paths through u_fbn/Q_N: [llength \$pe_list]"
}
report_check_types
exit
EOF
}

run_sta ideal.sdc ref_ideal.json
run_sta propagated.sdc ref_propagated.json
run_sta te_slow.sdc ref_te_slow.json
run_probe ideal.sdc probe_check_arcs_ideal.txt
run_probe propagated.sdc probe_check_arcs_propagated.txt
run_probe te_slow.sdc probe_check_arcs_te_slow.txt

MODE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
{
    printf '{\n  "design": "sta_edge_check_arcs",\n'
    printf '  "ideal": '
    cat "${SCRATCH_DIR}/ref_ideal.json"
    printf ',\n  "propagated": '
    cat "${SCRATCH_DIR}/ref_propagated.json"
    printf ',\n  "te_slow": '
    cat "${SCRATCH_DIR}/ref_te_slow.json"
    printf '\n}\n'
} > "${MODE_DIR}/check_arcs.reference.json"
echo "Wrote check_arcs.reference.json"
