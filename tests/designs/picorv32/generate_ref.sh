#!/usr/bin/env bash
# Reproduction script for picorv32 reference artifacts in grus-eda.
# Generates picorv32.synth.v, picorv32.synth.json, picorv32.floorplan.def, picorv32.placed.def,
# picorv32.routed.spef (OpenROAD's placement estimate), picorv32.routed.def, picorv32.drc.rpt,
# picorv32.rcx.spef (routed extraction), picorv32.guide (OpenROAD global_route -guide_file),
# picorv32.guide.caps.txt (estimate_parasitics -global_routing on that guide; the
# ~5 MB picorv32.guide.spef stays gitignored),
# picorv32.orient.def, picorv32.orient.pins.txt,
# picorv32.util65.def, picorv32.gcell_usage.txt, picorv32.reference.json and
# picorv32.sta.reference.json using LibreLane, OpenROAD, and OpenSTA inside the
# canonical container ghcr.io/librelane/librelane:3.0.16.

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

# If running on the host outside Docker, invoke this script inside the container
if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    echo "==> Running reference generation inside container ${CONTAINER_IMAGE}..."
    # Prefer a native Docker daemon (Linux hosts); fall back to the
    # Colima route used on macOS.
    if docker info >/dev/null 2>&1; then
        docker_cmd="docker"
    else
        docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"
    fi

    # Run as the invoking user so generated files are owned by them, not root:
    # CI checkouts (and any later cleanup) run as the runner user and cannot
    # remove root-owned files.
    ${docker_cmd} run --rm \
        -u "$(id -u):$(id -g)" \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/picorv32" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/picorv32/generate_ref.sh
    exit 0
fi

echo "==> Running LibreLane flow for picorv32 on sky130_fd_sc_hd..."
PDK_LIB="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"

# 1. Run LibreLane flow up to OpenROAD.DetailedPlacement
RUN_DIR="/work/tests/designs/picorv32/runs/canonical"
rm -rf "${RUN_DIR}"
librelane config.yaml \
    --run-tag canonical \
    --manual-pdk \
    --pdk-root /pdk \
    --pdk sky130A \
    -s sky130_fd_sc_hd \
    --from Yosys.JsonHeader \
    --to OpenROAD.DetailedPlacement

# 2. Extract synthesized netlists (emitted by the exact same Yosys run)
SYNTH_DIR="${RUN_DIR}/02-yosys-synthesis"
cp "${SYNTH_DIR}/picorv32.nl.v" "${SCRIPT_DIR}/picorv32.synth.v"
cp "${SYNTH_DIR}/picorv32.nl.v.json" "${SCRIPT_DIR}/picorv32.synth.json"

# 3. Create canonical picorv32.floorplan.def from post-IO-placement DEF with logic cells marked UNPLACED
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/runs/canonical/21-openroad-ioplacement/picorv32.def
set block [ord::get_db_block]
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        $inst setPlacementStatus UNPLACED
    }
}
write_def /work/tests/designs/picorv32/picorv32.floorplan.def
exit
EOF

# 4. Create canonical picorv32.placed.def (global placement + detailed placement without filler cells)
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.floorplan.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
global_placement -density 0.55
detailed_placement
write_def /work/tests/designs/picorv32/picorv32.placed.def
exit
EOF

# 5. Placement-estimate SPEF (estimate_parasitics -placement). Despite the file name, this is
#    not a routed extraction; the routed extraction is step 5b.
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_wire_rc -layer met2
estimate_parasitics -placement -spef_file /work/tests/designs/picorv32/picorv32.routed.spef
exit
EOF

# 5b. Routed SPEF: global + detailed routing of picorv32.placed.def, then RC extraction on the routed
#     shapes. Writes picorv32.routed.def, picorv32.drc.rpt and picorv32.rcx.spef. The routing flow is
#     the one LibreLane's own OpenROAD steps run in this image.
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
global_route -guide_file /work/tests/designs/picorv32/runs/canonical/picorv32.guide -congestion_iterations 100
detailed_route -output_drc /work/tests/designs/picorv32/picorv32.drc.rpt -verbose 0
write_def /work/tests/designs/picorv32/picorv32.routed.def
define_process_corner -ext_model_index 0 X
extract_parasitics -ext_model_file /pdk/sky130A/libs.tech/openlane/rules.openrcx.sky130A.nom.calibre -lef_res
write_spef /work/tests/designs/picorv32/picorv32.rcx.spef
exit
EOF

