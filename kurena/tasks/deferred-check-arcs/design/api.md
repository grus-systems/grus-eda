# deferred-check-arcs — Rust-side design

Branches are per arc type and chosen by the probe — nothing below is final
until the measured verdicts land. The always-true parts (new fixture tests,
the pinned skip map) live in §A; each type's evaluated variant is a separate
§B. `Clear`/`Preset` stay in `is_deferred_check()` under every branch (R6
evidence); they appear only in the pinned map.

## A. Always: fixture tests + pinned skip record

- `test_sta_edge_check_arcs_{ideal,propagated,te_slow}` —
  `compare_sta_edge("check_arcs", "check_arcs/check_arcs.def", <sdc>, None,
  <key>, Tol::STRICT)`. Under the all-deferred outcome the golden's endpoint
  set is the ordinary one (`u_fbp`/`u_fbn` D+SET_B+RESET_B recovery/removal,
  `u_cap*/D`, `u_ff/D`, `y`) and `te_slow`'s `u_cap*/D` values equal
  `ideal`'s (measured non-propagation); under a propagated-`three_state`
  outcome `te_slow` is the discriminator that must differ (and still match
  STRICT, since the golden records OpenSTA's numbers either way).
- `test_check_arcs_deferred_counts` — run the fixture, assert
  `report.skipped.recognized_check_arcs` equals the literal map measured on
  the first run (predicted: `NonSeqSetupRising:4, NonSeqHoldRising:4,
  ThreeStateEnable:3, ThreeStateDisable:3, Clear:…, Preset:…` — the dfbb
  clear/preset counts come from the run, not the census). This is the
  acceptance's "stays counted with an explicit record": it fails loudly if a
  type silently leaves or enters the deferred bucket.
- Fail-proof (AGENTS.md §7.7): for the all-deferred outcome, temporarily
  drop `NonSeq*` from `is_deferred_check()` → the pinned test goes red
  (`unsupported_arcs` gains them); restore. For any evaluated outcome, break
  one measured rule and confirm the STRICT compare goes red.

## B1. `three_state_*` measured as propagating delay arcs

Smallest change consistent with the measured rule:

- Remove `Self::ThreeStateEnable | Self::ThreeStateDisable` (or whichever
  type the probe shows propagating — enable-only is possible) from
  `is_deferred_check()` in `lib_db.rs`; doc comment updated.
- In `build_graph` (`analysis.rs:1282-1286`) extend the `Combinational`
  arm to include the propagating type(s) → `graph.edges.push(ArcEdge {
  src, dst, lib_arc })`. `src` = `TE_B`/`TE` pin, `dst` = `Z`; existing eval
  applies `timing_sense` and the `cell_rise`/`cell_fall`/`del_1_7_7`
  tables. If the probe shows the disable arc does *not* propagate, keep it
  deferred — do not propagate both from symmetry.
- Check first whether OpenSTA's polarity mapping matches our
  `timing_sense` handling (`three_state_enable` `negative_unate` on
  `ebufn`: does `TE_B` fall → `Z` rise?). The `z1`/`z2`/`z3` goldens pin
  whatever it does; if our sense-eval produces the wrong polarity the arc
  needs the measured remap recorded in `results.md`, not a guess.
- Scalar-0 `rise_transition`/`fall_transition` tables (the disable arc)
  flow through the existing slew path — if OpenSTA reports nonzero `Z`
  slew through an enable arc, the enable arc's tables decide it; verify
  against `max_slew_ns`/`min_slew_ns` golden fields at STRICT.
- Counting: promoted arcs leave `recognized_check_arcs`; the pinned map in
  §A shrinks accordingly (e.g. no `ThreeState*` keys if both propagate).
  `total_endpoints`/`arcs_skipped` unchanged in shape — edges are not
  skipped.

## B2. `non_seq_*` measured as evaluated checks (data checks)

Two measured sub-shapes:

- **B2a — they join the endpoint surface** (`find_timing_paths` /
  `report_checks` PathEnds): collect into `RegisterCheck`-style records in
  `build_graph` — `NonSeqCheck { pin: dst, ref_pin: src, setup_arc, hold_arc
  }` on `TimingGraph` — and emit `EndpointReport`s through
  `collect_endpoints` with the measured anchor (which edge of the *related*
  pin is the reference: `non_seq_setup_rising` checks the constrained pin
  against the related pin's **rising** arrival; the fixture's differing
  `rst_n`/`set_n` delays expose the anchor arithmetic), the measured corner
  (max/min of the related pin's data arrival), the constraint-table lookup
  (`vio_3_3_1`: index_1 = related-pin transition slew, index_2 =
  constrained-pin transition slew — verify against golden which slew each
  index takes), and the measured slack formula. `total_endpoints` grows per
  the measured row granularity.
- **B2b — a separate report family** (`report_check_types -data_setup/
  -data_hold` rows not in `find_timing_paths`): mirror
  `pulse_width_checks` — `StaReport::data_checks:
  Vec<DataCheckReport>` `{pin, ref_pin, kind (Setup|Hold), required_ns,
  arrival_ns, slack_ns}`, excluded from `endpoints`/WNS/TNS if that is what
  OpenSTA does (the probe measures whether data-check slack enters
  `worst_slack`); `merge_corner_reports` merges on `(pin, ref_pin, kind)`
  keeping smaller slack; golden `data_checks` section + `compare_sta_edge`
  block cloned from the `pulse_width` compare.
- Under either sub-shape `NonSeq*` leaves `is_deferred_check()`; the §A
  pinned map loses the `NonSeq*` keys.

## B3. Ignored (any type)

No `analysis.rs`/`lib_db.rs` change for that type. Its record is the §A
pinned map + the committed probe transcript + `results.md`/`roadmap.md`
notes. `preserve` is definitionally here (zero lib arcs — the census line is
its whole record).

## Notes for every evaluated branch

- `EndpointKind` (`analysis.rs:201-208`) gains a variant per new evaluated
  family (e.g. `DataCheck`) for `skipped_endpoints` when a check pin is
  unreached — same convention as `MinPulseWidth`.
- `expected_latch_skips` (`analysis.rs:7116`) needs no change: dfbb cells
  are flops (`is_latch` false), ebuf cells are combinational; deferred types
  are already excluded from the recount.
- `test_skip_report_counts_latch_arcs` expected maps are untouched — none
  of the loop's designs instantiate dfbb/ebuf cells.
- `--strict`/`is_clean` unchanged: promoted arcs stop counting (evaluated);
  ignored types stay exempt — that is the matching behaviour the probe
  proves, not a judgment call.
- CLI (`main.rs`): the summary line already prints the deferred counts via
  `nonzero()`; a new evaluated family gets one summary line like
  `pulse-width checks:` only if its golden surface is a separate family
  (B2b). No new flags.
- Existing goldens must stay byte-identical: no real design instantiates
  `dfbb*`/`sdfbb*`/`ebuf*`/`einv*` cells (census confirmed), so promoting a
  type cannot change any committed golden — verify by full-suite diff.
