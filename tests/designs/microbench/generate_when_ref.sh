#!/usr/bin/env bash
# Reference generation for the microbench `when`-power fixture. mbw is a single
# sky130_fd_sc_hd__ebufn_1 (the library's only `internal_power` `when` shape:
# Z carries an unconditional `related_pin:"A"` block plus `related_pin:"TE_B"`
# blocks gated `when:"A"` and `when:"!A"`). The script runs OpenROAD sessions
# on identical design inputs: `read_vcd -scope tb/dut mbw.vcd` +
# `report_power -digits 6` (filtered stdout committed as mbw.openroad.txt) and
# `read_saif -scope tb/dut mbw.saif` (the SAIF is emitted by the committed
# vcd2saif.py — plain conversion, no flags). Both report tables are parsed
# into mbw.reference.json. A final section re-runs report_power over the
# committed probe VCDs (mbw_duty55/duty80/a0/a0te16/a1/quiet/s00..s11) that
# discriminate OpenSTA's `when` semantics (marginal-vs-joint probability,
# count net, z-edge handling, leakage `when` states); probe stdout is
# recorded in docs/results.md, not committed.
# Runs inside ghcr.io/librelane/librelane:3.0.16.
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
        bash /work/tests/designs/microbench/generate_when_ref.sh
    exit 0
fi

cd /work/tests/designs/microbench

# 1. Common reads: tlef + lef + tt lib + mbw.def + mbw.sdc. mbw.sdc carries a
#    dedicated `clk` port clock (period 20) — required by our SDC subset; it
#    drives no cell so it cannot alias `a`/`teb`/`z` activity.
cat > /tmp/mbw_reads.tcl <<'TCL'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/microbench/mbw.def
read_sdc /work/tests/designs/microbench/mbw.sdc
TCL

# 2. Golden session: VCD-driven report_power. The committed mbw.openroad.txt
#    is FILTERED stdout — grep -v drops INFO/WARNING/banner/blank lines; it is
#    not verbatim.
{
    cat /tmp/mbw_reads.tcl
    echo "read_vcd -scope tb/dut /work/tests/designs/microbench/mbw.vcd"
    echo "report_power -digits 6"
    echo "report_activity_annotation"
} > /tmp/session_vcd.tcl
openroad -no_init -no_splash -exit /tmp/session_vcd.tcl > /tmp/mbw_vcd_power.txt
grep -v '^\[INFO\|^\[WARNING\|^The NOWIREEXTENSIONATPIN\|^$' \
    /tmp/mbw_vcd_power.txt > /work/tests/designs/microbench/mbw.openroad.txt

# 3. SAIF session: vcd2saif.py computes T0/T1/TC from the waveform (nothing
#    hand-typed; no --ghost-scope/--window-end — mb.saif already owns the
#    odd-TC coverage). Table parsed into mbw.reference.json as power_saif.
python3 vcd2saif.py mbw.vcd mbw.saif
{
    cat /tmp/mbw_reads.tcl
    echo "read_saif -scope tb/dut /work/tests/designs/microbench/mbw.saif"
    echo "report_power -digits 6"
    echo "report_activity_annotation"
} > /tmp/session_saif.tcl
openroad -no_init -no_splash -exit /tmp/session_saif.tcl > /tmp/mbw_saif_power.txt

# 4. Parse both report tables into mbw.reference.json.
python3 - <<'PY'
import json
import re

ROW = re.compile(
    r"^\s*(Sequential|Combinational|Clock|Macro|Pad|Total)\s+"
    r"(\S+)\s+(\S+)\s+(\S+)\s+(\S+)"
)


def parse_table(path):
    groups = {}
    for line in open(path):
        m = ROW.match(line)
        if not m:
            continue
        name = "total" if m.group(1) == "Total" else m.group(1)
        groups[name] = {
            "internal_w": float(m.group(2)),
            "switching_w": float(m.group(3)),
            "leakage_w": float(m.group(4)),
            "total_w": float(m.group(5)),
        }
    return groups


out = {
    "power_vcd": parse_table("/tmp/mbw_vcd_power.txt"),
    "power_saif": parse_table("/tmp/mbw_saif_power.txt"),
}
with open("/work/tests/designs/microbench/mbw.reference.json", "w") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
print("wrote mbw.reference.json")
PY

# 5. `when`-semantics probes over the committed variant VCDs (probe.md P1-P4):
#    duty55/duty80 discriminate marginal-vs-ignored/joint probability; a0 /
#    a0te16 / a1 / quiet discriminate the count net and z-edge handling;
#    s00..s11 read the leakage `when` state model. Stdout goes to the console
#    for docs/results.md; nothing else is committed.
for v in duty55 duty80 a0 a0te16 a1 quiet s00 s01 s10 s11; do
    echo "===== probe: $v ====="
    {
        cat /tmp/mbw_reads.tcl
        echo "read_vcd -scope tb/dut /work/tests/designs/microbench/mbw_$v.vcd"
        echo "report_power -digits 6"
        echo "report_activity_annotation"
    } > /tmp/probe_$v.tcl
    openroad -no_init -no_splash -exit /tmp/probe_$v.tcl
done
