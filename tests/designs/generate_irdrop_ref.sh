#!/usr/bin/env bash
# PDNSim (IR drop / power-grid) reference generation on a committed placed DEF.
# Measures both current-source conventions on every SPECIALNETS power/ground
# net (gcd: VPWR + VGND):
#
#   (a) embedded        — analyze_power_grid with PDNSim's own per-instance
#                         power estimate (the P-IRD 0.591 mV regime);
#   (b) sta_per_instance — set_pdnsim_inst_power -inst <name> -power <watts>
#                         fed by report_power -instances [get_cells *] from the
#                         same container run;
#   (z) zeroed          — every instance annotated -power 0.0. The decisive
#                         semantics probe: if annotations substituted computed
#                         power the drop would collapse to ~0; measured instead
#                         (2026-11-23): reported total 0.00 W while the solve's
#                         drop is unchanged, i.e. annotations are ADDITIVE to
#                         OpenSTA's computed power. Convention (b) therefore
#                         stacks rather than substitutes, and the E8 engine
#                         mirrors (a) — the embedded path is where an external
#                         per-instance power estimate maps onto the solve.
#
# Step A (openroad process, transcript tee'd to runs/irdrop/irdrop_a.log):
# command-surface probe, check_power_grid verdicts, design + per-instance
# report_power, and the embedded analyze_power_grid (-voltage_file CSV) per net.
# Step B (python3 heredoc) asserts the probe/parser surface, cross-checks the
# per-instance report rows against the DEF COMPONENTS name set, and emits
# runs/irdrop/inst_power.tcl. Step C (a second openroad process, clean state —
# analyze_power_grid carries -allow_reuse state) sources inst_power.tcl and
# re-measures both nets; step C2 (a third process) annotates every instance to
# 0.0 and re-measures. Step D (python3 heredoc) assembles
# <design>.irdrop.reference.json, names/copies the committed voltage CSVs —
# one file per net when the two conventions' maps are byte-identical, two when
# they differ — and publishes the step-B feed as
# <design>.irdrop.inst_power.tcl (the e2e decomposition's reference-power
# input; its size/SHA-256 land in `inst_power_feed`). Every value is emitted
# by the Tcl/python itself; nothing is hand-written.
#
# No LibreLane flow is re-run; only the design's own DEF/SDC and the public
# Sky130 PDK are read. For picorv32 the placed DEF is git-ignored — run
# tests/designs/picorv32/generate_ref.sh first to produce it (gcd's is
# committed). Usage: generate_irdrop_ref.sh <design>   (gcd, picorv32).
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

DESIGN="${1:?usage: generate_irdrop_ref.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
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
        -e IRD_DESIGN="${DESIGN}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_irdrop_ref.sh "${DESIGN}"
    exit 0
fi

export IRD_DESIGN="${IRD_DESIGN:-${DESIGN}}"
IRD_RUN_DIR="/work/tests/designs/${DESIGN}/runs/irdrop"
mkdir -p "${IRD_RUN_DIR}"

# Tcl on stdin: `!` is history expansion (write `== 0`) and `\` does not continue
# long lines — every Tcl command below is one physical line.

