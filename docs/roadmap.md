# Roadmap (`docs/roadmap.md`)

grus-eda is a research prototype. It reads the outputs of a sky130 flow (netlist, placed DEF,
SDC, optionally SPEF) and estimates placement, congestion and timing in seconds instead of
minutes. Each stage is checked against OpenROAD or OpenSTA on the same inputs; the current
state of each stage is in [`status.md`](status.md).

This file lists what is missing before the tool can be trusted on designs beyond plain
flip-flop logic. It is a plan, not a promise: items can change order or be dropped when a
measurement says so.

## Principles

- **The reference tool decides.** An item is done when grus-eda matches OpenSTA (or OpenROAD)
  on the same inputs within a stated tolerance, recorded in [`results.md`](results.md).
  Passing our own unit tests is not enough (`AGENTS.md` Section 2).
- **Public designs only.** Every test input is a public design (the OpenROAD-flow-scripts
  `sky130hd` designs, `picorv32`, `ibex`, or a small hand-written fixture that uses only
  `sky130_fd_sc_hd` cells). Designs or numbers from private work never enter this repository.
- **Fail loudly, but say how much is unsupported.** A feature we do not model must not
  silently shrink the answer. Either the run stops, or the report states what was skipped
  and how much of the design it covers.
- **OpenSTA source is not read or copied.** OpenROAD is BSD-licensed and may be read;
  OpenSTA is GPL and is used only through its published papers and its output.

## What fails today

Running grus-eda on the output of a real LibreLane run of a design with latches and clock
gating shows four classes of difference from OpenSTA:

1. Setup slack is too optimistic and hold violations do not appear: the clock is ideal, but
   the real clock tree has cell-driven nets with large latency and skew. (Hold analysis
   itself exists and matches OpenSTA on ideal clocks; what is missing is the clock tree.)
2. Many latch and clear/preset arcs are skipped. The skipped count is printed as a warning
   but does not say what fraction of the design is affected.
3. The LibreLane SDC is rejected as written; it had to be rewritten by hand.
4. `set_timing_derate` is dropped, which shifts every path.

The items below close these gaps, in the order proposed at the end.

## Work items

### R0. Say what was skipped

- Summarise skipped arcs, cells and SDC commands by kind and count, and as a share of the
  design's arcs and endpoints, at the end of every run. Today non-`dlxtn` latches and the
  `D -> Q` arcs of latches are dropped without being counted.
- Machine-readable output (JSON) with the same summary; a `--strict` flag that exits
  non-zero when anything was skipped.
- A short status note at the top of the README (research prototype; known unsupported
  features).
- Done when: a run on a design with latches prints a count and a share per skipped kind
  (latches included), the numbers match an independent count from the Liberty and the
  netlist, the JSON carries the same numbers, `--strict` fails on that design, and the
  README note is in.

### R1. Accept real SDC files

- Commands that change timing must be modelled, not ignored: `set_driving_cell`,
  `set_clock_transition`, `set_propagated_clock`, and `-min`/`-max`/`-add_delay` on
  `set_input_delay`/`set_output_delay`. `-pin_load` and the `[get_clocks ...]` forms
  OpenSTA accepts are parsing work.
- Commands that do not change timing in our model (`set_max_transition`,
  `set_max_capacitance`, `set_max_fanout`) may be skipped, and are reported by R0.
- Decide the policy for unsupported commands: stop (today) or warn and continue behind an
  explicit `--lenient` flag. R0 makes the lenient mode safe to offer; a lenient run never
  counts as agreeing with OpenSTA.
- Track what a stock LibreLane SDC can also contain and decide per item whether it is in
  scope: a second `create_clock`, `create_generated_clock`, `set_case_analysis`,
  `set_false_path`, `set_multicycle_path`, wildcards and `get_pins`.
- Done when: the SDC emitted by a stock LibreLane run on a public design parses unchanged,
  every command is modelled or listed in the R0 summary, and the STA result on that design
  agrees with OpenSTA within the default tolerance (not only "it parses").

### R2. Propagated clock

- Compute clock arrival through buffers and gates on the clock network, per register pin,
  for both rise and fall.
- Latency semantics follow OpenSTA: `set_clock_latency` without `-source` is the ideal
  network latency and is ignored once the clock is propagated; `-source` latency is added
  on top of the propagated arrival. `set_clock_uncertainty` applies in both modes.
- Keep the ideal-clock mode when the SDC does not ask for propagation.
- Clock propagation through an integrated clock-gating cell (R4) is a prerequisite for
  designs that have one.
- CRPR (clock reconvergence pessimism removal) is not needed while early and late delays
  are equal; it becomes necessary as soon as R7 applies different early and late derates
  to a propagated clock. Schedule it with R7.
- Done when: on a public design with a buffered clock tree, setup and hold WNS/TNS and the
  per-endpoint arrival match OpenSTA within the default tolerance.

### R3. Hold with propagated clocks