# write_spef puts the wall-clock time in the *DATE line; pin it so regeneration is bit-identical.
# sed is not installed in the pinned container image, so python3 does the edit.
python3 - "${SCRIPT_DIR}/picorv32.rcx.spef" <<'PYEOF'
import sys
path = sys.argv[1]
lines = open(path).read().splitlines(keepends=True)
dated = [i for i, line in enumerate(lines) if line.startswith("*DATE ")]
if len(dated) != 1:
    sys.exit(f"expected exactly one *DATE line in {path}, found {len(dated)}")
lines[dated[0]] = '*DATE "normalized"\n'
open(path, "w").writelines(lines)
PYEOF
if [ "$(grep -c '^\*DATE "normalized"$' "${SCRIPT_DIR}/picorv32.rcx.spef")" -ne 1 ]; then
    echo "Error: expected exactly one *DATE line in picorv32.rcx.spef to normalize." >&2
    exit 1
fi

# A golden taken from a dirty route is worthless. The DRC report is empty when the route is clean.
if [ -s "${SCRIPT_DIR}/picorv32.drc.rpt" ]; then
    drc_count="$(wc -l < "${SCRIPT_DIR}/picorv32.drc.rpt")"
    echo "Error: detailed routing reported ${drc_count} DRC line(s) in picorv32.drc.rpt; refusing to continue." >&2
    exit 1
fi
echo "DRC violations: 0 (picorv32.drc.rpt is empty)"

# 5c. Committed route-guide golden: global_route -guide_file on picorv32.placed.def with the
#     same signal band and layer adjustments GCellGrid::build applies by default
#     (met1 0.30, met2..met5 0.20). Step 5b's runs/canonical guide is untouched; this
#     one is the comparison reference for our emitted guides.
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_routing_layers -signal met1-met5 -clock met3-met5
set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
global_route -guide_file /work/tests/designs/picorv32/picorv32.guide -congestion_iterations 100
exit
EOF

# 5d. Guide-parasitics golden: read_guides on the committed picorv32.guide, then
#     estimate_parasitics -global_routing. The SPEF is regenerated here but
#     gitignored (size); the caps table is the assertion input for the
#     guide-parasitics tests.
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc
set_wire_rc -layer met2
read_guides /work/tests/designs/picorv32/picorv32.guide
estimate_parasitics -global_routing -spef_file /work/tests/designs/picorv32/picorv32.guide.spef
exit
EOF

python3 /work/tests/designs/guide_caps.py extract \
    "${SCRIPT_DIR}/picorv32.guide.spef" "${SCRIPT_DIR}/picorv32.guide.caps.txt"

# The Tcl below is fed to openroad on stdin, where the interactive shell treats `!` as
# history expansion. Write negations as `== 0`, never `![...]`.

# 6. Count the gate-level Verilog netlist on its own (read_verilog + link_design),
#    so the Verilog reference numbers do not come from the placed DEF.
mkdir -p "${RUN_DIR}"
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_verilog /work/tests/designs/picorv32/picorv32.synth.v
link_design picorv32
set block [ord::get_db_block]
set inst_pins 0
foreach inst [$block getInsts] { incr inst_pins [llength [$inst getITerms]] }
set signal_nets 0
foreach net [$block getNets] { if {[$net isSpecial] == 0} { incr signal_nets } }
set fp [open "/work/tests/designs/picorv32/runs/canonical/verilog_counts.tcl" "w"]
puts $fp "set verilog_logic_instances [llength [$block getInsts]]"
puts $fp "set verilog_signal_nets $signal_nets"
puts $fp "set verilog_bterms [llength [$block getBTerms]]"
puts $fp "set verilog_inst_pins $inst_pins"
close $fp
exit
EOF

# 7. Orientation fixture: picorv32.placed.def with the logic cells cycled through all 8
#    orientations, plus OpenROAD's getAvgXY for every signal instance pin. Cells may
#    leave the die; only the pin arithmetic is checked.
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
set orients {R0 R180 R90 R270 MY MX MXR90 MYR90}
set k 0
foreach inst [[ord::get_db_block] getInsts] {
    if {[[$inst getMaster] getType] ne "CORE"} continue
    $inst setOrient [lindex $orients [expr {$k % 8}]]
    incr k
}
write_def /work/tests/designs/picorv32/picorv32.orient.def
exit
EOF
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.orient.def
set fp [open "/work/tests/designs/picorv32/picorv32.orient.pins.txt" "w"]
puts $fp "# inst/pin x_dbu y_dbu (OpenROAD dbITerm::getAvgXY)"
set hpwl 0
foreach net [[ord::get_db_block] getNets] {
    if {[$net isSpecial]} continue
    set bbox [$net getTermBBox]
    incr hpwl [expr {[$bbox dx] + [$bbox dy]}]
    foreach it [$net getITerms] {
        set xy [$it getAvgXY]
        puts $fp "[[$it getInst] getName]/[[$it getMTerm] getName] [lindex $xy 1] [lindex $xy 2]"
    }
}
close $fp
set fp [open "/work/tests/designs/picorv32/runs/canonical/orient_counts.tcl" "w"]
puts $fp "set orient_hpwl_dbu $hpwl"
close $fp
exit
EOF

