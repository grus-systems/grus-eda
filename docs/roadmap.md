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

### R3. Hold with propagated clocks — Done

- Fixture `sta_edge/hold_violation/` (`hold.def` + `ideal.sdc` + `propagated.sdc`):
  2 of 4 endpoints hold-violate in both modes, matched at `Tol::STRICT` by
  `test_sta_edge_hold_violation_{ideal,propagated}` (`analysis.rs` — v0.1.61,
  `docs/results.md` 2026-11-17 "R3").
- `StaReport::hold_violations` is printed by `grus-eda sta` next to WNS/TNS
  (`main.rs`) and serialised in `--json`; `compare_sta_edge` asserts the counts
  exactly when the golden carries them.
- Residual note kept verbatim: the propagated source-slew fix seeds the clock
  port's `set_input_transition` (not `set_clock_transition`) into the propagated
  network — measured in the same entry.

### R4. Clock gating — Done

- Fixture `sta_edge/clock_gate/` covers `and2`/`or2`/`nand2`/`nor2` plus the
  `dlclkp` integrated clock-gating cell, with `dfrtn` flops behind the
  inverting gates so both edge polarities are exercised — STRICT in ideal and
  propagated (`test_sta_edge_clock_gate_{ideal,propagated}`, v0.1.66,
  `docs/results.md` 2026-11-17 "Roadmap R4").
- The measured rule: gate kind comes from the output pin's Liberty `function`
  (AND/NAND = rising check, OR/NOR = falling, both edges swapped); muxes and
  partial functions are rejected by `gate_kind`, locked by
  `test_gate_kind_classifies_liberty_functions`. `dlclkp`'s GATE check is an
  ordinary register check and `CLK -> GCLK` uses the combinational arc.
- Residuals: cells without a Liberty `function` keep the positive-unate AND
  guess (they cannot be classified); `gating_enables` checks get no CRPR
  credit under derate (pessimistic, recorded in the R7 entry); the four
  `CrossDomain` deferrals in `docs/multi-clock.md` §2 are unrelated —
  resolved in v0.2.12 (`xdomain-deferrals`). Ideal-mode clock-network
  inversion is no longer open —
  `ideal_clock_parity` has tracked edge phase through cell-driven clock nets
  since v0.1.71 (same-day follow-up entry).

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

### R6. Asynchronous arcs — Done (already matching, re-measured)

- Re-measured on `sta_edge/async_arcs` (`docs/results.md` 2026-11-21): OpenSTA
  3.0.16 does **not** propagate `Clear`/`Preset` arcs into timed paths. The
  unclocked-`CLK` discriminators `u_ar`/`u_ap`/`u_aq` carry raw arrivals on
  `Q`/`Q_N` but no clock tag, so `find_timing_paths -to u_d*/D` returns zero
  paths and disabling `u_ff0`'s `CLK -> Q` arc leaves `u_ff2/D` pathless.
  `report_checks -unconstrained` names no unclocked endpoints at all; OpenSTA
  emits no warning for them. Keeping `Clear`/`Preset` in
  `recognized_check_arcs` is the matching behaviour — propagating would
  diverge.
- Check coverage now spans every async check shape in `sky130_fd_sc_hd`:
  `dfrtp`/`dfstp`/`dfrbp`/`dfbbp`/`dfbbn` flop variants (rising and falling
  recovery/removal, dual async pins) in this fixture at STRICT
  (`test_sta_edge_async_arcs_{ideal,propagated}`), `dfrtn` in
  `async_fall*`/`clock_gate`, `dlrt*`/`dlrb*` latches in `latch_borrow`;
  `sdf*` scan cells are family-equivalent supersets (`SCD`/`SCE` pins extra).
- Named residuals: `non_seq_*` arcs on `dfbb*`/`sdfbb*` stay deferred —
  measured on `sta_edge/check_arcs` (`docs/results.md` 2026-11-21): OpenSTA
  3.0.16 has no data-check report surface at all (`report_check_types
  -data_setup` → STA-0562 "not a known keyword or flag",
  `find_timing_paths -path_delay non_seq_setup` → STA-0510), and each
  `SET_B`/`RESET_B` pin reports exactly one path — the recovery/removal
  check, not a `non_seq` row. The deferral is parity, not a gap. `preserve`
  stays unmeasurable (zero arcs in `sky130_fd_sc_hd`), so the re-measurement
  clause stands for it alone.

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

