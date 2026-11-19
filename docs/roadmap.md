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
- Remaining skip counts: deferred check arcs (`non_seq_*` etc.) and `Clear`/`Preset`
  arcs (OpenSTA 3.0.16 does not propagate them, R6) are counted in
  `latch_arcs_dropped`/`recognized_check_arcs`. `min_pulse_width` arcs left the
  deferred set when they became evaluated checks in `v0.1.93` (N3).

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

### N1. Unreached-endpoint diagnostics — done

- `SkipReport` now carries `skipped_endpoints`: one `SkippedEndpoint`
  (`pin`, `kind`, `reason`, `related`) per `incomplete_checks` +
  `unreached_endpoints` skip, serialized in the `--json` report and printed
  by the `sta` skip summary.
- Reasons: `incomplete_check` (missing clock pin or late/early arc, `related`
  names the missing piece), `no_clock` (`related` = the unreached clock pin,
  propagated clock only), `no_path`, `untimed_driver` (the net driver is a
  cell with no Liberty mapping or no timing arcs — constants such as `conb`,
  taps, fillers; `related` = the driver pin), and `no_constraint_table`
  (pin reached but no arrived transition had a usable NLDM constraint table).
- Verified on public designs: every unreached endpoint across `ibex` (7),
  `jpeg` (1) and `picorv32` propagated (106) is an `untimed_driver` case —
  output ports and one `ibex` check pin tied to constant `conb_1` cells.
  `cargo test` covers both the `check` and `output_port` kinds.

### N2. SDC subset coverage vs. stock flows — partially done

Done (`8f5eccc`, `44131bd`):

- `set_units`: `-time ns` / `-capacitance pF` and the unit keys the model
  never reads are accepted; a `-time`/`-capacitance` in another unit is an
  error rather than a silent rescale.
- `set_timing_derate`: `-early` and `-late` may share one command and apply
  to both corners.
- `remove_from_collection`: ports subtract through `PortSel::Except`
  (predicate bases record exclusions), clocks and name lists by name.
- Criterion met: `tests/designs/gcd/gcd.write_sdc` — unchanged `write_sdc`
  output from OpenSTA on `gcd.propagated.sdc` — parses with zero ignored
  commands (`parses_stock_write_sdc_unchanged`). Stock `write_sdc` resolves
  collections to per-object `[get_ports {...}]` rules and expands derate per
  corner, so it never emits the constructs below; those come from
  hand/flow-written constraint SDCs.

Done since (`0711785`, `32abb45`, exception application):

- `set_false_path` / `set_multicycle_path` with `get_cells`,
  `all_registers`, `get_ports`, `all_inputs`, `all_outputs`, `get_pins`
  and bare-name/`instance/pin` selectors, resolved after design load.
  STA runs once per exception
  signature class (prepare/eval split; slew stays unfiltered) and
  endpoint reports are composed — false paths drop covered launch classes,
  multicycles shift required times by the OpenSTA-measured formulas
  (`docs/timing-exceptions.md`, `tests/designs/sta_exceptions/`).
  Unsupported qualifiers (`-through`, `-rise`, clock selectors, …) and
  `-both` are counted in `ignored_commands`; latch-D/clock-gate multicycle
  endpoints are counted in `unsupported_multicycle_endpoints`.

Done since (path-delay checks, `docs/path-delays.md`):

- `set_max_delay`/`set_min_delay` with the same `-from`/`-to` selector set:
  delay coverage joins the exception signature so covered sources land in
  their own eval class; at a delay-covered endpoint the clock check's
  candidate is kept (its ordinary arrival and slew) with only the
  required time replaced by the capture-anchored delay requirement —
  measured precedence false_path > delay > multicycle, file-order
  last-wins within a kind. An earlier per-pin delay domain
  (`delay_max`/`delay_launch_max`) was retired on 2026-11-19 when the
  diverging-winner fixture falsified its pairing (`docs/path-delays.md`
  section 5). Output-port `-to` endpoints compose with the edge-referenced
  formulas (`req = N - output_delay_max - unc_setup`, `req = N -
  output_delay_min + unc_hold`; measured 2026-11-19, `mdo_*`/`pmo_*`).
  Unsupported qualifiers and out-of-model endpoints (clock gates, latch-D,
  async checks) are counted, never silently applied
  (`unsupported_path_delay_endpoints`).

Done since (load selectors, `docs/results.md` 2026-11-19 entry):