- Ideal-clock hold analysis exists (`arrival_min`, hold and removal constraints, hold
  uncertainty, `wns_min`/`tns_min`) and is checked on the five benchmark designs. What is
  left is the propagated-clock case: the min-path launch must use the early clock arrival
  and slew, not the late ones.
- Report the hold violation count next to WNS/TNS.
- Done when: a fixture with a real hold violation (ideal clock and propagated clock) matches
  OpenSTA on the violation count, worst hold slack and per-endpoint min arrival. This item
  is part of R2's acceptance and is listed separately because it needs its own fixture.

### R4. Clock gating

- R4a, combinational gates: AND/NAND/OR/NOR gates on the clock path, with the setup and
  hold edges that match each gate's polarity. Today only a positive-unate `and2` is
  matched, with fixed edge timing.
- R4b, latch-based integrated clock-gating cells (`sky130_fd_sc_hd__dlclkp`): clock
  propagation from `CLK` to `GCLK`, and the enable-pin checks. Today a latch cell
  intercepts the arc and its output is treated as a half-period-delayed latch launch.
- The `dlxtn` + `and2` gate in `ibex` is already handled for endpoints
  ([`history/step4/10-latch-and-clock-gating.md`](history/step4/10-latch-and-clock-gating.md)).
- Done when: for each of R4a and R4b there is a public design or a small fixture where all
  endpoints (the gating checks and the gated registers) match OpenSTA on max/min arrival,
  required time, slack and slew within the default tolerance.

### R5. Latches

- Done (`sta_edge/latch_borrow`, strict PASS in ideal and propagated modes on all 31
  endpoints incl. the latch `D` and `RESET_B` pins). The transparent `D -> Q` path
  borrows up to the closing edge minus the per-transition setup constraint, and data
  already waiting at the opening edge launches through the clock-to-Q arc. The model
  covers every `sky130_fd_sc_hd` latch shape (`dlxt[n/p]`, `dlxb[n/p]`, `dlrt[n/p]`,
  `dlrb[n/p]`, `lpflow_inputisolatch`) in both polarities, ideal and propagated clocks,
  and setup and hold.
- Restriction recorded in `docs/results.md`: latch loops are *not* phase-iterated —
  a `D -> Q` cycle is a combinational cycle and levelisation rejects it as an error,
  matching the tool's honest-failure rule instead of inventing a fixed point.
- Remaining skip counts: `min_pulse_width` arcs and `Clear`/`Preset` arcs (OpenSTA
  3.0.16 does not propagate them, R6) are counted in `latch_arcs_dropped`.

### R6. Asynchronous arcs

- Rising recovery and removal checks are already collected. What is missing: propagation
  through `Clear`/`Preset` arcs (`RESET_B -> Q`, `SET_B -> Q`), and the falling
  recovery/removal variants where a cell needs them.
- Done when: a design with asynchronous resets matches OpenSTA on the async check pins and
  on every downstream endpoint reached through a `Clear`/`Preset` arc, within the default
  tolerance.

### R7. Timing derate (with CRPR)

- `set_timing_derate` with `-early`/`-late`, cell and net delay selection (`-cell_delay`,
  `-net_delay`), and `-cell_check`. Which flags are in scope is decided here; anything
  else is reported by R0.
- CRPR from R2 lands here, because derates are what create the common-path pessimism.
- Done when: the same design with and without a derate matches OpenSTA within the default
  tolerance, on both an ideal-clock and a propagated-clock run.

### R8. Libraries and corners

- Check which sky130 library variants and PDK layouts are accepted, and which corner
  files can be loaded. Today the verified setup is `sky130_fd_sc_hd__tt_025C_1v80`.
- Decide whether multi-corner analysis (separate libraries for setup and hold, `-min` and
  `-max` parasitics) is in scope.
- Done when: the accepted variants are listed in `status.md`, each with a run whose STA
  result agrees with OpenSTA within the default tolerance.

### R9. Known accuracy gaps (carried over)

- The `placement RC` tests on `aes`, `ibex` and `jpeg` were `#[ignore]`d.
  (Placement versus routed was closed as an inherent estimator gap in E3.4.)
  `picorv32` was un-ignored in `v0.1.69` after the tree builder became a port of
  OpenROAD's `pdr::primDijkstra` — edge-identical to its placement SPEF on all
  78,749 nets; `aes` and `ibex` followed once `test_pd_vs_spef_parasitics_agree`
  and `test_placement_vs_spef_sta_agree` proved the built parasitics produce
  STA results matching the SPEF readback to <= 0.013 ps per endpoint field
  (they now share the SPEF tolerances). Only `jpeg` stays ignored: its residual is the
  `OpenSTA #582` out-of-table case identical to `placed_spef`. Done when each
  test passes within a stated tolerance, or a decision recorded in `results.md`
  drops the test — `jpeg` is recorded out of scope below.
- The E1 hot-set overlap on `picorv32` is `0.4698`, below the `0.50` criterion. Done when
  it reaches `0.50` and the test is un-ignored, or a recorded decision relaxes the
  criterion.
