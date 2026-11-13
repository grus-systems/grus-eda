# Status (`docs/status.md`)

The single source for the current status of each stage: designs covered, tolerance actually used,
and known gaps. Done criteria are in `README.md` (Stages) and the Section 1 tables of `docs/e*-*.md`.
The measurement log, including failed runs, is `docs/results.md`. Per-test E3 detail is in
`docs/e3-sta.md` Section 4; open E3 tasks are in its Section 5.

Last updated: 2026-11-13.

| Stage | Designs covered | Tolerance actually used | Status | Known gaps |
|---|---|---|---|---|
| **E0** Readers (Liberty, LEF, DEF, Yosys JSON, Verilog) | `sky130_fd_sc_hd__tt_025C_1v80.lib`; sky130hd technology and cell LEF; `gcd` | Exact: cell, pin, net and row counts; table samples match OpenSTA's printed values | PASS | No E0 comparison recorded for `picorv32` |
| **E1** Congestion (GCell overflow) | `gcd` placed DEF: uniform layer adjustment α ∈ {0.50, 0.70, 0.80, 0.85, 0.90}; utilization variants 25/45/65/78% at α = 0.75 and 0.80 | Ordering only: overflow must rank the variants in the same order as OpenROAD `global_route` | PASS (ordering) | Overflow counts differ from OpenROAD (for example 94 vs 69 at α = 0.85); only the ordering is checked. No `picorv32` E1 comparison recorded |
| **E2** Global placement and Abacus legalization | `gcd` (target density 0.60) | HPWL within 5% of OpenROAD (measured −2.83%); OpenROAD `check_placement` reports 0 violations | PASS on `gcd` only | `picorv32` does not converge within the iteration limit (`place` reports non-convergence, F1); no `picorv32` E2 test yet |
| **E3** Static timing (zero wire, placement RC, SPEF) | `gcd` and `picorv32` (edge-triggered flip-flop designs) | Default `max(5 ps, 1%)` on worst slack and endpoint arrival and required time. Looser values per test: `gcd` placement RC arrival 2%, slew 5%; `gcd` SPEF slew 3%; `picorv32` SPEF arrival floor 6 ps, setup slack 40 ps, slew 10%, TNS 2% | Zero wire PASS on both designs; SPEF PASS on both; `gcd` placement RC PASS; `picorv32` placement RC fails (`#[ignore]`d, arrival 9% off) | Placement RC on `picorv32` needs OpenROAD's placement Steiner tree reproduced (E3.4 — its tree is measured to be neither the minimum tree nor FLUTE's); the driver model is fitted, not derived (E3.5); coupling capacitors and resistor loops are untested against OpenSTA; latches are skipped with a warning; `set_clock_uncertainty` applies to register checks only; speed (E3.6) not started |
