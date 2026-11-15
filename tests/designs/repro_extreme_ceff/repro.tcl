# Standalone reproducer for the OpenSTA report (parallaxsw/OpenSTA#582): Ceff falls as the input slew grows.
# Run in the pinned LibreLane image: LIB=<path to sky130_fd_sc_hd__tt_025C_1v80.lib> sta -no_init -exit repro.tcl
read_liberty $::env(LIB)
read_verilog top.v
link_design top
create_clock -name vclk -period 100
set_input_transition 0.1 [get_ports b]
read_spef top.spef
foreach s {0.05 0.5 1.5 2.5 4 6 9.142787} {
  set_input_transition $s [get_ports a]
  puts "SWEEP input_slew $s"
  report_dcalc -from [get_pins u1/A] -to [get_pins u1/Y] -max -digits 6
}
exit
