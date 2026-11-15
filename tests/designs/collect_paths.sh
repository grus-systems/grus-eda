#!/usr/bin/env bash
# Dumps OpenSTA's worst setup paths (every stage with slew, cap, delay, arrival) for one design on one SPEF,
# in the pinned LibreLane image.
#
# usage: tests/designs/collect_paths.sh <design> <placed.def> <spef> <out.txt> [paths] [report_checks_args]
#   paths   number of worst paths, one per endpoint (default 5)
#   report_checks_args   extra Tcl arguments for report_checks, for example "-from [all_inputs]"
# Output (relative to tests/designs/<design>) is OpenSTA's `report_checks -format full` text as printed, with
# `-fields {slew cap fanout input_pins} -digits 6`.
set -euo pipefail

DESIGN="$1"; DEF="$2"; SPEF="$3"; OUT="$4"; PATHS="${5:-5}"; EXTRA="${6:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"
RUN_DIR="${SCRIPT_DIR}/${DESIGN}/runs/paths_collect"
mkdir -p "${RUN_DIR}"

if [ ! -f /.dockerenv ]; then
    if docker info >/dev/null 2>&1; then docker_cmd="docker"; else docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"; fi
    exec ${docker_cmd} run --rm -v "${REPO_ROOT}:/work" -v "${PDK_HOST_ROOT}:/pdk" -e PDK_ROOT=/pdk \
        "${IMAGE}" bash "/work/tests/designs/collect_paths.sh" "$@"
fi

D="/work/tests/designs/${DESIGN}"
cat > "${RUN_DIR}/paths.tcl" <<EOF
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def ${D}/${DEF}
read_sdc ${D}/${DESIGN}.sdc
read_spef ${D}/${SPEF}
report_checks -path_delay max ${EXTRA} -group_path_count ${PATHS} -unique_paths_to_endpoint -fields {slew cap fanout input_pins} -digits 6 -format full
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/paths.tcl" > "${RUN_DIR}/paths.log" 2>&1
# Keep only the report (drop the tool's start-up messages).
python3 - "${RUN_DIR}/paths.log" "${SCRIPT_DIR}/${DESIGN}/${OUT}" <<'PYEOF'
import sys
text = open(sys.argv[1]).read()
open(sys.argv[2], "w").write(text[text.index("Startpoint:"):])
PYEOF
echo "wrote ${DESIGN}/${OUT}"
