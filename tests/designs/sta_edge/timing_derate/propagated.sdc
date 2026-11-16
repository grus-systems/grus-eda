create_clock -name core_clk -period 2.0 [get_ports clk]
set_clock_latency 0.6 [get_clocks core_clk]
set_clock_latency -source 0.1 [get_clocks core_clk]
set_clock_transition 0.1 [get_clocks core_clk]
set_clock_uncertainty -setup 0.05 [get_clocks core_clk]
set_clock_uncertainty -hold 0.1 [get_clocks core_clk]
set_propagated_clock [get_clocks core_clk]
set_input_delay -min 0.0 -clock core_clk [get_ports d_in]
set_input_delay -max 0.7 -clock core_clk [get_ports d_in]
set_output_delay -min 0.1 -clock core_clk [get_ports y]
set_output_delay -max 0.7 -clock core_clk [get_ports y]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports y]
# `write_sdc`-style derates plus selector coverage: -cell_check, -clock/-data and
# -rise/-fall (the latter select the transition entering each arc, measured on OpenSTA).
set_timing_derate -early 0.95
set_timing_derate -late 1.05
set_timing_derate -early 0.90 -cell_check
set_timing_derate -late 1.10 -cell_check
set_timing_derate -early 0.85 -clock -cell_delay
set_timing_derate -late 1.15 -data -cell_delay
set_timing_derate -early 0.80 -cell_delay -rise
set_timing_derate -late 1.20 -cell_delay -fall
