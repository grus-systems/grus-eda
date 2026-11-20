create_clock -name core_clk -period 5.0 [get_ports clk]
set_clock_latency 1.0 [get_clocks core_clk]
set_clock_latency -source 0.2 [get_clocks core_clk]
set_clock_transition 0.1 [get_clocks core_clk]
set_clock_uncertainty 0.1 [get_clocks core_clk]
set_propagated_clock [get_clocks core_clk]
set_input_delay 0.5 -clock core_clk [all_inputs -no_clocks]
set_output_delay 0.5 -clock core_clk [all_outputs]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
# Derates open an early/late arrival spread on the clock network (corner rule)
# and on check required times (required-derate rule).
set_timing_derate -early 0.80 -clock -cell_delay
set_timing_derate -late 1.20 -clock -cell_delay
set_timing_derate -early 0.90 -cell_check
set_timing_derate -late 1.10 -cell_check
