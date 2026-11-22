#!/usr/bin/env bash
# Dumps OpenSTA's worst-3 setup (max) and hold (min) paths for gcd on the
# placement-estimate SPEF — the same inputs `gcd.sta.reference.json`'s
# `placed_spef` block uses (gcd.placed.def + gcd.routed.spef + gcd.sdc) — as
# `gcd.paths_max.txt` / `gcd.paths_min.txt`, in the pinned LibreLane image.
#
# Probed flag surface (`sta -no_init -no_splash`, `help report_checks`,
# 2026-11-23): -path_delay min|max, -group_path_count, -endpoint_path_count,
# -unique_paths_to_endpoint, -fields, -digits, -format full all exist.
# `-group_path_count 3 -unique_paths_to_endpoint` is the same spelling
# `collect_paths.sh` uses for "3 worst paths with distinct endpoints";
# `-endpoint_path_count 1` makes the one-trace-per-endpoint semantics explicit.
#
# usage: tests/designs/gcd/generate_paths_ref.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"
RUN_DIR="${SCRIPT_DIR}/runs/paths_ref"
mkdir -p "${RUN_DIR}"

if [ ! -f /.dockerenv ]; then
    if docker info >/dev/null 2>&1; then docker_cmd="docker"; else docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"; fi
    exec ${docker_cmd} run --rm -v "${REPO_ROOT}:/work" -v "${PDK_HOST_ROOT}:/pdk" -e PDK_ROOT=/pdk \
        "${IMAGE}" bash "/work/tests/designs/gcd/generate_paths_ref.sh" "$@"
fi

D="/work/tests/designs/gcd"
cat > "${RUN_DIR}/paths.tcl" <<EOF
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def ${D}/gcd.placed.def
read_sdc ${D}/gcd.sdc
read_spef ${D}/gcd.routed.spef
report_checks -path_delay max -group_path_count 3 \
    -unique_paths_to_endpoint -endpoint_path_count 1 \
    -fields {slew cap fanout input_pins} -digits 6 -format full
report_checks -path_delay min -group_path_count 3 \
    -unique_paths_to_endpoint -endpoint_path_count 1 \
    -fields {slew cap fanout input_pins} -digits 6 -format full
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/paths.tcl" > "${RUN_DIR}/paths.log" 2>&1
# Split the two reports out of the log; each starts at its first "Startpoint:".
python3 - "${RUN_DIR}/paths.log" "${SCRIPT_DIR}/gcd.paths_max.txt" "${SCRIPT_DIR}/gcd.paths_min.txt" <<'PYEOF'
import re, sys
text = open(sys.argv[1]).read()
blocks = re.split(r"(?=Startpoint: )", text)
paths = []
cur = []
for b in blocks:
    if not b.startswith("Startpoint: "):
        continue
    m = re.search(r"^Path Type: (max|min)", b, re.M)
    cur.append((m.group(1) if m else "?", b))
paths = cur
# OpenSTA prints all max paths then all min paths in command order.
out = {"max": [], "min": []}
for kind, b in paths:
    out[kind].append(b)
for kind, path in (("max", sys.argv[2]), ("min", sys.argv[3])):
    if not out[kind]:
        raise SystemExit(f"no {kind} paths parsed from {sys.argv[1]}")
    with open(path, "w") as f:
        f.write("".join(out[kind]))
    print(f"wrote {path} ({len(out[kind])} paths)")
PYEOF
