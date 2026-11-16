# Ticket 16: a routed SPEF for picorv32 — E3.5 scale evidence and the coupling cross-check

## Why

Step 6 closed E3.4 and narrowed E3.5 (`docs/e3-sta.md` Section 5, `docs/results.md` step 6 part 3). Two
measurements are named as the follow-up, and both need the same input: a routed SPEF for picorv32, built the
way ticket 13 built gcd's.

1. **The E3.5 scale question.** On gcd, our delay engine agrees with OpenSTA to ~1 ps median on identical
   parasitics, and every placement estimator (ours and OpenROAD's) sits ~150 ps median from routed truth. The
   picorv32 wire-mode tolerances (SPEF arrival floor 6 ps, slack 40 ps) and the aes/jpeg/ibex failures say the
   error grows with scale; nobody has separated estimator error from engine error above gcd. Scoring picorv32's
   estimators against a routed golden does that.
2. **The coupling-rule cross-check.** Ticket 14's rule (full value on the first node, half on the second node
   in its owning net) is measured on gcd only (269 nets, 3,544 coupling entries). picorv32's routed SPEF has
   ~6,800 nets and ~77,000 coupling caps — the rule either holds at that scale or the residual table shows it.

Maintainer spike (M4, pinned container, 2026-11-15): the gcd step-5b sequence run by hand on
`picorv32.placed.def` completes in about 5 minutes; the detailed-route DRC report is **empty**; OpenRCX
extracts 6,811 nets, 41,603 RC segments and 77,561 coupling caps; `picorv32.rcx.spef` is 8.8 MB raw and
`picorv32.routed.def` 7.0 MB. So the gcd gates (fail on non-empty DRC) apply unchanged.

## What

### Generation (`tests/designs/picorv32/generate_ref.sh`)

- Add a step 5b after step 5, mirroring gcd's step 5b exactly: `set_routing_layers -signal met1-met5 -clock
  met3-met5`, `global_route -guide_file ... -congestion_iterations 100`, `detailed_route` with the DRC report
  written to `picorv32.drc.rpt`, fail the script if the report is non-empty, `write_def picorv32.routed.def`,
  then `define_process_corner -ext_model_index 0 X` and `extract_parasitics -ext_model_file
  /pdk/sky130A/libs.tech/openlane/rules.openrcx.sky130A.nom.calibre -lef_res`, `write_spef picorv32.rcx.spef`.
  Normalise the SPEF's `*DATE` line with the python3 heredoc and exactly-one guard from gcd's script (the
  pinned image has no `sed`/`awk`/`perl`; do not use them anywhere in the script's container steps).
- Add a step 9b that appends a fourth mode `routed_spef` to `picorv32.sta.reference.json`: a second OpenROAD
  process on `picorv32.placed.def` + `picorv32.rcx.spef`, mirroring gcd's step 9b. gcd factors the endpoint
  collection into a generated `collect_endpoints.tcl` sourced by both dump scripts; picorv32's proc is
  currently inline in the step-9 heredoc — factor it the same way so both processes share one implementation.
  The first three modes' collected values must not change (the maintainer deep-compares the regenerated
  golden against the committed SHA-recorded one).

### Tests (`crates/grus-core/src/sta/analysis.rs`)

- `test_picorv32_routed_spef_matches_opensta`: `compare_design_with_opensta("picorv32", "routed_spef",
  Rc::RoutedSpef, Tol::PICORV32_SPEF)` (both already exist from tickets 14/06-era work).
- `measure_picorv32_estimators_against_routed_truth`: the ticket-15 scoring pattern for picorv32, reusing the
  `Timing`/`score`/`print_scores` helpers in this test module. Configurations against the `routed_spef`
  golden: (a) ours, placement RC, exact tree (shipped); (b) ours, placed SPEF; (c) OpenSTA placed-SPEF golden;
  and the residual (d) ours, routed SPEF vs the routed golden. Measurement only: print the tables, assert
  endpoint presence/count only, no tolerance assertions. Add a picorv32 golden-pair loader next to
  `gcd_golden_pair`.

## Files

- `tests/designs/picorv32/generate_ref.sh`
- `.gitignore` (add `tests/designs/picorv32/picorv32.routed.def`, `tests/designs/picorv32/picorv32.rcx.spef`,
  `tests/designs/picorv32/picorv32.drc.rpt`)
- `crates/grus-core/src/sta/analysis.rs`

Not in scope: docs and `tests/designs/README.md` SHA entries (the maintainer writes them after generation),
any tolerance change, any `#[ignore]` change, the E1 congestion code, `sta/parasitics.rs` (the tree-builder
parameter from ticket 15 is already there).

## Acceptance

- `bash -n tests/designs/picorv32/generate_ref.sh` passes; the script's container steps use no
  `sed`/`awk`/`perl`.
- `cargo test --release -p grus-core --no-run` compiles; `cargo clippy --release -p grus-core --all-targets`
  is clean.
- The new tests fail loudly when the picorv32 PDK inputs or the regenerated golden are absent (no silent
  skip), per the repo rule.
- Report, with numbers: the endpoint count of the picorv32 golden's modes as far as this host can read them,
  and anything in the picorv32 script that does not mirror gcd cleanly. The maintainer generates the
  artifacts, runs the full suite, deep-compares the golden's first three modes, and records the tables and
  the pass/fail outcome in `docs/results.md`; if the routed test fails at `Tol::PICORV32_SPEF`, it is recorded
  and `#[ignore]`d with the measured table by the maintainer — tolerances are never widened to fit.
