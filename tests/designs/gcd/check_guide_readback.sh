#!/usr/bin/env bash
# Read-back check for an emitted route guide: runs OpenROAD `read_guides` and
# `estimate_parasitics -global_routing` on our file inside the canonical
# container ghcr.io/librelane/librelane:3.0.16. Any read_guides/parasitics
# error is a hard failure (non-zero exit), never a warning.
#
# Usage: check_guide_readback.sh [path-to-guide]
#   default guide path: tests/designs/gcd/runs/guide-check/gcd.ours.guide

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

if [ ! -d "${PDK_HOST_ROOT}/sky130A" ]; then
    echo "Error: sky130A PDK not found at ${PDK_HOST_ROOT}/sky130A." >&2
    echo "Please ensure Ciel/Volare sky130A is installed in ~/.ciel or set PDK_ROOT." >&2
    exit 1
fi

GUIDE_PATH="${1:-${SCRIPT_DIR}/runs/guide-check/gcd.ours.guide}"
case "${GUIDE_PATH}" in
    /*) ;;
    *) GUIDE_PATH="$(pwd)/${GUIDE_PATH}" ;;
esac
if [ ! -f "${GUIDE_PATH}" ]; then
    echo "Error: guide file not found at ${GUIDE_PATH}" >&2
    exit 1
fi
# The container sees the repo at /work; the guide path must live under it.
case "${GUIDE_PATH}" in
    "${REPO_ROOT}"/*) GUIDE_IN_CONTAINER="/work/${GUIDE_PATH#"${REPO_ROOT}"/}" ;;
    *)
        echo "Error: guide ${GUIDE_PATH} is outside the repository; copy it under ${REPO_ROOT} first." >&2
        exit 1
        ;;
esac

if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    echo "==> Running guide read-back inside container ${CONTAINER_IMAGE}..."
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
        -e "GUIDE_PATH=${GUIDE_IN_CONTAINER}" \
        -w "/work/tests/designs/gcd" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/gcd/check_guide_readback.sh "${GUIDE_IN_CONTAINER}"
    exit 0
fi

mkdir -p "${SCRIPT_DIR}/runs/guide-check"
SPEF_OUT="${SCRIPT_DIR}/runs/guide-check/$(basename "${GUIDE_PATH}").spef"

# set_wire_rc before read_guides: estimate_parasitics needs the RC model (EST-0018).
openroad -no_init -no_splash << EOF
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_wire_rc -layer met2
read_guides ${GUIDE_PATH}
estimate_parasitics -global_routing -spef_file ${SPEF_OUT}
exit
EOF

echo "read_guides + estimate_parasitics -global_routing accepted ${GUIDE_PATH}"
