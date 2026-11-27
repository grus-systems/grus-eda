#!/usr/bin/env bash
# Committed route-guide golden generation for any design: the step-5c
# recipe of the per-design generate_ref.sh scripts, generalized.
# Runs OpenROAD `global_route -guide_file` on <design>.placed.def with
# the same signal band and layer adjustments GCellGrid::build applies
# by default (met1 0.30, met2..met5 0.20), emitting the committed
# <design>.guide — the input the whole E6 chain is pinned on.
#
# gcd's and picorv32's guides were emitted by the identical Tcl inline
# in their generate_ref.sh; this script exists so additional DRT leg
# designs (aes, ibex, ...) do not each grow a copy. It deliberately
# does NOT regenerate the guides of designs whose per-design
# generate_ref.sh already covers them — pass --force for that.
#
# Self-reentering: on the host it re-invokes itself inside the
# canonical container ghcr.io/librelane/librelane:3.0.16 with the repo
# at /work and the PDK at /pdk (the generate_drt_ref.sh pattern).
#
# Usage: generate_guide_ref.sh <design> [--force]
set -euo pipefail

DESIGN="${1:?usage: generate_guide_ref.sh <design> [--force]}"
FORCE=0
if [ "${2:-}" = "--force" ]; then
    FORCE=1
elif [ -n "${2:-}" ]; then
    echo "usage: generate_guide_ref.sh <design> [--force]" >&2
    exit 1
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

D="${SCRIPT_DIR}/${DESIGN}"
for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
    if [ ! -f "${D}/${f}" ]; then
        echo "Error: missing ${D}/${f}." >&2
        exit 1
    fi
done

if [ -f "${D}/${DESIGN}.guide" ] && [ "${FORCE}" -eq 0 ]; then
    echo "Error: ${D}/${DESIGN}.guide already exists. This script is for designs without a" >&2
    echo "committed guide (aes, ibex, ...); gcd/picorv32 regenerate theirs via their own" >&2
    echo "generate_ref.sh. Pass --force only if you mean to overwrite." >&2
    exit 1
fi

if [ ! -d "${PDK_HOST_ROOT}/sky130A" ]; then
    echo "Error: sky130A PDK not found at ${PDK_HOST_ROOT}/sky130A." >&2
    exit 1
fi

if [ ! -f /.dockerenv ]; then
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
        -e GUIDE_DESIGN="${DESIGN}" \
        -w "/work/tests/designs/${DESIGN}" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/generate_guide_ref.sh "${DESIGN}" --force
    exit 0
fi

DESIGN="${GUIDE_DESIGN:-${DESIGN}}"

# Tcl on stdin: `!` is history expansion in the interactive shell, and
# `\` does not continue long lines — every command is one physical line.
openroad -no_init -no_splash << EOF
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/${DESIGN}/${DESIGN}.placed.def
read_sdc /work/tests/designs/${DESIGN}/${DESIGN}.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
global_route -guide_file /work/tests/designs/${DESIGN}/${DESIGN}.guide -congestion_iterations 100
exit
EOF

if [ ! -s "/work/tests/designs/${DESIGN}/${DESIGN}.guide" ]; then
    echo "Error: global_route did not emit a non-empty ${DESIGN}.guide." >&2
    exit 1
fi
echo "wrote /work/tests/designs/${DESIGN}/${DESIGN}.guide ($(wc -l < "/work/tests/designs/${DESIGN}/${DESIGN}.guide") lines)"