- Multi-clock SDC (`docs/multi-clock.md`): OpenSTA's two-clock rules are
  measured on the committed `sta_clocks` fixture (2026-11-19) and the
  implementation ran as a five-ticket sequence (`mc-model` → `mc-eval` →
  `mc-xdomain` → `mc-selectors` → `mc-golden`), now complete: `mc-model`
  (the clock table, v0.2.0), `mc-eval` (domain tagging + same-domain
  pairing, v0.2.1), `mc-xdomain` (cross-domain min-gap edge pairing +
  inter-clock uncertainty, v0.2.2), `mc-selectors` (clock-object
  selectors, `set_clock_groups`, `create_clock -waveform`, v0.2.5) and
  `mc-golden` (probe promoted to `two_clock.reference.json`, v0.2.6).
  All 52 golden `sta_clocks` scenarios match OpenSTA at strict
  tolerance with `cross_domain == 0`, and `docs/path-delays.md`
  section 5 has no open items left. `xdomain-deferrals` (v0.2.12)
  resolved the four parked `CrossDomain` corners in
  `docs/multi-clock.md` §2: undeclared-launch zero masks keep an
  edge-0 clk->Q launch (ordinary checks dropped like OpenSTA's
  `t2_u1`, named `UndeclaredLaunch` skips; `-to` delay checks
  evaluate per `t2_u2`), latch-D foreign launches borrow through the
  latch's own window (`t2_xlat`/`t2_xlat2`), and hold pairing is
  capture-edge-driven inside the measured `pair_window` bound
  (`t2_lcm`/`t2_lcm2`). The only surviving deferral is the defensive
  no-pair-in-window `CrossDomain` — unreachable on the measured
  grids.

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
- `three_state_enable`/`three_state_disable` are evaluated as of `v0.2.11`
  (see `docs/results.md` 2026-11-21): OpenSTA propagates both as delay arcs
  firing on the sense-declared source edge, both destination transitions —
  `Z1`/`0Z` merge into dst rise, `Z0`/`1Z` into dst fall — and counts the
  tristate driver pin's own capacitance on the net load. Verified at strict
  tolerance against `sta_edge/check_arcs` goldens (ideal, propagated,
  `te_slow` discriminator).
