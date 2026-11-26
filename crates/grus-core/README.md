# grus-core

Physical-design engine core for the open SkyWater sky130 PDK (`sky130_fd_sc_hd`).

Library crate behind the [`grus-eda`](https://crates.io/crates/grus-eda) CLI:
readers for Liberty/LEF/DEF/SPEF/SDC/VCD/SAIF and Yosys JSON or gate-level
Verilog, analytical placement, static timing analysis, global and detailed
routing, power and IR-drop analysis, and a LEF-expressible DRC subset.

Each stage is validated against the reference open-source tools (OpenROAD,
OpenSTA, Magic, PDNSim) on the same public designs; the verification log is in
the repository's `docs/results.md`. This is a research prototype — sign-off,
GDS output, and LVS are out of scope.

- Repository: <https://github.com/grus-systems/grus-eda>
- Homepage: <https://eda.grus.systems>

License: Apache-2.0