# 7a. Utilisation-variant placement picorv32.util65.def (the 65% placement). It runs
#     before step 7b because 7b reads this placement.
cat << 'EOF' > "${RUN_DIR}/place_variant.tcl"
set util $env(UTIL)
set dens $env(DENS)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_verilog /work/tests/designs/picorv32/picorv32.synth.v
link_design picorv32
initialize_floorplan -utilization $util -aspect_ratio 1.0 -core_space 5.52 -site unithd
make_tracks
place_pins -hor_layers met3 -ver_layers met2
global_placement -density $dens
detailed_placement
write_def /work/tests/designs/picorv32/picorv32.util${util}.def
exit
EOF
UTIL=65 DENS=0.73 openroad -no_init -no_splash "${RUN_DIR}/place_variant.tcl"

# 7b. Compute GCell grid and routing resource counts via OpenROAD dbGCellGrid (on picorv32.util65.def)
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.util65.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

set_routing_layers -signal met1-met5
global_route -verbose

set ggrid [$block getGCellGrid]
set gx_list [$ggrid getGridX]
set gy_list [$ggrid getGridY]
set gcell_nx [llength $gx_list]
set gcell_ny [llength $gy_list]
set gcell_tile_size_dbu [expr {[lindex $gx_list 1] - [lindex $gx_list 0]}]

set grt_layers {met1 met2 met3 met4 met5}
set raw_tracks {}
set orig_tracks {}

foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
    set total_cap 0
    set total_use 0
    if {$is_h} {
        for {set y 0} {$y < $gcell_ny} {incr y} {
            for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_cap [expr {$total_cap + $c}]
                set total_use [expr {$total_use + $u}]
            }
        }
    } else {
        for {set x 0} {$x < $gcell_nx} {incr x} {
            for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_cap [expr {$total_cap + $c}]
                set total_use [expr {$total_use + $u}]
            }
        }
    }
    lappend raw_tracks [expr {int($total_cap)}]
    lappend orig_tracks [expr {int($total_cap - $total_use)}]
}

set_global_routing_layer_adjustment met1-met5 0.3
global_route -verbose

set derated_tracks {}
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
    set total_avail 0
    if {$is_h} {
        for {set y 0} {$y < $gcell_ny} {incr y} {
            for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_avail [expr {$total_avail + ($c - $u)}]
            }
        }
    } else {
        for {set x 0} {$x < $gcell_nx} {incr x} {
            for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_avail [expr {$total_avail + ($c - $u)}]
            }
        }
    }
    lappend derated_tracks [expr {int($total_avail)}]
}

set fp [open "/work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl" "w"]
puts $fp "set gcell_nx $gcell_nx"
puts $fp "set gcell_ny $gcell_ny"
puts $fp "set gcell_tile_size_dbu $gcell_tile_size_dbu"
puts $fp "set gcell_raw_edge_tracks {\[[join $raw_tracks {, }]\]}"
puts $fp "set gcell_orig_edge_tracks {\[[join $orig_tracks {, }]\]}"
puts $fp "set gcell_derated_edge_tracks {\[[join $derated_tracks {, }]\]}"
close $fp
exit
EOF

# Query routed demand on the util65 placement without destroying nets
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.util65.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

set_routing_layers -signal met1-met5
set_global_routing_layer_adjustment met1-met5 0.3
global_route -verbose -allow_congestion

set ggrid [$block getGCellGrid]
set gx_list [$ggrid getGridX]
set gy_list [$ggrid getGridY]
set gcell_nx [llength $gx_list]
set gcell_ny [llength $gy_list]

set grt_layers {met1 met2 met3 met4 met5}
set total_avail_routed 0
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
    if {$is_h} {
        for {set y 0} {$y < $gcell_ny} {incr y} {
            for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_avail_routed [expr {$total_avail_routed + ($c - $u)}]
            }
        }
    } else {
        for {set x 0} {$x < $gcell_nx} {incr x} {
            for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                set total_avail_routed [expr {$total_avail_routed + ($c - $u)}]
            }
        }
    }
}

source /work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl
set dt_clean [string map {"," "" "\[" "" "\]" ""} $gcell_derated_edge_tracks]
set total_derated 0
foreach dt $dt_clean {
    set total_derated [expr {$total_derated + $dt}]
}
set grt_total_edge_demand [expr {$total_derated - int($total_avail_routed)}]

