#!/usr/bin/env bash
# DRC reference surfaces on a routed DEF (E7 ticket 1 — reference only).
#
# On the routed DEF named by $2, run inside the pinned image:
#
#   Step A (openroad, runs/drc/antenna.log): read tlef + cell lef + the
#     input DEF, probe the *antenna* command surface, then
#     `check_antennas -verbose` (bare `check_antennas` fallback if the
#     flag grammar rejects it — pinned on first run).
#   Step B (openroad, conditional): the `-output_drc` DRT report is NOT
#     re-run on the golden path — the report written alongside the input
#     DEF is discovered (e6 `runs/drt/*.drc*.rpt` first, then
#     `<d>.drc.rpt` from `generate_ref.sh` step 5b) and counted verbatim.
#     If none exists, the exact step-5b route sequence (no layer
#     adjustments) is re-run into runs/drc/ scratch and the re-routed
#     DEF's SHA-256 is compared against the input's — recorded as
#     `existing` / `regenerated_identical` / `regenerated_divergent`.
#   Step C (magic process, runs/drc/magic_clean.log): lef read (tlef +
#     cell lef) + def read + load + select top cell + drc euclidean on +
#     drc style drc(full) + drc check + drc catchup + drc count +
#     drc count total + drc listall why — the canonical sky130 deck on
#     the input DEF. `drc count` is top-level; `drc count total` counts
#     merged error regions hierarchy-wide; `drc listall why` enumerates
#     {why, bbox-list} fragments (a larger number by construction).
#   Step D (python3 + a second magic process, magic_broken.log): the
#     `add_spacing_probe_wire` sanity control — one disjoint `NEW` wire
#     appended to a different signal net at a sub-minimum edge gap from
#     an existing wire, then the identical Magic sequence on the broken
#     DEF. Must report nonzero errors with named rules; a 0 means the
#     reference surface cannot see violations and the run aborts.
#   Step E (python3): assembles <design>.drc.reference.json — Magic
#     total + per-rule counts + version, the tech-file `drc`-section
#     rule taxonomy (each `why` string classified lef_expressible /
#     antenna / magic_only / unclassified by the keyword classifier in
#     step E), the DRT report count, the antenna count, and the
#     sanity-control block. Every value is script-emitted; nothing is
#     hand-written.
#
# No LibreLane flow is re-run on the golden path (only the optional
# step-B DRT-report regeneration routes). Input-DEF preference order for
# callers: `tests/designs/<d>/<d>.drt.routed.def` once `e6-drt-ref` has
# landed, else `tests/designs/<d>/<d>.routed.def` from `generate_ref.sh`
# step 5b. The script never auto-selects — $2 is required so the checked
# DEF is never implicit. For picorv32 the routed DEF is git-ignored:
# run `tests/designs/picorv32/generate_ref.sh` first (step 5b/7 routes).
#
# Usage: generate_drc_ref.sh <design> <routed-def>   (gcd, picorv32)
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

DESIGN="${1:?usage: generate_drc_ref.sh <design> <routed-def>}"
DEF_ARG="${2:?usage: generate_drc_ref.sh <design> <routed-def>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

