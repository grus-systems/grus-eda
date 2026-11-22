#!/usr/bin/env bash
# Host-side generation of the ours-side E4 e2e artifacts: run our own pipeline
# end to end and emit <design>.guide.ours.spef — `route --guide-out` (our guide
# file) feeding `sta --parasitics guide --guide --spef-out` (parse_guides ->
# resolve_guide_nets -> estimate_guide_parasitics -> write_spef). Runs on the
# host with the release binary, not inside the container.
#
# The emitted guide lands at tests/designs/<design>/runs/guide-check/
# <design>.ours.guide — the same path check_guide_readback.sh defaults to —
# and is gitignored under tests/designs/<design>/runs. write_spef pins its
# *DATE line and is deterministic, so a rerun is byte-identical.
#
# picorv32's placed DEF is gitignored — run
# tests/designs/picorv32/generate_ref.sh first to produce it (gcd's is
# committed). generate_ref.sh step 9c consumes both outputs of this script;
# run this first.
#
# Usage: generate_guide_ours_spef.sh <design>   (designs: gcd, picorv32)
set -euo pipefail

DESIGN="${1:?usage: generate_guide_ours_spef.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PDK_ROOT_DIR="${PDK_ROOT:-${HOME}/.ciel}"

D="${SCRIPT_DIR}/${DESIGN}"
for f in "${DESIGN}.placed.def" "${DESIGN}.sdc"; do
    if [ ! -f "${D}/${f}" ]; then
        echo "Error: missing ${D}/${f} (regenerate via tests/designs/${DESIGN}/generate_ref.sh)" >&2
        exit 1
    fi
done

PDK_LIB="${PDK_ROOT_DIR}/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="${PDK_ROOT_DIR}/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="${PDK_ROOT_DIR}/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"
for f in "${PDK_LIB}" "${PDK_TLEF}" "${PDK_LEF}"; do
    if [ ! -f "${f}" ]; then
        echo "Error: missing PDK file ${f} (set PDK_ROOT or install sky130A into ~/.ciel)" >&2
        exit 1
    fi
done

cd "${REPO_ROOT}"
cargo build --release -p grus-eda
GRUS="${REPO_ROOT}/target/release/grus-eda"

GUIDE_OUT="${D}/runs/guide-check/${DESIGN}.ours.guide"
mkdir -p "$(dirname "${GUIDE_OUT}")"

"${GRUS}" route \
    --tlef "${PDK_TLEF}" --lef "${PDK_LEF}" \
    --def "${D}/${DESIGN}.placed.def" \
    --guide-out "${GUIDE_OUT}"

"${GRUS}" sta \
    --lib "${PDK_LIB}" --tlef "${PDK_TLEF}" --lef "${PDK_LEF}" \
    --def "${D}/${DESIGN}.placed.def" --sdc "${D}/${DESIGN}.sdc" \
    --parasitics guide --guide "${GUIDE_OUT}" \
    --spef-out "${D}/${DESIGN}.guide.ours.spef"

echo "wrote ${GUIDE_OUT} and ${D}/${DESIGN}.guide.ours.spef"
