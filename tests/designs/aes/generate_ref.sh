#!/usr/bin/env bash
# Reproduction script for aes reference artifacts in grus-eda.
# Generates aes.synth.v, aes.synth.json, aes.floorplan.def, aes.placed.def, aes.routed.spef,
# aes.orient.def, aes.orient.pins.txt, aes.util65.def, aes.gcell_usage.txt, aes.reference.json,
# and aes.sta.reference.json using LibreLane, Yosys, OpenROAD, and OpenSTA
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
        -w "/work/tests/designs/aes" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/aes/generate_ref.sh
    exit 0
fi

echo "==> Running LibreLane flow for aes_cipher_top on sky130_fd_sc_hd..."
PDK_LIB="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"
PDK_TLEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef"
PDK_LEF="/pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef"

# 1. Run LibreLane flow up to OpenROAD.DetailedPlacement
RUN_DIR="/work/tests/designs/aes/runs/canonical"
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
cp "${SYNTH_DIR}/aes_cipher_top.nl.v" "${SCRIPT_DIR}/aes.synth.v"
cp "${SYNTH_DIR}/aes_cipher_top.nl.v.json" "${SCRIPT_DIR}/aes.synth.json"

# 3. Create canonical aes.floorplan.def from post-IO-placement DEF with logic cells marked UNPLACED
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/runs/canonical/21-openroad-ioplacement/aes_cipher_top.def
set block [ord::get_db_block]
foreach inst [$block getInsts] {
    set type [[$inst getMaster] getType]
    if {$type eq "CORE"} {
        $inst setPlacementStatus UNPLACED
    }
}
write_def /work/tests/designs/aes/aes.floorplan.def
exit
EOF

# 4. Create canonical aes.placed.def (global placement + detailed placement without filler cells)
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.floorplan.def
read_sdc /work/tests/designs/aes/aes.sdc
global_placement -density 0.55
detailed_placement
write_def /work/tests/designs/aes/aes.placed.def
exit
EOF

# 5. Extract SPEF parasitics
openroad -no_init -no_splash << 'EOF'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.placed.def
read_sdc /work/tests/designs/aes/aes.sdc
set_wire_rc -layer met2
estimate_parasitics -placement -spef_file /work/tests/designs/aes/aes.routed.spef
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
read_verilog /work/tests/designs/aes/aes.synth.v
link_design aes_cipher_top
set block [ord::get_db_block]
set inst_pins 0
foreach inst [$block getInsts] { incr inst_pins [llength [$inst getITerms]] }
set signal_nets 0
foreach net [$block getNets] { if {[$net isSpecial] == 0} { incr signal_nets } }
set fp [open "/work/tests/designs/aes/runs/canonical/verilog_counts.tcl" "w"]
puts $fp "set verilog_logic_instances [llength [$block getInsts]]"
puts $fp "set verilog_signal_nets $signal_nets"
puts $fp "set verilog_bterms [llength [$block getBTerms]]"
puts $fp "set verilog_inst_pins $inst_pins"
close $fp
exit
EOF

# 7. Orientation fixture: aes.placed.def with the logic cells cycled through all 8
#    orientations (the flow only uses N/S/FN/FS), plus OpenROAD's getAvgXY for every
#    signal instance pin. Cells may leave the die; only the pin arithmetic is checked.
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.placed.def
set orients {R0 R180 R90 R270 MY MX MXR90 MYR90}
set k 0
foreach inst [[ord::get_db_block] getInsts] {
    if {[[$inst getMaster] getType] ne "CORE"} continue
    $inst setOrient [lindex $orients [expr {$k % 8}]]
    incr k
}
write_def /work/tests/designs/aes/aes.orient.def
exit
EOF
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.orient.def
set fp [open "/work/tests/designs/aes/aes.orient.pins.txt" "w"]
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
set fp [open "/work/tests/designs/aes/runs/canonical/orient_counts.tcl" "w"]
puts $fp "set orient_hpwl_dbu $hpwl"
close $fp
exit
EOF

# 7a. Utilisation-variant placement aes.util65.def (the 65% placement of step 7e). It runs
#     before step 7b because 7b reads this placement.
cat << 'EOF' > "${RUN_DIR}/place_variant.tcl"
set util $env(UTIL)
set dens $env(DENS)
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_verilog /work/tests/designs/aes/aes.synth.v
link_design aes_cipher_top
initialize_floorplan -utilization $util -aspect_ratio 1.0 -core_space 5.52 -site unithd
make_tracks
place_pins -hor_layers met3 -ver_layers met2
global_placement -density $dens
detailed_placement
write_def /work/tests/designs/aes/aes.util${util}.def
exit
EOF
UTIL=65 DENS=0.73 openroad -no_init -no_splash "${RUN_DIR}/place_variant.tcl"

# 7b. Compute GCell grid and routing resource counts via OpenROAD dbGCellGrid (on aes.util65.def)
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.util65.def

set db [ord::get_db]
set tech [$db getTech]
set block [ord::get_db_block]

# aes routes the full design here (gcd's single-net experiment is not carried over).

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

set fp [open "/work/tests/designs/aes/runs/canonical/gcell_counts.tcl" "w"]
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
read_def /work/tests/designs/aes/aes.util65.def

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