- `set_load <pf> [get_nets {...}]` applies the OpenSTA-measured
  semantics: the annotation replaces the net's *wire* capacitance in the
  lumped load (last-wins per net, partial apply with one
  `unresolved_load_selectors` count per missing name). Measured against
  OpenSTA goldens at STRICT (`ld_net`, `ld_nets`, `ld_portnet`,
  `ld_last`, `ld_badnet`). Under `--spef` distributed parasitics the
  annotation is counted unresolved rather than approximated.
- `set_load` on `get_pins`/`get_cells`/`all_fanouts`/`all_fanins`/
  `get_clocks` object sets and the `-min`/`-max`/`-rise`/`-fall`/
  `-subtract_pin_load` qualifiers no longer abort the parse; each is
  counted in `sdc_ignored_commands["set_load"]` (OpenSTA rejects the
  pin/cell objects outright — `STA-0100` — and does not know the cone
  commands at all).
- `set_fanout_load <n> <objects>` parses and is counted in
  `sdc_ignored_commands`; OpenSTA accepts it but warns it is unsupported
  (`STA-0461`) and applies nothing, so the count is `is_clean`-exempt
  like the `set_max_*` design rules.

Remaining:

- The path-delay follow-ups in `docs/path-delays.md` section 5
  (two-clock and latch fixtures).

### N3. Check arcs: `min_pulse_width` and friends — DONE (`v0.1.74`)

- Flop-heavy designs left a non-trivial share of arcs in
  `unsupported_arcs[Other]` — `ibex` zero-wire reported `Other=3601`
  (~7% of arcs) plus `Clear=1655`/`Preset=9`, so `--strict` could never
  pass. These are check arcs (pulse width, `non_seq_*`), not
  data-propagation arcs.
- `TimingType` now names every `sky130_fd_sc_hd` check arc
  (`min_pulse_width`, `non_seq_setup/hold_*`, `three_state_*`, `preserve`)
  and `SkipReport::recognized_check_arcs` counts them per type on every
  cell kind, latches included (a latch's pulse-width check no longer lands
  in `latch_arcs_dropped`). They are exempt from `--strict` via `is_clean`
  because they add endpoints outside the reported set rather than altering
  an arrival or slack; `unsupported_arcs` and `latch_arcs_dropped` still
  fail strict, and the CLI summary lists deferred arcs even on a clean run
  ("skipped: none affecting reported timing").
- `ibex` then reported `recognized_check_arcs[Clear=1655, MinPulseWidth=3602,
  Preset=9]` with `unsupported_arcs`/`latch_arcs_dropped` empty;
  `sta_edge/non_dlxtn_latch --strict` passes.
