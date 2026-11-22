#!/usr/bin/env bash
# Reference generation for the sta_exceptions fixture: OpenSTA timing-exception
# semantics probe on a 3-FF design (exceptions.def). Each scenario is one SDC
# file = base.sdc + exception commands; every scenario runs in its own OpenSTA
# process because read_sdc accumulates. Per-scenario endpoint tables land in
# exceptions.reference.json. Runs inside ghcr.io/librelane/librelane:3.0.16.
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
        -w "/work/tests/designs/sta_exceptions" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/sta_exceptions/generate_ref.sh
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

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/sta_exceptions/exceptions.def

set sdc_err ""
if {[catch {read_sdc /work/tests/designs/sta_exceptions/$sdc_file} err]} {
    regsub -all {"} \$err "'" sdc_err
    regsub -all {\n} \$sdc_err " " sdc_err
}

set fp [open "/work/tests/designs/sta_exceptions/$out_file" "w"]
if {\$sdc_err ne ""} {
    puts \$fp "{\"error\": \"\$sdc_err\"}"
    close \$fp
    puts "SDC error in $sdc_file: \$sdc_err"
    exit 0
}

set wns_max [format "%.6f" [sta::worst_slack -max]]
set tns_max [format "%.6f" [sta::total_negative_slack -max]]
set wns_min [format "%.6f" [sta::worst_slack -min]]
set tns_min [format "%.6f" [sta::total_negative_slack -min]]
set viol_max [count_violations [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]]
set viol_min [count_violations [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]]
set endpoints [collect_endpoints_json]

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

SCENARIOS="base mc_setup2 mc_hold1 mc_setup2_hold1 mc_setup2_pos mc_bare2 mc_s2_h1_split mc_holdneg mc_both mc_overlap mc_setup2_nofrom mc_hold0 mc_to_y mc_allregs fp_uf0_uf1 fp_mc_same fp_allregs fp_to_y fp_from_din fp_pin_from fp_pin_to fp_pin_inst fp_pin_port fp_pin_badpin fp_pin_nondrive fp_bare_pin mc_pin fp_pin_clk fp_pin_clk2 fp_pin_allpins fp_pin_badinst fp_pin_clkport mc_pin_clk fp_bare_clk fp_bare_clk2 fp_bare_to md_basic md_nofrom md_loose md_tight md_port md_min md_min0 md_min1 md_minport md_overlap md_overlap_rev md_both md_mc md_mc_rev md_fp md_through md_min_mc md_min_mc_rev md_min_mcp_h1 md_min_mcp_s2 mhu_base mhu_min1 mhu_max1 base_prop pmd_basic pmd_min pmd_multi md_early md_datapath m2_max0 m2_max2 m2_both m2_min2 pm2_prop pm2_min2 pm2_both mdo_max05 mdo_max1 mdo_max2 mdo_max3 mdo_min0 mdo_min05 mdo_min1 mdo_mindl mdo_minhu mdo_all mdo_from0 mdo_from1 mdo_from2 mdo_fp mdo_mc mdo_mc_rev mdo_min_mch mdo_min_mcs mdo_both pmo_max05 pmo_max1 pmo_min0 pmo_min05 ml_both ml_max0 ml_max2 ml_all ml_min pml_both pml_min pmp_port pmp_minport pmp_port2 pmp_alls pcr_clk pcr_clkfp pcr_max pcr_min pcr_off ld_net ld_pin_sink ld_pin_drv ld_pins ld_nets ld_portnet ld_last ld_badnet ld_badpin ld_afo ld_afi ld_fo ld_cells"

for name in $SCENARIOS; do
    run_sta "$name.sdc" "ref_$name.json"
done

{
    printf '{\n  "design": "sta_exceptions",\n  "scenarios": {\n'
    first=1
    for name in $SCENARIOS; do
        if [ $first -eq 0 ]; then printf ',\n'; fi
        first=0
        printf '    "%s": ' "$name"
        cat "ref_$name.json"
    done
    printf '\n  }\n}\n'
} > exceptions.reference.json
rm -f ref_*.json
echo "Wrote exceptions.reference.json"