source /work/tests/designs/aes/runs/canonical/gcell_counts.tcl
set dt_clean [string map {"," "" "\[" "" "\]" ""} $gcell_derated_edge_tracks]
set total_derated 0
foreach dt $dt_clean {
    set total_derated [expr {$total_derated + $dt}]
}
set grt_total_edge_demand [expr {$total_derated - int($total_avail_routed)}]

set fp [open "/work/tests/designs/aes/runs/canonical/gcell_counts.tcl" "a"]
puts $fp "set grt_total_edge_demand $grt_total_edge_demand"
close $fp
exit
EOF

# 7d. Variant Set A: Uniform layer adjustment sweep on aes.placed.def
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.placed.def

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

set fp [open "/work/tests/designs/aes/runs/canonical/gcell_counts.tcl" "a"]
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
read_def /work/tests/designs/aes/aes.util${util}.def

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

set fp [open "/work/tests/designs/aes/runs/canonical/set_b_${util}.tcl" "w"]
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
    source "/work/tests/designs/aes/runs/canonical/set_b_${u}.tcl"
    lappend ovfs_75 $ovf_75
    lappend ovfs_80 $ovf_80
}

set fp [open "/work/tests/designs/aes/runs/canonical/gcell_counts.tcl" "a"]
puts $fp "set grt_util_sweep_utils {\[[join $utils {, }]\]}"
puts $fp "set grt_util_sweep_overflow_075 {\[[join $ovfs_75 {, }]\]}"
puts $fp "set grt_util_sweep_overflow_080 {\[[join $ovfs_80 {, }]\]}"
close $fp
exit
EOF

# 7f. Per-edge GCell usage and capacity on aes.util65.def, written to aes.gcell_usage.txt.
#     Usage and capacity both come from dbGCellGrid (getUsage / getCapacity, the same calls as
#     step 7b), summed over the layers of the matching direction: horizontal layers give H
#     edges, vertical layers give V edges. The 0.3 layer adjustment is set before routing, so
#     getCapacity returns the adjusted capacity. -allow_congestion keeps the router from
#     aborting on overflow at 65% utilisation.
openroad -no_init -no_splash << 'EOF'
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.util65.def

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

set out [open "/work/tests/designs/aes/aes.gcell_usage.txt" "w"]
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

# 8. Generate aes.reference.json via live OpenROAD and OpenSTA API queries
openroad -no_init -no_splash << 'EOF'
source /work/tests/designs/aes/runs/canonical/verilog_counts.tcl
source /work/tests/designs/aes/runs/canonical/orient_counts.tcl
source /work/tests/designs/aes/runs/canonical/gcell_counts.tcl
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.placed.def
read_sdc /work/tests/designs/aes/aes.sdc

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

read_spef /work/tests/designs/aes/aes.routed.spef
set placed_spef_wns_ns [format "%.4f" [sta::worst_slack -max]]
set placed_spef_tns_ns [format "%.4f" [sta::total_negative_slack -max]]

set fp [open "/work/tests/designs/aes/aes.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"aes_cipher_top\","
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
puts "Generated aes.reference.json via live OpenROAD/OpenSTA queries"
exit
EOF

# 9. Generate aes.sta.reference.json with per-endpoint OpenSTA values for E3 (zero_wire, placement_rc, placed_spef)
#    Units: PathEnd/Vertex methods return SI seconds, so they go through sta::time_sta_ui (-> ns).
#    sta::worst_slack / sta::total_negative_slack already return user units (ns); do not convert them.
#    Endpoints are collected max-first: calling find_timing_paths -min before iterating the -max
#    PathEnds invalidates them and segfaults OpenROAD 2026-02-17.
cat << 'EOF' > "${RUN_DIR}/dump_sta_ref.tcl"
proc collect_endpoints_json {} {
    array set max_arr {}
    array set max_req {}
    array set max_slack {}
    array set max_slew {}
    set pins {}

    set paths_max [find_timing_paths -path_delay max -endpoint_path_count 1 -group_path_count 1000]
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

    set paths_min [find_timing_paths -path_delay min -endpoint_path_count 1 -group_path_count 1000]
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

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/aes/aes.placed.def
read_sdc /work/tests/designs/aes/aes.sdc

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

read_spef /work/tests/designs/aes/aes.routed.spef
set spef_wns_max [format "%.6f" [sta::worst_slack -max]]
set spef_tns_max [format "%.6f" [sta::total_negative_slack -max]]
set spef_wns_min [format "%.6f" [sta::worst_slack -min]]
set spef_tns_min [format "%.6f" [sta::total_negative_slack -min]]
set spef_endpoints [collect_endpoints_json]

set fp [open "/work/tests/designs/aes/aes.sta.reference.json" "w"]
puts $fp "{"
puts $fp "  \"design\": \"aes_cipher_top\","
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
puts $fp "  }"
puts $fp "}"
close $fp
puts "Generated aes.sta.reference.json via live OpenROAD/OpenSTA queries"
exit
EOF
openroad -no_init -no_splash "${RUN_DIR}/dump_sta_ref.tcl"

echo "==> Successfully generated all aes reference artifacts!"
