create_clock -name clk -period 20.0 [get_ports clk]
set_input_transition 0.15 [all_inputs]
set_load 0.05 [all_outputs]
