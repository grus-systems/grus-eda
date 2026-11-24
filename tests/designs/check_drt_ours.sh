#!/usr/bin/env bash
# e6-drt-e2e container-side scoring of <design>.drt.ours.routed.def —
# the sibling of generate_drc_ref.sh: the same Docker bootstrap, the
# same Magic drc(full) sequence, the same antenna probe, the same
# add_spacing_probe_wire sanity control, and the same step-E assembly
# idiom (every unmatched expectation aborts; no default-fill).
#
#   Step A (openroad, runs/drt-ours/antenna.log): read tlef + cell lef
#     + OUR routed DEF, DRC-READDEF-OK (a foreign-tool parse of our
#     text is evidence), the *antenna*/*drc*/*check* command-surface
#     probes, then check_antennas -verbose (bare fallback).
#   Step B (magic, ours_magic.log): the canonical sky130 drc(full)
#     sequence on our DEF — lef read x2, def read, load, select top
#     cell, drc euclidean on, drc style drc(full), drc check, drc
#     catchup, drc count, drc count total, drc listall why.
#   Step C (magic, placed_magic.log): the identical sequence on
#     <d>.placed.def — the inherited-baseline measurement. Master-
#     internal (magic_only) findings are placement/routing-invariant
#     by construction; this run turns that into measured per-rule
#     data instead of an assumption.
#   Step D (python3 + magic, broken_magic.log): the committed
#     add_spacing_probe_wire mutation on our DEF, then the identical
#     Magic sequence. Nonzero new violations with named rules are
#     required — proof Magic flags a lef_expressible violation inside
#     our emitted text.
#   Step E (python3): assembles <d>.drt.ours.reference.json — ours/
#     placed/sanity Magic surfaces, the antenna block, routed-DEF
#     metrics (the generate_drt_ref.sh grammar, strict on our own
#     text), the taxonomy join against the committed
#     <d>.drc.reference.json classification (lef_expressible 50-name
#     map / magic_only union rows / unclassified_fired), the
#     vs_baseline/vs_delegated delta block, and the ours_router
#     provenance block (emit-log census lines lifted verbatim).
#
# The script NEVER re-routes: there is no TritonRoute -output_drc
# analog for a natively-routed DEF (re-running detailed_route would
# measure OpenROAD's output, not ours) — the JSON records
# "drt_report_source": "not_applicable_native_router" and Magic
# drc(full) is the geometry opinion.
#
# The input DEF is fixed by convention (<d>.drt.ours.routed.def) —
# produce it with tests/designs/generate_drt_ours.sh <design>. For
# picorv32 the placed DEF and ours DEF are git-ignored; run
# tests/designs/picorv32/generate_ref.sh first (gcd's are committed).
#
# Usage: check_drt_ours.sh <design>   (gcd, picorv32)
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

DESIGN="${1:?usage: check_drt_ours.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

D="${SCRIPT_DIR}/${DESIGN}"
if [ ! -f "${D}/${DESIGN}.drt.ours.routed.def" ]; then
    echo "Error: missing ${D}/${DESIGN}.drt.ours.routed.def (produced by tests/designs/generate_drt_ours.sh ${DESIGN})" >&2
    exit 1
fi
if [ ! -f "${D}/${DESIGN}.placed.def" ]; then
    echo "Error: missing ${D}/${DESIGN}.placed.def — the inherited-baseline Magic input (picorv32's is gitignored; regenerate via tests/designs/${DESIGN}/generate_ref.sh)" >&2
    exit 1
fi
for f in "${DESIGN}.drc.reference.json" "${DESIGN}.drt.reference.json" "${DESIGN}.guide"; do
    if [ ! -f "${D}/${f}" ]; then
        echo "Error: missing ${D}/${f} — a join input of the check (generate via tests/designs/generate_drc_ref.sh / generate_drt_ref.sh / the committed guide)" >&2
        exit 1
    fi
done

if [ ! -d "${PDK_HOST_ROOT}/sky130A" ]; then
    echo "Error: sky130A PDK not found at ${PDK_HOST_ROOT}/sky130A." >&2
    exit 1
fi