- Still deferred: `non_seq_*`, `preserve`, and async
  `Clear`/`Preset` stay counted in `recognized_check_arcs` — measured:
  OpenSTA 3.0.16 reports no `non_seq`/`preserve` rows on any surface
  (`report_check_types` rejects `-data_setup`/`-data_hold`, no per-pin
  extra rows) and does not propagate `Clear`/`Preset` (R6). A
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
  `TC` aggregates and reconstructs the per-net edge counts (`--vcd`
  counts minus clk's one fall outside the truncated `[10,1990]` SAIF
  window), matching OpenROAD `read_saif` + `report_power` within ~0.4%
  total on the committed golden (`mb.saif.reference.json`); OpenSTA SAIF
  field semantics were measured in-container (docs/results.md). On `gcd`
  the earlier "statistical propagation vs exact counts" gap turned out
  to be mostly VCD-reader corruption (same-named `pin_signals` folding,
  dropped short `b` writes) — with the reader fixes and the AP1–AP4
  propagated model, `report_power` lands within ~2% of OpenSTA on every
  group under the same VCD seeds (`docs/results.md` 2026-11-22); the
  named residuals are the zero-wire-parasitics switching term and the
  state-weighted `when` leakage attribution. `when`-conditional
  `internal_power`
  and `leakage_power` blocks now evaluate at the measured OpenSTA model
  — marginal `P(when)` products, related-pin activity apportioning,
  half-weight x/z edges — verified at ~1e-7% on the `mbw` ebufn fixture
  in both VCD and SAIF paths (docs/results.md 2026-11-19); only
  inexpressible `when` forms remain counted. Statistical activity
  propagation is sequenced AP1–AP4 on the `mba` fixture: **AP1 done**
  (v0.2.14 — `resolve_activity` skeleton + measured seed defaults,
  `grus-eda power` runs without annotation); **AP2 done** (v0.2.15 —
  bounded Liberty `function` evaluator, Kahn-ordered combinational
  propagation with `Default`-taints, pin-namespaced annotation; the
  golden's surviving divergent set is the 13-pin AP3 residual)
  (v0.2.16 — constant marking is 3-valued absorptive; the stored
  constant marginal is the measured 0.0 — cp.def probe); **AP3 done**
  (v0.2.17 — sequential data-pin propagation: `ff.next_state` /
  `latch.data_in` parsed into `LibCell::seq`, Q copies the D net's
  `{P, d}` verbatim (origin `propagated`), resolution iterates to a
  fixed point, leftover `Default` nets become `unknown {0,0}` and
  taint; all 12 `mba` sessions match the golden per-pin and
  `EXPECTED_DIVERGENT` is empty); **AP4 done** (v0.3.0 —
  `ActivityOverrides::pins` per-pin `-pins` overrides and the native
  `user` origin (also emitted by `-input_ports`), `dead_user_entries`,
  `pins`≡`input_ports` equivalence pinned on the golden; two VCD
  reader bugs the pin-namespace comparison exposed — flat-name
  `pin_signals` folding and dropped short `b` writes — fixed, and the
  gcd `report_power` gap narrowed to within ~2% on every group).
  Remaining named residuals: the per-pin/per-net annotation gap
  (`s_full` `y`), the `unknown`-net `prob_one: Some(0.0)` /
  `when`-literal semantics note (OpenSTA's own `when` weight on
  `unknown` unmeasured), the unmeasured precedence choices
  (`user`-vs-dump, `-pins`-vs-`-input_ports`, duplicate `-pins`
  last-wins), OpenSTA counting `user` under its `input N` split where
  we report `user N` per net, and the zero-wire parasitics switching
  plus state-weighted `when` leakage residuals on gcd. See
  `docs/results.md` 2026-11-22 and
  `kurena/tasks/ap4-activity-precedence/design/`.

### N8. Performance (measured baseline — 2026-11-22, v0.3.2)

Not a correctness item; a speed track. `GRUS_PHASE_TIMING=1` emits
stderr phase timers at the whole-pass boundaries of `sta`, `power`,
`place`, `congestion` (off by default, zero-cost gate). The committed
matrix (`kurena/tasks/perf-profile/measure.sh`, 5 warm-cache reps on the
M4 mini) is logged in `docs/results.md` 2026-11-22. Measured, the
dominant phase is `sta.prepare.clock.slew` — a *serial* Kahn-level slew
propagation (`propagate_slew`) at 77–81% of `sta` wall on the three
largest designs and ~94% of `power` wall via `sta_pin_slews`; `sample`
puts ~80% of main-thread samples inside `sta::dmp`
(`rc_crossing`/`fit_ramp_duration`/`solve_dmp_with`). Candidate
follow-on tickets, in measured order (details + Amdahl ceilings in
`kurena/tasks/perf-profile/design/plan.md` — candidates, not
commitments): ~~`perf-par-slew`~~ **done** (per-level `par_iter`, jpeg
`sta` placement RC 6.0 s → 2.06 s, `sta.prepare.clock.slew` phase
×5.4 — `docs/results.md` 2026-11-22 perf-par-slew entry);
~~`perf-slewonly-power`~~ **done** (slew-only `sta_pin_slews` —
`power` placement RC jpeg 2170 → 1030 ms, ×1.5–2.5 across the five
designs; `power.slews` now nests `sta.slew.seeds|walk` and skips the
pulse-width-only `sta.prepare.clock.*` block — `docs/results.md`
2026-11-22 perf-slewonly-power entry); ~~`perf-legal-passes`~~ **done**
(shared incremental `NetExtrema` cache + exact site-grid argmin slide —
jpeg `place` 2170 → 1560 ms on the same-session baseline, `pass2`/`pass3`
~×12, `pass1` flat; HPWL improved 0.3–0.6% on all five designs —
`docs/results.md` 2026-11-22 perf-legal-passes entry; `place.gp.*` is
now ~73% of the `place` wall and the dominant remaining phase), then
~~the parser tickets
`perf-spef-parse`, `perf-def-parse`, `perf-vcd-parse`~~ **done** —
landed together as `perf-parsers` (allocation-free scans in all three
readers: `sta.spef.read` ×1.34–1.39, `*.load.def`/`cong.load` ~×1.2,
`power.activity.load` ×1.52; jpeg `sta.spef` cell wall 1650 → 1380 ms,
`power.vcd.gcd` 440 → 290 ms — `docs/results.md` 2026-11-22
perf-parsers entry), and
~~`perf-dmp-hotloop`~~ **done** (per-call `to_bits()`-keyed memo of
the DMP `table` closure — instrumented counts first: 0.0% cross-call
`driver_response` exact repeats killed the run-scoped memo, 21.6%
intra-call `table(c)` repeats chose the mechanism, 43.7%
`rc_crossing` Brent share named residual; jpeg `sta.placement`
1290 → 1090 ms, `power.placement` 740 → 630 ms, `cmp`-clean
throughout — `docs/results.md` 2026-11-22 perf-dmp-hotloop entry).
`sta.prepare.clock.slew` / `sta.eval.walk` remain the co-dominant
phases on `sta` and `power` (the shared DMP inner loop, now minus
the `eval_2d` redundancy). Every optimization ticket lands only with
a bit-identical-or-tolerance-named fast path and a `docs/results.md`
number, per the usual discipline.

## Candidate next stages (E4–E8, scoped 2026-11-22)

The readers → placement → STA/power pipeline is complete; what is missing for
a usable flow is routing, clock-tree, and sign-off-adjacent analyses. The
full scoping — inputs/outputs, reusable modules by name, per-metric
tolerances, probe evidence — is in
`kurena/tasks/next-stage-scoping/design/`; probe outputs are logged in
`docs/results.md` (2026-11-22 "next-stage-scoping"). Recommended order is
**E4 → E5 → E8**, with E6/E7 behind a decision gate. Update 2026-11-23:
the gate is lifted — E6/E7 are authorized; **E9** (reporting/usability
surface, from the first large-design run's feedback
`docs/history/run-2026-11-22-large-design.md`) joins as a small stage.

### E4. Global routing → route guides + guide-based parasitics

- In: `tlef` + `lef` + placed `def`. Out: OpenROAD-format guide file +
  guide-based SPEF via the existing `--spef-out` path.
- Reuses almost everything E1 built: `GCellGrid::build`, `decompose_net`
  RSMT, `route_segments` committed paths (preserved in `RoutedDesign` since
  `v0.3.9`), `NetRcTree`/`NetParasitics`, SPEF writer.
- Done when (on `gcd.placed.def`, then `picorv32.placed.def`): per-net
  guide-box overlap ≥ 0.70 vs `global_route -guide_file`; guide-box total
  within 15%; our RC on OpenROAD's guides *and* our guides fed back via
  `read_guides` agree with `estimate_parasitics -global_routing` SPEF within
  `max(5%, 1 fF)` per net; end-to-end `sta` within `max(5 ps, 1%)`.
- Progress 2026-11-22 (`v0.3.9`, ticket `e4-guides-emit`): guide emission
  and the topology criteria are done — `grus-eda route --guide-out` writes
  OpenROAD-format guides with weighted overlap `0.975` gcd / `0.960`
  picorv32 and box deltas `+2.4%` / `-6.2%`; `read_guides` +
  `estimate_parasitics -global_routing` accept our guide in the pinned
  image.
- Progress 2026-11-22 (`v0.3.10`, ticket `e4-guide-parasitics`): the guide
  parasitics criterion is met — `sta --parasitics guide [--guide <file>]`
  builds `NetRcTree`s reproducing `estimate_parasitics -global_routing`'s
  per-net `*D_NET` totals: **269/269 gcd + 6744/6744 picorv32 nets** within
  `max(5%, 1 fF)` (max rel diff 0.11%). The read-back direction (our
  guides → OpenROAD `estimate_parasitics`) is recorded at 98.1%/98.4% —
  its residual is guide coverage, not the RC model. The end-to-end `sta`
  criterion remains open.
- Progress 2026-11-22 (`v0.3.11`, ticket `e4-sta-e2e`): criterion 4
  measured end to end — **not met** (gcd 0/53, picorv32 94/1,798 endpoints
  inside bound). The decomposition names both causes: the
  `SPEF_STUB_RES_KOHM` island-link lands OpenROAD-dead island RC on the
  Elmore path (parasitics axis, +202 ps / +1,408 ps median) and guide
  coverage is the topology axis (+65 ps / +523 ps); the STA engine stays
  inside the recorded band. Remaining open item: guide-quality
  improvements (met3–met5 band, stub coverage) and island-faithful
  parasitics — not the e2e harness. See `docs/results.md` `e4-sta-e2e`.
- Progress 2026-11-22 (`v0.3.12`, ticket `e4-guide-islands`): the named
  dominant cause is fixed — dead components stay dead in `NetRcTree`
  (island caps retained, `n n r` self-loops emitted, dead pins get zero
  wire delay) and OpenSTA's lumped fallback is modelled (dead port →
  lumped, ~1/3 dead-cap share → lumped, probed approximation). The
  parasitics-axis median collapses +202/+1,408 ps → ~0; the e2e
  residual is now the e4-1 topology axis (gcd `resp_val` +250.8 ps,
  picorv32 `_108xx_` +627 ps). See `docs/results.md` `e4-guide-islands`.