set fp [open "/work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl" "a"]
puts $fp "set grt_total_edge_demand $grt_total_edge_demand"
close $fp
exit
EOF

# 7d. Variant Set A: Uniform layer adjustment sweep on picorv32.placed.def
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

set grt_layers {met1 met2 met3 met4 met5}
set alphas {0.50 0.70 0.80 0.85 0.90}
set set_a_ovfs {}

foreach a $alphas {
    set_routing_layers -signal met1-met5
    set_global_routing_layer_adjustment met1-met5 $a
    global_route -verbose -allow_congestion
    set ggrid [$block getGCellGrid]
    set gx_list [$ggrid getGridX]
    set gy_list [$ggrid getGridY]
    set gcell_nx [llength $gx_list]
    set gcell_ny [llength $gy_list]
    set total_ovf 0
    foreach lname $grt_layers {
        set lyr [$tech findLayer $lname]
        set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
        if {$is_h} {
            for {set y 0} {$y < $gcell_ny} {incr y} {
                for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                    set c [$ggrid getCapacity $lyr $x $y]
                    set u [$ggrid getUsage $lyr $x $y]
                    if {$u > $c} {
                        set total_ovf [expr {$total_ovf + ($u - $c)}]
                    }
                }
            }
        } else {
            for {set x 0} {$x < $gcell_nx} {incr x} {
                for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                    set c [$ggrid getCapacity $lyr $x $y]
                    set u [$ggrid getUsage $lyr $x $y]
                    if {$u > $c} {
                        set total_ovf [expr {$total_ovf + ($u - $c)}]
                    }
                }
            }
        }
    }
    lappend set_a_ovfs [expr {int($total_ovf)}]
}

set fp [open "/work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl" "a"]
puts $fp "set grt_layer_adj_sweep_alpha {\[[join $alphas {, }]\]}"
puts $fp "set grt_layer_adj_sweep_overflow {\[[join $set_a_ovfs {, }]\]}"
close $fp
exit
EOF

# 7e. Variant Set B: routing overflow of the util65 placement at 0.75 and 0.80 layer adjustment
cat << 'EOF' > "${RUN_DIR}/route_variant.tcl"
set util $env(UTIL)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.util${util}.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]
set grt_layers {met1 met2 met3 met4 met5}

# Route 0.75
set_routing_layers -signal met1-met5
set_global_routing_layer_adjustment met1-met5 0.75
global_route -verbose -allow_congestion
set ggrid [$block getGCellGrid]
set gx_list [$ggrid getGridX]
set gy_list [$ggrid getGridY]
set gcell_nx [llength $gx_list]
set gcell_ny [llength $gy_list]
set total_ovf_75 0
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
    if {$is_h} {
        for {set y 0} {$y < $gcell_ny} {incr y} {
            for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                if {$u > $c} {
                    set total_ovf_75 [expr {$total_ovf_75 + ($u - $c)}]
                }
            }
        }
    } else {
        for {set x 0} {$x < $gcell_nx} {incr x} {
            for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                if {$u > $c} {
                    set total_ovf_75 [expr {$total_ovf_75 + ($u - $c)}]
                }
            }
        }
    }
}

# Route 0.80
set_routing_layers -signal met1-met5
set_global_routing_layer_adjustment met1-met5 0.80
global_route -verbose -allow_congestion
set total_ovf_80 0
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    set is_h [expr {[$lyr getDirection] eq "HORIZONTAL"}]
    if {$is_h} {
        for {set y 0} {$y < $gcell_ny} {incr y} {
            for {set x 0} {$x < [expr {$gcell_nx - 1}]} {incr x} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                if {$u > $c} {
                    set total_ovf_80 [expr {$total_ovf_80 + ($u - $c)}]
                }
            }
        }
    } else {
        for {set x 0} {$x < $gcell_nx} {incr x} {
            for {set y 0} {$y < [expr {$gcell_ny - 1}]} {incr y} {
                set c [$ggrid getCapacity $lyr $x $y]
                set u [$ggrid getUsage $lyr $x $y]
                if {$u > $c} {
                    set total_ovf_80 [expr {$total_ovf_80 + ($u - $c)}]
                }
            }
        }
    }
}

set fp [open "/work/tests/designs/picorv32/runs/canonical/set_b_${util}.tcl" "w"]
puts $fp "set ovf_75 [expr {int($total_ovf_75)}]"
puts $fp "set ovf_80 [expr {int($total_ovf_80)}]"
close $fp
exit
EOF

UTIL=65 openroad -no_init -no_splash "${RUN_DIR}/route_variant.tcl"