# Provenance for the ours DEF — read host-side so the JSON carries the
# emitting binary's version even though the check runs in-container.
GRUS_VERSION="$(python3 -c 'import re,sys; print(re.search(r"^version = \"(.+?)\"\s*$", open(sys.argv[1]).read(), re.M).group(1))' "${REPO_ROOT}/Cargo.toml")"
if [ -z "${GRUS_VERSION}" ]; then
    echo "Error: could not read workspace.package version from ${REPO_ROOT}/Cargo.toml" >&2
    exit 1
fi

if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    if docker info >/dev/null 2>&1; then
        docker_cmd="docker"
    else
        docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"
    fi
    IMAGE_ID="$(${docker_cmd} image inspect --format '{{.Id}}' "${CONTAINER_IMAGE}")"
    ${docker_cmd} run --rm \
        -u "$(id -u):$(id -g)" \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -e DRC_DESIGN="${DESIGN}" \
        -e GRUS_VERSION="${GRUS_VERSION}" \
        -e DRC_IMAGE_ID="${IMAGE_ID}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/check_drt_ours.sh "${DESIGN}"
    exit 0
fi

export DRC_DESIGN="${DRC_DESIGN:-${DESIGN}}"
DRC_RUN_DIR="/work/tests/designs/${DESIGN}/runs/drt-ours"
mkdir -p "${DRC_RUN_DIR}"
DEF_ABS="/work/tests/designs/${DESIGN}/${DESIGN}.drt.ours.routed.def"

PDK_TLEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
PDK_LEF=/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
MAGICRC=/pdk/sky130A/libs.tech/magic/sky130A.magicrc

DRC_TOP="$(python3 -c 'import re,sys; m=re.search(r"^DESIGN\s+(\S+)", open(sys.argv[1]).read(), re.M); print(m.group(1) if m else "")' "${DEF_ABS}")"
if [ -z "${DRC_TOP}" ]; then
    echo "Error: no DESIGN line in ${DEF_ABS}." >&2
    exit 1
fi
export DRC_TOP

T_ANT=0; T_MCLEAN=0; T_MPLACED=0; T_MBROKEN=0; T_ASM=0

# Tcl on stdin: `!` is history expansion (write `== 0`) and `\` does not
# continue long lines — every Tcl command below is one physical line.
# Transcript echoes can contain marker text inside `puts` commands, so
# every parser regex anchors at `^` — Magic markers are `DRCREF-`,
# OpenROAD markers `DRC-`.

# Step A. OpenROAD read_def + command surfaces + check_antennas on OUR
#         DEF. DRC-READDEF-OK after read_def is recorded proof that
#         OpenROAD's DEF reader accepts our grammar — a parse failure
#         aborts the process (recorded partial log).
SECONDS=0
openroad -no_init -no_splash 2>&1 << 'EOF' | tee "${DRC_RUN_DIR}/antenna.log"
set D $env(DRC_DESIGN)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/$D/$D.drt.ours.routed.def
puts "DRC-READDEF-OK"
puts "DRC-SURFACE-ANT [info commands *antenna*]"
puts "DRC-SURFACE-DRC [info commands *drc*]"
puts "DRC-SURFACE-CHECK [info commands *check*]"
set acode [catch {check_antennas -verbose} ares]
puts "DRC-ANT-RESULT code=$acode result=$ares"
if {$acode != 0} {
    set bcode [catch {check_antennas} bres]
    puts "DRC-ANT-RESULT-BARE code=$bcode result=$bres"
}
exit
EOF
T_ANT=$SECONDS