- Progress 2026-11-22 (`v0.3.13`, ticket `e4-topology`): emission-side
  connectivity — bound pin cells emit contiguous rung stacks anchored on
  the covering strip, unbound pin cells climb to access+1 (the conn+1
  coverage OpenROAD gives port cells, probed on picorv32's io cells), and
  every non-pin H∩V GCell emits a consecutive `met1`/`met2` bend pair.
  Dead `*P` ports are **0** on both designs and all remaining dead
  components are one-node stubs (multi-node islands eliminated; the
  OpenROAD read-back keeps an 8/105-net residual). Criterion 4: gcd
  topology max `210.6 < 250.8 ps` — the `resp_val` dead-port extreme is
  gone. picorv32 is the named residual: fully-connected emission vs the
  reference's dead detour islands on high-fanout nets (`_00012_`
  fanout 273, +2.33 ns worst endpoint). See `docs/results.md`
  `e4-topology`.
- Probe note: guide parasitics halve the placement-estimate gap to routed
  truth on gcd (median per-net C diff vs RCX: 27.6% vs 44.2%).
- **Closed 2026-11-22**: criteria 1–3 pass; criterion 4 closed as named
  residual — the remaining e2e error is route-path topology divergence vs
  OpenROAD's GRT (gcd total max 232.75 ps; picorv32 named high-fanout
  dead-detour class), not emission, parasitics (~0 median), or engine
  (≤74 ps). The delegation path (our guides → `read_guides` →
  `detailed_route`) is the recorded use case. See `docs/status.md` E4.
