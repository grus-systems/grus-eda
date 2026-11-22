#!/usr/bin/env bash
# Detailed-routing delegation reference (E6 ticket 1). Runs two routing
# variants per design inside ghcr.io/librelane/librelane:3.0.16:
#
#   baseline  — the reference's natural input: global_route -guide_file
#               + detailed_route (the verbatim step-5b spelling of each
#               design's generate_ref.sh: set_routing_layers -signal
#               met1-met5 -clock met3-met5, no layer adjustments).
#   delegated — the E6 product path: read_guides on our own emitted guide
#               (runs/guide-check/<d>.ours.guide, `grus-eda route
#               --guide-out`) + the same detailed_route.
#
# Both processes are clean-state (the 9b/9c / generate_cts_ref.sh step-C
# precedent), end with check_antennas + write_def, and tee their
# transcripts to runs/drt/{baseline,delegated}.log — the metrics parser's
# input. Step C (python3 heredoc) assembles <design>.drt.reference.json:
# per variant the DRC count (non-blank lines of the -output_drc report,
# empty = 0 — the generate_ref.sh convention), antenna counts (the ANT-
# log grammar), routed-DEF wirelength and per-via-name via counts (parsed
# off the tool's own emitted DEF, arithmetic documented in
# kurena/tasks/e6-drt-ref/design/golden.md), routed-DEF SHA-256 prefix +
# size, and per-phase runtime from DRT-TIME markers. Every value is
# script-emitted; nothing is hand-written. `timing` is the named
# non-reproducing field — everything else must git-diff clean on rerun.
#
# Routed DEFs (<d>.drt.<variant>.def) are gitignored (>1 MB on picorv32);
# DRC/antenna reports are copied up to <d>.drc.<variant>.rpt /
# <d>.antenna.<variant>.rpt only when under 1 MB, else they stay under
# the gitignored runs/drt/ with size/SHA recorded (a flooded report is
# data, not a failure). A nonzero DRC or antenna count on the delegated
# path is recorded data, not a blocker; an unclean openroad exit still
# aborts the script (pipefail) with its partial transcript under
# runs/drt/.
#
# The delegated input is tests/designs/<d>/runs/guide-check/<d>.ours.guide
# — regenerated host-side via tests/designs/generate_guide_ours_spef.sh
# when absent (release build; it also re-emits the deterministic
# <d>.guide.ours.spef). For picorv32 the placed DEF is gitignored — run
# tests/designs/picorv32/generate_ref.sh first (gcd's is committed).
# A prior <d>.routed.def, when present in the worktree, is byte-compared
# against the baseline DEF as a determinism cross-check — it is a
# cross-check, not an input.
#
# Usage: generate_drt_ref.sh <design>   (gcd, picorv32)
set -euo pipefail

DESIGN="${1:?usage: generate_drt_ref.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
    if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${f}" ]; then
        echo "Error: missing ${SCRIPT_DIR}/${DESIGN}/${f} (picorv32's placed DEF regenerates via tests/designs/${DESIGN}/generate_ref.sh)" >&2
        exit 1
    fi
done

# The delegated variant's input; regenerate host-side when absent.
OURS_GUIDE="${SCRIPT_DIR}/${DESIGN}/runs/guide-check/${DESIGN}.ours.guide"
if [ ! -f "${OURS_GUIDE}" ]; then
    echo "==> ${OURS_GUIDE} missing; running generate_guide_ours_spef.sh ${DESIGN} (host, release build)..."
    bash "${SCRIPT_DIR}/generate_guide_ours_spef.sh" "${DESIGN}"
fi
if [ ! -f "${OURS_GUIDE}" ]; then
    echo "Error: ${OURS_GUIDE} still missing after generate_guide_ours_spef.sh ${DESIGN}." >&2
    exit 1
fi

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
        -e DRT_DESIGN="${DESIGN}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_drt_ref.sh "${DESIGN}"
    exit 0
fi

export DRT_DESIGN="${DRT_DESIGN:-${DESIGN}}"
DRT_RUN_DIR="/work/tests/designs/${DESIGN}/runs/drt"
mkdir -p "${DRT_RUN_DIR}"

# Tcl on stdin: `!` is history expansion (write `== 0`) and `\` does not
# continue long lines — every Tcl command below is one physical line.
# Marker lines (DRT-*) match at `^` in the parser only: the transcript
# echoes the puts commands themselves behind openroad>/...> prompts.