# Step B/C/D magic runs share the canonical sky130 drc(full) sequence —
# DRC_DEF is set per run; cwd inside runs/drt-ours/ so .mag byproducts
# land in scratch.
run_magic() {
    local def_rel="$1" log="$2"
    export DRC_DEF="${def_rel}"
    (cd "${DRC_RUN_DIR}" && magic -dnull -noconsole -rcfile "${MAGICRC}" << 'EOF' | tee "${log}"
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
set cupcode [catch {drc catchup} cupres]
puts "DRCREF-CATCHUP-STATUS $cupcode"
puts "DRCREF-CATCHUP {$cupres}"
puts "DRCREF-COUNT-BEGIN"
set c1code [catch {drc count} c1]
puts "DRCREF-COUNT-RAW-STATUS $c1code"
puts "DRCREF-COUNT-RAW {$c1}"
set c2code [catch {drc count total} c2]
puts "DRCREF-COUNT-TOTAL-STATUS $c2code"
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
    if ! grep -q "^DRCREF-END" "${DRC_RUN_DIR}/${log}"; then
        echo "Error: Magic run on ${def_rel} ended without DRCREF-END (crash/hang; see ${DRC_RUN_DIR}/${log})." >&2
        exit 1
    fi
}

# Step B. Magic on our routed DEF.
SECONDS=0
run_magic "tests/designs/${DESIGN}/${DESIGN}.drt.ours.routed.def" "ours_magic.log"
T_MCLEAN=$SECONDS

# Step C. Magic on the placed DEF — the inherited-baseline measurement.
SECONDS=0
run_magic "tests/designs/${DESIGN}/${DESIGN}.placed.def" "placed_magic.log"
T_MPLACED=$SECONDS

# Step D. add_spacing_probe_wire mutation on OUR DEF + the identical
#         Magic sequence on the broken DEF.
SECONDS=0
python3 - "${DEF_ABS}" "${PDK_TLEF}" "${DRC_RUN_DIR}/${DESIGN}.drt.ours.broken.def" "${DRC_RUN_DIR}/mutation.json" <<'PYEOF'
import json
import re
import sys

def_path, tlef_path, out_path, mut_json = sys.argv[1:5]
text = open(def_path).read()

def fail(msg):
    sys.exit(f"drt-ours mutation failed: {msg}")

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

# Insert the disjoint wire inside net_b's path-statement region — same
# NEW grammar detailed_route emits, so every DEF reader accepts it.
# Path statements must precede `+`-clauses: TritonRoute puts `+ USE`
# before `+ ROUTED`, ours emit it after; a NEW dropped between `+ USE`
# and `;` is grammar Magic's DEF reader silently discards. Insert
# before the first `+`-clause that follows a path statement (`+ USE`),
# falling back to the terminating `;` when no such clause exists.
s0, s1 = nb_span
blk = nets_text[s0:s1]
if not blk.rstrip().endswith(";"):
    fail(f"net {net_b} block does not end with ';'")
PATH_STMT_KWS = ("ROUTED", "COVERED", "FIXED", "SHAPE", "MASK")
cut = None
for pm in re.finditer(r"\+ (\S+)", blk):
    if pm.group(1) in PATH_STMT_KWS:
        continue
    if ")" in blk[:pm.start()]:       # a path statement precedes this clause
        cut = pm.start()
        break
if cut is None:
    cut = blk.rindex(";")
broken = text[:mn.start(0) + s0] + blk[:cut].rstrip() + "\n" + wire + "\n" + \
    blk[cut:] + text[mn.start(0) + s1:]
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

if [ ! -f "${DRC_RUN_DIR}/${DESIGN}.drt.ours.broken.def" ]; then
    echo "Error: step D mutation did not produce ${DESIGN}.drt.ours.broken.def." >&2
    exit 1
fi

run_magic "tests/designs/${DESIGN}/runs/drt-ours/${DESIGN}.drt.ours.broken.def" "broken_magic.log"
T_MBROKEN=$SECONDS

cat > "${DRC_RUN_DIR}/runtime.json" <<EOF
{"antenna": ${T_ANT}, "magic_ours": ${T_MCLEAN}, "magic_placed": ${T_MPLACED}, "magic_broken": ${T_MBROKEN}, "assembly": 0}
EOF

# Step E. Assembly: parse the four transcripts + the mutation record +
#         our DEF + the committed join inputs; assert every expectation
#         loudly; emit <design>.drt.ours.reference.json.
SECONDS=0
python3 - "${DESIGN}" <<'PYEOF'
import hashlib
import json
import os
import re
import sys

design = sys.argv[1]
outdir = f"/work/tests/designs/{design}"
run = f"{outdir}/runs/drt-ours"
def_rel = f"tests/designs/{design}/{design}.drt.ours.routed.def"
def_abs = f"{outdir}/{design}.drt.ours.routed.def"
TLEF = "/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"

def fail(msg):
    sys.exit(f"drt-ours step-E assembly failed: {msg}")

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

# ---------- Magic transcript parsing (generate_drc_ref.sh step E) ----
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
    ms = re.search(r"^DRCREF-CATCHUP-STATUS (\d+)$", log, re.M)
    if ms is None or ms.group(1) != "0":
        fail(f"{log_path}: drc catchup failed or status marker missing")
    for key, pat in (("count_raw", r"^DRCREF-COUNT-RAW \{(.*)\}$"),
                     ("count_total", r"^DRCREF-COUNT-TOTAL \{(.*)\}$")):
        m = re.search(pat, log, re.M)
        if m is None:
            fail(f"{log_path}: missing marker for {key}")
        out[key] = m.group(1)
        ms = re.search(r"^DRCREF-" + key.upper().replace("_", "-")
                       + r"-STATUS (\d+)$", log, re.M)
        if ms is None:
            fail(f"{log_path}: missing catch-status marker for {key}")
        if ms.group(1) != "0":
            fail(f"{log_path}: drc command for {key} failed: {out[key]}")
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

ours = parse_magic(require(f"{run}/ours_magic.log"))
placed = parse_magic(require(f"{run}/placed_magic.log"))
broken = parse_magic(require(f"{run}/broken_magic.log"))

# ---------- OpenROAD transcript (surfaces + antenna) -----------------
alog = open(require(f"{run}/antenna.log")).read()
if not re.search(r"^DRC-READDEF-OK$", alog, re.M):
    fail("antenna.log: missing DRC-READDEF-OK — OpenROAD's read_def did not reach the marker")
surfaces = {}
for key, marker in (("antenna", "DRC-SURFACE-ANT"),
                    ("drc", "DRC-SURFACE-DRC"),
                    ("check", "DRC-SURFACE-CHECK")):
    m = re.search(rf"^{marker} (.*)$", alog, re.M)
    if m is None:
        fail(f"antenna.log: missing {marker}")
    surfaces[key] = m.group(1).strip()
if "check_antennas" not in surfaces["antenna"].split():
    fail("check_antennas absent from *antenna* surface")
m = re.search(r"^DRC-ANT-RESULT code=(\d+) result=(.*)$", alog, re.M)
if m is None:
    fail("antenna.log: missing DRC-ANT-RESULT")
ant_code, ant_result = int(m.group(1)), m.group(2).strip()
mb = re.search(r"^DRC-ANT-RESULT-BARE code=(\d+) result=(.*)$", alog, re.M)
ant_cmd = "check_antennas -verbose" if ant_code == 0 else "check_antennas"
if ant_code != 0 and mb is None:
    fail("antenna.log: verbose rejected but no bare fallback result")
ant_result_final = ant_result if ant_code == 0 else mb.group(2).strip()
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
# net: indented `Pin:` rows, per-layer `Partial/Cumulative area ratio`
# vs `Required ratio` lines, and a `(VIOLATED)` suffix on the failing
# requirement.
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
if ant_code == 0 and len(ant_per_net) != ant_viol:
    fail(f"antenna.log: {len(ant_per_net)} verbose Net blocks != "
         f"{ant_viol} reported net violations")
ant_report_lines = [ln.strip() for ln in alog.splitlines()
                    if re.match(r"\[(INFO|WARNING|ERROR) ANT", ln)]

# ---------- LEF via map (via-def name -> cut layer) ------------------
# VIA <name> blocks in the techlef list the cut layer first, then the
# two routing layers.

via_meaning = {}
tlef_text = open(require(TLEF)).read()
for m in re.finditer(r"^VIA (\S+).*?^END \1", tlef_text, re.M | re.S):
    layers = re.findall(r"^\s*LAYER (\S+)\s*;", m.group(0), re.M)
    if len(layers) < 3:
        fail(f"LEF VIA {m.group(1)} has {len(layers)} LAYER lines, expected >= 3")
    via_meaning[m.group(1)] = layers[0]

# ---------- routed-DEF metrics on OUR DEF ----------------------------
# The generate_drt_ref.sh step-C grammar, copied self-contained (these
# sibling scripts deliberately do not share a library). A non-Manhattan
# pair, unknown token, width extension, or `*` with no previous point
# aborts — the desired strictness on our own emitted text.

PATH_KEYWORDS = {"NEW", "MASK", "VIRTUAL", "TAPER", "TAPERRULE",
                 "POLYGON", "PATTERN", "STYLE", "SOURCE", "USE", "SHIELD",
                 "VPIN", "SUBNET", "NOSHIELD", "NONDEFAULTRULE", "SYNTHESIZED",
                 "PROPERTY", "ESTCAP"}

# `+`-clauses that may legally follow the routed-path statements of a net
# entry (DEF grammar): none carry geometry, so the rest of the entry is
# metadata once one appears.
PATH_END_KEYWORDS = {"USE", "SOURCE", "PROPERTY", "SYNTHESIZED",
                     "NONDEFAULTRULE", "ESTCAP", "DESIGN", "SHIELDNET",
                     "SHIELD", "NOSHIELD", "SUBNET", "VPIN", "XTALK",
                     "FREQUENCY", "FPARS", "ORIGINAL", "PATTERN", "STYLE",
                     "FIXEDBUMP"}

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
            if kw in PATH_END_KEYWORDS:
                # A `+`-clause after the path statements (`+ USE SIGNAL`,
                # `+ SOURCE`, `+ PROPERTY`, ...) carries no geometry; ours
                # emit it *after* the routed path where TritonRoute puts
                # `+ USE` first. Skip to the entry terminator.
                j = i + 2
                while j < len(tokens) and tokens[j] != ";":
                    j += 1
                if j >= len(tokens):
                    fail(f"net {net_name}: unterminated +{kw} clause")
                i = j + 1
                break
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
            # ( x y ) or ( x y ext ) — the optional third element is the
            # DEF wire-extension value at the point, not a distance axis.
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
    # DEF-written via names (VIAS section) map to their + VIARULE, not a
    # LEF cut layer — kept under their own key, never dropped.
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

metrics = routed_metrics(def_abs)

placed_text = open(require(f"{outdir}/{design}.placed.def")).read()
signal_nets_in_def = len(nets_entries(placed_text))

def via_meaning_for(name, def_vias):
    if name in via_meaning:
        return via_meaning[name]
    if name in def_vias:
        return def_vias[name]
    return "unknown"

ours_via_meaning = {name: via_meaning_for(name, metrics["def_vias"])
                    for name in metrics["vias_per_cut_layer"]}

def bucket_by_cut(via_counts, meaning_map, def_vias):
    """Re-bucket via-name counts under their cut-layer names."""
    out = {}
    for name, n in via_counts.items():
        cut = meaning_map.get(name) if meaning_map is not None else None
        if cut is None:
            cut = via_meaning_for(name, def_vias)
        out[cut] = out.get(cut, 0) + n
    return out

# ---------- taxonomy join (committed classification) -----------------
# `classification` maps tech-file why TEMPLATES ("... < %d (met2.2)")
# to classes; `drc listall why` renders them ("... < 0.14um (met2.2)").
# The join is by template match — `%.` widens to `.*?`.

drc_ref_path = require(f"{outdir}/{design}.drc.reference.json")
drt_ref_path = require(f"{outdir}/{design}.drt.reference.json")
drc_ref = json.load(open(drc_ref_path))
drt_ref = json.load(open(drt_ref_path))
classification = drc_ref["magic_rules_checked"]["classification"]
routed_ref_per_rule = drc_ref["magic_per_rule"]

def tmpl_match(template, observed):
    pat = re.escape(template)
    pat = re.sub(r"%.", ".*?", pat)
    return re.fullmatch(pat, observed) is not None

def class_of(rendered):
    for t, c in classification.items():
        if t == rendered or tmpl_match(t, rendered):
            return c
    return None

def rule_id(rendered):
    m = re.search(r"\(([^()]*)\)\s*$", rendered)
    return m.group(1).strip() if m else None

def count_by_id(per_rule):
    """Aggregate a rendered per_rule map under (rule_id, class) keys."""
    by_id = {}
    unclassified = []
    for name, n in per_rule.items():
        cls = class_of(name)
        rid = rule_id(name)
        if cls is None or rid is None:
            unclassified.append(name)
            continue
        by_id[(rid, cls)] = by_id.get((rid, cls), 0) + n
    return by_id, unclassified

ours_by_id, ours_uncl = count_by_id(ours["per_rule"])
placed_by_id, placed_uncl = count_by_id(placed["per_rule"])
ref_by_id, ref_uncl = count_by_id(routed_ref_per_rule)

# Rendered names the committed taxonomy cannot classify: on a clean
# DEF this set never fires (Magic's generic overlap/abutment messages
# have no tech-file rule id, so the deck extraction never saw them).
# Exactly two such names exist — Magic's built-in hierarchy-boundary
# messages (drcOverlapCookie / drcSubcellCookie literals). They stay
# recorded with per-input counts (the gate asserts `ours == 0`
# post-fix), but any *new* unclassified name is a deck change or a new
# violation class — abort loudly rather than grow the set silently.
ALLOWED_UNCLASSIFIED = {
    "Can't overlap those layers",
    "This layer can't abut or partially overlap between subcells",
}
unclassified_names = sorted(set(ours_uncl) | set(placed_uncl) | set(ref_uncl))
new_uncl = sorted(set(unclassified_names) - ALLOWED_UNCLASSIFIED)
if new_uncl:
    fail(f"unclassified Magic rule names outside the pinned allowlist: "
         f"{new_uncl}")
unclassified_fired = [
    {
        "name": name,
        "ours": ours["per_rule"].get(name, 0),
        "placed": placed["per_rule"].get(name, 0),
        "routed_ref": routed_ref_per_rule.get(name, 0),
    }
    for name in unclassified_names
]

# lef_expressible: the full 50-name map keyed by rule id, explicit
# zeros — absent keys prove nothing.
lef_template_ids = {}
for t, c in classification.items():
    if c != "lef_expressible":
        continue
    rid = rule_id(t)
    if rid is None:
        fail(f"classification template without a rule id: {t!r}")
    if rid in lef_template_ids:
        fail(f"duplicate lef_expressible rule id: {rid!r}")
    lef_template_ids[rid] = t
lef_expressible = {}
for rid in sorted(lef_template_ids):
    ours_n = ours_by_id.get((rid, "lef_expressible"), 0)
    placed_n = placed_by_id.get((rid, "lef_expressible"), 0)
    ref_n = ref_by_id.get((rid, "lef_expressible"), 0)
    lef_expressible[rid] = {
        "ours": ours_n,
        "placed": placed_n,
        "routed_ref": ref_n,
        "inherited": ours_n == placed_n,
    }

# magic_only: every nonzero union member across {ours, placed,
# routed_ref}, keyed by rule id with all three counts.
mo_ids = sorted({rid for (rid, cls) in
                 set(ours_by_id) | set(placed_by_id) | set(ref_by_id)
                 if cls == "magic_only"})
magic_only = []
for rid in mo_ids:
    ours_n = ours_by_id.get((rid, "magic_only"), 0)
    placed_n = placed_by_id.get((rid, "magic_only"), 0)
    ref_n = ref_by_id.get((rid, "magic_only"), 0)
    if ours_n == 0 and placed_n == 0 and ref_n == 0:
        continue
    magic_only.append({
        "rule": rid,
        "ours": ours_n,
        "placed": placed_n,
        "routed_ref": ref_n if (rid, "magic_only") in ref_by_id else None,
        "inherited": ours_n == placed_n,
    })

# ---------- ours_router provenance -----------------------------------
emit_log_rel = f"tests/designs/{design}/runs/drt-ours/{design}.drt.ours.log"
emit_log_abs = f"{outdir}/runs/drt-ours/{design}.drt.ours.log"
guide_rel = f"tests/designs/{design}/{design}.guide"
guide_abs = f"{outdir}/{design}.guide"
require(guide_abs)

ours_router = {
    "grus_eda_version": os.environ.get("GRUS_VERSION") or None,
    "guide": {"file": guide_rel, **stamp(guide_abs)},
    "emit_log": emit_log_rel,
    "emit_log_present": os.path.isfile(emit_log_abs),
    "cli_lines": None,
    "routed": None,
    "unrouted_count": None,
    "unrouted_kinds": None,
    "unrouted_blockers": None,
    "unrouted": None,
    "collisions": None,
    "area_patches": None,
    "area_residuals": None,
    "hull_masked": None,
}
if ours_router["emit_log_present"]:
    elog = open(emit_log_abs).read()
    cli = []
    for ln in elog.splitlines():
        s = ln.rstrip()
        if (s.startswith("== drt emit") or s.startswith("guided=")
                or s.lstrip().startswith("unrouted ")
                or s.lstrip().startswith("unrouted_kinds ")
                or s.lstrip().startswith("unrouted_blockers ")
                or s.lstrip().startswith("area_residual ")
                or s.lstrip().startswith("stub_outside_cover ")
                or s.startswith("self_drc")):
            cli.append(s)
    # The golden must byte-reproduce on any checkout: the `wrote routed
    # DEF` line is dropped above (checkout-absolute path, redundant with
    # `input_def`), and any other path-bearing line slipping into the
    # keep-list is a loud failure, not a committed diff.
    if any(re.search(r"(?<![A-Za-z_.])/", ln) for ln in cli):
        fail("cli_lines carries an absolute path — golden would not "
             "byte-reproduce")
    ours_router["cli_lines"] = cli
    mg = re.search(
        r"^guided=(\d+) routed=(\d+) unrouted=(\d+) projected=(\d+) "
        r"collisions=(\d+) area_patches=(\d+) area_residuals=(\d+) "
        r"iterations=(\d+) route_ms=(\d+) hull_masked=(\d+)", elog, re.M)
    if mg is None:
        fail("emit log present but its guided= census line is missing")
    ours_router["routed"] = int(mg.group(2))
    ours_router["unrouted_count"] = int(mg.group(3))
    ours_router["collisions"] = int(mg.group(5))
    ours_router["area_patches"] = int(mg.group(6))
    ours_router["area_residuals"] = int(mg.group(7))
    ours_router["hull_masked"] = int(mg.group(10))
    ours_router["unrouted"] = re.findall(r"^\s+unrouted (\S+)$", elog, re.M)
    mk = re.search(
        r"^unrouted_kinds NoPath=(\d+) Unattachable=(\d+)$", elog, re.M)
    ours_router["unrouted_kinds"] = (
        {"NoPath": int(mk.group(1)), "Unattachable": int(mk.group(2))}
        if mk else None)
    mb = re.search(r"^unrouted_blockers (.*)$", elog, re.M)
    ours_router["unrouted_blockers"] = (
        dict(
            (kv.split("=")[0], int(kv.split("=")[1]))
            for kv in mb.group(1).split())
        if mb else None)

# ---------- sanity control -------------------------------------------
mutation = json.load(open(require(f"{run}/mutation.json")))
violated = sorted(broken["per_rule"])
if broken["total"] <= 0 or not violated:
    fail("sanity control failed: broken DEF reported 0 errors / no named rules")
new_vs_clean = sorted(set(broken["per_rule"]) - set(ours["per_rule"]))
grew = [w for w in broken["per_rule"]
        if broken["per_rule"][w] > ours["per_rule"].get(w, 0)]
if not new_vs_clean and not grew:
    fail("sanity control failed: broken DEF adds no violations over ours")
# Every new rule name must classify lef_expressible — the injected
# probe is a spacing violation the checked deck covers.
new_ids = {}
for name in new_vs_clean:
    cls = class_of(name)
    rid = rule_id(name)
    if cls is None:
        fail(f"sanity new rule {name!r} absent from the classification")
    new_ids[name] = cls
in_deck = {rule_id(n) or n: (new_ids[n] == "lef_expressible")
           for n in new_vs_clean}
violated_in_deck = all(c == "lef_expressible" for c in new_ids.values())

# ---------- delta block ----------------------------------------------
ours_by_cut = bucket_by_cut(metrics["vias_per_cut_layer"],
                            ours_via_meaning, metrics["def_vias"])
ours_ant_total = (ant_viol or 0) + (ant_pin or 0)

def delta_vs(variant):
    v = drt_ref["variants"][variant]
    var_cut = bucket_by_cut(v["vias_per_cut_layer"],
                            v["via_names_meaning"], metrics["def_vias"])
    per_cut = {c: ours_by_cut.get(c, 0) - var_cut.get(c, 0)
               for c in sorted(set(ours_by_cut) | set(var_cut))}
    wl_delta = metrics["total_wirelength_um"] - v["total_wirelength_um"]
    return {
        "total_wirelength_um": round(wl_delta, 6),
        "total_wirelength_pct": (round(wl_delta / v["total_wirelength_um"] * 100, 4)
                               if v["total_wirelength_um"] else None),
        "via_count_total": metrics["via_count_total"] - v["via_count_total"],
        "vias_per_cut_layer": per_cut,
        "antenna_violations": ours_ant_total - v["antenna_violations"],
        "antenna_pin_violations": (ant_pin or 0) - v["antenna_pin_violations"],
        "signal_nets_routed": metrics["signal_nets_routed"] - v["signal_nets_routed"],
    }

delta = {
    "vs_delegated": delta_vs("delegated"),
    "vs_baseline": delta_vs("baseline"),
}

# ---------- emit ------------------------------------------------------
input_def_bytes = open(def_abs, "rb").read()
out = {
    "design": design,
    "image": "ghcr.io/librelane/librelane:3.0.16",
    "image_id": os.environ.get("DRC_IMAGE_ID") or None,
    "generated_by": "tests/designs/check_drt_ours.sh",
    "input_def": def_rel,
    "input_def_sha_prefix": hashlib.sha256(input_def_bytes).hexdigest()[:16],
    "input_def_size_bytes": len(input_def_bytes),
    "input_def_provenance": "grus-eda drt via tests/designs/generate_drt_ours.sh",
    "guide": {"file": guide_rel, **stamp(guide_abs)},
    "ours_router": ours_router,
    "openroad": {
        "read_def": "ok",
        "surfaces": surfaces,
        "drt_report": None,
        "drt_report_source": "not_applicable_native_router",
        "antenna_violations": ant_viol,
        "antenna_pin_violations": ant_pin,
        "antenna_per_net": ant_per_net,
        "antenna_report_lines": ant_report_lines,
        "antenna_command": ant_cmd,
    },
    "magic": {
        "version": ours["version"],
        "style": "drc(full)",
        "rcfile": "/pdk/sky130A/libs.tech/magic/sky130A.magicrc",
        "count_flat": ours["count_raw_printed"],
        "count_total": ours["count_total_printed"],
        "listall_items": ours["listall_items"],
        "bbox_total": ours["bbox_total"],
        "per_rule": ours["per_rule"],
        "lef_expressible": lef_expressible,
        "magic_only": magic_only,
        "unclassified_fired": unclassified_fired,
    },
    "placed_baseline": {
        "count_flat": placed["count_raw_printed"],
        "count_total": placed["count_total_printed"],
        "per_rule": placed["per_rule"],
    },
    "def_metrics": {
        "signal_nets_in_def": signal_nets_in_def,
        "signal_nets_routed": metrics["signal_nets_routed"],
        "total_wirelength_um": metrics["total_wirelength_um"],
        "vias_per_cut_layer": metrics["vias_per_cut_layer"],
        "via_names_meaning": ours_via_meaning,
        "via_count_total": metrics["via_count_total"],
        "dbu_per_micron": metrics["dbu_per_micron"],
    },
    "delta": delta,
    "drc_reference": {"file": f"tests/designs/{design}/{design}.drc.reference.json",
                      "sha256_16": sha16(drc_ref_path)},
    "drt_reference": {"file": f"tests/designs/{design}/{design}.drt.reference.json",
                      "sha256_16": sha16(drt_ref_path)},
    "sanity_control": {
        "mutation": mutation,
        "count_flat": broken["count_raw_printed"],
        "count_total": broken["count_total_printed"],
        "per_rule": broken["per_rule"],
        "new_rules_vs_clean": new_vs_clean,
        "in_deck": in_deck,
        "violated_rules_in_checked_deck": violated_in_deck,
    },
}
path = f"{outdir}/{design}.drt.ours.reference.json"
with open(path, "w") as fp:
    json.dump(out, fp, indent=2)
    fp.write("\n")
lef_nonzero = {k: v for k, v in lef_expressible.items()
               if v["ours"] != 0}
print(f"DRC-SUMMARY ours count_flat={ours['count_raw_printed']} "
      f"count_total={ours['count_total_printed']} "
      f"lef_expressible_nonzero={len(lef_nonzero)} "
      f"magic_only_rows={len(magic_only)} "
      f"unclassified={len(unclassified_fired)} "
      f"antenna={ant_viol} wirelength_um={metrics['total_wirelength_um']} "
      f"vias={metrics['via_count_total']}")
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

if [ ! -f "${SCRIPT_DIR}/${DESIGN}/${DESIGN}.drt.ours.reference.json" ]; then
    echo "Error: step E did not produce ${DESIGN}.drt.ours.reference.json." >&2
    exit 1
fi

echo "==> ours-DEF check artifacts for ${DESIGN} written."
