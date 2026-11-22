#!/usr/bin/env bash
# Reference generation for the microbench `mba` activity-propagation fixture.
# mba isolates every mechanism of OpenSTA's statistical activity model into
# its own nets: a correlated and/or/inv/xor cone (reconvergent fanout through
# `b`), a plain dfxtp (D annotated), a toggling flop and a 2-bit counter
# (sequential feedback), a conb_1 tie cell feeding bufs, and an undriven net.
# The script runs one openroad process per session inside
# ghcr.io/librelane/librelane:3.0.16 and emits mba.activity.reference.json:
# per-session {annotation split, per-pin "get_property <pin> activity"
# (density, P(1), source), report_power groups}. Every committed number is
# parsed from tool output — nothing is hand-typed.
#
# Sessions: s_none (no annotation), s_global / s_global_density / s_input /
# s_input_ports (the set_power_activity defaults), s_ports / s_ports_a2x
# (input-port VCD seeds), s_full (all 18 nets annotated — the exact-activity
# control), s_partial (inconsistent internal annotation), s_xz (x-run on a).
# s_none's filtered stdout is committed as mba.none.openroad.txt; every other
# session's stdout stays in /tmp and is quoted into docs/results.md.
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
        -w "/work/tests/designs/microbench" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/microbench/generate_activity_ref.sh
    exit 0
fi

cd /work/tests/designs/microbench

# 1. Common reads: tlef + lef + tt lib + mba.def + mba.sdc. The 40 ns variant
#    (generated, not committed) isolates whether the input default tracks the
#    SDC clock period — s_p40 answers "fixed rate vs clock-normalized".
cat > /tmp/mba_reads.tcl <<'TCL'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/microbench/mba.def
read_sdc /work/tests/designs/microbench/mba.sdc
TCL
# The container image has no sed; the period-40 SDC is written out directly
# (mba.sdc with only the period changed).
cat > /tmp/mba40.sdc <<'SDC'
create_clock -name clk -period 40.0 [get_ports clk]
set_input_delay 1.0 -clock clk [all_inputs -no_clocks]
set_output_delay 1.0 -clock clk [all_outputs]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
SDC
cat > /tmp/mba_reads40.tcl <<'TCL'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/microbench/mba.def
read_sdc /tmp/mba40.sdc
TCL

# Per-pin activity dump: `get_property <pin> activity` yields
# "<density/s> <P(1)> <source>" (measured 2026-11-19, docs/results.md N7).
# Object interpolation prints pointers, so every pin is queried by name.
cat > /tmp/mba_dump.tcl <<'TCL'
report_activity_annotation
foreach p {clk a b y yc qd q0 q1 yh yl} {
    puts "ACTP $p [get_property [get_port $p] activity]"
}
foreach p {u_and/A u_and/B u_and/X u_inv/A u_inv/Y u_or/A u_or/B u_or/X \
           u_xor/A u_xor/B u_xor/X u_df/D u_df/CLK u_df/Q u_f0/D u_f0/CLK \
           u_f0/Q u_i0/A u_i0/Y u_f1/D u_f1/CLK u_f1/Q u_x1/A u_x1/B u_x1/X \
           u_c/HI u_c/LO u_bh/A u_bh/X u_bl/A u_bl/X u_flt/A u_flt/X} {
    puts "ACTI $p [get_property [get_pin $p] activity]"
}
report_power -digits 6
TCL

# 2. P0 command-surface discovery — console only, recorded in docs/results.md.
#    Tcl is fed from files, never stdin (AGENTS.md section 7 rule 3).
cat > /tmp/p0.tcl <<'TCL'
source /tmp/mba_reads.tcl
puts "=== info commands *power* ==="
puts [info commands *power*]
puts "=== info commands *activ* ==="
puts [info commands *activ*]
puts "=== info commands *annotation* ==="
puts [info commands *annotation*]
catch {set_power_analysis} m; puts "set_power_analysis ERR:$m"
catch {set_power_activity} m; puts "set_power_activity ERR:$m"
catch {set_power_activity -help} m; puts "set_power_activity -help ERR:$m"
catch {set_power_activity -pins [get_pins u_and/A] -activity 0.9 -duty 0.9} m
puts "set_power_activity -pins ERR:$m"
catch {read_power_activities} m; puts "read_power_activities ERR:$m"
catch {report_activity_annotation -verbose} m; puts "raa -verbose ERR:$m"
catch {report_activity_annotation -report_unannotated} m; puts "raa -ru ERR:$m"
TCL
echo "########## P0: command surface ##########"
openroad -no_init -no_splash -exit /tmp/p0.tcl | tee /tmp/out_p0.txt