# Step A. Baseline: global_route + detailed_route (the step-5b spelling),
#         then check_antennas + write_def. stdout+stderr tee'd to
#         baseline.log.
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${DRT_RUN_DIR}/baseline.log"
set D $env(DRT_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

puts "DRT-SURFACE [info commands *route*]"
puts "DRT-SURFACE [info commands *antenna*]"
puts "DRT-SURFACE [info commands *guide*]"
puts "DRT-SURFACE [info commands *wire_length*]"
foreach cmd {global_route read_guides detailed_route check_antennas write_def} {
    if {[llength [info commands $cmd]] == 0} {
        puts "DRT-FATAL missing command: $cmd"
        exit 1
    }
}
# Bare call before routing must error (ANT-0008), not silently default; the
# step-C parser records the probe verbatim.
puts "DRT-PROBE check_antennas [catch {check_antennas} r]; $r"

set_routing_layers -signal met1-met5 -clock met3-met5
set t0 [clock milliseconds]
global_route -guide_file /work/tests/designs/$D/runs/drt/$D.baseline.guide -congestion_iterations 100
set t1 [clock milliseconds]
detailed_route -output_drc /work/tests/designs/$D/runs/drt/$D.baseline.drc.rpt -verbose 0
set t2 [clock milliseconds]
check_antennas -report_file /work/tests/designs/$D/runs/drt/$D.baseline.antenna.rpt
set t3 [clock milliseconds]
write_def /work/tests/designs/$D/$D.drt.baseline.def
puts "DRT-TIME global_route_ms [expr {$t1 - $t0}]"
puts "DRT-TIME detailed_route_ms [expr {$t2 - $t1}]"
puts "DRT-TIME check_antennas_ms [expr {$t3 - $t2}]"
puts "DRT-TIME total_route_ms [expr {$t3 - $t0}]"
exit
EOF

if grep -q "^DRT-FATAL" "${DRT_RUN_DIR}/baseline.log"; then
    echo "Error: step A reported DRT-FATAL (see ${DRT_RUN_DIR}/baseline.log)." >&2
    exit 1
fi

# Step B. Delegated: read_guides on our emitted guide + the same
#         detailed_route, check_antennas, write_def — a second clean-state
#         process tee'd to delegated.log. set_routing_layers stays in the
#         prelude so the layer window is identical on both variants (the
#         delta then means guide provenance, not routing setup).
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${DRT_RUN_DIR}/delegated.log"
set D $env(DRT_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc

puts "DRT-SURFACE [info commands *route*]"
puts "DRT-SURFACE [info commands *antenna*]"
puts "DRT-SURFACE [info commands *guide*]"
puts "DRT-SURFACE [info commands *wire_length*]"
foreach cmd {global_route read_guides detailed_route check_antennas write_def} {
    if {[llength [info commands $cmd]] == 0} {
        puts "DRT-FATAL missing command: $cmd"
        exit 1
    }
}
puts "DRT-PROBE check_antennas [catch {check_antennas} r]; $r"

set_routing_layers -signal met1-met5 -clock met3-met5
set t0 [clock milliseconds]
read_guides /work/tests/designs/$D/runs/guide-check/$D.ours.guide
set t1 [clock milliseconds]
detailed_route -output_drc /work/tests/designs/$D/runs/drt/$D.delegated.drc.rpt -verbose 0
set t2 [clock milliseconds]
check_antennas -report_file /work/tests/designs/$D/runs/drt/$D.delegated.antenna.rpt
set t3 [clock milliseconds]
write_def /work/tests/designs/$D/$D.drt.delegated.def
puts "DRT-TIME read_guides_ms [expr {$t1 - $t0}]"
puts "DRT-TIME detailed_route_ms [expr {$t2 - $t1}]"
puts "DRT-TIME check_antennas_ms [expr {$t3 - $t2}]"
puts "DRT-TIME total_route_ms [expr {$t3 - $t0}]"
exit
EOF

if grep -q "^DRT-FATAL" "${DRT_RUN_DIR}/delegated.log"; then
    echo "Error: step B reported DRT-FATAL (see ${DRT_RUN_DIR}/delegated.log)." >&2
    exit 1
fi

# Step C. Assemble <design>.drt.reference.json. Every unmatched
#         expectation aborts — no default-fill.
python3 - "${DESIGN}" <<'PYEOF'
import hashlib
import json
import os
import re
import shutil
import sys

design = sys.argv[1]
outdir = f"/work/tests/designs/{design}"
run = f"{outdir}/runs/drt"
IMAGE = "ghcr.io/librelane/librelane:3.0.16"
TLEF = "/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
COMMIT_LIMIT = 1_000_000

LOG_CODES = re.compile(r"^\[(INFO|WARNING|ERROR) (\S+?)-")


def fail(msg):
    sys.exit(f"drt step-C parse failed: {msg}")


def sha16(path):
    h = hashlib.sha256()
    with open(path, "rb") as fp:
        for chunk in iter(lambda: fp.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()[:16]


def stamp(path):
    return {"size_bytes": os.path.getsize(path), "sha256_16": sha16(path)}


def require(path):
    if not os.path.isfile(path):
        fail(f"missing artifact {path}")
    return path


def nonblank_lines(path):
    return sum(1 for line in open(path) if line.strip())


# -- transcripts -------------------------------------------------------

logs = {}
for variant in ("baseline", "delegated"):
    logs[variant] = open(require(f"{run}/{variant}.log")).read()


def code_lines(log, prefix):
    out = []
    for line in log.splitlines():
        m = LOG_CODES.match(line)
        if m and m.group(2).startswith(prefix):
            out.append(line.rstrip())
    return out


def variant_surface(log, variant):
    surface = re.findall(r"^DRT-SURFACE (.+)$", log, re.M)
    if len(surface) != 4:
        fail(f"{variant}: expected 4 DRT-SURFACE lines, found {len(surface)}")
    route_cmds = surface[0].split()
    for cmd in ("global_route", "detailed_route"):
        if cmd not in route_cmds:
            fail(f"{variant}: {cmd} missing from *route* surface: {surface[0]!r}")
    if "check_antennas" not in surface[1].split():
        fail(f"{variant}: check_antennas missing from *antenna* surface")
    if "read_guides" not in surface[2].split():
        fail(f"{variant}: read_guides missing from *guide* surface")
    return {"route": surface[0], "antenna": surface[1],
            "guide": surface[2], "wire_length": surface[3]}


def variant_probe(log, variant):
    probes = {}
    last_error = None
    for line in log.splitlines():
        m = re.match(r"\[ERROR (\S+)\] (.*)", line)
        if m:
            last_error = (m.group(1), m.group(2).strip())
            continue
        m = re.match(r"DRT-PROBE (\S+) (\d+); (.*)", line)
        if m:
            cmd, code, res = m.group(1), int(m.group(2)), m.group(3).strip()
            probes[cmd] = {"catch": code, "result": res,
                           "error_text": last_error[1] if last_error and last_error[0] == res else None}
    if "check_antennas" not in probes:
        fail(f"{variant}: missing DRT-PROBE line for check_antennas")
    return probes["check_antennas"]


def variant_times(log, variant):
    times = dict(re.findall(r"^DRT-TIME (\S+) (\d+)$", log, re.M))
    need = ("detailed_route_ms", "check_antennas_ms", "total_route_ms")
    need += ("global_route_ms",) if variant == "baseline" else ("read_guides_ms",)
    for key in need:
        if key not in times:
            fail(f"{variant}: missing DRT-TIME {key}")
    out = {k: round(int(v) / 1000, 3) for k, v in times.items()}
    first = "global_route_ms" if variant == "baseline" else "read_guides_ms"
    return {"global_route_s" if variant == "baseline" else "read_guides_s": out[first],
            "detailed_route_s": out["detailed_route_ms"],
            "check_antennas_s": out["check_antennas_ms"],
            "total_route_s": out["total_route_ms"]}


# -- LEF via map (via-def name -> cut layer) -----------------------------
# VIA <name> blocks in the techlef list the cut layer first, then the two
# routing layers (VIA L1M1_PR_MR: LAYER mcon ; LAYER li1 ; LAYER met1 ;).

via_meaning = {}
tlef_text = open(require(TLEF)).read()
for m in re.finditer(r"^VIA (\S+).*?^END \1", tlef_text, re.M | re.S):
    layers = re.findall(r"^\s*LAYER (\S+)\s*;", m.group(0), re.M)
    if len(layers) < 3:
        fail(f"LEF VIA {m.group(1)} has {len(layers)} LAYER lines, expected >= 3")
    via_meaning[m.group(1)] = layers[0]


# -- routed-DEF metrics ---------------------------------------------------
# Arithmetic per kurena/tasks/e6-drt-ref/design/golden.md: inside NETS, each
# `+ ROUTED <layer>` / `NEW <layer>` chain is a point list; `*` repeats the
# corresponding axis of the previous point (across NEW breaks). Manhattan
# distance between consecutive same-chain points sums to wirelength; via
# tokens and NEW breaks contribute 0. A bare identifier token following a
# point is a via-def name. A non-Manhattan point pair or an unrecognized
# token aborts — grammar surprise, not data.

PATH_KEYWORDS = {"NEW", "MASK", "VIRTUAL", "TAPER", "TAPERRULE",
                 "POLYGON", "PATTERN", "STYLE", "SOURCE", "USE", "SHIELD",
                 "VPIN", "SUBNET", "NOSHIELD", "NONDEFAULTRULE", "SYNTHESIZED",
                 "PROPERTY", "ESTCAP"}


def def_units(text):
    m = re.search(r"^UNITS DISTANCE MICRONS (\d+)\s*;", text, re.M)
    if m is None:
        fail("DEF UNITS DISTANCE MICRONS line not found")
    return int(m.group(1))


def nets_entries(text):
    m = re.search(r"^NETS (\d+) ;(.*?)^END NETS", text, re.M | re.S)
    if m is None:
        fail("DEF NETS section not found")
    entries = re.split(r"^\s+- ", m.group(2), flags=re.M)[1:]
    if int(m.group(1)) != len(entries):
        fail(f"DEF NETS count {m.group(1)} != parsed entries {len(entries)}")
    return entries


def parse_path(tokens, net_name):
    length_dbu = 0
    via_counts = {}
    last_point = None   # '*' resolution carries across NEW breaks
    seg_prev = None     # continuity only within one ROUTED/NEW chain
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if tok == ";":
            i += 1
            break
        if tok == "+":
            kw = tokens[i + 1] if i + 1 < len(tokens) else ""
            if kw in ("ROUTED", "COVERED", "FIXED"):
                if i + 2 >= len(tokens):
                    fail(f"net {net_name}: +{kw} without layer token")
                i += 3
                if i < len(tokens) and re.fullmatch(r"\d+", tokens[i]):
                    i += 1  # optional path width
                seg_prev = None
                continue
            if kw in ("SHAPE", "MASK"):
                i += 3
                continue
            fail(f"net {net_name}: unrecognized +{kw} in routed path: "
                 f"{' '.join(tokens[max(0, i - 4):i + 8])!r}")
        elif tok == "NEW":
            if i + 1 >= len(tokens):
                fail(f"net {net_name}: NEW without layer token")
            i += 2
            if i < len(tokens) and re.fullmatch(r"\d+", tokens[i]):
                i += 1
            seg_prev = None
        elif tok == "(":
            # ( x y ) or ( x y ext ) — the optional third element is the DEF
            # wire-extension value at the point, not a distance axis.
            j = i + 1
            coords = []
            while j < len(tokens) and tokens[j] != ")":
                coords.append(tokens[j])
                j += 1
            if j >= len(tokens) or len(coords) not in (2, 3):
                fail(f"net {net_name}: malformed point at token {i}: "
                     f"{' '.join(tokens[i:i + 7])!r}")
            if len(coords) == 3:
                if coords[2] == "*":
                    fail(f"net {net_name}: '*' extension coordinate: "
                         f"{' '.join(coords)!r}")
                if int(coords[2]) != 0:
                    fail(f"net {net_name}: nonzero wire-extension value "
                         f"{coords[2]!r} (tapered wire is unmeasured grammar)")
            xy = []
            for axis, t in enumerate(coords[:2]):
                if t == "*":
                    if last_point is None:
                        fail(f"net {net_name}: '*' coordinate with no previous point")
                    xy.append(last_point[axis])
                else:
                    xy.append(int(t))
            pt = (xy[0], xy[1])
            if seg_prev is not None:
                dx, dy = abs(pt[0] - seg_prev[0]), abs(pt[1] - seg_prev[1])
                if dx and dy:
                    fail(f"net {net_name}: non-Manhattan segment "
                         f"{seg_prev} -> {pt} in routed path")
                length_dbu += dx + dy
            seg_prev = pt
            last_point = pt
            i = j + 1
        elif tok == "RECT":
            # Pin-access patch after a point: RECT ( dx1 dy1 dx2 dy2 ),
            # zero path length like a via token.
            if (i + 6 >= len(tokens) or tokens[i + 1] != "("
                    or tokens[i + 6] != ")"):
                fail(f"net {net_name}: malformed RECT at token {i}: "
                     f"{' '.join(tokens[i:i + 8])!r}")
            i += 7
        elif tok in PATH_KEYWORDS or tok == ")":
            fail(f"net {net_name}: unexpected token {tok!r} in routed path")
        elif re.fullmatch(r"[A-Za-z_][\w.\[\]/:]*", tok):
            via_counts[tok] = via_counts.get(tok, 0) + 1
            i += 1
        else:
            fail(f"net {net_name}: unrecognized token {tok!r} in routed path")
    return length_dbu, via_counts


def routed_metrics(def_path):
    text = open(require(def_path)).read()
    dbu = def_units(text)
    # DEF-written via names (VIAS section) map to their + VIARULE, not a LEF
    # cut layer — kept under their own key, never dropped.
    def_vias = {}
    m = re.search(r"^VIAS \d+ ;(.*?)^END VIAS", text, re.M | re.S)
    if m:
        for vm in re.finditer(r"^\s+- (\S+)\s+\+ VIARULE (\S+)", m.group(1), re.M):
            def_vias[vm.group(1)] = f"def_viarule:{vm.group(2)}"
    total_dbu = 0
    via_counts = {}
    routed = 0
    for entry in nets_entries(text):
        net = entry.split()[0]
        idx = entry.find("+ ROUTED")
        if idx == -1:
            continue
        routed += 1
        tokens = re.sub(r"([();])", r" \1 ", entry[idx:]).split()
        if tokens[0] != "+" or tokens[1] != "ROUTED":
            fail(f"net {net}: + ROUTED present but token parse lost it")
        length, vias = parse_path(tokens, net)
        total_dbu += length
        for name, n in vias.items():
            via_counts[name] = via_counts.get(name, 0) + n
    return {"signal_nets_routed": routed,
            "total_wirelength_um": round(total_dbu / dbu, 6),
            "dbu_per_micron": dbu,
            "vias_per_cut_layer": dict(sorted(via_counts.items())),
            "via_count_total": sum(via_counts.values()),
            "def_vias": def_vias}


# -- signal nets in the placed DEF ---------------------------------------

placed_text = open(require(f"{outdir}/{design}.placed.def")).read()
signal_nets_in_def = len(nets_entries(placed_text))


def via_meaning_for(name, def_vias):
    if name in via_meaning:
        return via_meaning[name]
    if name in def_vias:
        return def_vias[name]
    return "unknown"


# -- per-variant assembly ------------------------------------------------

variants = {}
for variant in ("baseline", "delegated"):
    log = logs[variant]
    surface = variant_surface(log, variant)
    probe = variant_probe(log, variant)
    timing = variant_times(log, variant)
    ant_lines = code_lines(log, "ANT")
    drt_lines = code_lines(log, "DRT")
    grt_lines = code_lines(log, "GRT")

    net_v = re.findall(r"Found (\d+) net violations", "\n".join(ant_lines))
    pin_v = re.findall(r"Found (\d+) pin violations", "\n".join(ant_lines))
    if len(net_v) != 1 or len(pin_v) != 1:
        fail(f"{variant}: antenna verdict grammar not found "
             f"(net matches {len(net_v)}, pin matches {len(pin_v)})")
    antenna_net, antenna_pin = int(net_v[0]), int(pin_v[0])

    drc_path = require(f"{run}/{design}.{variant}.drc.rpt")
    ant_path = require(f"{run}/{design}.{variant}.antenna.rpt")
    def_path = require(f"{outdir}/{design}.drt.{variant}.def")
    metrics = routed_metrics(def_path)

    reports = {}
    for tag, src, dst_name in (
            ("drc", drc_path, f"{design}.drc.{variant}.rpt"),
            ("antenna", ant_path, f"{design}.antenna.{variant}.rpt")):
        st = stamp(src)
        if st["size_bytes"] < COMMIT_LIMIT:
            shutil.copyfile(src, f"{outdir}/{dst_name}")
            reports[tag] = {"file": dst_name, "committed": True, **st}
        else:
            reports[tag] = {"file": f"runs/drt/{design}.{variant}.{tag}.rpt",
                            "committed": False, **st}

    if variant == "baseline":
        guide_path = require(f"{run}/{design}.baseline.guide")
        guide = {"file": f"runs/drt/{design}.baseline.guide",
                 "source": "emitted_by_global_route", **stamp(guide_path)}
        flow = "global_route+detailed_route"
    else:
        guide_path = require(f"{outdir}/runs/guide-check/{design}.ours.guide")
        guide = {"file": f"runs/guide-check/{design}.ours.guide",
                 "source": "grus-eda route --guide-out", **stamp(guide_path)}
        flow = "read_guides+detailed_route"

    via_names = {name: via_meaning_for(name, metrics["def_vias"])
                 for name in metrics["vias_per_cut_layer"]}
    variants[variant] = {
        "flow": flow,
        "guide": guide,
        "signal_nets_in_def": signal_nets_in_def,
        "signal_nets_routed": metrics["signal_nets_routed"],
        "drc_violations": nonblank_lines(drc_path),
        "drc_report": reports["drc"],
        "antenna_check_command": "check_antennas -report_file",
        "antenna_violations": antenna_net + antenna_pin,
        "antenna_net_violations": antenna_net,
        "antenna_pin_violations": antenna_pin,
        "antenna_report": {**reports["antenna"],
                           "nonblank_lines": nonblank_lines(ant_path)},
        "antenna_log_lines": ant_lines,
        "antenna_probe": probe,
        "surface": surface,
        "total_wirelength_um": metrics["total_wirelength_um"],
        "wirelength_source": "routed_def_parse",
        "def_dbu_per_micron": metrics["dbu_per_micron"],
        "vias_per_cut_layer": metrics["vias_per_cut_layer"],
        "via_names_meaning": via_names,
        "via_count_total": metrics["via_count_total"],
        "routed_def": {"file": f"{design}.drt.{variant}.def",
                       "committed": False, **stamp(def_path)},
        "drt_summary_lines": drt_lines,
        "grt_log_lines": grt_lines,
        "timing": timing,
    }
    print(f"DRT-SUMMARY {variant}: drc={variants[variant]['drc_violations']} "
          f"antenna={variants[variant]['antenna_violations']} "
          f"wirelength_um={variants[variant]['total_wirelength_um']} "
          f"vias={variants[variant]['via_count_total']} "
          f"route_s={timing['total_route_s']}")

# -- prior routed DEF cross-check ----------------------------------------

prior_path = f"{outdir}/{design}.routed.def"
if os.path.isfile(prior_path):
    base_def = f"{outdir}/{design}.drt.baseline.def"
    prior = {"file": f"{design}.routed.def", **stamp(prior_path),
             "baseline_def_identical":
                 open(prior_path, "rb").read() == open(base_def, "rb").read()}
    prior_present = True
else:
    prior = None
    prior_present = False

# -- delta (delegated - baseline) -----------------------------------------

base, dele = variants["baseline"], variants["delegated"]
via_delta = {}
for name in sorted(set(base["vias_per_cut_layer"]) | set(dele["vias_per_cut_layer"])):
    via_delta[name] = (dele["vias_per_cut_layer"].get(name, 0)
                       - base["vias_per_cut_layer"].get(name, 0))
wl_delta = dele["total_wirelength_um"] - base["total_wirelength_um"]
delta = {
    "drc_violations": dele["drc_violations"] - base["drc_violations"],
    "antenna_violations": dele["antenna_violations"] - base["antenna_violations"],
    "total_wirelength_um": round(wl_delta, 6),
    "total_wirelength_pct": (round(wl_delta / base["total_wirelength_um"] * 100, 4)
                            if base["total_wirelength_um"] else None),
    "via_count_total": dele["via_count_total"] - base["via_count_total"],
    "vias_per_cut_layer": via_delta,
    "total_route_s": round(dele["timing"]["total_route_s"]
                           - base["timing"]["total_route_s"], 3),
}

out = {
    "design": design,
    "input_def": f"{design}.placed.def",
    "input_sdc": f"{design}.sdc",
    "image": IMAGE,
    "generated_by": "tests/designs/generate_drt_ref.sh",
    "prior_routed_def": prior,
    "prior_routed_def_present": prior_present,
    "variants": variants,
    "delta": delta,
}
path = f"{outdir}/{design}.drt.reference.json"
with open(path, "w") as fp:
    json.dump(out, fp, indent=2)
    fp.write("\n")
print(f"Wrote {path}")
PYEOF

if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.drt.reference.json" ]; then
    echo "Error: step C did not produce ${DESIGN}.drt.reference.json." >&2
    exit 1
fi

echo "==> DRT delegation reference artifacts for ${DESIGN} written."