# Collect Variant Set B results into gcell_counts.tcl
openroad -no_init -no_splash << 'EOF'
set utils {65}
set ovfs_75 {}
set ovfs_80 {}
foreach u $utils {
    source "/work/tests/designs/picorv32/runs/canonical/set_b_${u}.tcl"
    lappend ovfs_75 $ovf_75
    lappend ovfs_80 $ovf_80
}

set fp [open "/work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl" "a"]
puts $fp "set grt_util_sweep_utils {\[[join $utils {, }]\]}"
puts $fp "set grt_util_sweep_overflow_075 {\[[join $ovfs_75 {, }]\]}"
puts $fp "set grt_util_sweep_overflow_080 {\[[join $ovfs_80 {, }]\]}"
close $fp
exit
EOF

# 7f. Per-edge GCell usage and capacity on picorv32.util65.def, written to picorv32.gcell_usage.txt.
#     Usage and capacity both come from dbGCellGrid (getUsage / getCapacity, the same calls as
#     step 7b), summed over the layers of the matching direction: horizontal layers give H
#     edges, vertical layers give V edges. The 0.3 layer adjustment is set before routing, so
#     getCapacity returns the adjusted capacity. -allow_congestion keeps the router from
#     aborting on overflow at 65% utilisation.
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.util65.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]
set grt_layers {met1 met2 met3 met4 met5}

set_routing_layers -signal met1-met5
set_global_routing_layer_adjustment met1-met5 0.3
global_route -verbose -allow_congestion

set ggrid [$block getGCellGrid]
set gx_list [$ggrid getGridX]
set gy_list [$ggrid getGridY]
set nx [llength $gx_list]
set ny [llength $gy_list]
set ox [lindex $gx_list 0]
set oy [lindex $gy_list 0]
set tile [expr {[lindex $gx_list 1] - $ox}]
if {$tile ne [expr {[lindex $gy_list 1] - $oy}]} {
    error "GCell tiles are not square; the usage dump assumes one tile size"
}

proc value_or_zero {d k} {
    if {[dict exists $d $k]} {
        return [dict get $d $k]
    }
    return 0
}

set hdir [dict create]
foreach lname $grt_layers {
    dict set hdir $lname [expr {[[$tech findLayer $lname] getDirection] eq "HORIZONTAL"}]
}

set hcap [dict create]
set vcap [dict create]
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    if {[dict get $hdir $lname]} {
        for {set y 0} {$y < $ny} {incr y} {
            for {set x 0} {$x < [expr {$nx - 1}]} {incr x} {
                dict incr hcap "$x,$y" [expr {int([$ggrid getCapacity $lyr $x $y])}]
            }
        }
    } else {
        for {set x 0} {$x < $nx} {incr x} {
            for {set y 0} {$y < [expr {$ny - 1}]} {incr y} {
                dict incr vcap "$x,$y" [expr {int([$ggrid getCapacity $lyr $x $y])}]
            }
        }
    }
}

set hu [dict create]
set vu [dict create]
foreach lname $grt_layers {
    set lyr [$tech findLayer $lname]
    if {[dict get $hdir $lname]} {
        for {set y 0} {$y < $ny} {incr y} {
            for {set x 0} {$x < [expr {$nx - 1}]} {incr x} {
                dict incr hu "$x,$y" [expr {int([$ggrid getUsage $lyr $x $y])}]
            }
        }
    } else {
        for {set x 0} {$x < $nx} {incr x} {
            for {set y 0} {$y < [expr {$ny - 1}]} {incr y} {
                dict incr vu "$x,$y" [expr {int([$ggrid getUsage $lyr $x $y])}]
            }
        }
    }
}

set out [open "/work/tests/designs/picorv32/picorv32.gcell_usage.txt" "w"]
puts $out "# origin_x_dbu $ox"
puts $out "# origin_y_dbu $oy"
puts $out "# tile_dbu $tile"
puts $out "# nx $nx"
puts $out "# ny $ny"
for {set y 0} {$y < $ny} {incr y} {
    for {set x 0} {$x < [expr {$nx - 1}]} {incr x} {
        set k "$x,$y"
        set u [value_or_zero $hu $k]
        set c [value_or_zero $hcap $k]
        if {$u > 0 || $c > 0} {
            puts $out "H $x $y $u $c"
        }
    }
}
for {set x 0} {$x < $nx} {incr x} {
    for {set y 0} {$y < [expr {$ny - 1}]} {incr y} {
        set k "$x,$y"
        set u [value_or_zero $vu $k]
        set c [value_or_zero $vcap $k]
        if {$u > 0 || $c > 0} {
            puts $out "V $x $y $u $c"
        }
    }
}
close $out
exit
EOF

