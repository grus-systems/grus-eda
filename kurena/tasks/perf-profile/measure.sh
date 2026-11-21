#!/bin/sh
# perf-profile measurement driver: designs x commands x reps, one raw log.
# Usage: REPS=5 sh kurena/tasks/perf-profile/measure.sh [log-file]
# Run from the repo root against target/release/grus-eda.
set -u

REPS="${REPS:-5}"
OUT="${1:-perf-measure-$(date +%Y%m%d-%H%M%S).log}"
BIN="${BIN:-target/release/grus-eda}"
PDK="${PDK_ROOT:-$HOME/.ciel}/sky130A/libs.ref/sky130_fd_sc_hd"
LIB="$PDK/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
TLEF="$PDK/techlef/sky130_fd_sc_hd__nom.tlef"
LEF="$PDK/lef/sky130_fd_sc_hd.lef"
D=tests/designs

DESIGNS="gcd picorv32 aes ibex jpeg"

run() { # cell_name command...
    cell="$1"; shift
    for rep in $(seq 1 "$REPS"); do
        {
            echo "### cell=$cell rep=$rep"
            GRUS_PHASE_TIMING=1 /usr/bin/time -l "$BIN" "$@"
            echo "### end cell=$cell rep=$rep status=$?"
        } >>"$OUT" 2>&1
    done
}

: >"$OUT"
for d in $DESIGNS; do
    DEF="$D/$d/$d.placed.def"
    SDC="$D/$d/$d.sdc"
    run "sta.zerowire.$d"   sta --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC" --json "/tmp/$d.sta.json"
    run "sta.placement.$d"  sta --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC" --parasitics placement --json "/tmp/$d.sta.json"
    run "sta.spef.$d"       sta --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC" --spef "$D/$d/$d.routed.spef" --json "/tmp/$d.sta.json"
    if [ -f "$D/$d/$d.rcx.spef" ]; then
        run "sta.spef_rcx.$d" sta --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC" --spef "$D/$d/$d.rcx.spef" --json "/tmp/$d.sta.json"
    fi
    run "power.zerowire.$d"  power --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC"
    run "power.placement.$d" power --lib "$LIB" --tlef "$TLEF" --lef "$LEF" --def "$DEF" --sdc "$SDC" --parasitics placement
    run "congestion.$d"     congestion --tlef "$TLEF" --lef "$LEF" --def "$DEF"
    run "place.$d"          place --tlef "$TLEF" --lef "$LEF" --def "$D/$d/$d.floorplan.def" --out-def "/tmp/$d.out.def"
done

# VCD-annotated power, gcd only (the committed dump lives under power/).
if [ -f "$D/gcd/power/gcd.vcd" ]; then
    run "power.vcd.gcd" power --lib "$LIB" --tlef "$TLEF" --lef "$LEF" \
        --def "$D/gcd/gcd.placed.def" --sdc "$D/gcd/gcd.sdc" \
        --parasitics placement --vcd "$D/gcd/power/gcd.vcd"
fi

echo "wrote $OUT"