- Sizing: ~6–8 tickets (M). Ticket sketch in `design/order.md`.

### E5. Clock tree synthesis

- In: placed `def` + `sdc` + `lib`/`lef`. Out: DEF with inserted clock
  buffers + skew/insertion-delay report.
- Reuses the Abacus legalizer, `write_def_full`, and the propagated-clock
  STA mode (already matching OpenSTA) as the scoring path; the real new
  infrastructure is `DesignDb` cell/net insertion — a mutation API every
  future buffering/repair step needs.
- Done when (on `gcd.placed.def`/`gcd.sdc` vs `clock_tree_synthesis` +
  `repair_clock_nets`): skew within `max(25 ps, 25%)` (gcd reference ≈ 0 ps),
  max insertion delay within 15% (reference ≈ 0.31 ns), propagated-clock STA
  on the TritonCTS DEF vs ours within `max(5 ps, 1%)` per endpoint.
- Progress: ticket 1 `e5-cts-ref` committed the TritonCTS goldens; ticket 2
  `e5-insert-api` landed the `DesignDb` mutation API; ticket 3 `e5-cts-tree`
  (`v0.5.0`) landed the depth-balanced constructor; ticket 4 `e5-cts-e2e`
  (`v0.5.1`) scored our CTS DEFs against the committed goldens — criteria 2
  (insertion) met on both designs, criterion 1 (skew) met on gcd, and the
  named residual is **tree-shape** (our depth-balanced tree equalizes
  buffer count, not per-pin RC): picorv32 skew +34.4 ps over bound (engine
  residual on the same tree ≤0.41 ps), gcd hold-side endpoint fields ~9.5
  ps early, picorv32 endpoints up to 94.4 ps with a secondary engine axis
  on the `_1145x` reset-launched cluster; see `docs/e5-cts.md` and
  `docs/results.md`. Remaining: dummy-load repair
  (`repair_clock_nets`-class work) and, if the skew residual is to be
  closed, a per-pin RC/latency balancing pass.