# 8. Generate picorv32.reference.json via live OpenROAD and OpenSTA API queries
openroad -no_init -no_splash << 'EOF'
source /work/tests/designs/picorv32/runs/canonical/verilog_counts.tcl
source /work/tests/designs/picorv32/runs/canonical/orient_counts.tcl
source /work/tests/designs/picorv32/runs/canonical/gcell_counts.tcl
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

set lib_cells [llength [get_lib_cells *]]
set lib_pins 0
set lib_sequential_cells 0
foreach c [get_lib_cells *] {
    incr lib_pins [llength [get_lib_pins -of_objects $c]]
    if {[llength [get_lib_pins -of_objects $c -filter "name == IQ"]] > 0} {
        incr lib_sequential_cells
    }
}

set tech_dbu_per_micron [$tech getDbUnitsPerMicron]
set tech_routing_layers 0
foreach layer [$tech getLayers] {
    if {[$layer getType] eq "ROUTING"} {
        incr tech_routing_layers
    }
}

set tech_macros 0
set tech_macro_pins 0
set site_width_dbu 0
set site_height_dbu 0
foreach lib [$db getLibs] {
    set m_list [$lib getMasters]
    incr tech_macros [llength $m_list]
    foreach m $m_list {
        incr tech_macro_pins [llength [$m getMTerms]]
    }
    foreach site [$lib getSites] {
        if {[$site getName] eq "unithd"} {
            set site_width_dbu [$site getWidth]
            set site_height_dbu [$site getHeight]
        }
    }
}

set die_area [$block getDieArea]
set die_xl [$die_area xMin]
set die_yl [$die_area yMin]
set die_xh [$die_area xMax]
set die_yh [$die_area yMax]

set instances [llength [$block getInsts]]
set logic_instances 0
set physical_instances 0
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        incr logic_instances
    } else {
        incr physical_instances
    }
}

set def_inst_pins [llength [$block getITerms]]
set signal_nets 0
set special_nets 0
set connected_net_pins 0
set placed_hpwl_dbu 0

foreach net [$block getNets] {
    if {[$net isSpecial]} {
        incr special_nets
    } else {
        incr signal_nets
        incr connected_net_pins [expr {[llength [$net getITerms]] + [llength [$net getBTerms]]}]
        set bbox [$net getTermBBox]
        set dx [expr {[$bbox xMax] - [$bbox xMin]}]
        set dy [expr {[$bbox yMax] - [$bbox yMin]}]
        incr placed_hpwl_dbu [expr {$dx + $dy}]
    }
}

set bterms_total [llength [$block getBTerms]]
set bterms_signal 0
foreach bt [$block getBTerms] {
    if {[$bt isSpecial] == 0} {
        incr bterms_signal
    }
}

set rows [llength [$block getRows]]
set track_grids_layers [llength [$block getTrackGrids]]
set def_tracks_statements 0
foreach tg [$block getTrackGrids] {
    incr def_tracks_statements [expr {[$tg getNumGridPatternsX] + [$tg getNumGridPatternsY]}]
}

set zero_wire_wns_ns [format "%.4f" [sta::worst_slack -max]]
set zero_wire_tns_ns [format "%.4f" [sta::total_negative_slack -max]]

read_spef /work/tests/designs/picorv32/picorv32.routed.spef
set placed_spef_wns_ns [format "%.4f" [sta::worst_slack -max]]
set placed_spef_tns_ns [format "%.4f" [sta::total_negative_slack -max]]

