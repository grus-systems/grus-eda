#!/usr/bin/env bash
# Dumps OpenSTA's per-pin max/min slew and arrival (rise and fall) for every pin of one design on one SPEF,
# in the pinned LibreLane image.
#
# usage: tests/designs/collect_pins.sh <design> <placed.def> <spef> <out.txt>
# Output (relative to tests/designs/<design>, git-ignored: large, regenerate with this script) has one line per pin:
#   pin slew_max_rise slew_max_fall arrival_max_rise arrival_max_fall arrival_min_rise arrival_min_fall
# (ns; `-` where OpenSTA has no value). Input ports are not listed (OpenSTA has no arrival property for them).
set -euo pipefail

DESIGN="$1"; DEF="$2"; SPEF="$3"; OUT="$4"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"
RUN_DIR="${SCRIPT_DIR}/${DESIGN}/runs/pins_collect"
mkdir -p "${RUN_DIR}"

if [ ! -f /.dockerenv ]; then
    if docker info >/dev/null 2>&1; then docker_cmd="docker"; else docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"; fi
    exec ${docker_cmd} run --rm -u "$(id -u):$(id -g)" -v "${REPO_ROOT}:/work" -v "${PDK_HOST_ROOT}:/pdk" -e PDK_ROOT=/pdk \
        "${IMAGE}" bash "/work/tests/designs/collect_pins.sh" "$@"
fi

D="/work/tests/designs/${DESIGN}"
cat > "${RUN_DIR}/pins.tcl" <<TCL
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def ${D}/${DEF}
read_sdc ${D}/${DESIGN}.sdc
read_spef ${D}/${SPEF}
set fp [open ${D}/${OUT} w]
proc val {p prop} {
  if {[catch {set v [get_property \$p \$prop]}] || \$v eq "" } { return "-" }
  return \$v
}
foreach p [get_pins -hierarchical *] {
  puts \$fp "[get_full_name \$p] [val \$p slew_max_rise] [val \$p slew_max_fall] [val \$p arrival_max_rise] [val \$p arrival_max_fall] [val \$p arrival_min_rise] [val \$p arrival_min_fall]"
}
close \$fp
exit
TCL
openroad -no_init -no_splash "${RUN_DIR}/pins.tcl" > "${RUN_DIR}/pins.log" 2>&1
echo "wrote ${DESIGN}/${OUT}: $(wc -l < ${D}/${OUT}) pins"
