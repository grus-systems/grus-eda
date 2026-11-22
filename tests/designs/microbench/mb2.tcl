read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_def /work/tests/designs/microbench/mb.def
read_sdc /work/tests/designs/microbench/mb.sdc
read_vcd -scope tb/dut /work/tests/designs/microbench/mb2.vcd
report_power