# Step A. Command-surface probe + connectivity verdicts + design/per-instance
#         report_power + the embedded convention's analyze_power_grid per net.
#         The transcript is the metrics parser's input.
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${IRD_RUN_DIR}/irdrop_a.log"
set D $env(IRD_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

puts "IRD-SURFACE [info commands *power_grid*]"
puts "IRD-SURFACE [info commands *pdnsim*]"
foreach cmd {check_power_grid analyze_power_grid set_pdnsim_inst_power} {
    if {[llength [info commands $cmd]] == 0} {
        puts "IRD-FATAL missing command: $cmd"
        exit 1
    }
}
# The image has no -help surface (STA-0562): probe flag signatures by executing
# a bare call, which must error rather than silently default. The step-B parser
# asserts each catch reports an error; an unmatched surface aborts the script.
puts "IRD-PROBE set_pdnsim_inst_power [catch {set_pdnsim_inst_power} r]; $r"
puts "IRD-PROBE analyze_power_grid [catch {analyze_power_grid} r]; $r"
puts "IRD-PROBE check_power_grid [catch {check_power_grid} r]; $r"

# Power/ground nets come from the DEF itself (SPECIALNETS + USE), not a
# hardcoded list — the same loop measures VPWR and VGND on gcd and whatever
# the picorv32 DEF carries.
set block [ord::get_db_block]
set pdn_nets {}
foreach net [$block getNets] {
    if {[$net isSpecial]} {
        puts "IRD-NET [$net getName] [$net getSigType]"
        lappend pdn_nets [$net getName]
    }
}
if {[llength $pdn_nets] == 0} {
    puts "IRD-FATAL no SPECIALNETS power/ground nets found"
    exit 1
}

# The catch keeps a connectivity failure from aborting the process: an
# unconnected verdict is a real finding recorded via IRD-CONN, not a script
# failure. code is the catch status, result the command's own return value.
foreach n $pdn_nets {
    set code [catch {check_power_grid -net $n} res]
    puts "IRD-CONN $n code=$code result=$res"
}

report_power -digits 6
report_power -instances [get_cells *] -digits 12 > /work/tests/designs/$D/runs/irdrop/inst_power.rpt

set fp [open "/work/tests/designs/$D/runs/irdrop/a_scalars.tcl" "w"]
puts $fp "set inst_count [llength [get_cells *]]"
puts $fp "set def_components [llength [$block getInsts]]"
close $fp

foreach n $pdn_nets {
    analyze_power_grid -net $n -voltage_file /work/tests/designs/$D/runs/irdrop/[string tolower $n].embedded.csv
}
exit
EOF

# IRD-FATAL matches at line start only: the transcript echoes the `puts
# "IRD-FATAL ..."` commands themselves, so a bare grep would false-positive.
if grep -q "^IRD-FATAL" "${IRD_RUN_DIR}/irdrop_a.log"; then
    echo "Error: step A reported IRD-FATAL (see ${IRD_RUN_DIR}/irdrop_a.log)." >&2
    exit 1
fi

# Step B. Parse the step-A transcript + per-instance report, cross-check the
#         feed against the DEF COMPONENTS name set, and emit inst_power.tcl —
#         one `set_pdnsim_inst_power -inst {<name>} -power <total>` line per
#         report row (braced names: DEF names may carry [ ] escapes). Every
#         unmatched expectation is a script failure — no default-fill.
python3 - "${DESIGN}" <<'PYEOF'
import json
import re
import sys

design = sys.argv[1]
outdir = f"/work/tests/designs/{design}"
run = f"{outdir}/runs/irdrop"
log = open(f"{run}/irdrop_a.log").read()


def fail(msg):
    sys.exit(f"irdrop step-B parse failed: {msg}")


def parse_ir_reports(text):
    # One fenced block per analyze_power_grid call; field lines are
    # "<label padded> : <value> <unit>" with no colons inside labels.
    blocks = {}
    for m in re.finditer(
            r"########## IR report #################\n(.*?)######################################",
            text, re.S):
        fields = {}
        report_lines = []
        for raw in m.group(1).splitlines():
            line = raw.rstrip()
            if not line.strip():
                continue
            report_lines.append(line)
            k, sep, v = line.partition(":")
            if sep:
                fields[k.strip()] = v.strip()
        net = fields.get("Net")
        if net is None:
            fail(f"IR report block without Net line: {report_lines!r}")
        if net in blocks:
            fail(f"duplicate IR report for net {net}")
        blocks[net] = {"fields": fields, "report_lines": report_lines}
    return blocks


def conv_metrics(block, net):
    f = block["fields"]
    need = ("Net", "Corner", "Total power", "Supply voltage",
            "Worstcase voltage", "Average voltage", "Percentage drop")
    for key in need:
        if key not in f:
            fail(f"IR report net {net}: missing field {key!r}")
    # The deviation label pins the tool's own wording; an unrecognized label
    # aborts rather than being mapped silently.
    if "Average IR drop" in f and "Worstcase IR drop" in f:
        wording = "drop"
        avg_key, worst_key = "Average IR drop", "Worstcase IR drop"
    elif "Average IR bounce" in f and "Worstcase IR bounce" in f:
        wording = "bounce"
        avg_key, worst_key = "Average IR bounce", "Worstcase IR bounce"
    else:
        fail(f"IR report net {net}: unrecognized deviation label "
             f"(fields: {sorted(f)})")

    def val(key):
        return float(f[key].split()[0])

    return {
        "total_power_w": val("Total power"),
        "total_power_source": "analyze_report",
        "supply_voltage_v": val("Supply voltage"),
        "worstcase_voltage_v": val("Worstcase voltage"),
        "average_voltage_v": val("Average voltage"),
        "average_drop_v": val(avg_key),
        "worst_drop_v": val(worst_key),
        "drop_percent": val("Percentage drop"),
        "report_wording": wording,
        "corner": f["Corner"],
        "report_lines": block["report_lines"],
    }


# -- probe surface: every IRD-PROBE must be a catch-1, and the [ERROR] line it
#    triggered must sit immediately before it (that [ERROR] text is the only
#    place the tool names the missing argument; the catch result is the bare
#    PSM code).
probes = {}
last_error = None
for line in log.splitlines():
    m = re.match(r"\[ERROR (\S+)\] (.*)", line)
    if m:
        last_error = (m.group(1), m.group(2).strip())
        continue
    # Marker lines match at column 0 only: the transcript echoes the puts
    # commands themselves behind `openroad>` / `...>` prompts, and a mid-line
    # match would pick up the literal command text (e.g. `IRD-NET [$net`).
    m = re.match(r"IRD-PROBE (\S+) (\d+); (.*)", line)
    if m:
        cmd, code, err = m.group(1), int(m.group(2)), m.group(3).strip()
        if code != 1:
            fail(f"probe {cmd}: bare call returned {code}, expected error (1)")
        if last_error is None or last_error[0] != err:
            fail(f"probe {cmd}: catch error {err!r} != last [ERROR] {last_error!r}")
        if "not specified" not in last_error[1]:
            fail(f"probe {cmd}: unexpected error text {last_error[1]!r}")
        probes[cmd] = {"catch": code, "code": err, "error_text": last_error[1]}
for cmd in ("set_pdnsim_inst_power", "analyze_power_grid", "check_power_grid"):
    if cmd not in probes:
        fail(f"missing IRD-PROBE line for {cmd}")

surface = [m.group(1) for m in re.finditer(r"^IRD-SURFACE (.+)", log, re.M)]
if len(surface) != 2:
    fail(f"expected 2 IRD-SURFACE lines, found {len(surface)}")
for cmd in ("analyze_power_grid", "check_power_grid"):
    if cmd not in surface[0].split():
        fail(f"{cmd} missing from *power_grid* surface: {surface[0]}")
if "set_pdnsim_inst_power" not in surface[1].split():
    fail(f"set_pdnsim_inst_power missing from *pdnsim* surface: {surface[1]}")

# -- SPECIALNETS power/ground nets (IRD-NET lines, in odb order).
pdn_nets = [(m.group(1), m.group(2))
            for m in re.finditer(r"^IRD-NET (\S+) (\S+)", log, re.M)]
if not pdn_nets:
    fail("no IRD-NET lines")

# -- connectivity: the standalone check_power_grid verdicts are the first
#    PSM-0040/verdict occurrence per net (analyze_power_grid re-checks and
#    prints the same line later in the transcript).
conn = {}
for m in re.finditer(r"^IRD-CONN (\S+) code=(\d+) result=(.*)", log, re.M):
    conn[m.group(1)] = {"code": int(m.group(2)), "result": m.group(3).strip()}
verdict = {}
for m in re.finditer(r"All shapes on net (\S+) are connected\.", log):
    verdict.setdefault(m.group(1), m.group(0))
for name, _ in pdn_nets:
    if name not in conn:
        fail(f"missing IRD-CONN for {name}")

embedded = parse_ir_reports(log)
for name, _ in pdn_nets:
    if name not in embedded:
        fail(f"no IR report block for net {name} in irdrop_a.log")

# -- design-total cross-check from the standalone report_power table.
m = re.search(r"^Total\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+100\.0%",
              log, re.M)
if m is None:
    fail("report_power design-total row not found")
design_total_w = float(m.group(4))

# -- a_scalars.tcl
scalars = {}
for line in open(f"{run}/a_scalars.tcl"):
    m = re.match(r"set (\S+) (.+)", line.strip())
    if m:
        scalars[m.group(1)] = m.group(2)
for key in ("inst_count", "def_components"):
    if key not in scalars:
        fail(f"a_scalars.tcl missing {key}")
inst_count = int(scalars["inst_count"])
def_components = int(scalars["def_components"])

# -- DEF: COMPONENTS name set + SPECIALNETS USE map.
def_text = open(f"{outdir}/{design}.placed.def").read()
m = re.search(r"COMPONENTS (\d+) ;(.*?)END COMPONENTS", def_text, re.S)
if m is None:
    fail("DEF COMPONENTS section not found")
comp_names = set(re.findall(r"^\s*-\s+(\S+)\s+\S+", m.group(2), re.M))
if int(m.group(1)) != len(comp_names):
    fail(f"DEF COMPONENTS count {m.group(1)} != parsed names {len(comp_names)}")
m = re.search(r"SPECIALNETS \d+ ;(.*?)END SPECIALNETS", def_text, re.S)
if m is None:
    fail("DEF SPECIALNETS section not found")
def_use = {}
for nm, use in re.findall(r"^\s*-\s+(\S+)\s.*?\+\s*USE\s+(\S+)", m.group(1), re.M):
    if nm in def_use and def_use[nm] != use:
        fail(f"DEF net {nm} carries conflicting USE {def_use[nm]} vs {use}")
    def_use[nm] = use

net_use = {}
for name, sigtype in pdn_nets:
    if name not in def_use:
        fail(f"SPECIALNETS net {name} missing from DEF USE map")
    if sigtype != def_use[name]:
        fail(f"net {name}: getSigType {sigtype} != DEF USE {def_use[name]}")
    net_use[name] = sigtype

# -- inst_power.rpt: exactly two header lines + a dash separator, then
#    "<internal> <switching> <leakage> <total> <name>" rows only (the name is
#    the LAST column). Any other shape aborts — no tolerance for table drift.
rpt_lines = open(f"{run}/inst_power.rpt").read().splitlines()
if len(rpt_lines) < 4:
    fail(f"inst_power.rpt too short ({len(rpt_lines)} lines)")
if "Internal" not in rpt_lines[0] or "Power" not in rpt_lines[1]:
    fail(f"inst_power.rpt header unrecognized: {rpt_lines[:2]!r}")
if set(rpt_lines[2].strip()) != {"-"}:
    fail(f"inst_power.rpt separator unrecognized: {rpt_lines[2]!r}")
rows = []
for line in rpt_lines[3:]:
    if not line.strip():
        continue
    parts = line.split()
    if len(parts) != 5:
        fail(f"malformed inst_power row: {line!r}")
    try:
        internal, switching, leakage, total = (float(x) for x in parts[:4])
    except ValueError:
        fail(f"non-numeric inst_power row: {line!r}")
    name = parts[4]
    if name not in comp_names:
        fail(f"inst_power row name {name!r} absent from DEF COMPONENTS")
    rows.append((name, total))
report_rows = len(rows)
if report_rows != inst_count:
    fail(f"inst_power rows {report_rows} != inst_count {inst_count} "
         f"(report_power -instances silently dropped fed instances)")

# -- emit inst_power.tcl; braces protect [ ] in names, reject the one char
#    braces cannot hold.
applied = 0
with open(f"{run}/inst_power.tcl", "w") as fp:
    for name, total in rows:
        if "{" in name or "}" in name:
            fail(f"instance name {name!r} cannot be Tcl-braced")
        fp.write(f"set_pdnsim_inst_power -inst {{{name}}} -power {total}\n")
        applied += 1

json.dump({
    "design": design,
    "pdn_nets": [n for n, _ in pdn_nets],
    "net_use": net_use,
    "probes": probes,
    "connectivity": conn,
    "verdict": verdict,
    "embedded": {n: conv_metrics(embedded[n], n) for n, _ in pdn_nets},
    "inst_power_feed": {
        "source": "report_power -instances [get_cells *] -digits 12",
        "def_components": def_components,
        "inst_count": inst_count,
        "report_rows": report_rows,
        "applied": applied,
        "design_report_total_w": design_total_w,
        "inst_feed_total_w": sum(r[1] for r in rows),
    },
}, open(f"{run}/step_a.json", "w"), indent=2)
print(f"step B: {applied} set_pdnsim_inst_power lines emitted "
      f"(report_rows={report_rows}, inst_count={inst_count}, "
      f"def_components={def_components}, design_total={design_total_w:.6e})")
PYEOF

for f in "${IRD_RUN_DIR}/inst_power.tcl" "${IRD_RUN_DIR}/step_a.json"; do
    if [ ! -f "${f}" ]; then
        echo "Error: step B did not produce ${f}." >&2
        exit 1
    fi
done

# Step C. The explicit convention in a second, clean-state process (the
#         9b/9c clean-parasitics precedent — analyze_power_grid's -allow_reuse
#         keeps solution state within a process).
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${IRD_RUN_DIR}/irdrop_b.log"
set D $env(IRD_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

set block [ord::get_db_block]
set pdn_nets {}
foreach net [$block getNets] {
    if {[$net isSpecial]} {
        lappend pdn_nets [$net getName]
    }
}

# A set_pdnsim_inst_power error here (e.g. a findInst miss on a name-format
# edge) aborts the process uncaught — investigate, do not catch.
source /work/tests/designs/$D/runs/irdrop/inst_power.tcl

foreach n $pdn_nets {
    analyze_power_grid -net $n -voltage_file /work/tests/designs/$D/runs/irdrop/[string tolower $n].sta.csv
}
exit
EOF

# Step C2. The semantics probe in a third clean-state process: annotate every
#          instance to exactly 0.0. Substitution semantics would collapse the
#          drop to ~0; the additive behavior measured on 2026-11-23 instead
#          leaves the solve on OpenSTA's computed power (drop identical to
#          embedded) while the reported total reads 0.00 W.
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${IRD_RUN_DIR}/irdrop_c.log"
set D $env(IRD_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

set block [ord::get_db_block]
set pdn_nets {}
foreach net [$block getNets] {
    if {[$net isSpecial]} {
        lappend pdn_nets [$net getName]
    }
}

set zeroed 0
foreach inst [$block getInsts] {
    set_pdnsim_inst_power -inst [$inst getName] -power 0.0
    incr zeroed
}
puts "IRD-ZERO applied=$zeroed"

foreach n $pdn_nets {
    analyze_power_grid -net $n -voltage_file /work/tests/designs/$D/runs/irdrop/[string tolower $n].zeroed.csv
}
exit
EOF

# Step D. Parse the explicit-convention transcript, compare each net's two
#         voltage CSVs byte-for-byte (identical -> one committed file,
#         conventions_share_voltage_map=true; differing -> two), stamp
#         size/SHA-256/row count, and write <design>.irdrop.reference.json.
python3 - "${DESIGN}" <<'PYEOF'
import hashlib
import json
import re
import shutil
import sys

design = sys.argv[1]
outdir = f"/work/tests/designs/{design}"
run = f"{outdir}/runs/irdrop"
step_a = json.load(open(f"{run}/step_a.json"))
log_b = open(f"{run}/irdrop_b.log").read()
log_c = open(f"{run}/irdrop_c.log").read()
pdn_nets = step_a["pdn_nets"]
net_use = step_a["net_use"]


def fail(msg):
    sys.exit(f"irdrop step-D parse failed: {msg}")


def parse_ir_reports(text):
    blocks = {}
    for m in re.finditer(
            r"########## IR report #################\n(.*?)######################################",
            text, re.S):
        fields = {}
        report_lines = []
        for raw in m.group(1).splitlines():
            line = raw.rstrip()
            if not line.strip():
                continue
            report_lines.append(line)
            k, sep, v = line.partition(":")
            if sep:
                fields[k.strip()] = v.strip()
        net = fields.get("Net")
        if net is None:
            fail(f"IR report block without Net line: {report_lines!r}")
        if net in blocks:
            fail(f"duplicate IR report for net {net}")
        blocks[net] = {"fields": fields, "report_lines": report_lines}
    return blocks


def conv_metrics(block, net):
    f = block["fields"]
    need = ("Net", "Corner", "Total power", "Supply voltage",
            "Worstcase voltage", "Average voltage", "Percentage drop")
    for key in need:
        if key not in f:
            fail(f"IR report net {net}: missing field {key!r}")
    if "Average IR drop" in f and "Worstcase IR drop" in f:
        wording = "drop"
        avg_key, worst_key = "Average IR drop", "Worstcase IR drop"
    elif "Average IR bounce" in f and "Worstcase IR bounce" in f:
        wording = "bounce"
        avg_key, worst_key = "Average IR bounce", "Worstcase IR bounce"
    else:
        fail(f"IR report net {net}: unrecognized deviation label "
             f"(fields: {sorted(f)})")

    def val(key):
        return float(f[key].split()[0])

    return {
        "total_power_w": val("Total power"),
        "total_power_source": "analyze_report",
        "supply_voltage_v": val("Supply voltage"),
        "worstcase_voltage_v": val("Worstcase voltage"),
        "average_voltage_v": val("Average voltage"),
        "average_drop_v": val(avg_key),
        "worst_drop_v": val(worst_key),
        "drop_percent": val("Percentage drop"),
        "report_wording": wording,
        "corner": f["Corner"],
        "report_lines": block["report_lines"],
    }


def apply_semantics(metrics, net):
    # The tool prints "IR drop" labels for both rails; drop_semantics records
    # what the deviation physically is — a fall below the supply on a POWER
    # rail, a rise above the 0 V rail on a GROUND rail — and the sign check
    # aborts if the physics does not match the rail's USE.
    supply, worst = metrics["supply_voltage_v"], metrics["worstcase_voltage_v"]
    if net_use[net] == "POWER":
        if worst - supply > 1e-9:
            fail(f"net {net} is POWER but worst voltage {worst} > supply {supply}")
        metrics["drop_semantics"] = "drop"
    elif net_use[net] == "GROUND":
        if worst - supply < -1e-9:
            fail(f"net {net} is GROUND but worst voltage {worst} < supply {supply}")
        metrics["drop_semantics"] = "bounce"
    else:
        fail(f"net {net} has unrecognized USE {net_use[net]!r}")


sta_blocks = parse_ir_reports(log_b)
zero_blocks = parse_ir_reports(log_c)
for n in pdn_nets:
    if n not in sta_blocks:
        fail(f"no IR report block for net {n} in irdrop_b.log")
    if n not in zero_blocks:
        fail(f"no IR report block for net {n} in irdrop_c.log")
m = re.search(r"^IRD-ZERO applied=(\d+)", log_c, re.M)
if m is None:
    fail("missing IRD-ZERO applied line in irdrop_c.log")
zero_applied = int(m.group(1))
if zero_applied != step_a["inst_power_feed"]["def_components"]:
    fail(f"zeroed annotations {zero_applied} != def_components "
         f"{step_a['inst_power_feed']['def_components']}")

nets_json = {}
for n in pdn_nets:
    low = n.lower()
    emb_bytes = open(f"{run}/{low}.embedded.csv", "rb").read()
    sta_bytes = open(f"{run}/{low}.sta.csv", "rb").read()
    zero_bytes = open(f"{run}/{low}.zeroed.csv", "rb").read()
    identical = emb_bytes == sta_bytes

    def csv_stat(data, tag):
        text = data.decode()
        header = text.splitlines()[0]
        if header != "Instance,Terminal,Layer,X location,Y location,Voltage":
            fail(f"{n} {tag} voltage CSV has an unexpected header: {header!r}")
        return (len(text.splitlines()) - 1, len(data),
                hashlib.sha256(data).hexdigest()[:16])

    emb_metrics = step_a["embedded"][n]
    sta_metrics = conv_metrics(sta_blocks[n], n)
    apply_semantics(emb_metrics, n)
    apply_semantics(sta_metrics, n)

    if identical:
        fname = f"{design}.irdrop.voltage.{low}.csv"
        shutil.copyfile(f"{run}/{low}.embedded.csv", f"{outdir}/{fname}")
        rows, size, sha = csv_stat(emb_bytes, "embedded")
        for mm in (emb_metrics, sta_metrics):
            mm["csv_file"] = fname
            mm["csv_rows"] = rows
            mm["csv_size_bytes"] = size
            mm["csv_sha256_16"] = sha
    else:
        for tag, data, src in (("embedded", emb_bytes, f"{run}/{low}.embedded.csv"),
                               ("sta", sta_bytes, f"{run}/{low}.sta.csv")):
            fname = f"{design}.irdrop.voltage.{low}.{tag}.csv"
            shutil.copyfile(src, f"{outdir}/{fname}")
            rows, size, sha = csv_stat(data, tag)
            target = emb_metrics if tag == "embedded" else sta_metrics
            target["csv_file"] = fname
            target["csv_rows"] = rows
            target["csv_size_bytes"] = size
            target["csv_sha256_16"] = sha

    zeroed_probe = conv_metrics(zero_blocks[n], n)
    apply_semantics(zeroed_probe, n)
    zeroed_probe["voltage_map_identical_to_embedded"] = zero_bytes == emb_bytes
    zeroed_probe["csv_rows"] = len(zero_bytes.decode().splitlines()) - 1
    zeroed_probe["csv_sha256_16"] = hashlib.sha256(zero_bytes).hexdigest()[:16]

    delta = {
        "worst_drop_v": sta_metrics["worst_drop_v"] - emb_metrics["worst_drop_v"],
        "average_drop_v": sta_metrics["average_drop_v"] - emb_metrics["average_drop_v"],
        "total_power_w": sta_metrics["total_power_w"] - emb_metrics["total_power_w"],
        "average_voltage_v": sta_metrics["average_voltage_v"] - emb_metrics["average_voltage_v"],
        "voltage_maps_identical": identical,
    }
    nets_json[n] = {
        "use": net_use[n],
        "connectivity": step_a["verdict"].get(n),
        "connectivity_code": step_a["connectivity"][n]["code"],
        "connectivity_result": step_a["connectivity"][n]["result"],
        "conventions": {
            "embedded": emb_metrics,
            "sta_per_instance": sta_metrics,
        },
        "zeroed_probe": zeroed_probe,
        "delta": delta,
        "conventions_share_voltage_map": identical,
    }
    print(f"IRD-SUMMARY {n}: embedded worst_drop={emb_metrics['worst_drop_v']:.4e} "
          f"sta worst_drop={sta_metrics['worst_drop_v']:.4e} "
          f"delta={delta['worst_drop_v']:.4e} maps_identical={identical}")

# -- publish the step-B per-instance feed as a committed golden input: the
#    e2e decomposition re-solves our mesh on this exact file, so its size and
#    SHA-256 are recorded beside the coverage/total checks.
feed_bytes = open(f"{run}/inst_power.tcl", "rb").read()
feed_name = f"{design}.irdrop.inst_power.tcl"
shutil.copyfile(f"{run}/inst_power.tcl", f"{outdir}/{feed_name}")
inst_power_feed = dict(step_a["inst_power_feed"])
inst_power_feed["feed_file"] = feed_name
inst_power_feed["feed_size_bytes"] = len(feed_bytes)
inst_power_feed["feed_sha256_16"] = hashlib.sha256(feed_bytes).hexdigest()[:16]

out = {
    "design": design,
    "input_def": f"{design}.placed.def",
    "input_sdc": f"{design}.sdc",
    "image": "ghcr.io/librelane/librelane:3.0.16",
    # Mirrored convention: embedded. The sta_per_instance + zeroed_probe
    # measurements show set_pdnsim_inst_power is additive to OpenSTA's computed
    # power (annotating all-zero leaves the solve untouched while zeroing the
    # reported total), so the explicit feed stacks rather than substitutes —
    # an external per-instance estimator maps onto the embedded path.
    "convention_mirrored": "embedded",
    "conventions": ["embedded", "sta_per_instance"],
    "inst_power_feed": inst_power_feed,
    "nets": nets_json,
}
path = f"{outdir}/{design}.irdrop.reference.json"
with open(path, "w") as fp:
    json.dump(out, fp, indent=2)
    fp.write("\n")
print(f"Wrote {path}")
PYEOF

if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.irdrop.reference.json" ]; then
    echo "Error: step D did not produce ${DESIGN}.irdrop.reference.json." >&2
    exit 1
fi

echo "==> IR-drop reference artifacts for ${DESIGN} written."