- Out of scope: the extreme load/slew extrapolation on `jpeg`, which OpenSTA itself does
  not support (`OpenSTA #582`).

### R10. Test and CI foundations

- Done (`.github/workflows/ci.yml` `generate` job): a step runs every
  `tests/designs/sta_edge/*/generate_ref.sh` inside the pinned LibreLane image and then
  `git diff --exit-code` + `git status --porcelain` on `tests/designs/sta_edge/`, so a
  committed golden that no longer reproduces from its committed script fails CI before
  the test job runs. The sta_edge goldens are committed files, so there is no artifact
  to upload; the large design inputs keep the existing artifact path. All sta_edge
  fixtures' non-ignored tests run under `cargo test --release` in the `test` job, and
  every `#[ignore]` carries its measured reason in the attribute and `docs/results.md`.
- Verified locally 2026-11-17: all 11 `generate_ref.sh` scripts rerun clean and leave
  `git status` empty on `tests/designs/sta_edge/` (byte-identical goldens).

## Proposed order

1. R0 and R1: the tool becomes usable and honest on plain flip-flop designs.
2. R2 and R3: propagated clock and its hold case. This closes the largest difference.
3. R6 and R7 (with CRPR): small, high-impact fixes.
4. R4, then R5: gating, then latches (the most work).
5. R8 and R9.

R10 runs alongside every step. For a design with an integrated clock-gating cell, R4b
comes before R2.

## Candidate next work items

The R0–R10 items above closed the gaps found on plain flip-flop and
moderately-featured public designs. Exercising the tool further on
realistic gate-level netlists surfaced the following generic EDA gaps.
Each was re-verified on the public designs and `sta_edge` fixtures before
being listed; none depends on any private design or number. They are
candidates, not commitments — order and scope are open.

### N1. Unreached-endpoint diagnostics

- `SkipReport` only counts `unreached_endpoints`; it does not say which
  pins were unreached or why (no clock, no path, dropped arc).
- Done when: the report names each unreached endpoint and the reason it
  produced no check, verified on a public design that has unreached
  endpoints (`ibex` zero-wire reports 7).

### N2. SDC subset coverage vs. stock flows

- The parser hard-fails on constructs real flows emit: `set_units`,
  collection operations such as `remove_from_collection`, `all_registers`,
  and combined `-early`/`-late` on one `set_timing_derate` line. Stock
  `write_sdc` output for the simple committed fixtures does not exercise
  these, so the gap only shows on richer SDCs.
- Either support them or count them in the skip summary behind a
  `--lenient` path — never a silent pass.
- Done when: a `write_sdc` file emitted by a stock LibreLane run on a
  public design parses unchanged (the original R1 criterion, extended to
  the constructs above).

### N3. Check arcs: `min_pulse_width` and friends

- Flop-heavy designs leave a non-trivial share of arcs in
  `unsupported_arcs[Other]` — `ibex` zero-wire reports `Other=3601`
  (~7% of arcs) plus `Clear=1655`/`Preset=9`, so `--strict` can never
  pass. These are check arcs (pulse width, `non_seq_*`), not
  data-propagation arcs; modelling them clears the summary without
  changing path delays.
- Done when: each such arc is either modelled as a check or counted in a
  dedicated "recognized check, not endpoint-modeled" category, and the
  `--strict` semantics for what remains are documented.

### N4. SPEF write-out

- The placement-RC tree (`pdr::primDijkstra` port) already builds the
  same parasitics OpenROAD estimates; serializing it to SPEF lets
  downstream tools and cross-checks run on identical parasitics.
- Done when: an emitted SPEF re-reads through `readers/spef.rs` and
  reproduces the placement-RC STA result within tolerance.

### N5. Netlist ingest + floorplan generation

- `place`/`sta` only accept DEF produced elsewhere. Add a `floorplan`
  stage (or flags on `place`): read a gate-level netlist (Yosys JSON or
  Verilog) plus a floorplan spec (die area / target utilization,
  site/row generation, edge pin assignment) and emit a floorplan DEF,
  making `synth -> estimate` self-contained.
- Gotcha: Yosys JSON lists the same wire bit under multiple `netnames`
  aliases; the converter must dedupe by bit id or it silently connects a
  pin to several nets (shows up as a bogus combinational loop at
  levelization). Reproducible with any yosys-synthesized public design.
- Done when: a public synthesised netlist produces a legal floorplan DEF
  whose downstream `place`/`sta` runs match the DEF-fed path.

### N6. Multi-corner analysis (optional, previously out of scope)

- Separate min/max libraries for setup and hold; single-corner-per-run
  already works. Convenience plus correct worst-case across corners.
- Recorded out of scope in R9; revisit only if a public design needs it.

### N7. Power reporting (new stage, scope-gated)

- VCD/SAIF-driven power estimation checked against OpenROAD
  `report_power` within a stated tolerance. Largest item; only if power
  estimation is in scope for the project.
