#!/usr/bin/env bash
# Reproduction script for gcd reference artifacts in grus-eda.
# Generates gcd.synth.v, gcd.synth.json, gcd.floorplan.def, gcd.placed.def,
# gcd.routed.spef, gcd.orient.def, gcd.orient.pins.txt, and gcd.reference.json using LibreLane, Yosys, OpenROAD, and OpenSTA
# inside the canonical container ghcr.io/librelane/librelane:3.0.16.

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
    docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"

    ${docker_cmd} run --rm \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/gcd" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/gcd/generate_ref.sh
    exit 0
fi

echo "==> Running LibreLane flow for gcd on sky130_fd_sc_hd..."
PDK_LIB="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"

# 1. Run LibreLane flow up to OpenROAD.DetailedPlacement
RUN_DIR="/work/tests/designs/gcd/runs/canonical"
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
cp "${SYNTH_DIR}/gcd.nl.v" "${SCRIPT_DIR}/gcd.synth.v"
cp "${SYNTH_DIR}/gcd.nl.v.json" "${SCRIPT_DIR}/gcd.synth.json"

# 3. Create canonical gcd.floorplan.def from post-IO-placement DEF with logic cells marked UNPLACED
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/runs/canonical/21-openroad-ioplacement/gcd.def
set block [ord::get_db_block]
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        $inst setPlacementStatus UNPLACED
    }
}
write_def /work/tests/designs/gcd/gcd.floorplan.def
exit
EOF

# 4. Create canonical gcd.placed.def (global placement + detailed placement without filler cells)
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.floorplan.def
read_sdc /work/tests/designs/gcd/gcd.sdc
global_placement -density 0.55
detailed_placement
write_def /work/tests/designs/gcd/gcd.placed.def
exit
EOF

# 5. Extract SPEF parasitics
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc
set_wire_rc -layer met2
estimate_parasitics -placement -spef_file /work/tests/designs/gcd/gcd.routed.spef
exit
EOF

# The Tcl below is fed to openroad on stdin, where the interactive shell treats `!` as
# history expansion. Write negations as `== 0`, never `![...]`.

# 6. Count the gate-level Verilog netlist on its own (read_verilog + link_design),
#    so the Verilog reference numbers do not come from the placed DEF.
mkdir -p "${RUN_DIR}"
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_verilog /work/tests/designs/gcd/gcd.synth.v
link_design gcd
set block [ord::get_db_block]
set inst_pins 0
foreach inst [$block getInsts] { incr inst_pins [llength [$inst getITerms]] }
set signal_nets 0
foreach net [$block getNets] { if {[$net isSpecial] == 0} { incr signal_nets } }
set fp [open "/work/tests/designs/gcd/runs/canonical/verilog_counts.tcl" "w"]
puts $fp "set verilog_logic_instances [llength [$block getInsts]]"
puts $fp "set verilog_signal_nets $signal_nets"
puts $fp "set verilog_bterms [llength [$block getBTerms]]"
puts $fp "set verilog_inst_pins $inst_pins"
close $fp
exit
EOF

# 7. Orientation fixture: gcd.placed.def with the logic cells cycled through all 8
#    orientations (gcd itself only uses N/S/FN/FS), plus OpenROAD's getAvgXY for every
#    signal instance pin. Cells may leave the die; only the pin arithmetic is checked.
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
set orients {R0 R180 R90 R270 MY MX MXR90 MYR90}
set k 0
foreach inst [[ord::get_db_block] getInsts] {
    if {[[$inst getMaster] getType] ne "CORE"} continue
    $inst setOrient [lindex $orients [expr {$k % 8}]]
    incr k
}
write_def /work/tests/designs/gcd/gcd.orient.def
exit
EOF
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.orient.def
set fp [open "/work/tests/designs/gcd/gcd.orient.pins.txt" "w"]
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
set fp [open "/work/tests/designs/gcd/runs/canonical/orient_counts.tcl" "w"]
puts $fp "set orient_hpwl_dbu $hpwl"
close $fp
exit
EOF

# 7b. Compute GCell grid and routing resource counts via OpenROAD dbGCellGrid
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

foreach net [$block getNets] {
    if {[$net isSpecial] == 0 && [$net getName] ne "_004_"} {
        odb::dbNet_destroy $net
    }
}

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

set_global_routing_layer_adjustment met1 0.30
set_global_routing_layer_adjustment met2-met5 0.20
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

set fp [open "/work/tests/designs/gcd/runs/canonical/gcell_counts.tcl" "w"]
puts $fp "set gcell_nx $gcell_nx"
puts $fp "set gcell_ny $gcell_ny"
puts $fp "set gcell_tile_size_dbu $gcell_tile_size_dbu"
puts $fp "set gcell_raw_edge_tracks {\[[join $raw_tracks {, }]\]}"
puts $fp "set gcell_orig_edge_tracks {\[[join $orig_tracks {, }]\]}"
puts $fp "set gcell_derated_edge_tracks {\[[join $derated_tracks {, }]\]}"
close $fp
exit
EOF

# 8. Generate gcd.reference.json via live OpenROAD and OpenSTA API queries
openroad -no_init -no_splash << 'EOF'
source /work/tests/designs/gcd/runs/canonical/verilog_counts.tcl
source /work/tests/designs/gcd/runs/canonical/orient_counts.tcl
source /work/tests/designs/gcd/runs/canonical/gcell_counts.tcl
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/gcd/gcd.placed.def
read_sdc /work/tests/designs/gcd/gcd.sdc

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

read_spef /work/tests/designs/gcd/gcd.routed.spef
set placed_spef_wns_ns [format "%.4f" [sta::worst_slack -max]]
set placed_spef_tns_ns [format "%.4f" [sta::total_negative_slack -max]]

set fp [open "/work/tests/designs/gcd/gcd.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"gcd\","
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
puts $fp "  \"gcell_derated_edge_tracks\": $gcell_derated_edge_tracks"
puts $fp "}"
close $fp
puts "Generated gcd.reference.json via live OpenROAD/OpenSTA queries"
exit
EOF

echo "==> Successfully generated all gcd reference artifacts!"