- Sizing: ~6–8 tickets (M–L).

### E8. IR drop (power-grid analysis)

- In: `def` with `SPECIALNETS` PDN + per-instance power (N7). Out:
  per-instance voltage map + worst drop.
- Reference confirmed in the image (P-IRD): `analyze_power_grid -net VPWR` on
  gcd reports worst IR drop 0.591 mV and emits a per-instance voltage CSV via
  `-voltage_file`; `check_power_grid` verifies PDN connectivity.
- Done when: worst drop within `max(5%, 5 mV)` and per-instance voltages
  within `max(5%, 1 mV)` of the reference on `gcd.placed.def`; the
  current-source convention (embedded estimate vs `set_pdnsim_inst_power`)
  pinned by the golden ticket.
- Progress 2026-11-23 (ticket `e8-irdrop-ref`): reference committed —
  `tests/designs/generate_irdrop_ref.sh` runs `check_power_grid` +
  `analyze_power_grid -voltage_file` on VPWR+VGND in the pinned image on
  both designs and byte-reproduces its goldens on rerun. Embedded numbers
  match the P-IRD scoping (gcd VPWR total 1.04 mW, supply 1.80 V, avg
  drop 0.223 mV, worst 0.591 mV, 384 CSV rows; VGND worst bounce 0.559
  mV; picorv32 VPWR worst 4.30 mV / VGND 3.61 mV, 10,205 rows). The
  convention item resolved **embedded**: `set_pdnsim_inst_power` is
  additive to the computed estimate — feeding OpenSTA's per-instance
  power doubles the drop (gcd 0.591→1.18 mV) and the all-zero probe
  leaves the solve byte-identical to embedded while reporting 0.00 W —
  so the explicit feed cannot substitute an external estimator. See
  `docs/results.md` 2026-11-23.
- Progress 2026-11-23 (ticket `e8-pdn-parse`): per-net PDN model landed —
  `DesignDb::pdn_nets` (`PdnNet`: conn groups verbatim, `+ USE`,
  half-width-expanded `wire_rects`, `PdnVia` stacks). DEF `VIAS` records
  resolve generated (`+ VIARULE`/`+ LAYERS`/`+ ROWCOL`), fixed (`+ VIA`), and
  inline (`+ RECT`/`POLYGON`) forms; LEF `VIA` blocks give `ViaDef` layer
  triples so DEF via names resolve without name heuristics, and `CutLayer` +
  `via_res_ohm_above` carry the cut resistances (`via_res_ohm(cut, count)`).
  `pdn::check_net` expands conns, attaches each `MacroPin` port rect to
  same-layer wires, and unions rect components through via edges: on
  `gcd.placed.def` VPWR/VGND each give 1 node, `attached_terminals` 384 =
  embedded CSV rows, `unreachable_pins` empty — agreeing with
  `check_power_grid` in `gcd.irdrop.reference.json`; picorv32 gives
  10,205 = 10,205 rows on both rails. `special_wire_rects()` keeps the gcell
  blockage view. See `docs/results.md` 2026-11-23.
- Progress 2026-11-23 (ticket `e8-mesh-rc`): resistor network landed —
  `pdn::mesh::PdnMesh` (junction-piece nodes, `RPERSQ × L/W` wire runs,
  `via_res_ohm` stack links, sinks = the attach set, sources = the PIN's
  io-rect nodes), `validate()` (finite+positive R, source-reachability
  BFS) + `MeshCensus`. Both designs validate clean; census recorded in
  `docs/results.md` (no PDNSim mesh golden exists — the e2e ticket gates
  the solve).
- Progress 2026-11-23 (ticket `e8-solve`, v0.7.3): sparse DC solve
  landed — `pdn::solve` stamps the mesh into a conductance CSC, clamps
  source nodes to each golden `supply_voltage_v` (exact, assigned not
  factored), injects the N7 per-instance feed as `P/V` sink currents,
  and solves with `nalgebra-sparse` `CscCholesky` (direct, SPD reduced
  system). `grus-eda irdrop` emits PDNSim-shaped
  `*.irdrop.voltage.ours.<net>.csv` — first five columns verbatim-equal
  to the golden row set on all four nets — plus a summary JSON with
  ‖GV−I‖∞ (≤ 1.7e-14 A abs / ≤ 2.3e-16 rel), worst/avg drop and bounce,
  and deltas vs the golden scalars. Worst-drop deltas and the loose
  per-pin bound sit inside tolerance on both designs; the strict
  `max(5%, 1 mV)` per-pin claim was superseded by the e8-e2e gate below
  (picorv32 VGND does not meet it). The residual gap is the
  N7↔OpenSTA estimator feed (-72.5 % total on gcd, -43.5 % on
  picorv32), quantified in `docs/results.md` 2026-11-23.
