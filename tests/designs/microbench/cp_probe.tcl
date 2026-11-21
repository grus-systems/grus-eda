# Constant-polarity probe (cp.def): measures which marginal a `constant`
# net stores and which polarity the `constant` label itself uses —
# whether absorptive forms (or(1,X), and(0,X)) mark `constant` while a
# toggling input is present, and whether the mark survives
# `-pins`/`-global` overrides. Verbatim output is recorded in
# docs/results.md (2026-11-21 — N7 — AP2 r3).
#
# Run inside ghcr.io/librelane/librelane:3.0.16 (same container stanza
# as generate_activity_ref.sh):
#   docker run --rm -u "$(id -u):$(id -g)" \
#       -v "$REPO_ROOT:/work" -v "$PDK_ROOT:/pdk" -e PDK_ROOT=/pdk \
#       -w /work/tests/designs/microbench ghcr.io/librelane/librelane:3.0.16 \
#       openroad -no_init -no_splash -exit \
#       /work/tests/designs/microbench/cp_probe.tcl

read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/microbench/cp.def

# -density is an absolute rate (no clock in the design): 0.003/ns = 3e6/s.
set_power_activity -input_ports [get_ports a] -density 0.003 -duty 0.55

foreach p {u_andh/X u_xorh/X u_orh/X u_andl/X u_orl/X u_xorl/X u_andhh/X u_bufh/X} {
    puts "ACTI $p [get_property [get_pin $p] activity]"
}

puts "=== precedence: -pins on absorptive vs propagated ==="
set_power_activity -pins [get_pins u_orh/X] -density 0.01 -duty 0.9
set_power_activity -pins [get_pins u_andh/X] -density 0.01 -duty 0.9
foreach p {u_orh/X u_andh/X} {
    puts "PINS $p [get_property [get_pin $p] activity]"
}
puts "=== precedence: -global on absorptive vs propagated ==="
set_power_activity -global -density 0.02 -duty 0.7
foreach p {u_orh/X u_andl/X u_orl/X} {
    puts "GLOB $p [get_property [get_pin $p] activity]"
}