# 3. Sessions — one openroad process each; stdout -> /tmp/out_<s>.txt.
run_session() {
    local name="$1" reads="$2"; shift 2
    {
        cat "$reads"
        printf '%s\n' "$@"
        cat /tmp/mba_dump.tcl
    } > "/tmp/session_${name}.tcl"
    openroad -no_init -no_splash -exit "/tmp/session_${name}.tcl" \
        > "/tmp/out_${name}.txt" 2>&1
    echo "----- ${name} done"
}

run_session s_none /tmp/mba_reads.tcl
run_session s_global /tmp/mba_reads.tcl \
    "set_power_activity -global -activity 0.2 -duty 0.4"
run_session s_global_density /tmp/mba_reads.tcl \
    "set_power_activity -global -density 0.03 -duty 0.35"
run_session s_input /tmp/mba_reads.tcl \
    "set_power_activity -input -activity 0.2 -duty 0.4"
run_session s_input_ports /tmp/mba_reads.tcl \
    "set_power_activity -input_ports [all_inputs] -activity 0.2 -duty 0.4"
run_session s_ports /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_ports.vcd"
run_session s_ports_a2x /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_ports_a2x.vcd"
run_session s_full /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_full.vcd"
run_session s_partial /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_partial.vcd"
run_session s_partial2 /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_partial2.vcd"
run_session s_xz /tmp/mba_reads.tcl \
    "read_vcd -scope tb/dut /work/tests/designs/microbench/mba_xz.vcd"
run_session s_p40 /tmp/mba_reads40.tcl

# s_none's stdout is committed FILTERED (INFO/WARNING/banner/blank dropped;
# not verbatim) as mba.none.openroad.txt — same convention as mbw.openroad.txt.
grep -v '^\[INFO\|^\[WARNING\|^The NOWIREEXTENSIONATPIN\|^$' \
    /tmp/out_s_none.txt > /work/tests/designs/microbench/mba.none.openroad.txt

# 4. P0 side-probes that must not pollute the clean sessions: -input alone,
#    -input_ports then -global (source-label precedence), unset behaviour.
cat > /tmp/p0b.tcl <<'TCL'
source /tmp/mba_reads.tcl
puts "=== -input alone ==="
catch {set_power_activity -input -activity 0.9 -duty 0.9} m; puts "in ERR:$m"
puts "IN a [get_property [get_port a] activity]"
puts "IN u_and/A [get_property [get_pin u_and/A] activity]"
report_activity_annotation
puts "=== -global after -input (label precedence) ==="
catch {set_power_activity -global -activity 0.2 -duty 0.4} m; puts "g ERR:$m"
puts "GP a [get_property [get_port a] activity]"
report_activity_annotation
puts "=== unset_power_activity ==="
catch {unset_power_activity} m; puts "unset ERR:$m"
puts "UNSET a [get_property [get_port a] activity]"
report_activity_annotation
TCL
echo "########## P0b: label precedence / unset ##########"
openroad -no_init -no_splash -exit /tmp/p0b.tcl | tee /tmp/out_p0b.txt

# 5. Parse every session into mba.activity.reference.json.
python3 - <<'PY'
import json
import re

ACT = re.compile(r"^ACT([PI]) (\S+) (\S+) (\S+) (\w+)")
ROW = re.compile(
    r"^\s*(Sequential|Combinational|Clock|Macro|Pad|Total)\s+"
    r"(\S+)\s+(\S+)\s+(\S+)\s+(\S+)"
)
SPLIT = re.compile(r"^([a-z_]+)\s+(\d+)\s*$")
SESSIONS = [
    "s_none", "s_global", "s_global_density", "s_input", "s_input_ports",
    "s_ports", "s_ports_a2x", "s_full", "s_partial", "s_partial2", "s_xz",
    "s_p40",
]

out = {}
for s in SESSIONS:
    groups = {}
    annotation = {}
    activity = {}
    for line in open(f"/tmp/out_{s}.txt"):
        m = ACT.match(line)
        if m:
            activity[m.group(2)] = {
                "density": float(m.group(3)),
                "prob": float(m.group(4)),
                "source": m.group(5),
            }
            continue
        m = ROW.match(line)
        if m:
            name = "total" if m.group(1) == "Total" else m.group(1)
            groups[name] = {
                "internal_w": float(m.group(2)),
                "switching_w": float(m.group(3)),
                "leakage_w": float(m.group(4)),
                "total_w": float(m.group(5)),
            }
            continue
        m = SPLIT.match(line)
        if m and "Group" not in line:
            annotation[m.group(1)] = int(m.group(2))
    out[s] = {
        "annotation": annotation,
        "activity": activity,
        "groups": groups,
    }

with open("/work/tests/designs/microbench/mba.activity.reference.json", "w") as f:
    json.dump(out, f, indent=2, sort_keys=True)
    f.write("\n")
print("wrote mba.activity.reference.json")
PY
