# check-arcs — Implementation & verification steps

## Step 0 — Fixture skeleton + probe (acceptance #1)

1. Write `tests/designs/sta_edge/min_pulse_width/mpw.def` and the three
   SDCs per `fixture.md` (`ideal.sdc`, `propagated.sdc`, `viol.sdc`). Also
   a first `generate_ref.sh` cloned from `propagated_clock`'s — the probe
   heredoc can live inside it temporarily, or run as a one-off heredoc
   (name in `results.md` exactly what ran).
2. Start Colima if needed (`AGENTS.md` §2), run the probe Tcl inside
   `ghcr.io/librelane/librelane:3.0.16` for `probe.md` Q1–Q9 on each SDC.
3. Write the dated `docs/results.md` entry: the command/fields answer,
   the measured rules (constraint mapping, corner rule, slew arg,
   uncertainty/derate, wrap pairing), the WNS-inclusion verdict, the
   deferred-type census, and a raw `report_check_types` excerpt. This is
   a committable unit on its own.

## Step 1 — Golden generator + golden

1. Finalize `generate_ref.sh` per the measured surface
   (`find_timing_paths -path_delay min_pulse_width` PathEnds if they
   exist; else `report_check_types -min_pulse_width` text parsed in Tcl).
   Emit the `pulse_width` sections per `fixture.md` §3.
2. Run the committed script as written from the repo root; `git diff` the
   golden is all-new. Inspect the rows (jq/python3, not by eye):
   - `ideal`: every row MET, `actual` = 2.5 (P/2), `required` ≈ the lib
     table at slew 0.1.
   - `viol`: every row VIOLATED, margin per `fixture.md` §2 contingency.
   - `propagated`: `actual` differs across `c1`/`c2`/`c3` pins (asymmetry
     + inversion wrap); `c2` rows prove the `+period` wrap rule.
3. Record the golden numbers needed by `api.md` in the `results.md` entry
   (which candidates matched — e.g. "actual = arr_max[close] −
   arr_max[open]"; "rise_constraint → high pulse").

## Step 2 — Implement

Per `api.md`, smallest coherent order:

1. `PulseWidthCheck`/`TimingGraph::pulse_width_checks` + the
   `MinPulseWidth` arm in `build_graph`; drop `MinPulseWidth` from
   `is_deferred_check` (doc comment updated).
2. Seeding: add the check pins to the `register_clock_nets`,
   `consumer_nets`, `consumers`/`clock_pins` chains.
3. `PulseEdge`/`PulseWidthCheckReport`/`EndpointKind::MinPulseWidth`/
   `StaReport::pulse_width_checks` + `merge_corner_reports` merge.
4. `eval_pulse_width_checks` with the *measured* formulas; wire into
   `collect_endpoints` and `run_sta_partitioned`; `total_endpoints`
   accounting per the measured row granularity.
5. CLI: the one-line pulse-check summary in `main.rs`.
6. Update `expected_latch_skips` and `test_skip_report_counts_latch_arcs`
   expectations; new `test_sta_edge_min_pulse_width_{ideal,propagated,
   viol}` tests at `Tol::STRICT`.

## Step 3 — Verify

- `cargo test -p grus-core min_pulse_width` (new tests) then
  `cargo test --workspace` — STRICT (rel 1%, abs 5 ps), never loosened.
- Regression watch: every real-design skip summary now shows
  `recognized_check_arcs` *without* `MinPulseWidth` (ibex →
  `Clear=1655, Preset=9`); `pulse_width_checks` rows appear on all five
  designs. Any diff to `endpoints`/WNS/TNS of existing goldens is a bug —
  the new list is additive only.
- Fail-proof (AGENTS.md §7.7): break one measured rule on purpose —
  swap the rise/fall constraint mapping or use the other corner arrival —
  confirm `viol` (mapping/corner-sensitive rows) and `propagated`
  (spread-sensitive rows) go red; restore and re-verify.
- `cargo clippy --workspace --all-targets -- -D warnings`.

## Step 4 — Docs, version, commits

1. `docs/results.md`: complete the dated entry — surface command and
   fields, measured rules, violation observed (`viol` row excerpt), our
   values vs golden, fail-proof result, what actually ran.
2. `docs/roadmap.md` N3: mark `min_pulse_width` evaluated (new version);
   the deferred note keeps `non_seq_*`/`three_state_*`/`preserve`/
   `Clear`/`Preset` counted with the census result recorded.
3. `docs/e3-sta.md` (~line 220): replace "min_pulse_width and other
   non-delay arcs are ignored" with the evaluated-check note;
   `README.md` skip paragraph: drop `min_pulse_width` from the deferred
   list.
4. `Cargo.toml` `0.1.92` → `0.1.93` (+ `Cargo.lock`) in the same commit as
   the functional change.
5. Commits, split per AGENTS.md: (a) fixture + probe results entry;
   (b) implementation + docs + version. Conventional Commits
   (`feat:`/`add:`-style per repo log), agent trailer.

## Risk checklist

- **`find_timing_paths -path_delay min_pulse_width` unsupported** →
  golden falls back to parsing `report_check_types` text; still
  container-emitted, still honest. If neither gives per-check rows, the
  probe enumerates `info commands *pulse*` for a programmatic surface
  before giving up.
- **Worst-row-only surface**: if OpenSTA reports only the worst row per
  pin/type, the golden schema and `total_endpoints` counting adjust to
  the measured granularity (and our report emits the same set) — record
  it, don't extrapolate.
- **`viol.sdc` thin margin** → contingency in `fixture.md` §2.
- **`src != dst` on some arc** → record in results.md; `pin = dst`.
- **Unreached propagated clock pin carrying a check arc** →
  `unreached_endpoints` + `NoClock` skip (strict-safe), not silent.
- **Container/PDK**: Colima running, PDK at `~/.ciel` (`PDK_ROOT`
  override supported).