- Progress 2026-11-23 (ticket `e8-e2e`): **done with named residual** —
  `crates/grus-core/tests/irdrop.rs` gates our maps against the
  `embedded` goldens on both designs and rails. gcd passes both
  done-when bounds (per-pin max |dV| 5.4e-4 / 5.0e-4 V, worst-drop
  deltas −4.1e-4 / −3.3e-4 V). picorv32 meets the `max(5%, 5 mV)`
  worst-drop/bounce bound on both rails and the strict per-pin bound on
  VPWR; **VGND keeps a named 3.2 mV floor** (146/10,205 rows over the
  strict `max(5%, 1 mV)` rule, max 3.164 mV) decomposed by the
  newly-committed `<d>.irdrop.inst_power.tcl` feed: on OpenSTA's
  per-instance powers our mesh/solver lands inside the strict bound
  everywhere (mesh axis max 0.13–0.69 mV), so the residual is the
  **N7 power estimator**, not the engine. See `docs/results.md`.
- Remaining: nothing structural — E8's engine is complete and its
  mesh/solver axis verified; tighter drop agreement tracks the N7
  estimator, not the solver.
- Sizing: ~4–6 tickets (M).

### E9. Reporting / usability surface

- In: existing `sta` engine + JSON. Out: actionable reports — no new
  timing semantics.
- Source: `docs/history/run-2026-11-22-large-design.md` (first
  large-design exercise, ~132k cells, multiple async domains).
- Items in value order:
  1. `sta --paths N` / `critical_paths` JSON — worst-N
     startpoint→endpoint traces with cell/pin names and per-arc
     delays (the blocker for acting on a deep-slack outlier).
  2. Per-clock-domain WNS/TNS/violation summary.
  3. Slack histogram over endpoints.
  4. `--top` ergonomics — suffix match on the hierarchy separator /
     single-top auto-accept instead of the exact `$paramod$…` name.
  5. SDC examples doc — single clock, N async clocks +
     `set_clock_groups`, FF clock-enable gating.
- Sizing: ~3 small tickets (`ux-sta-paths`, `ux-sta-summary`,
  `ux-top-sdc`).

### E6/E7 — authorized 2026-11-23

- **E6 detailed routing** is a TritonRoute-class build (LEF `VIA`/`SPACING`
  and DEF `VIAS`/`NONDEFAULTRULES` parse, signal-net wire storage, DRC-aware
  maze routing) — ~15+ tickets (XL), an order of magnitude beyond any prior
  stage. The README's out-of-scope line needs updating when it lands. The
  delegation path is measured, not hypothetical: P-GR showed `read_guides`
  + `detailed_route` on a foreign guide file produces a clean routed DEF
  (0 DRC) on gcd, so our guides already drive OpenROAD's own DRT — native
  E6 is the replacement, not the enabler. Suggested first ticket:
  reference generation (`detailed_route` on our guide + Magic/OpenROAD
  DRC goldens on gcd), matching the per-stage pattern.
- Progress 2026-11-23 (ticket `e6-drt-ref`): delegation reference
  committed — `tests/designs/generate_drt_ref.sh <design>` runs the
  baseline (`global_route` → `detailed_route`) and the delegated
  variant (`read_guides <d>.ours.guide` → `detailed_route`) in the
  pinned image and emits `<d>.drt.reference.json`. Both designs route
  0-DRC on both variants; the baseline DEF is byte-identical to the
  prior `<d>.routed.def` on gcd **and** picorv32. Delegated deltas:
  gcd −1.76% wirelength / +10 vias / 0 antenna; picorv32 −2.11%
  wirelength / +1,279 vias / antenna 38 → 30. JSON byte-reproduces on
  rerun outside the `timing` block. See `docs/results.md` 2026-11-23.