set fp [open "/work/tests/designs/picorv32/picorv32.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"picorv32\","
puts $fp "  \"pdk\": \"sky130_fd_sc_hd\","
puts $fp "  \"corner\": \"tt_025C_1v80\","
puts $fp "  \"lib_cells\": $lib_cells,"
puts $fp "  \"lib_pins\": $lib_pins,"
puts $fp "  \"lib_sequential_cells\": $lib_sequential_cells,"
puts $fp "  \"tech_dbu_per_micron\": $tech_dbu_per_micron,"
puts $fp "  \"tech_routing_layers\": $tech_routing_layers,"
puts $fp "  \"tech_macros\": $tech_macros,"
puts $fp "  \"tech_macro_pins\": $tech_macro_pins,"
puts $fp "  \"site_width_dbu\": $site_width_dbu,"
puts $fp "  \"site_height_dbu\": $site_height_dbu,"
puts $fp "  \"die_area_dbu\": \[$die_xl, $die_yl, $die_xh, $die_yh\],"
puts $fp "  \"instances\": $instances,"
puts $fp "  \"logic_instances\": $logic_instances,"
puts $fp "  \"physical_instances\": $physical_instances,"
puts $fp "  \"def_inst_pins\": $def_inst_pins,"
puts $fp "  \"verilog_logic_instances\": $verilog_logic_instances,"
puts $fp "  \"verilog_signal_nets\": $verilog_signal_nets,"
puts $fp "  \"verilog_bterms\": $verilog_bterms,"
puts $fp "  \"verilog_inst_pins\": $verilog_inst_pins,"
puts $fp "  \"signal_nets\": $signal_nets,"
puts $fp "  \"special_nets\": $special_nets,"
puts $fp "  \"connected_net_pins\": $connected_net_pins,"
puts $fp "  \"bterms_total\": $bterms_total,"
puts $fp "  \"bterms_signal\": $bterms_signal,"
puts $fp "  \"rows\": $rows,"
puts $fp "  \"track_grids_layers\": $track_grids_layers,"
puts $fp "  \"def_tracks_statements\": $def_tracks_statements,"
puts $fp "  \"placed_hpwl_dbu\": $placed_hpwl_dbu,"
puts $fp "  \"placed_hpwl_um\": [expr {$placed_hpwl_dbu / 1000.0}],"
puts $fp "  \"orient_hpwl_dbu\": $orient_hpwl_dbu,"
puts $fp "  \"zero_wire_wns_ns\": $zero_wire_wns_ns,"
puts $fp "  \"zero_wire_tns_ns\": $zero_wire_tns_ns,"
puts $fp "  \"placed_spef_wns_ns\": $placed_spef_wns_ns,"
puts $fp "  \"placed_spef_tns_ns\": $placed_spef_tns_ns,"
puts $fp "  \"gcell_nx\": $gcell_nx,"
puts $fp "  \"gcell_ny\": $gcell_ny,"
puts $fp "  \"gcell_tile_size_dbu\": $gcell_tile_size_dbu,"
puts $fp "  \"gcell_raw_edge_tracks\": $gcell_raw_edge_tracks,"
puts $fp "  \"gcell_orig_edge_tracks\": $gcell_orig_edge_tracks,"
puts $fp "  \"gcell_derated_edge_tracks\": $gcell_derated_edge_tracks,"
puts $fp "  \"grt_total_edge_demand\": $grt_total_edge_demand,"
puts $fp "  \"grt_layer_adj_sweep_alpha\": $grt_layer_adj_sweep_alpha,"
puts $fp "  \"grt_layer_adj_sweep_overflow\": $grt_layer_adj_sweep_overflow,"
puts $fp "  \"grt_util_sweep_utils\": $grt_util_sweep_utils,"
puts $fp "  \"grt_util_sweep_overflow_075\": $grt_util_sweep_overflow_075,"
puts $fp "  \"grt_util_sweep_overflow_080\": $grt_util_sweep_overflow_080"
puts $fp "}"
close $fp
puts "Generated picorv32.reference.json via live OpenROAD/OpenSTA queries"
exit
EOF

# 9. Generate picorv32.sta.reference.json with per-endpoint OpenSTA values for E3 (zero_wire, placement_rc, placed_spef)
#    Units: PathEnd/Vertex methods return SI seconds, so they go through sta::time_sta_ui (-> ns).
#    sta::worst_slack / sta::total_negative_slack already return user units (ns); do not convert them.
#    Endpoints are collected max-first: calling find_timing_paths -min before iterating the -max
#    PathEnds invalidates them and segfaults OpenROAD 2026-02-17.
cat << 'EOF' > "${RUN_DIR}/collect_endpoints.tcl"
proc collect_endpoints_json {} {
    array set max_arr {}
    array set max_req {}
    array set max_slack {}
    array set max_slew {}
    set pins {}

    set paths_max [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 100000]
    foreach pe $paths_max {
        set name [get_full_name [$pe pin]]
        set v [$pe vertex]
        lappend pins $name
        set max_arr($name) [format "%.6f" [sta::time_sta_ui [$pe data_arrival_time]]]
        set max_req($name) [format "%.6f" [sta::time_sta_ui [$pe data_required_time]]]
        set max_slack($name) [format "%.6f" [sta::time_sta_ui [$pe slack]]]
        set max_slew($name) [format "%.6f" [sta::time_sta_ui [$v slew [$pe end_transition] max]]]
    }

    array set min_arr {}
    array set min_req {}
    array set min_slack {}
    array set min_slew {}

    set paths_min [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 100000]
    foreach pe $paths_min {
        set name [get_full_name [$pe pin]]
        set v [$pe vertex]
        set min_arr($name) [format "%.6f" [sta::time_sta_ui [$pe data_arrival_time]]]
        set min_req($name) [format "%.6f" [sta::time_sta_ui [$pe data_required_time]]]
        set min_slack($name) [format "%.6f" [sta::time_sta_ui [$pe slack]]]
        set min_slew($name) [format "%.6f" [sta::time_sta_ui [$v slew [$pe end_transition] min]]]
    }

    set sorted_pins [lsort -dictionary $pins]
    set entries {}
    foreach name $sorted_pins {
        lappend entries "      {\"pin\": \"$name\", \"max_arr_ns\": $max_arr($name), \"max_req_ns\": $max_req($name), \"max_slack_ns\": $max_slack($name), \"max_slew_ns\": $max_slew($name), \"min_arr_ns\": $min_arr($name), \"min_req_ns\": $min_req($name), \"min_slack_ns\": $min_slack($name), \"min_slew_ns\": $min_slew($name)}"
    }
    return [join $entries ",\n"]
}
EOF

# The routed mode runs in its own OpenROAD process (below) so that its read_spef starts from a
# clean parasitics state. This process writes the first three modes and leaves the JSON open;
# the routed process appends the fourth mode and closes it.
cat << 'EOF' > "${RUN_DIR}/dump_sta_ref.tcl"
source /work/tests/designs/picorv32/runs/canonical/collect_endpoints.tcl

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc

set zw_wns_max [format "%.6f" [sta::worst_slack -max]]
set zw_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set zw_wns_min [format "%.6f" [sta::worst_slack -min]]
set zw_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set zw_endpoints [collect_endpoints_json]

set_wire_rc -layer met2
estimate_parasitics -placement
set prc_wns_max [format "%.6f" [sta::worst_slack -max]]
set prc_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set prc_wns_min [format "%.6f" [sta::worst_slack -min]]
set prc_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set prc_endpoints [collect_endpoints_json]

read_spef /work/tests/designs/picorv32/picorv32.routed.spef
set spef_wns_max [format "%.6f" [sta::worst_slack -max]]
set spef_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set spef_wns_min [format "%.6f" [sta::worst_slack -min]]
set spef_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set spef_endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/picorv32/picorv32.sta.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"picorv32\","
puts $fp "  \"zero_wire\": {"
puts $fp "    \"wns_max_ns\": $zw_wns_max,"
puts $fp "    \"tns_max_ns\": $zw_tns_max,"
puts $fp "    \"wns_min_ns\": $zw_wns_min,"
puts $fp "    \"tns_min_ns\": $zw_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $zw_endpoints
puts $fp "    \]"
puts $fp "  },"
puts $fp "  \"placement_rc\": {"
puts $fp "    \"wns_max_ns\": $prc_wns_max,"
puts $fp "    \"tns_max_ns\": $prc_tns_max,"
puts $fp "    \"wns_min_ns\": $prc_wns_min,"
puts $fp "    \"tns_min_ns\": $prc_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $prc_endpoints
puts $fp "    \]"
puts $fp "  },"
puts $fp "  \"placed_spef\": {"
puts $fp "    \"wns_max_ns\": $spef_wns_max,"
puts $fp "    \"tns_max_ns\": $spef_tns_max,"
puts $fp "    \"wns_min_ns\": $spef_wns_min,"
puts $fp "    \"tns_min_ns\": $spef_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $spef_endpoints
puts $fp "    \]"
puts $fp "  },"
close $fp
puts "Wrote the first three modes of picorv32.sta.reference.json"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_ref.tcl"

# 9b. Routed SPEF mode (picorv32.rcx.spef from step 5b). Same DEF and SDC; only the parasitics differ.
cat << 'EOF' > "${RUN_DIR}/dump_sta_routed_ref.tcl"
source /work/tests/designs/picorv32/runs/canonical/collect_endpoints.tcl

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/picorv32/picorv32.placed.def
read_sdc /work/tests/designs/picorv32/picorv32.sdc

read_spef /work/tests/designs/picorv32/picorv32.rcx.spef
set routed_wns_max [format "%.6f" [sta::worst_slack -max]]
set routed_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set routed_wns_min [format "%.6f" [sta::worst_slack -min]]
set routed_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set routed_endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/picorv32/picorv32.sta.reference.json" "a"]
puts $fp "  \"routed_spef\": {"
puts $fp "    \"wns_max_ns\": $routed_wns_max,"
puts $fp "    \"tns_max_ns\": $routed_tns_max,"
puts $fp "    \"wns_min_ns\": $routed_wns_min,"
puts $fp "    \"tns_min_ns\": $routed_tns_min,"
puts $fp "    \"endpoints\": \["
puts $fp $routed_endpoints
puts $fp "    \]"
puts $fp "  }"
puts $fp "}"
close $fp
puts "Appended routed_spef mode to picorv32.sta.reference.json"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_routed_ref.tcl"

echo "==> Successfully generated all picorv32 reference artifacts!"
