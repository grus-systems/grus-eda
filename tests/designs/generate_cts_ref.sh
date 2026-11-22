#!/usr/bin/env bash
# TritonCTS reference generation on a committed placed DEF: clock_tree_synthesis with
# the sky130hd buffer list (-root_buf clkbuf_16, -buf_list "clkbuf_8 clkbuf_4 clkbuf_2",
# -wire_unit 20) + repair_clock_nets + estimate_parasitics -placement + the report
# surface (report_cts_config, report_cts, report_clock_skew -setup/-hold,
# worst_clock_skew -setup/-hold), an odb census of the inserted instances, and a
# propagated-clock OpenSTA endpoint golden on the resulting DEF.
#
# Step A (one openroad process, transcript tee'd to runs/cts/cts.log) writes
# <design>.cts.def and the census/scalar files; step B assembles
# <design>.cts.reference.json from the transcript + census (nothing hand-written);
# step C (a second openroad process, clean parasitics) writes
# <design>.cts.sta.reference.json from <design>.cts.def + <design>.cts.sdc.
#
# No LibreLane flow is re-run; only the design's own DEF/SDC and the public Sky130
# PDK are read. For picorv32 the placed DEF is git-ignored — run
# tests/designs/picorv32/generate_ref.sh first to produce it (gcd's is committed).
# Usage: generate_cts_ref.sh <design>   (designs with a placed DEF: gcd, picorv32).
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

DESIGN="${1:?usage: generate_cts_ref.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

for f in "${DESIGN}.placed.def" "${DESIGN}.sdc" "${DESIGN}.cts.sdc"; do
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
        -e CTS_DESIGN="${DESIGN}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_cts_ref.sh "${DESIGN}"
    exit 0
fi

export CTS_DESIGN="${CTS_DESIGN:-${DESIGN}}"
CTS_RUN_DIR="/work/tests/designs/${DESIGN}/runs/cts"
mkdir -p "${CTS_RUN_DIR}"

# Tcl on stdin: `!` is history expansion (write `== 0`) and `\` does not continue
# long lines — every Tcl command below is one physical line.

