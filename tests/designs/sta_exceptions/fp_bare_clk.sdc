create_clock -name core_clk -period 2.0 [get_ports clk]
set_clock_latency 0.6 [get_clocks core_clk]
set_clock_latency -source 0.1 [get_clocks core_clk]
set_clock_transition 0.1 [get_clocks core_clk]
set_clock_uncertainty -setup 0.05 [get_clocks core_clk]
set_clock_uncertainty -hold 0.1 [get_clocks core_clk]
# The -min input delay is deliberately tiny: the direct port -> D path cannot meet hold.
set_input_delay -min 0.0 -clock core_clk [get_ports d_in]
set_input_delay -max 0.7 -clock core_clk [get_ports d_in]
set_output_delay -min 0.1 -clock core_clk [get_ports y]
set_output_delay -max 0.7 -clock core_clk [get_ports y]
set_input_transition 0.1 [get_ports d_in]
set_load 0.05 [get_ports y]
set_false_path -from u_ff0/CLK -to u_ff1/D
