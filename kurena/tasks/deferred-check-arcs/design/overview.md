# deferred-check-arcs — Overview

## Goal

Resolve the last counted `recognized_check_arcs` family: `non_seq_setup_*`/
`non_seq_hold_*`, `three_state_*`, `preserve` (and re-confirm `Clear`/`Preset`
stay count-only). Each type's OpenSTA 3.0.16 behaviour is measured on a new
hand-written fixture (`tests/designs/sta_edge/check_arcs/`); whatever OpenSTA
evaluates gets implemented and STRICT-golden'd, whatever it ignores stays
counted with the measurement as its explicit record.

## Measured library census (sky130_fd_sc_hd__tt_025C_1v80, re-counted for this ticket)

Per-cell-family counts, drive-strength suffixes stripped; the arcs sit where
the check-arcs Q9 census and async-arc-probe census left them — this table is
the acceptance's "lib census recorded" item, verified against the pinned lib
under `~/.ciel`:

| arc type | cells | placement |
|---|---|---|
| `non_seq_setup_rising` + `non_seq_hold_rising` (12 arcs each type) | `dfbb[n/p]_{1,2}`, `sdfbb[n/p]_1` | timing groups on `RESET_B` *and* `SET_B`, each `related_pin` = the other pin; `rise_constraint` only, `vio_3_3_1` template (var1 `related_pin_transition`, var2 `constrained_pin_transition`) |
| `three_state_enable` + `three_state_disable` (13 each) | `ebufn_{1,2,4,8}`, `einvn_{0,1,2,4,8}`, `einvp_{1,2,4,8}` | timing groups on output `Z`, `related_pin` = `TE_B` (`ebufn`/`einvn`) or `TE` (`einvp`); real `cell_rise`/`cell_fall` `del_1_7_7` delay tables + scalar-0 transition tables — they *look like* delay arcs |
| `preserve` | — | **zero occurrences in the lib**; nothing to fixture or probe, stays counted with this census as its record |

Fixture-relevant cell details:

- `dfbbp`/`dfbbn` also carry `clear`/`preset` arcs and rising(`dfbbp`)/
  falling(`dfbbn`, `CLK_N`) recovery/removal on both `SET_B` and `RESET_B` —
  one cell exercises every remaining deferred type plus the live checks.
- `ebufn_1` `Z`: `function : "(A)"`, `three_state : "(TE_B)"` (hi-Z when
  `TE_B`=1 → enable active-low). `einvn_1`: `(!A)`, same enable. `einvp_1`:
  `(!A)`, `three_state : "(TE')"` (enable active-high). All three also have an
  ordinary `A -> Z` combinational arc, so `Z` is timed even if OpenSTA ignores
  the enable arcs.
- `non_seq` senses are odd: `non_seq_setup_rising` `rise_constraint` values
  are *negative* (−0.10 … −0.33 on `vio_3_3_1`) — a data-check whose reference
  edge may precede the constrained one.

## Prior measurements this ticket builds on

- `Clear`/`Preset`: OpenSTA 3.0.16 does not propagate them (`docs/results.md`
  2026-11-21 R6 — `u_a*/Q` arrivals carry no clock tag, `find_timing_paths
  -to u_d*/D` = 0, disable-`CLK->Q` leaves `u_ff2/D` pathless). This ticket
  does not re-litigate propagation; it pins that they stay in
  `recognized_check_arcs` on a fixture that instantiates them.
- `non_seq_*` on `dfbb*`: the same entry's Q4 shows `report_check_types`
  (default) and `report_checks -to` produce no non_seq rows — **but**
  `u_fbp/RESET_B`/`SET_B` were already endpoints via recovery/removal, and
  `report_checks -to` prints one worst row per pin, so an extra data-check
  row at the same pin would have been invisible. The dedicated surfaces
  (`-data_setup`/`-data_hold`-style flags, `find_timing_paths -path_delay`
  variants, multi-row endpoint queries) were never tried — that is this
  probe's Q1.

## What exists today (code)

- `TimingType::{NonSeq*, ThreeState*, Preserve, Clear, Preset}` all hit
  `is_deferred_check()` (`lib_db.rs:57-70`) → counted per-type in
  `SkipReport::recognized_check_arcs` at `analysis.rs:1240-1245`, exempt
  from `--strict` via `is_clean` (`analysis.rs:490-516`).
- `parse_timing_type` already maps every `timing_type` string
  (`liberty.rs:238-245`); `LibTimingArc` already carries
  `rise_constraint`/`fall_constraint` (non_seq) and
  `cell_rise`/`cell_fall` (three_state) tables — no reader change needed for
  either branch.
- The `pulse_width_checks` family (`analysis.rs:820-832`,
  `eval_pulse_width_checks` `analysis.rs:5170`, golden section collector in
  `min_pulse_width/generate_ref.sh`) is the template for any new "separate
  report family" the probe may uncover for `non_seq_*` (data checks).
- `compare_sta_edge` (`analysis.rs:9257`) already compares endpoints, WNS/TNS,
  violation counts and `pulse_width` rows at `Tol::STRICT`; a new optional
  golden section plugs in the same way.

## Decision structure (per arc type, independent branches)

- **Ignores**: stays in `is_deferred_check`, keeps counting in
  `recognized_check_arcs`. Explicit record = this ticket's committed probe
  transcript + the pinned skip map in the new fixture's test + `results.md`/
  `roadmap.md` notes.
- **Evaluated as a check/report family** (plausible for `non_seq_*` →
  OpenSTA "data checks"): implement per `api.md` §B2, golden-covered at
  STRICT.
- **Propagated as a delay arc** (plausible for `three_state_*` — the tables
  are delay tables): promote to `graph.edges` per `api.md` §B1, STRICT
  golden on `u_cap*/D`.
- `preserve` is a census-only item (no arcs exist). `Clear`/`Preset` stay
  deferred on the R6 evidence; the fixture instantiates them so the skip map
  proves they did not silently gain evaluation.

## Files

- `tests/designs/sta_edge/check_arcs/` — new fixture: `check_arcs.def`,
  `ideal.sdc`, `propagated.sdc`, `te_slow.sdc`, `generate_ref.sh`,
  `check_arcs.reference.json`, `probe_check_arcs_{ideal,propagated,te_slow}.txt`
- `crates/grus-core/src/sta/analysis.rs` — new tests always; data-check
  family or edge promotion only if measured
- `crates/grus-core/src/lib_db.rs` — only if a type leaves
  `is_deferred_check()` (measured)
- `docs/results.md` — dated probe + comparison entry
- `docs/roadmap.md` — N3 deferred-note update
- `docs/e3-sta.md` — the "other non-delay arcs are deferred" line
- `README.md` — skip paragraph (deferred list)
- `tests/designs/README.md` — sta_edge fixture line
- `Cargo.toml`, `Cargo.lock` — `0.2.10` → `0.2.11`
- `kurena/tasks/deferred-check-arcs/` — spec + these design docs
