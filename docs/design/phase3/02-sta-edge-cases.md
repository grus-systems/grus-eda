# Ticket 02 — STA edge cases vs OpenSTA

## Goal

`docs/e3-sta.md` Section 5 "Not scheduled" lists three E3 items that are
implemented (or deliberately skipped) but never compared against the reference
tool:

1. **SPEF resistor loops** — the reader builds a BFS spanning tree and drops
   self-loops and parallel resistors (`docs/e3-sta.md` E3.3 Stage 1.3);
   "implemented but untested against OpenSTA".
2. **`set_clock_uncertainty` on non-register endpoints** — ours is applied to
   register checks and clock-gate enables only, not output ports
   (`sta/analysis.rs` `collect_endpoints`); "gcd and picorv32 do not use it".
3. **Non-`dlxtn` latches** — `latch_enable` models only the `dlxtn` shape
   (setup/hold `*_rising` check on D whose related pin is the enable, opening
   half a period later). A `dlxtp` or otherwise-shaped latch is skipped with an
   `eprintln!` warning and its arcs are ignored.

Phase 3 validates each against OpenSTA on hand-written minimal fixtures and
records — or fixes — the divergence.

## Fixtures

New directory `tests/designs/sta_edge/` with one subdirectory per case. Each
case has a committed `generate_ref.sh` (same container route as the design
scripts) that emits `<case>.reference.json` via OpenSTA as written, plus the
fixture DEF/SDC(/SPEF). Fixtures are inputs and stay small (< 1 MB total);
goldens are emitted, never hand-written.

Fixture rules:

- Hand-written DEF must be readable by both OpenROAD `read_def` and our
  `parse_def`; start from the smallest committed DEF (`gcd.placed.def` is too
  big to hand-edit — write a minimal one: `PINS`, `NETS`, `COMPONENTS`, one
  `ROWS`/site block, sky130_fd_sc_hd cells only). Verify OpenROAD accepts it
  before generating goldens.
- SDC uses only the subset `sta/sdc.rs` parses; plain
  `set_clock_uncertainty <ns>` (no `-setup`/`-hold`/clock-list forms — those
  are rejected today; if a case needs them, record the parser gap instead of
  extending silently).
- Liberty/LEF/SDC/PTT: `sky130_fd_sc_hd__tt_025C_1v80.lib`,
  `sky130_fd_sc_hd__nom.tlef`, `sky130_fd_sc_hd.lef`, same as every other
  fixture.

### Case A — `resistor_loop`

- Minimal DEF: one register (`dfxtp`) clocked by a port clock, one combinational
  path (`buf`/`inv`) feeding its D, one output port sink so both register and
  port endpoints exist. Reuse the same DEF for cases B and C where possible.
- SPEF: hand-written `*D_NET` for the data net only — driver pin node, two
  intermediate nodes, resistors forming a loop (e.g. `n1--R--n2` plus a second
  `n1--R--n2` parallel or a `n2--R--n1` cycle), `*CAP` ground caps on nodes and
  pin caps. Other nets absent from the SPEF (verify first that OpenSTA treats
  SPEF-absent nets as zero parasitics; if it errors, emit a full SPEF with
  OpenROAD `estimate_parasitics`/`extract_parasitics` and inject the loop into
  one net instead — record which route was taken).
- Golden: per-endpoint arrival/required/slack/slew like
  `collect_endpoints_json`, plus `worst_slack`/`total_negative_slack` for
  `max`/`min`.
- Our side: `run_sta` with `NetParasitics` from the SPEF; compare per endpoint.

### Case B — `clock_uncertainty`

- Same minimal DEF plus `set_output_delay` on the output port and
  `set_clock_uncertainty 0.2` (plain form) in the SDC.
- Golden from OpenSTA; compare register endpoints **and** the output-port
  endpoint.
- Expected finding (to be measured, not assumed): OpenSTA applies clock
  uncertainty to the output-port required time; ours does not
  (`collect_endpoints` output branch has no `uncertainty_ns` term). If
  confirmed, the fix is to add the same `± uncertainty` terms to the output
  `req_max`/`req_min` — a real bug fix with the golden as proof. If OpenSTA
  also exempts ports, record that ours matches.

### Case C — `non_dlxtn_latch`

- Minimal DEF with one `sky130_fd_sc_hd__dlxtp_1` (positive latch; `dlxtn`
  is already covered by ibex `_21829_`) plus a `dfxtp` sink, SDC with the
  latch enable derived from the clock (however OpenSTA/the Liberty arcs
  require — measure what `latch_enable` sees first).
- Golden from OpenSTA (it models the latch's true arcs).
- Measure what ours does: warning text, whether the latch's arcs are dropped,
  which endpoints go unreached (`SkipReport`). Outcomes:
  - If ours skips and OpenSTA times the path: record the divergence and the
    missing-endpoint count; extending the latch model is a separate design
    decision (do not bolt it on here).
  - If `latch_enable` happens to match `dlxtp`'s arc shape, compare endpoint
    values directly.

## Acceptance

- `tests/designs/sta_edge/generate_ref.sh` (or per-case scripts) rerun as
  written reproduces the goldens bit-identically.
- One Rust test per case in `sta::analysis::tests` (or a new
  `sta/edge_cases.rs` test module) comparing our report to the golden with the
  standard `max(5 ps, 1%)` criterion where the models are meant to agree; a
  case that diverges by design is `#[ignore]`d with the measured numbers and
  root cause in the attribute, per the jpeg-SPEF precedent.
- `docs/results.md` entry per case: date, fixture, OpenSTA value, our value,
  difference, runtime; failed/divergent runs kept with root cause.
- `docs/e3-sta.md` Section 5 "Not scheduled" updated: each item marked
  verified / diverged / still open with a pointer to the fixture.
- `cargo test --release -p grus-core sta::` and `cargo clippy --all-targets
  -- -D warnings` clean.
