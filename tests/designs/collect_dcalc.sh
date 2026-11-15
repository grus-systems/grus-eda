#!/usr/bin/env bash
# Collects OpenSTA `report_dcalc` rows for many arcs of one design, on one SPEF, in the pinned LibreLane image.
#
# usage: tests/designs/collect_dcalc.sh <design> <placed.def> <spef> <out.txt> [stride] [fanout_always]
#   <design>        directory name under tests/designs (gcd, picorv32, ...); its <design>.sdc is read
#   <placed.def>    DEF path relative to tests/designs/<design>
#   <spef>          SPEF path relative to tests/designs/<design>
#   <out.txt>       output path relative to tests/designs/<design>
#   stride          keep one of every `stride` (input pin, output pin) pairs (default 1 = all)
#   fanout_always   keep every pair whose output net has at least this many pins (default 6)
#
# The arc sense is inferred from the transitions report_dcalc prints (in^->out^ and inv->outv: positive_unate; the
# opposite pairing: negative_unate; one input edge with both output edges: non_unate). Pairs that mix senses (several
# arcs, for example mux2 S->X) are skipped because the blocks do not say which arc each belongs to; counts go to the header.
# The output has the columns of picorv32/dcalc_routed_ref.txt:
#   from_pin to_pin arc_sense out_transition input_slew_ns C1_far_pF Rpi_kohm C2_near_pF Ceff_pF delay_ns slew_ns driver_waveform_slew_ns
set -euo pipefail

DESIGN="$1"; DEF="$2"; SPEF="$3"; OUT="$4"; STRIDE="${5:-1}"; FANOUT="${6:-6}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"
RUN_DIR="${SCRIPT_DIR}/${DESIGN}/runs/dcalc_collect"
mkdir -p "${RUN_DIR}"

if [ ! -f /.dockerenv ]; then
    if docker info >/dev/null 2>&1; then docker_cmd="docker"; else docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"; fi
    exec ${docker_cmd} run --rm -v "${REPO_ROOT}:/work" -v "${PDK_HOST_ROOT}:/pdk" -e PDK_ROOT=/pdk \
        "${IMAGE}" bash "/work/tests/designs/collect_dcalc.sh" "$@"
fi

D="/work/tests/designs/${DESIGN}"
cat > "${RUN_DIR}/collect.tcl" <<EOF
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def ${D}/${DEF}
read_sdc ${D}/${DESIGN}.sdc
read_spef ${D}/${SPEF}
set stride ${STRIDE}
set fanout_always ${FANOUT}
set pair 0
set kept 0
foreach inst [get_cells *] {
  set outs [get_pins -of_objects \$inst -filter "direction==output"]
  set ins [get_pins -of_objects \$inst -filter "direction==input"]
  foreach p \$outs {
    set net [get_nets -quiet -of_objects \$p]
    if {[llength \$net] == 0} { continue }
    set fanout [llength [get_pins -of_objects \$net]]
    foreach a \$ins {
      incr pair
      if {\$fanout < \$fanout_always && (\$pair % \$stride) != 0} { continue }
      incr kept
      puts "ARC [get_full_name \$a] [get_full_name \$p]"
      if {[catch {report_dcalc -from \$a -to \$p -max -digits 6} err]} { puts "ERR \$err" }
    }
  }
}
puts "SUMMARY pairs \$pair kept \$kept"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/collect.tcl" > "${RUN_DIR}/collect.log" 2>&1

python3 - "${RUN_DIR}/collect.log" "${SCRIPT_DIR}/${DESIGN}/${OUT}" "${DESIGN}" "${DEF}" "${SPEF}" "${STRIDE}" "${FANOUT}" <<'PYEOF'
import re, sys
log, out, design, d, spef, stride, fanout = sys.argv[1:8]
rows, errs, summary = [], 0, ""
pairs = {}   # (from, to) -> list of block dicts, in order
order = []
cur = None
key = None
def flush():
    global cur
    if cur and key and all(k in cur for k in ("pi", "delay", "slew", "drv", "tin", "out", "edge")):
        pairs.setdefault(key, []).append(cur)
    cur = None
for line in open(log):
    line = line.rstrip("\n")
    if line.startswith("ARC "):
        flush(); key = tuple(line.split()[1:3]); order.append(key)
    elif line.startswith("ERR "):
        errs += 1
    elif line.startswith("SUMMARY"):
        summary = line
    elif re.match(r"^\S+ [\^v] -> \S+ [\^v]$", line):
        flush(); w = line.split()
        cur = {"edge": w[1] + w[4], "out": "rise" if w[4] == "^" else "fall"}
    elif cur is not None:
        m = re.match(r"Pi model C2=(\S+) Rpi=(\S+) C1=(\S+), Ceff=(\S+)", line)
        if m: cur["pi"] = m.groups()
        m = re.match(r"-+ input_net_transition = (\S+)", line)
        if m and "tin" not in cur: cur["tin"] = m.group(1)
        m = re.match(r"Delay = (\S+)", line)
        if m: cur["delay"] = m.group(1)
        m = re.match(r"Slew = (\S+)", line)
        if m: cur["slew"] = m.group(1)
        m = re.match(r"Driver waveform slew = (\S+)", line)
        if m: cur["drv"] = m.group(1)
flush()
# The arc sense is read off the printed transitions; pairs whose blocks mix senses (several arcs) are skipped.
skipped = 0
for key in order:
    blocks = pairs.get(key)
    if not blocks:
        continue
    edges = {b["edge"] for b in blocks}
    if edges <= {"^^", "vv"}: sense = "positive_unate"
    elif edges <= {"^v", "v^"}: sense = "negative_unate"
    elif edges <= {"^^", "^v"} or edges <= {"vv", "v^"}: sense = "non_unate"
    else:
        skipped += 1
        continue
    for b in blocks:
        c2n, rpi, c1f, ceff = b["pi"]
        rows.append(f'{key[0]} {key[1]} {sense} {b["out"]} {b["tin"]} {c1f} {rpi} {c2n} {ceff} {b["delay"]} {b["slew"]} {b["drv"]}')
summary += f"; pairs with no arc or mixed senses skipped {len(order) - len(pairs) + skipped}"
with open(out, "w") as f:
    f.write(f"# OpenSTA report_dcalc rows for {design} on {d} + {spef}, collected with tests/designs/collect_dcalc.sh (stride {stride}, fanout_always {fanout}),\n")
    f.write(f"# pinned LibreLane image ghcr.io/librelane/librelane:3.0.16. {summary}; report_dcalc errors {errs}; rows {len(rows)}.\n")
    f.write("# Columns: from_pin to_pin arc_sense out_transition(rise|fall) input_slew_ns C1_far_pF Rpi_kohm C2_near_pF Ceff_pF delay_ns slew_ns driver_waveform_slew_ns\n")
    f.write("\n".join(rows) + "\n")
print(summary, "errors", errs, "rows", len(rows))
PYEOF