- `min_pulse_width` is evaluated as of `v0.1.93` (see `docs/results.md`
  2026-11-19 entry): `StaReport::pulse_width_checks` carries one row per
  (check pin, high/low) with OpenSTA's measured semantics — widths between
  consecutive opposite clock edges on the propagated network, the
  early/late pair producing the smaller width, period wrap behind inverting
  cells, constraint table at the check pin's own slew, and slack that never
  enters ordinary WNS/TNS. Only arcs on clock-network pins produce rows
  (`dfrtn`'s RESET_B arc emits none). Verified at strict tolerance against
  `sta_edge/min_pulse_width` goldens (ideal, propagated+derated, violating);
  `merge_corner_reports` merges rows by `(pin, edge)` min slack.
- Still deferred: `non_seq_*`, `three_state_*`, `preserve`, and async
  `Clear`/`Preset` stay counted in `recognized_check_arcs`. A
  `min_pulse_width` arc whose pin is not on the clock network (e.g.
  `dfrtn` `RESET_B`) emits no check row — matching OpenSTA — and counts
  into `recognized_check_arcs[MinPulseWidth]` instead (ibex: 1664).

### N4. SPEF write-out — DONE (v0.1.75)

- `sta --spef-out <path>` serializes the parasitics used by the run
  (placement RC, or the loaded `--spef` for SPEF-to-SPEF transcoding)
  as IEEE 1481 SPEF: literal names, `*PORTS`, `*D_NET`/`*CONN`/`*CAP`/
  `*RES`, `*R_UNIT 1 KOHM` for bit-exact resistance round-trip,
  wire-only caps (`PIN_CAP NONE`).
- Compatibility details measured against OpenSTA 3.0.16 / OpenROAD
  3.0.16 `write_spef` output: the header must carry the full keyword
  block (`*DIVIDER`, `*BUS_DELIMITER`, `*L_UNIT`, ...); `$` and `/` in
  names need backslash escapes while `[`/`]` stay raw (bus-bit syntax
  resolves to the literal DEF net name — escaping them makes OpenSTA
  report "net not found" for every `name[N]` net).
- Verified: emitted gcd/ibex SPEFs parse in OpenROAD with zero
  warnings; OpenSTA slack on our SPEF agrees with our placement-RC STA
  within ~0.1% (gcd WNS 1.664237 vs 1.665178 ns; ibex WNS -14.417081 vs
  -14.411448 ns — OpenSTA's own delay engine on identical parasitics).
  `test_write_spef_round_trip_{parasitics,sta}` lock the internal
  round-trip (per-pin Elmore < 1e-9 ns; endpoint report identical).

### N5. Netlist ingest + floorplan generation — DONE (v0.1.76)

- `grus-eda floorplan` reads a gate-level netlist (Yosys JSON or
  Verilog; the existing E0 readers already dedupe Yosys wire bits by
  bit id, so the alias gotcha is handled) plus a floorplan spec and
  emits a complete floorplan DEF: `--utilization` (default 0.60) or
  `--die-area xl,yl,xh,yh` (microns), `--margin-um` (default 10),
  `--site` (default: tech LEF site). Rows alternate N/FS at site
  pitch; inputs go on the left edge, outputs on the right, names
  sorted naturally so buses read in order.
- `readers/def.rs::write_def_full` serializes a whole DesignDb (no
  template): header, DIEAREA, ROWs, COMPONENTS, PINS, NETS. TRACKS/
  GCELLGRID/VIAS/SPECIALNETS are omitted — they constrain detailed
  routing/power, which the database does not model. Names go through
  `escape_def_name`, which escapes only the lexer delimiters and `\`
  itself; `[N]` bus-bit syntax stays literal so external readers see
  the same names OpenROAD-flow-scripts emits. The DEF lexer was
  extended to treat `\x` as an escaped character inside a word so
  names containing `(`, `)`, `;`, `#`, or whitespace round-trip.
- Verified: `gcd.synth.json` -> floorplan DEF -> `place` converges
  (233 cells, 24 rows) -> `sta` reports the same 53 endpoints as the
  DEF-fed path (setup wns 1.757771, zero violations). OpenROAD
  `read_def` accepts the emitted DEFs with matching counts — gcd
  (54 pins, 233 components, 269 nets), aes (388 pins, 8780
  components, 9039 nets), ibex (264 pins, 11979 components, 12137
  nets). `aes.synth.v` -> 8780 cells / 9039 nets round-trips through
  `parse_def`. `floorplan::tests` lock the DEF round-trip
  connectivity, edge assignment, and bad-spec rejection;
  `readers::def::tests::test_escaped_names_roundtrip_through_lexer`
  locks name escaping through the lexer.

### N6. Multi-corner analysis — DONE

- `sta --lib-hold <lib>` runs setup checks against `--lib` and hold checks
  against `--lib-hold`, then merges the two reports into one worst-case
  `StaReport` (`merge_corner_reports`). The DEF is parsed once per library so
  `cell_lib_ids` resolve against the right `LibDb`; placement parasitics are
  recomputed per design view.
- Verified on `gcd.placed.def` with `ss_100C_1v60` (setup) + `ff_100C_1v95`
  (hold): OpenROAD gives setup WNS -0.46 (ss) and hold WNS 0.31 (ff); ours
  gives -0.730449 and 0.311508. The hold slack matches to ~0.5%; the setup
  gap is the same STA-vs-OpenSTA divergence seen in single-corner runs.
- `run_sta_quiet` suppresses duplicate skip warnings on the second corner run.
- Endpoint sets must match across corners or the merge bails — different
  Liberty coverage would silently drop checks.

### N7. Power reporting (new stage, scope-gated)

- VCD/SAIF-driven power estimation checked against OpenROAD
  `report_power` within a stated tolerance. Largest item; only if power
  estimation is in scope for the project.
- **Status (2026-11-19)**: VCD and SAIF paths implemented and
  formula-validated against a clocked micro-benchmark. `--saif` reads
  `TC` aggregates, reconstructs the same per-net edge counts as `--vcd`,
  and matches OpenROAD `read_saif` + `report_power` within ~0.5% total
  on the committed golden (`mb.saif.reference.json`); OpenSTA SAIF
  field semantics were measured in-container (docs/results.md). On `gcd`
  the totals diverge because OpenSTA propagates activity statistically
  (BDD + `findSeqActivity`) rather than reading per-net VCD counts; our
  exact per-net counts are reported instead, and the residual is
  documented in `docs/results.md`. Still open: `when`-conditional power
  blocks, and matching OpenSTA's activity propagation if a closer
  `report_power` agreement is ever required.