# Resolve $2: a path containing / is repo-relative; a bare filename
# resolves under tests/designs/<design>/.
if [[ "${DEF_ARG}" == */* ]]; then
    DEF_REL="${DEF_ARG}"
else
    DEF_REL="tests/designs/${DESIGN}/${DEF_ARG}"
fi
if [ ! -f "${REPO_ROOT}/${DEF_REL}" ]; then
    echo "Error: routed DEF ${REPO_ROOT}/${DEF_REL} not found." >&2
    if [ "${DESIGN}" = "picorv32" ]; then
        echo "  picorv32.routed.def is git-ignored — run tests/designs/picorv32/generate_ref.sh first (it routes in step 5b)." >&2
    fi
    exit 1
fi

# A DRT report is discovered, not re-run — but when none exists the
# step-B fallback re-routes from <d>.placed.def + <d>.sdc, so those
# become conditional requirements.
drt_report_exists() {
    compgen -G "${SCRIPT_DIR}/${DESIGN}/runs/drt/*.drc*.rpt" > /dev/null && return 0
    compgen -G "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.drc.*.rpt" > /dev/null && return 0
    [ -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.drc.rpt" ]
}
if ! drt_report_exists; then
    for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
        if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${f}" ]; then
            echo "Error: no DRT report found and regeneration input ${SCRIPT_DIR}/${DESIGN}/${f} is missing (produced by tests/designs/${DESIGN}/generate_ref.sh)." >&2
            exit 1
        fi
    done
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
        -e DRC_DESIGN="${DESIGN}" \
        -e DRC_DEF="${DEF_REL}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_drc_ref.sh "${DESIGN}" "${DEF_REL}"
    exit 0
fi

export DRC_DESIGN="${DRC_DESIGN:-${DESIGN}}"
DEF_ABS="/work/${DEF_REL}"
DRC_RUN_DIR="/work/tests/designs/${DESIGN}/runs/drc"
mkdir -p "${DRC_RUN_DIR}"

PDK_LIB=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
MAGICRC=/pdk/sky130A/libs.tech/magic/sky130A.magicrc
MAGICTECH=/pdk/sky130A/libs.tech/magic/sky130A.tech

DRC_TOP="$(python3 -c 'import re,sys; m=re.search(r"^DESIGN\s+(\S+)", open(sys.argv[1]).read(), re.M); print(m.group(1) if m else "")' "${DEF_ABS}")"
if [ -z "${DRC_TOP}" ]; then
    echo "Error: no DESIGN line in ${DEF_ABS}." >&2
    exit 1
fi
export DRC_TOP

T_ANT=0; T_REGEN=0; T_MCLEAN=0; T_MBROKEN=0; T_ASM=0

# Tcl on stdin: `!` is history expansion (write `== 0`) and `\` does not
# continue long lines — every Tcl command below is one physical line.
# Transcript echoes can contain marker text inside `puts` commands, so
# every parser regex anchors at `^` — Magic markers are `DRCREF-`,
# OpenROAD markers `DRC-`.

# Step A. Antenna surface: check_antennas on the input DEF. Minimal
#         reads first; a missing-liberty class error would be added
#         here and recorded in docs/results.md.
SECONDS=0
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${DRC_RUN_DIR}/antenna.log"
set D $env(DRC_DESIGN)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/$env(DRC_DEF)

puts "DRC-ANT-SURFACE [info commands *antenna*]"
if {[llength [info commands check_antennas]] == 0} {
    puts "DRC-FATAL missing command: check_antennas"
    exit 1
}
set acode [catch {check_antennas -verbose} ares]
puts "DRC-ANT-RESULT code=$acode result=$ares"
if {$acode != 0} {
    set bcode [catch {check_antennas} bres]
    puts "DRC-ANT-RESULT-BARE code=$bcode result=$bres"
}
exit
EOF
T_ANT=$SECONDS

if grep -q "^DRC-FATAL" "${DRC_RUN_DIR}/antenna.log"; then
    echo "Error: step A reported DRC-FATAL (see ${DRC_RUN_DIR}/antenna.log)." >&2
    exit 1
fi

# Step B. DRT report discovery (never re-run on the golden path) or
#         regeneration via the verbatim step-5b route sequence.
DRT_RPT=""
DRT_SOURCE="existing"
shopt -s nullglob
e6_reports=( "/work/tests/designs/${DESIGN}/runs/drt/"*.drc*.rpt "/work/tests/designs/${DESIGN}/${DESIGN}.drc."*.rpt )
shopt -u nullglob
if [ "${#e6_reports[@]}" -gt 0 ]; then
    # For *.drt.routed.def inputs prefer the baseline variant's report.
    if [[ "${DEF_REL}" == *.drt.routed.def ]]; then
        for f in "${e6_reports[@]}"; do
            if [[ "${f}" == *baseline* ]]; then DRT_RPT="${f}"; break; fi
        done
    fi
    [ -z "${DRT_RPT}" ] && DRT_RPT="${e6_reports[0]}"
elif [ -f "/work/tests/designs/${DESIGN}/${DESIGN}.drc.rpt" ]; then
    DRT_RPT="/work/tests/designs/${DESIGN}/${DESIGN}.drc.rpt"
fi

if [ -z "${DRT_RPT}" ]; then
    # No report found: regenerate via the exact generate_ref.sh step-5b
    # sequence — read placed.def + sdc, met1-met5 signal / met3-met5
    # clock band, NO set_global_routing_layer_adjustment (those belong to
    # the step-5c guide golden, a different process). The re-routed DEF's
    # sha decides regenerated_identical vs regenerated_divergent.
    for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
        if [ ! -f "/work/tests/designs/${DESIGN}/${f}" ]; then
            echo "Error: no DRT report found and regen input ${f} missing (run tests/designs/${DESIGN}/generate_ref.sh)." >&2
            exit 1
        fi
    done
    SECONDS=0
    openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${DRC_RUN_DIR}/drt_regen.log"
set D $env(DRC_DESIGN)
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.placed.def
read_sdc /work/tests/designs/$D/$D.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
global_route -guide_file /work/tests/designs/$D/runs/drc/$D.regen.guide -congestion_iterations 100
detailed_route -output_drc /work/tests/designs/$D/runs/drc/$D.drc.regen.rpt -verbose 0
write_def /work/tests/designs/$D/runs/drc/$D.routed.regen.def
exit
EOF
    T_REGEN=$SECONDS
    DRT_RPT="${DRC_RUN_DIR}/${DESIGN}.drc.regen.rpt"
    if [ ! -f "${DRT_RPT}" ]; then
        echo "Error: DRT regeneration produced no report (see ${DRC_RUN_DIR}/drt_regen.log)." >&2
        exit 1
    fi
    in_sha="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "${DEF_ABS}")"
    rg_sha="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "${DRC_RUN_DIR}/${DESIGN}.routed.regen.def")"
    if [ "${in_sha}" = "${rg_sha}" ]; then
        DRT_SOURCE="regenerated_identical"
    else
        DRT_SOURCE="regenerated_divergent"
        echo "note: regenerated routed DEF sha ${rg_sha:0:16} != input sha ${in_sha:0:16} — the report describes a different route (recorded)." >&2
    fi
fi

# Step C. Magic clean run — the P-DRC sequence verbatim, marker-wrapped.
#         cwd inside runs/drc/ so `def read`/`load` .mag byproducts land
#         in scratch, not the design dir.
SECONDS=0
(cd "${DRC_RUN_DIR}" && magic -dnull -noconsole -rcfile "${MAGICRC}" << 'EOF' | tee magic_clean.log
puts "DRCREF-VERSION-BEGIN"
catch {exec magic --version} vres
puts "DRCREF-VERSION {$vres}"
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
def read /work/$env(DRC_DEF)
load $env(DRC_TOP)
select top cell
drc euclidean on
drc style drc(full)
drc check
catch {drc catchup} cupres
puts "DRCREF-CATCHUP {$cupres}"
puts "DRCREF-COUNT-BEGIN"
catch {drc count} c1
puts "DRCREF-COUNT-RAW {$c1}"
catch {drc count total} c2
puts "DRCREF-COUNT-TOTAL {$c2}"
puts "DRCREF-WHY-BEGIN"
set whys [drc listall why]
puts "DRCREF-WHY-COUNT [llength $whys]"
foreach w $whys { puts "DRCREF-WHY {$w}" }
puts "DRCREF-WHY-END"
puts "DRCREF-END"
exit
EOF
)
T_MCLEAN=$SECONDS

if ! grep -q "^DRCREF-END" "${DRC_RUN_DIR}/magic_clean.log"; then
    echo "Error: Magic clean run ended without DRCREF-END (crash/hang; see ${DRC_RUN_DIR}/magic_clean.log)." >&2
    exit 1
fi

# Step D. add_spacing_probe_wire mutation + identical Magic sequence on
#         the broken DEF. The mutation python writes mutation.json with
#         every parameter (nets, layer, gap, segment, parsed layer
#         constants) for step E.
SECONDS=0
python3 - "${DEF_ABS}" "${PDK_TLEF}" "${DRC_RUN_DIR}/${DESIGN}.drc.broken.def" "${DRC_RUN_DIR}/mutation.json" <<'PYEOF'
import json
import re
import sys

def_path, tlef_path, out_path, mut_json = sys.argv[1:5]
text = open(def_path).read()

def fail(msg):
    sys.exit(f"drc mutation failed: {msg}")

m = re.search(r"^UNITS DISTANCE MICRONS (\d+) ;", text, re.M)
if m is None:
    fail("no UNITS DISTANCE MICRONS line")
upu = int(m.group(1))

# --- tlef layer constants: WIDTH plus min spacing (plain SPACING, else
#     the smallest SPACINGTABLE PARALLELRUNLENGTH WIDTH-row value).
def layer_consts(tlef, layer):
    m = re.search(rf"^LAYER {re.escape(layer)}\n(.*?)^END {re.escape(layer)}",
                  tlef, re.M | re.S)
    if m is None:
        fail(f"tlef has no LAYER {layer} block")
    blk = m.group(1)
    mw = re.search(r"^\s*WIDTH\s+([\d.]+)\s*;", blk, re.M)
    if mw is None:
        fail(f"LAYER {layer}: no WIDTH")
    width = float(mw.group(1))
    ms = re.search(r"^\s*SPACING\s+([\d.]+)\s*;", blk, re.M)
    if ms is not None:
        spacing = float(ms.group(1))
    else:
        vals = [float(x) for x in
                re.findall(r"^\s*WIDTH\s+[\d.]+\s+([\d.]+)", blk, re.M)]
        if not vals:
            fail(f"LAYER {layer}: no SPACING or SPACINGTABLE")
        spacing = min(vals)
    return width, spacing

tlef = open(tlef_path).read()

# --- NETS section: each entry "- <name> ... ;" may span lines.
mn = re.search(r"^NETS \d+ ;\n(.*?)^END NETS", text, re.M | re.S)
if mn is None:
    fail("no NETS section")
nets_text = mn.group(0)
net_spans = [(m.group(1), m.start(0), m.end(0))
             for m in re.finditer(r"-\s+(\S+)\s(.*?);", nets_text, re.S)]
if not net_spans:
    fail("NETS section parsed zero nets")

pt_re = re.compile(r"\(\s*(-?\d+|\*)\s+(-?\d+|\*)\s*\)")

def segments(body):
    """Axis-aligned (layer, p1, p2) wire segments inside one net body.

    The body is split at ROUTED/NEW path boundaries; each path's leading
    token is its layer and consecutive resolved points form segments.
    `*` inherits the previous coordinate on that axis.
    """
    segs = []
    parts = re.split(r"(?:^|\s)(ROUTED|NEW)\s+", body)
    it = iter(parts[1:])
    for _kw, chunk in zip(it, it):
        layer_m = re.match(r"(\S+)", chunk)
        if layer_m is None:
            continue
        layer = layer_m.group(1)
        prev = None
        for pm in pt_re.finditer(chunk):
            x, y = pm.group(1), pm.group(2)
            if prev is None:
                prev = [None if x == "*" else int(x),
                        None if y == "*" else int(y)]
                continue
            cur = [prev[0] if x == "*" else int(x),
                   prev[1] if y == "*" else int(y)]
            if cur[0] is None or cur[1] is None or \
               prev[0] is None or prev[1] is None:
                prev = cur
                continue
            if (cur[0] == prev[0]) != (cur[1] == prev[1]):
                segs.append((layer, tuple(prev), tuple(cur)))
            prev = cur
    return segs

# net_a: first net (file order) with an axis-aligned segment on the
# first usable layer in preference order met2 -> met1 -> met3.
net_a = seg_a = layer = None
for cand in ("met2", "met1", "met3"):
    for name, s0, s1 in net_spans:
        body = nets_text[s0:s1]
        if "NONDEFAULTRULE" in body:
            continue
        segs = [s for s in segments(body) if s[0] == cand]
        if segs:
            net_a, seg_a, layer = name, segs[0], cand
            break
    if net_a is not None:
        break
if net_a is None:
    fail("no routable met1-met3 segment found in any signal net")

# net_b: first other signal net.
net_b = None
for name, s0, s1 in net_spans:
    if name != net_a and "NONDEFAULTRULE" not in nets_text[s0:s1]:
        net_b, nb_span = name, (s0, s1)
        break
if net_b is None:
    fail("no second signal net found")

width_um, spacing_um = layer_consts(tlef, layer)
w_dbu = round(width_um * upu)
gap_dbu = round(0.03 * upu)          # 30 nm — under min spacing, nonzero
len_dbu = round(0.5 * upu)
_, p1, p2 = seg_a

# Probe wire parallels the host segment, offset by W + gap perpendicular
# (W/2 + gap + W/2 center-to-center => `gap` edge distance). For a
# vertical host (met2 preferred direction) the offset is +x.
if p1[0] == p2[0]:  # vertical host
    x1 = x2 = p1[0] + w_dbu + gap_dbu
    y1 = min(p1[1], p2[1])
    y2 = y1 + len_dbu
    orient = "vertical"
else:               # horizontal host
    y1 = y2 = p1[1] + w_dbu + gap_dbu
    x1 = min(p1[0], p2[0])
    x2 = x1 + len_dbu
    orient = "horizontal"
wire = f"      NEW {layer} ( {x1} {y1} ) ( {x2} {y2} )"

# Insert the disjoint wire before net_b's terminating `;` — same NEW
# grammar detailed_route emits, so every DEF reader accepts it.
s0, s1 = nb_span
blk = nets_text[s0:s1]
if not blk.rstrip().endswith(";"):
    fail(f"net {net_b} block does not end with ';'")
cut = blk.rindex(";")
broken = text[:mn.start(0) + s0] + blk[:cut].rstrip() + "\n" + wire + " ;\n" + \
    blk[cut + 1:] + text[mn.start(0) + s1:]
with open(out_path, "w") as fp:
    fp.write(broken)

params = {
    "name": "add_spacing_probe_wire",
    "statement": "NEW",
    "net_a": net_a,
    "net_b": net_b,
    "layer": layer,
    "host_orientation": orient,
    "host_segment_dbu": [p1[0], p1[1], p2[0], p2[1]],
    "gap_um": round(gap_dbu / upu, 6),
    "segment_dbu": [x1, y1, x2, y2],
    "layer_width_dbu": w_dbu,
    "layer_min_spacing_dbu": round(spacing_um * upu),
}
json.dump(params, open(mut_json, "w"), indent=2)
print("DRC-MUTATION " + json.dumps(params))
PYEOF

if [ ! -f "${DRC_RUN_DIR}/${DESIGN}.drc.broken.def" ]; then
    echo "Error: step D mutation did not produce ${DESIGN}.drc.broken.def." >&2
    exit 1
fi

export DRC_BROKEN_DEF="tests/designs/${DESIGN}/runs/drc/${DESIGN}.drc.broken.def"
SECONDS=0
(cd "${DRC_RUN_DIR}" && magic -dnull -noconsole -rcfile "${MAGICRC}" << 'EOF' | tee magic_broken.log
puts "DRCREF-VERSION-BEGIN"
catch {exec magic --version} vres
puts "DRCREF-VERSION {$vres}"
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
lef read /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
def read /work/$env(DRC_BROKEN_DEF)
load $env(DRC_TOP)
select top cell
drc euclidean on
drc style drc(full)
drc check
catch {drc catchup} cupres
puts "DRCREF-CATCHUP {$cupres}"
puts "DRCREF-COUNT-BEGIN"
catch {drc count} c1
puts "DRCREF-COUNT-RAW {$c1}"
catch {drc count total} c2
puts "DRCREF-COUNT-TOTAL {$c2}"
puts "DRCREF-WHY-BEGIN"
set whys [drc listall why]
puts "DRCREF-WHY-COUNT [llength $whys]"
foreach w $whys { puts "DRCREF-WHY {$w}" }
puts "DRCREF-WHY-END"
puts "DRCREF-END"
exit
EOF
)
T_MBROKEN=$SECONDS

if ! grep -q "^DRCREF-END" "${DRC_RUN_DIR}/magic_broken.log"; then
    echo "Error: Magic broken run ended without DRCREF-END (see ${DRC_RUN_DIR}/magic_broken.log)." >&2
    exit 1
fi

cat > "${DRC_RUN_DIR}/runtime.json" <<EOF
{"antenna": ${T_ANT}, "drt_regen": ${T_REGEN}, "magic_clean": ${T_MCLEAN}, "magic_broken": ${T_MBROKEN}, "assembly": 0}
EOF

# Step E. Assembly: parse the three transcripts + DRT report + tech-file
#         drc section; classify every checked rule; assert every
#         expectation loudly; emit <design>.drc.reference.json.
SECONDS=0
python3 - "${DESIGN}" "${DEF_REL}" "${DRT_RPT}" "${DRT_SOURCE}" <<'PYEOF'
import hashlib
import json
import re
import sys

design, def_rel, drt_rpt, drt_source = sys.argv[1:5]
outdir = f"/work/tests/designs/{design}"
run = f"{outdir}/runs/drc"
def_abs = f"/work/{def_rel}"

def fail(msg):
    sys.exit(f"drc step-E assembly failed: {msg}")

def sha16(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()[:16]

# ---------- Magic transcript parsing ----------
def parse_magic(log_path):
    log = open(log_path).read()
    if "DRCREF-END" not in log:
        fail(f"{log_path}: missing DRCREF-END")
    out = {}
    m = re.search(r"^DRCREF-VERSION \{(.*)\}$", log, re.M)
    if m is not None and "couldn't" not in m.group(1) and "error" not in m.group(1).lower():
        out["version"], out["version_source"] = m.group(1), "version_cmd"
    else:
        mb = re.search(r"Magic (\d+\.\d+\.\d+)", log) or \
             re.search(r"Magic version (\S+)", log) or \
             re.search(r"Magic \S+ revision (\S+)", log)
        if mb is None:
            fail(f"{log_path}: no DRCREF-VERSION and no banner version")
        out["version"], out["version_source"] = mb.group(1), "banner"
    # `drc count` / `drc count total` print "Total DRC errors found: N" to
    # stdout and return "" — the printed line immediately before each
    # marker belongs to that command. `drc count` is top-level only;
    # `drc count total` is hierarchy-wide. Neither equals the listall
    # bbox-fragment sum: count tallies merged error regions.
    for key, pat in (("count_raw", r"^DRCREF-COUNT-RAW \{(.*)\}$"),
                     ("count_total", r"^DRCREF-COUNT-TOTAL \{(.*)\}$")):
        m = re.search(pat, log, re.M)
        if m is None:
            fail(f"{log_path}: missing marker for {key}")
        out[key] = m.group(1)
        prev = re.findall(r"Total DRC errors found:\s*(-?\d+)",
                          log[:m.start()])
        if not prev:
            fail(f"{log_path}: no 'Total DRC errors found' before {key}")
        out[key + "_printed"] = int(prev[-1])
    out["printed_total"] = out["count_total_printed"]
    # `drc listall why` returns a Tcl list alternating {why-string}
    # {bbox-list}: an element starting with "{" is the preceding rule's
    # violating-bbox list; every other element is a verbatim rule name.
    items = re.findall(r"^DRCREF-WHY \{(.*)\}$", log, re.M)
    m = re.search(r"^DRCREF-WHY-COUNT (\d+)$", log, re.M)
    if m is None:
        fail(f"{log_path}: missing DRCREF-WHY-COUNT")
    declared = int(m.group(1))
    if len(items) != declared:
        fail(f"{log_path}: {len(items)} DRCREF-WHY items != declared {declared}")
    bbox_re = re.compile(r"\{-?\d+ -?\d+ -?\d+ -?\d+\}")
    per_rule = {}
    i = 0
    while i < len(items):
        if items[i].startswith("{"):
            fail(f"{log_path}: bbox list before any why string (item {i})")
        why = items[i]
        if i + 1 >= len(items) or not items[i + 1].startswith("{"):
            fail(f"{log_path}: why {why!r} not followed by a bbox list")
        nb = len(bbox_re.findall(items[i + 1]))
        if nb == 0:
            fail(f"{log_path}: empty bbox list after why {why!r}")
        per_rule[why] = per_rule.get(why, 0) + nb
        i += 2
    out["whys"] = [w for w in items if not w.startswith("{")]
    out["listall_items"] = len(items)
    out["per_rule"] = per_rule
    out["bbox_total"] = sum(per_rule.values())
    out["total"] = out["printed_total"]
    # The two totals differ by construction (merged regions vs bbox
    # fragments) — record both, never assert them equal.
    out["count_matches_bbox_sum"] = out["printed_total"] == out["bbox_total"]
    return out

clean = parse_magic(f"{run}/magic_clean.log")
broken = parse_magic(f"{run}/magic_broken.log")

# ---------- antenna transcript ----------
alog = open(f"{run}/antenna.log").read()
m = re.search(r"^DRC-ANT-SURFACE (.*)$", alog, re.M)
if m is None:
    fail("antenna.log: missing DRC-ANT-SURFACE")
ant_surface = m.group(1).split()
if "check_antennas" not in ant_surface:
    fail("check_antennas absent from *antenna* surface")
m = re.search(r"^DRC-ANT-RESULT code=(\d+) result=(.*)$", alog, re.M)
if m is None:
    fail("antenna.log: missing DRC-ANT-RESULT")
ant_code, ant_result = int(m.group(1)), m.group(2).strip()
mb = re.search(r"^DRC-ANT-RESULT-BARE code=(\d+) result=(.*)$", alog, re.M)
ant_verbose = "accepted" if ant_code == 0 else "rejected"
ant_cmd = "check_antennas -verbose" if ant_code == 0 else "check_antennas"
if ant_code != 0 and mb is None:
    fail("antenna.log: verbose rejected but no bare fallback result")
ant_result_final = ant_result if ant_code == 0 else mb.group(2).strip()
# Grammar pinned 2026-11-23: "[INFO ANT-0002] Found N net violations."
# and "[INFO ANT-0001] Found N pin violations." — the antenna metric is
# the net count (OpenLane convention); the Tcl return mirrors it.
mn_v = re.search(r"Found (\d+) net violations", alog)
mp_v = re.search(r"Found (\d+) pin violations", alog)
if mn_v is None:
    if re.fullmatch(r"\d+", ant_result_final):
        ant_viol, ant_pin = int(ant_result_final), None
    else:
        fail("antenna.log: could not parse an antenna violation count")
else:
    ant_viol = int(mn_v.group(1))
    ant_pin = int(mp_v.group(1)) if mp_v else None
    if re.fullmatch(r"\d+", ant_result_final) and \
            int(ant_result_final) != ant_viol:
        fail(f"antenna.log: Tcl result {ant_result_final} != "
             f"net violations {ant_viol}")
# `check_antennas -verbose` prints one `Net: <n>` block per violating
# net (nothing for a clean design): indented `Pin:` rows, per-layer
# `Partial/Cumulative area ratio` vs `Required ratio` lines, and a
# `(VIOLATED)` suffix on the failing requirement.
ant_per_net = []
cur = None
layer = None
for ln in alog.splitlines():
    mn = re.match(r"^Net:\s+(\S+)", ln)
    if mn:
        cur = {"net": mn.group(1), "pins": set(), "violated_layers": set()}
        ant_per_net.append(cur)
        layer = None
        continue
    if cur is None:
        continue
    mp = re.match(r"^\s+Pin:\s+(\S+)", ln)
    if mp:
        cur["pins"].add(mp.group(1))
    ml = re.match(r"^\s+Layer:\s+(\S+)", ln)
    if ml:
        layer = ml.group(1)
    if "(VIOLATED)" in ln and layer:
        cur["violated_layers"].add(layer)
for e in ant_per_net:
    e["pins"] = sorted(e["pins"])
    e["violated_layers"] = sorted(e["violated_layers"])
if ant_verbose == "accepted" and len(ant_per_net) != ant_viol:
    fail(f"antenna.log: {len(ant_per_net)} verbose Net blocks != "
         f"{ant_viol} reported net violations")
ant_report_lines = [ln.strip() for ln in alog.splitlines()
                    if re.match(r"\[(INFO|WARNING|ERROR) ANT", ln)]

# ---------- DRT report ----------
drt_count = sum(1 for _ in open(drt_rpt))
drt_json_viol = None
drt_ref = f"{outdir}/{design}.drt.reference.json"
try:
    ref = json.load(open(drt_ref))
    for key in ("drc_violations", "violations"):
        if isinstance(ref.get(key), int):
            drt_json_viol = ref[key]
            break
    if drt_json_viol is None:
        variants = ref.get("variants") or {}
        for v in variants.values() if isinstance(variants, dict) else []:
            if isinstance(v, dict) and isinstance(v.get("drc_violations"), int):
                drt_json_viol = v["drc_violations"]
                break
except OSError:
    pass

# ---------- tech-file rule taxonomy ----------
tech = open("/pdk/sky130A/libs.tech/magic/sky130A.tech").read()
md = re.search(r"^drc\n(.*?)^end\n", tech, re.M | re.S)
if md is None:
    fail("sky130A.tech: no drc...end section")
# Extract quoted why-strings per logical line (continuation lines joined
# first). Two sky130A.tech statements carry an unclosed quote (`rect_only
# *mimcap`/`rect_only *mimcap2`), which desyncs whole-section pairing —
# an odd-quote line's tail after the last `"` is the intended name.
logical = []
buf = ""
for ln in md.group(1).splitlines():
    ln = ln.rstrip()
    if ln.lstrip().startswith("#"):
        continue
    buf = (buf + " " + ln.lstrip()) if buf else ln.lstrip()
    if buf.endswith("\\"):
        buf = buf[:-1]
        continue
    logical.append(buf)
    buf = ""
if buf:
    logical.append(buf)
names = []
for ln in logical:
    names.extend(re.findall(r'"([^"]*)"', ln))
    if ln.count('"') % 2 == 1:
        tail = ln.rsplit('"', 1)[1].strip()
        if tail:
            names.append(tail)
if not names:
    fail("sky130A.tech drc section yielded zero rule names")

def classify(name):
    n = name.lower()
    if "antenn" in n or "ratio" in n:
        return "antenna"
    if any(k in n for k in ("spacing", "space", "distance", "width",
                            "enclosure", "overlap", "area", "length")):
        return "lef_expressible"
    if any(k in n for k in ("corner", "must", "surround", "notch", "hole",
                            "halo", "abut", "junction", "butted", "angle",
                            "permitted", "bend", "rectangular", "cross",
                            "contain", "straddle", "unrelated", "exact",
                            "extend", "overhang", "photodiode", "sonos")):
        return "magic_only"
    return "unclassified"

classification = {n: classify(n) for n in names}

# ---------- sanity control ----------
mutation = json.load(open(f"{run}/mutation.json"))
broken_def = f"{run}/{design}.drc.broken.def"
violated = sorted(broken["per_rule"])
if broken["total"] <= 0 or not violated:
    fail("sanity control failed: broken DEF reported 0 errors / no named rules")
new_vs_clean = sorted(set(broken["per_rule"]) - set(clean["per_rule"]))
grew = [w for w in broken["per_rule"]
        if broken["per_rule"][w] > clean["per_rule"].get(w, 0)]
if not new_vs_clean and not grew:
    fail("sanity control failed: broken DEF adds no violations over clean")
# The deck's why-strings are printf templates ("... < %d (met2.2)");
# `drc listall why` renders them. Template-match for the cross-check.
def tmpl_match(template, observed):
    pat = re.escape(template)
    pat = re.sub(r"%.", ".*?", pat)
    return re.fullmatch(pat, observed) is not None
in_deck = all(any(tmpl_match(t, v) for t in names) for v in violated)

input_def_bytes = open(def_abs, "rb").read()

prov_name = def_rel.rsplit("/", 1)[-1]
if prov_name == f"{design}.drt.routed.def":
    provenance = "e6-drt-ref baseline"
elif prov_name == f"{design}.routed.def":
    provenance = "generate_ref.sh step 5b"
else:
    provenance = "explicit arg"

broken_rel = f"tests/designs/{design}/runs/drc/{design}.drc.broken.def"
drt_rel = drt_rpt.replace("/work/", "", 1)

out = {
    "design": design,
    "image": "ghcr.io/librelane/librelane:3.0.16",
    "input_def": def_rel,
    "input_def_sha_prefix": hashlib.sha256(input_def_bytes).hexdigest()[:16],
    "input_def_size_bytes": len(input_def_bytes),
    "input_def_provenance": provenance,
    "magic_version": clean["version"],
    "magic_version_source": clean["version_source"],
    "magic_rcfile": "/pdk/sky130A/libs.tech/magic/sky130A.magicrc",
    "magic_style": "drc(full)",
    "magic_count_flat": clean["count_raw_printed"],
    "magic_total": clean["total"],
    "magic_per_rule": clean["per_rule"],
    "magic_listall_items": clean["listall_items"],
    "magic_listall_bbox_total": clean["bbox_total"],
    "magic_count_matches_bbox_sum": clean["count_matches_bbox_sum"],
    "magic_rules_checked": {
        "source": "/pdk/sky130A/libs.tech/magic/sky130A.tech drc section",
        "count": len(names),
        "names": names,
        "classification": classification,
        "classification_method": "keyword classifier in generate_drc_ref.sh step E; reviewed in docs/e7-drc.md",
    },
    "drt_drc_count": drt_count,
    "drt_report_file": drt_rel,
    "drt_report_source": drt_source,
    "drt_report_sha256_16": sha16(drt_rpt),
    "drt_reference_json_violations": drt_json_viol,
    "antenna_violations": ant_viol,
    "antenna_pin_violations": ant_pin,
    "antenna_per_net": ant_per_net,
    "antenna_report_lines": ant_report_lines,
    "antenna_command": ant_cmd,
    "antenna_command_surface": ant_surface,
    "antenna_verbose_flag": ant_verbose,
    "sanity_control": {
        "mutation": mutation,
        "broken_def": broken_rel,
        "broken_def_sha256_16": sha16(broken_def),
        "magic_total": broken["total"],
        "magic_per_rule": broken["per_rule"],
        "violated_rule_names": violated,
        "new_rules_vs_clean": new_vs_clean,
        "violated_rules_in_checked_deck": in_deck,
    },
}
path = f"{outdir}/{design}.drc.reference.json"
with open(path, "w") as fp:
    json.dump(out, fp, indent=2)
    fp.write("\n")
print(f"DRC-SUMMARY magic_total={clean['total']} drt_count={drt_count} "
      f"antenna={ant_viol} broken_total={broken['total']} "
      f"violated={violated[:3]}")
print(f"Wrote {path}")
PYEOF
T_ASM=$SECONDS

# Per-phase seconds live only in the uncommitted runs/ dir — the golden
# JSON must byte-reproduce, so runtime stays out of it.
python3 - "${DRC_RUN_DIR}/runtime.json" "${T_ASM}" <<'PYEOF'
import json, sys
p, sec = sys.argv[1], int(sys.argv[2])
rt = json.load(open(p))
rt["assembly"] = sec
json.dump(rt, open(p, "w"))
print("DRC-RUNTIME " + json.dumps(rt, sort_keys=True))
PYEOF

if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.drc.reference.json" ]; then
    echo "Error: step E did not produce ${DESIGN}.drc.reference.json." >&2
    exit 1
fi

echo "==> DRC reference artifacts for ${DESIGN} written."