# Step A. TritonCTS + repair + placement parasitics + report surface + census +
#         write_def, all in one process so the DEF is exactly the post-CTS state
#         the reports describe. The transcript is the metrics parser's input.
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${CTS_RUN_DIR}/cts.log"
set D $env(CTS_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

set block [ord::get_db_block]
foreach inst [$block getInsts] { set pre([$inst getName]) 1 }

clock_tree_synthesis -root_buf sky130_fd_sc_hd__clkbuf_16 -buf_list "sky130_fd_sc_hd__clkbuf_8 sky130_fd_sc_hd__clkbuf_4 sky130_fd_sc_hd__clkbuf_2" -wire_unit 20
repair_clock_nets
set_wire_rc -layer met2
estimate_parasitics -placement

report_cts_config
report_cts
report_clock_skew -setup -digits 6
report_clock_skew -hold -digits 6
set wcs_setup [worst_clock_skew -setup]
set wcs_hold [worst_clock_skew -hold]

# report_clock_skew prints only the worst skew pair, so max/min insertion delay
# is collected per pin here: the propagated rise arrival at every register clock
# pin (the active edge — fall arrivals sit a half-period out and are not the
# insertion delay), max corner, matching the -setup skew surface.
set_propagated_clock [get_clocks *]
set lat_max -1.0
set lat_min 1.0e9
set sink_pins 0
foreach cell [all_registers] {
    foreach p [get_pins -of $cell] {
        if {[get_property $p is_clock] == 1} {
            incr sink_pins
            set a [get_property $p arrival_max_rise]
            if {$a > $lat_max} { set lat_max $a }
            if {$a < $lat_min} { set lat_min $a }
        }
    }
}

# Inserted-instance census: master + output-net ITerm count (unconnected output
# reads as the literal object NULL; fanout <= 1 is the dummy-load classifier).
set fp [open "/work/tests/designs/$D/runs/cts/cts_census.txt" "w"]
foreach inst [$block getInsts] {
    if {[info exists pre([$inst getName])] == 0} {
        set outfan 0
        foreach it [$inst getITerms] {
            if {[[$it getMTerm] getIoType] eq "OUTPUT"} {
                set n [$it getNet]
                if {$n ne "NULL"} { set outfan [llength [$n getITerms]] }
            }
        }
        puts $fp "[$inst getName] [[$inst getMaster] getName] $outfan"
    }
}
close $fp

# Every net attached to an inserted instance is on the clock tree: the root net
# through the root buffer's input, the subnets between buffers, and the leaf
# nets to the sinks and dummy loads.
array set is_clk_net {}
foreach inst [$block getInsts] {
    if {[info exists pre([$inst getName])] == 0} {
        foreach it [$inst getITerms] {
            set n [$it getNet]
            if {$n ne "NULL"} { set is_clk_net([$n getName]) 1 }
        }
    }
}
set clk_hpwl_dbu 0
set signal_nets 0
foreach net [$block getNets] {
    if {[$net isSpecial] == 0} {
        incr signal_nets
        if {[info exists is_clk_net([$net getName])]} {
            set bbox [$net getTermBBox]
            incr clk_hpwl_dbu [expr {[$bbox xMax] - [$bbox xMin] + [$bbox yMax] - [$bbox yMin]}]
        }
    }
}
set clock_net_count [llength [array names is_clk_net]]

set fp [open "/work/tests/designs/$D/runs/cts/cts_scalars.tcl" "w"]
puts $fp "set def_components [llength [$block getInsts]]"
puts $fp "set def_signal_nets $signal_nets"
puts $fp "set wcs_setup $wcs_setup"
puts $fp "set wcs_hold $wcs_hold"
puts $fp "set lat_max $lat_max"
puts $fp "set lat_min $lat_min"
puts $fp "set sink_pins $sink_pins"
puts $fp "set clock_net_hpwl_dbu $clk_hpwl_dbu"
puts $fp "set clock_net_count $clock_net_count"
close $fp

write_def /work/tests/designs/$D/$D.cts.def
exit
EOF

for f in "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.cts.def" \
         "${CTS_RUN_DIR}/cts_census.txt" \
         "${CTS_RUN_DIR}/cts_scalars.tcl"; do
    if [ ! -f "${f}" ]; then
        echo "Error: step A did not produce ${f}." >&2
        exit 1
    fi
done

# Step B. Assemble <design>.cts.reference.json from the step-A transcript and the
#         census/scalar files. Every unmatched expectation is a script failure —
#         no default-fill.
python3 - "${DESIGN}" "${SCRIPT_DIR}/${DESIGN}" <<'PYEOF'
import json
import re
import sys

design, outdir = sys.argv[1], sys.argv[2]
log = open(f"{outdir}/runs/cts/cts.log").read()


def fail(msg):
    sys.exit(f"cts metrics parse failed: {msg}")


def grab(pattern, what, flags=0):
    m = re.search(pattern, log, flags)
    if m is None:
        fail(f"missing {what}")
    return m


scalars = {}
for line in open(f"{outdir}/runs/cts/cts_scalars.tcl"):
    m = re.match(r"set (\S+) (.+)", line.strip())
    if m:
        scalars[m.group(1)] = m.group(2)
for key in ("def_components", "def_signal_nets", "wcs_setup", "wcs_hold",
            "lat_max", "lat_min", "sink_pins", "clock_net_hpwl_dbu"):
    if key not in scalars:
        fail(f"cts_scalars.tcl missing {key}")

census = []  # (name, master, out_fanout)
for line in open(f"{outdir}/runs/cts/cts_census.txt"):
    parts = line.split()
    if len(parts) != 3:
        fail(f"malformed census line: {line!r}")
    census.append((parts[0], parts[1], int(parts[2])))

root_buf = grab(r"CTS-0050\] Root buffer is (\S+)\.", "root buffer line").group(1)
clock = grab(r'Net "\S+" found for clock "(\S+)"', "clock-net binding").group(1)
buf_list = grab(r"^-buf_list:\s+(.+?)\s*$", "config -buf_list", re.M).group(1).split()
wire_unit = int(grab(r"^-wire_unit:\s+(\d+)\s*$", "config -wire_unit", re.M).group(1))

# report_cts block: scalars, then "Cells used:" / "Dummys used:" per-master counts.
def rcts_count(label):
    return int(grab(rf"Total number of {label}: (\d+)\.", f"report_cts {label}").group(1))


clock_roots = rcts_count("Clock Roots")
buffers_inserted = rcts_count("Buffers Inserted")
clock_subnets = rcts_count("Clock Subnets")
sinks = rcts_count("Sinks")


def master_table(header, what):
    block = grab(rf"{header}\n((?:  \S+: \d+\n)+)", f"report_cts {what}")
    return {m.group(1): int(m.group(2))
            for m in re.finditer(r"  (\S+): (\d+)", block.group(1))}


cells_used = master_table("Cells used:", "cell census")
dummys_used = master_table("Dummys used:", "dummy census")

# report_clock_skew tables (one each for -setup/-hold, in call order): clock
# header, the worst-pair source/target latencies, and the skew line.
skew_tables = re.findall(
    r"Clock (\S+)\n\s*([\d.\-]+) source latency (\S+) [v^]\n\s*([\d.\-]+) target latency (\S+) [v^]\n\s*[\d.\-]+ CRPR\n-+\n\s*([\d.\-]+) (setup|hold) skew",
    log)
if len(skew_tables) != 2:
    fail(f"expected 2 report_clock_skew tables, found {len(skew_tables)}")
report_skew = {}
for clk, src_lat, _sp, tgt_lat, _tp, skew, kind in skew_tables:
    if clk != clock:
        fail(f"skew table clock {clk} != CTS clock {clock}")
    report_skew[kind] = (float(skew), float(src_lat), float(tgt_lat))
if set(report_skew) != {"setup", "hold"}:
    fail(f"skew tables cover {sorted(report_skew)}, need setup+hold")

tree_depth = int(grab(r"CTS-0102\]\s+Path depth (\d+) - (\d+)", "path depth").group(2))
avg_sink_wl = float(grab(r"CTS-0101\]\s+Average sink wire length ([\d.]+) um", "avg sink wirelength").group(1))
dummy_total = int(grab(r"CTS-0207\]\s+Dummy loads inserted (\d+)", "dummy count").group(1))

# Census cross-checks. inserted = tree buffers + dummy loads exactly; any surplus
# is a repair_clock_nets buffer or an unexpected cell and must not be absorbed.
census_tree = {}
census_dummy = {}
for name, master, fan in census:
    target = census_dummy if fan <= 1 else census_tree
    target[master] = target.get(master, 0) + 1
inserted = len(census)
if inserted != buffers_inserted + sum(dummys_used.values()):
    fail(f"inserted {inserted} != report_cts buffers {buffers_inserted} + dummys {sum(dummys_used.values())}")
if census_tree != cells_used:
    print(f"WARNING: census tree buffers {census_tree} != report_cts {cells_used}", file=sys.stderr)
if census_dummy != dummys_used:
    print(f"WARNING: census dummies {census_dummy} != report_cts {dummys_used}", file=sys.stderr)
if dummy_total != sum(dummys_used.values()):
    fail(f"CTS-0207 dummy count {dummy_total} != report_cts dummys {dummys_used}")
if int(scalars["sink_pins"]) != sinks:
    print(f"WARNING: STA clock-pin count {scalars['sink_pins']} != report_cts sinks {sinks}", file=sys.stderr)
for kind in ("setup", "hold"):
    wcs = float(scalars[f"wcs_{kind}"])
    if abs(wcs - report_skew[kind][0]) > 1e-4:
        print(f"WARNING: worst_clock_skew -{kind} {wcs} vs report {report_skew[kind][0]}", file=sys.stderr)

# report_cts carries no clock wirelength; the census Tcl measured per-net HPWL.
wirelength_um = float(scalars["clock_net_hpwl_dbu"]) / 1000.0

out = {
    "design": design,
    "clock": clock,
    "cts_root_buf": root_buf,
    "cts_buf_list": buf_list,
    "cts_wire_unit": wire_unit,
    "inserted_instances": inserted,
    "tree_buffers": cells_used,
    "dummy_loads": dummys_used,
    "clock_subnets": clock_subnets,
    "sinks": sinks,
    "tree_depth": tree_depth,
    "avg_sink_wirelength_um": avg_sink_wl,
    "clock_net_wirelength_um": round(wirelength_um, 6),
    "clock_net_wirelength_basis": "hpwl",
    "skew_setup_ns": float(scalars["wcs_setup"]),
    "skew_hold_ns": float(scalars["wcs_hold"]),
    "latency_max_ns": float(scalars["lat_max"]),
    "latency_min_ns": float(scalars["lat_min"]),
    "def_components": int(scalars["def_components"]),
    "def_signal_nets": int(scalars["def_signal_nets"]),
}
path = f"{outdir}/{design}.cts.reference.json"
with open(path, "w") as fp:
    json.dump(out, fp, indent=2)
    fp.write("\n")
print(f"Wrote {path} (clock_roots={clock_roots})")
PYEOF

# Step C. Propagated-clock STA golden on the committed CTS DEF — a separate
#         process so parasitics state does not inherit step A's estimate (the
#         9b/9c clean-parasitics precedent). The collection procs are the same
#         verbatim bodies as generate_prop_ref.sh (collect_endpoints.tcl lives in
#         runs/canonical and is not guaranteed present).
cat << 'EOF' > "${CTS_RUN_DIR}/cts_collect.tcl"
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

# Per-clock-pin propagated arrival/slew on `all_registers` `is_clock` pins — see
# generate_prop_ref.sh for why this is not a get_pins -hierarchical scan.
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
EOF

openroad -no_init -no_splash << 'EOF'
set D $env(CTS_DESIGN)
source /work/tests/designs/$D/runs/cts/cts_collect.tcl
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.cts.def
read_sdc /work/tests/designs/$D/$D.cts.sdc
set_wire_rc -layer met2
estimate_parasitics -placement

set wns_max [format "%.6f" [sta::worst_slack -max]]
set tns_max [format "%.6f" [sta::total_negative_slack -max]]
set wns_min [format "%.6f" [sta::worst_slack -min]]
set tns_min [format "%.6f" [sta::total_negative_slack -min]]
set endpoints [collect_endpoints_json]
set clock_pins [collect_clock_pins_json]

set fp [open "/work/tests/designs/$D/$D.cts.sta.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"$D\","
puts $fp "  \"cts_placement_rc\": {"
puts $fp "    \"wns_max_ns\": $wns_max,"
puts $fp "    \"tns_max_ns\": $tns_max,"
puts $fp "    \"wns_min_ns\": $wns_min,"
puts $fp "    \"tns_min_ns\": $tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $endpoints
puts $fp "    \],"
puts $fp "    \"clock_pins\": \["
puts $fp $clock_pins
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Wrote $D.cts.sta.reference.json"
exit
EOF

if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.cts.sta.reference.json" ]; then
    echo "Error: step C did not produce ${DESIGN}.cts.sta.reference.json." >&2
    exit 1
fi

echo "==> CTS reference artifacts for ${DESIGN} written."
