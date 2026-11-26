#!/usr/bin/env bash
# Host-side generation of the ours-side E6 e2e input artifact: release
# `grus-eda drt` on <design>.placed.def + the committed <design>.guide
# (OpenROAD `global_route -guide_file` output — the input the whole E6
# chain is pinned on; it is an input, never regenerated here), emitting
# <design>.drt.ours.routed.def plus the tee'd emit census log under
# runs/drt-ours/ — the same `generate_cts_ours.sh` shape: ours-side
# products are emitted on the host with the release binary, never
# inside the container.
#
# No --occ/--max-iters flags — the CLI defaults (OccMode::Enabled,
# MAX_ITERS = 8) are the contract the test legs use
# (MazeConfig::default()). Self-DRC is default-on (re-parse ->
# assemble -> run_all on the emitted DEF) and gates the exit code
# above --self-drc-tolerance; the verdict line and findings flow into
# the log, which check_drt_ours.sh step E lifts verbatim into
# <design>.drt.ours.reference.json's `ours_router` block. picorv32
# carries a pinned emit-visible residual (3 of the 21 met1.2 sites —
# docs/e6-drt.md §9), so its budget is 3; every other design is 0.
#
# gcd's routed DEF is committed (< 1 MB) and asserted byte-equal to a
# fresh emission by test_e2e_ours_def_deterministic_gcd; picorv32's is
# git-ignored like its other generated artifacts. picorv32's placed DEF
# is gitignored too — run tests/designs/picorv32/generate_ref.sh first.
#
# --verify runs the identical `grus-eda drt` invocation a second time
# into runs/drt-ours/ scratch and cmps the two DEFs: equal prints
# OURS-DETERMINISTIC <sha>; different prints the first cmp offset and
# exits 1 — a route/emit nondeterminism is a bug, never a committed
# artifact. It is a separate flag because the second route doubles wall
# clock (picorv32's emit alone is a multi-hour detached leg; the
# results entry records which verify arms actually ran).
#
# Usage: generate_drt_ours.sh <design> [--verify]   (gcd, picorv32)
set -euo pipefail

DESIGN="${1:?usage: generate_drt_ours.sh <design> [--verify]}"
VERIFY=0
if [ "${2:-}" = "--verify" ]; then
    VERIFY=1
elif [ -n "${2:-}" ]; then
    echo "usage: generate_drt_ours.sh <design> [--verify]" >&2
    exit 1
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
PDK_ROOT_DIR="${PDK_ROOT:-${HOME}/.ciel}"

D="${SCRIPT_DIR}/${DESIGN}"
if [ ! -f "${D}/${DESIGN}.placed.def" ]; then
    echo "Error: missing ${D}/${DESIGN}.placed.def (regenerate via tests/designs/${DESIGN}/generate_ref.sh)" >&2
    exit 1
fi
if [ ! -f "${D}/${DESIGN}.guide" ]; then
    echo "Error: missing ${D}/${DESIGN}.guide (OpenROAD global_route -guide_file output; the committed E6 guide input)" >&2
    exit 1
fi
if [ ! -f "${D}/${DESIGN}.drt.reference.json" ]; then
    echo "Error: missing ${D}/${DESIGN}.drt.reference.json (produced by tests/designs/generate_drt_ref.sh ${DESIGN})" >&2
    exit 1
fi

PDK_TLEF="${PDK_ROOT_DIR}/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="${PDK_ROOT_DIR}/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"
for f in "${PDK_TLEF}" "${PDK_LEF}"; do
    if [ ! -f "${f}" ]; then
        echo "Error: missing PDK file ${f} (set PDK_ROOT or install sky130A into ~/.ciel)" >&2
        exit 1
    fi
done

RUN_DIR="${D}/runs/drt-ours"
mkdir -p "${RUN_DIR}"

cd "${REPO_ROOT}"
cargo build --release -p grus-eda
GRUS="${REPO_ROOT}/target/release/grus-eda"

OUT_DEF="${D}/${DESIGN}.drt.ours.routed.def"

# Per-design self-DRC findings budget: picorv32's pinned emit-visible
# met1.2 residual (3 of 21 sites — docs/e6-drt.md §9) would otherwise
# abort the emit under pipefail; the strict per-rule gate stays with
# PINNED_LEF_RESIDUALS in tests/drt.rs.
case "${DESIGN}" in
    picorv32) SELF_DRC_TOL=3 ;;
    *) SELF_DRC_TOL=0 ;;
esac

"${GRUS}" drt \
    --tlef "${PDK_TLEF}" --lef "${PDK_LEF}" \
    --def "${D}/${DESIGN}.placed.def" \
    --guide "${D}/${DESIGN}.guide" \
    --out-def "${OUT_DEF}" \
    --reference "${D}/${DESIGN}.drt.reference.json" \
    --self-drc-tolerance "${SELF_DRC_TOL}" | tee "${RUN_DIR}/${DESIGN}.drt.ours.log"

SHA="$(shasum -a 256 "${OUT_DEF}" | cut -d' ' -f1)"
echo "OURS-SHA ${SHA}"

if [ "${VERIFY}" = "1" ]; then
    RERUN_DEF="${RUN_DIR}/${DESIGN}.drt.ours.rerun.def"
    "${GRUS}" drt \
        --tlef "${PDK_TLEF}" --lef "${PDK_LEF}" \
        --def "${D}/${DESIGN}.placed.def" \
        --guide "${D}/${DESIGN}.guide" \
        --out-def "${RERUN_DEF}" \
        --reference "${D}/${DESIGN}.drt.reference.json" \
        --self-drc-tolerance "${SELF_DRC_TOL}" | tee "${RUN_DIR}/${DESIGN}.drt.ours.rerun.log"
    if cmp -s "${OUT_DEF}" "${RERUN_DEF}"; then
        echo "OURS-DETERMINISTIC ${SHA}"
    else
        echo "Error: emit nondeterminism — first differing byte:" >&2
        cmp "${OUT_DEF}" "${RERUN_DEF}" >&2 || true
        exit 1
    fi
fi
