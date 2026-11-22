#!/usr/bin/env bash
# Host-side generation of the ours-side E5 e2e artifacts: `grus-eda cts` on
# <design>.placed.def + <design>.cts.sdc, emitting <design>.cts.ours.def and
# <design>.cts.ours.report.json — the literal "regenerated from committed
# commands" input the e2e comparison scores against the TritonCTS references.
# Runs on the host with the release binary, not inside the container (the
# `route --guide-out`/`generate_guide_ours_spef.sh` precedent).
#
# gcd's outputs are committed (< 1 MB); picorv32's are gitignored like its
# other generated artifacts. picorv32's placed DEF is gitignored too — run
# tests/designs/picorv32/generate_ref.sh first to produce it (gcd's is
# committed). write_def_full is deterministic, so a rerun is byte-identical;
# the committed gcd pair is asserted equal to a fresh emission by
# test_gcd_cts_ours_def_deterministic.
#
# Usage: generate_cts_ours.sh <design>   (designs: gcd, picorv32)
set -euo pipefail

DESIGN="${1:?usage: generate_cts_ours.sh <design>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PDK_ROOT_DIR="${PDK_ROOT:-${HOME}/.ciel}"

D="${SCRIPT_DIR}/${DESIGN}"
if [ ! -f "${D}/${DESIGN}.placed.def" ]; then
    echo "Error: missing ${D}/${DESIGN}.placed.def (regenerate via tests/designs/${DESIGN}/generate_ref.sh)" >&2
    exit 1
fi
if [ ! -f "${D}/${DESIGN}.cts.sdc" ]; then
    echo "Error: missing ${D}/${DESIGN}.cts.sdc (produced by tests/designs/generate_cts_ref.sh inputs)" >&2
    exit 1
fi

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

"${GRUS}" cts \
    --tlef "${PDK_TLEF}" --lef "${PDK_LEF}" --lib "${PDK_LIB}" \
    --def "${D}/${DESIGN}.placed.def" \
    --sdc "${D}/${DESIGN}.cts.sdc" \
    --def-out "${D}/${DESIGN}.cts.ours.def" \
    --report "${D}/${DESIGN}.cts.ours.report.json"

echo "wrote ${D}/${DESIGN}.cts.ours.def and ${D}/${DESIGN}.cts.ours.report.json"
