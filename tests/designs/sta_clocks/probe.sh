#!/usr/bin/env bash
# Multi-clock SDC probe: two-clock semantics measured against OpenSTA on the
# hand-written two-domain fixture two_clock.def (cka on clk1 -> u_fa0/u_fa1/u_fy,
# ckb on clk2 -> u_fb0/u_fb1/u_fx, cross-domain paths in both directions),
# plus the second-domain latch fixture two_clock_latch.def (a dlxtn on ckb
# feeding a ckb flop, cka flop feeding ya — every endpoint same-domain).
# Each t2_*.sdc scenario runs in its own OpenSTA process because read_sdc
# accumulates. Two artifacts per scenario: ref_<name>.json (wns/tns/violations
# + per-endpoint max/min arr/req/slack/slew, same shape as the sta_exceptions
# goldens) consolidated into two_clock.probe.json, and probe_<name>.txt —
# full-format report_checks plus the -unconstrained listing, which prints the
# launch/capture clock name and edge times per path (the pairing evidence the
# endpoint table alone does not carry). Runs inside
# ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
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
        -w "/work/tests/designs/sta_clocks" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_clocks/probe.sh
    exit 0
fi

if [ ! -d "${PDK_ROOT}/sky130A/libs.ref/sky130_fd_sc_hd" ]; then
    echo "PDK not found under PDK_ROOT=${PDK_ROOT}" >&2
    exit 1
fi

run_sta () {
    local def_file="$1"
    local sdc_file="$2"
    local out_file="$3"
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

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_clocks/$def_file

set sdc_err ""
if {[catch {read_sdc /work/tests/designs/sta_clocks/$sdc_file} err]} {
    regsub -all {"} \$err "'" sdc_err
    regsub -all {\n} \$sdc_err " " sdc_err
}

set fp [open "/work/tests/designs/sta_clocks/$out_file" "w"]
if {\$sdc_err ne ""} {
    puts \$fp "{\"error\": \"\$sdc_err\"}"
    close \$fp
    puts "SDC error in $sdc_file: \$sdc_err"
    exit 0
}

set endpoints [collect_endpoints_json]
# A false path that cuts every launch leaves "No paths found." — worst_slack
# returns Inf there, which is not valid JSON, so emit null aggregates; the
# empty endpoint list is the measurement.
if {\$endpoints eq ""} {
    set wns_max "null"
    set tns_max "null"
    set wns_min "null"
    set tns_min "null"
    set viol_max 0
    set viol_min 0
} else {
    set wns_max [format "%.6f" [sta::worst_slack -max]]
    set tns_max [format "%.6f" [sta::total_negative_slack -max]]
    set wns_min [format "%.6f" [sta::worst_slack -min]]
    set tns_min [format "%.6f" [sta::total_negative_slack -min]]
    set viol_max [count_violations [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]]
    set viol_min [count_violations [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]]
}

puts \$fp "{"
puts \$fp "  \"wns_max_ns\": \$wns_max,"
puts \$fp "  \"tns_max_ns\": \$tns_max,"
puts \$fp "  \"wns_min_ns\": \$wns_min,"
puts \$fp "  \"tns_min_ns\": \$tns_min,"
puts \$fp "  \"setup_violations\": \$viol_max,"
puts \$fp "  \"hold_violations\": \$viol_min,"
puts \$fp "  \"endpoints\": \["
puts \$fp \$endpoints
puts \$fp "  \]"
puts \$fp "}"
close \$fp
puts "Wrote $out_file"
exit
EOF
}

# Full-format path reports: prints the launch and capture clock name and edge
# times per path, plus the unconstrained listing that shows which endpoints
# left the timed set.
run_report () {
    local def_file="$1"
    local sdc_file="$2"
    local out_file="$3"
    openroad -no_init -no_splash << EOF > "/work/tests/designs/sta_clocks/$out_file" 2>&1
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_clocks/$def_file

puts "=== scenario $sdc_file ==="
if {[catch {read_sdc /work/tests/designs/sta_clocks/$sdc_file} err]} {
    puts "SDC error: \$err"
    exit 0
}
puts "=== report_checks -path_delay max -format full -endpoint_path_count 1 -group_path_count 100000 ==="
report_checks -path_delay max -format full -endpoint_path_count 1 -group_path_count 100000
puts "=== report_checks -path_delay min -format full -endpoint_path_count 1 -group_path_count 100000 ==="
report_checks -path_delay min -format full -endpoint_path_count 1 -group_path_count 100000
puts "=== report_checks -unconstrained ==="
report_checks -unconstrained
exit
EOF
}

SCENARIOS="t2_int t2_frac t2_unc t2_uncx t2_lat t2_md t2_mdl t2_mdo t2_prop t2_prop1 t2_grp t2_mcp t2_mcps t2_mds t2_mdx t2_selfa t2_selfb t2_selmd t2_selmdt t2_selmc t2_selmct t2_grpl t2_grpp t2_grpa t2_grpnm t2_grpd t2_grpd2 t2_wave t2_wave05 t2_selbare t2_selac t2_selunion t2_selunk t2_selport t2_selpto t2_grp1 t2_grpe t2_grpb t2_wavebig"

for name in $SCENARIOS; do
    run_sta "two_clock.def" "$name.sdc" "ref_$name.json"
    run_report "two_clock.def" "$name.sdc" "probe_$name.txt"
done

# Probe-only scenarios: the error/divergence forms, kept out of the golden.
# t2_grpn (missing mode flag) / t2_wave3 (odd instant count) / t2_waverev
# (non-increasing instants) are SDC errors in OpenSTA and named bails in ours;
# t2_wave4 (>2 instants) is accepted by OpenSTA but a deliberate named bail in
# ours. `run_report` records the outcome ("SDC error:" or a table) without
# folding a ref_*.json block into two_clock.probe.json.
PROBE_ONLY="t2_grpn t2_wave3 t2_wave4 t2_waverev"

for name in $PROBE_ONLY; do
    run_report "two_clock.def" "$name.sdc" "probe_$name.txt"
done

{
    printf '{\n  "design": "sta_clocks",\n  "scenarios": {\n'
    first=1
    for name in $SCENARIOS; do
        if [ $first -eq 0 ]; then printf ',\n'; fi
        first=0
        printf '    "%s": ' "$name"
        cat "ref_$name.json"
    done
    printf '\n  }\n}\n'
} > two_clock.probe.json

# Second-domain latch fixture: every endpoint is same-domain, so the probe
# covers the borrow window on ckb's own lattice end to end.
run_sta "two_clock_latch.def" "t2_latch.sdc" "ref_t2_latch.json"
run_report "two_clock_latch.def" "t2_latch.sdc" "probe_t2_latch.txt"

{
    printf '{\n  "design": "sta_clocks_latch",\n  "scenarios": {\n'
    printf '    "%s": ' "t2_latch"
    cat "ref_t2_latch.json"
    printf '\n  }\n}\n'
} > two_clock_latch.probe.json

rm -f ref_*.json
echo "Wrote two_clock.probe.json and two_clock_latch.probe.json"