- Progress 2026-11-23 (`v0.12.1`, ticket `e6-drt-tracks`): the on-grid
  substrate landed — LEF `TRACKS` parse into `RoutingLayer.tracks_{x,y}`,
  `drt::tracks` merges the DEF `TRACKS` grids (sky130's techlef declares
  none — grid is DEF-sourced) with named preferred-axis `TrackGap`s, and
  `drt::access` enumerates candidate access points per signal-net member
  pin — census green on both designs (off_track=0, no_shapes=0), fail-
  proofs recorded. Next tickets: the maze/grid graph this substrate
  feeds (this ticket is substrate-only — no routing, vias, TA, or
  DRC-aware pruning).
- **E7 DRC** has two entry shapes — paired with E6 (0 violations on gcd
  under Magic `drc style drc(full)`, which P-DRC confirmed reads foreign
  routed DEFs in this image), or independently as a LEF-rule subset
  checker on the committed `gcd.routed.def`/`picorv32.routed.def`
  matching Magic's per-rule `drc listall why` counts exactly (~4 tickets
  as a subset checker). The subset-checker shape does not depend on E6
  and can run first.
- **`e7-drc-ref` landed 2026-11-23** (reference-only): committed
  `tests/designs/generate_drc_ref.sh` + `<d>.drc.reference.json` on
  gcd/picorv32 — Magic 8.3.623 `drc(full)` counts, `check_antennas`,
  the DRT report, a `met2.2` sanity control, and the 278-statement deck
  taxonomy (`docs/e7-drc.md`). Two corrections vs the P-DRC scoping
  probe: the clean deck is **not** zero (305 merged nwell violations
  inside the cell masters on gcd — the probe counted before
  `drc catchup`), and `drc listall why` bbox fragments ≠ `drc count
  total` merged regions.
- **`e7-drc-geom` landed 2026-11-23** (`v0.9.0`): the LEF+DEF geometry
  substrate + first rule family — `NETS` routed-geometry retention
  (square-cap wires, `( x y 0 )` points, `NONDEFAULTRULES` width
  overrides), `drc::geom` per-layer shape assembly (signal/special
  wires, via cut+metal, placed macro pin/obs, io ports), and
  `drc::width` covering the 13 `lef_expressible` width rules — **0
  violations on both routed DEFs** joined to the Magic goldens by
  rule-name template; `NEW met1 100` mutation flags exactly `met1.1`;
  `<`→`<=` fail-proof recorded in `docs/results.md`.
- **`e7-drc-spacing` landed 2026-11-23** (`v0.10.0`): the spacing
  family — LEF `SPACING`/`SPACINGTABLE` retention, `ShapeRect.group`
  via-instance ids, union-find connectivity islands (same-net touching
  merges; via cuts chain layers through the instance's metals), and
  `drc::spacing` covering all 20 `lef_expressible` spacing rules
  (same-layer min on li/mcon/via*/met1–5 + the met1–4 wide-metal
  pairs) — **0 violations on both routed DEFs**; the committed met2
  probe mutation flags exactly `met2.2`; merge-disable fail-proof
  recorded. Pair enumeration uses `LayerShapes::for_each_pair_within`
  (xl-sweep) after per-rect probes proved O(n²) on rail-poisoned
  `max_xh` prefixes.
- **`e7-drc-enc-area` landed 2026-11-23** (`v0.11.0`): the subset
  checker completes the `lef_expressible` set — LEF
  `ENCLOSURE {BELOW|ABOVE}` / `AREA` / `MINENCLOSEDAREA` retention,
  `drc::enclosure` (9 cut-overlap rules, symmetric min/max pair
  reading over the same-net metal union), `drc::union` +
  `drc::area` (6 min-area + 2 min-hole-area rules over net-agnostic
  merged polygons, OBS void discard) — **0 violations on both routed
  DEFs across all 50 unique rule ids**. Mutations: shrunk `via2_3`
  enclosure + moved covering stripe → exactly 5 `via.5a`; a `RECT`
  met1 stub → exactly `met1.6`. Three fail-proofs recorded.
- **`e7-drc-e2e` landed 2026-11-23** (`v0.12.0`): `grus-eda drc` CLI,
  `drc::report` unified subset report, full-set gate vs
  `magic_per_rule` (50/50 rules, `ours == magic` at 0 on both
  designs), honest claim-boundary output on every run; E7 stage gate
  passed — the `magic_only` deck remainder stays out of scope.
