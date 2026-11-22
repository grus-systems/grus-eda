# Goal

Measure-first ticket for R6 (asynchronous arcs) plus a stale-
roadmap correction — R3 (propagated-clock hold) and R4 (clock
gating) already have STRICT golden tests and should be marked
done.

## Work

1. **Probe OpenSTA** (LibreLane container, committed script +
   artifacts in a fixture dir):
   - does a `Clear`/`Preset` arc (`RESET_B -> Q`) propagate an
     arrival to `Q` in OpenSTA 3.0.16 — i.e. does a downstream
     endpoint reached *through* the async arc get timed, or
     does the reset pin only host recovery/removal checks?
     (roadmap R5 note says OpenSTA does not propagate them —
     verify on a fixture that distinguishes "no arrival" from
     "arrival present")
   - falling recovery/removal: does a cell's `removal_falling`
     / `recovery_falling` (or negative-edge async pin) produce
     checks, and on which edge pair? `sky130_fd_sc_hd` async
     cells are `dfrtp`/`dfrtn`-family — check which variants
     exist in the lib and cover the ones that do.
   - what does OpenSTA report for an endpoint *only* reachable
     via an async-arc path (unreached vs timed)?
2. **Our side today**: `Clear`/`Preset` arcs land in
   `latch_arcs_dropped`/`recognized_check_arcs` counts — run
   the probe fixture through `grus-eda sta` and record where
   we diverge (counted vs evaluated).
3. **Roadmap/doc fix** (same commit, no probe needed): R3 and
   R4 entries marked DONE with the evidence — R3:
   `test_sta_edge_hold_violation_propagated` at STRICT +
   `hold_violations` in the CLI summary; R4:
   `test_sta_edge_clock_gate_{ideal,propagated}` covering
   and2/or2/nand2/nor2 + `dlclkp`. Any residual sub-items (e.g.
   R4 gate-edge cases already covered per the docstring) noted
   accurately.
4. **Deliverable**: `docs/results.md` dated entry with the
   probe numbers + verbatim, and either a sequenced
   implementation plan in `docs/roadmap.md`/design docs (if
   OpenSTA does propagate and the gap is real) or a recorded
   'already matching' close of R6. Version bump.

## Files

- `tests/designs/sta_edge/async_arcs/` (new fixture + probe
  script + committed artifacts — reuse `sta_edge/async_reset`
  if it already covers this; check first)
- `docs/roadmap.md`, `docs/results.md`
- `kurena/tasks/async-arc-probe/` (design docs)
- `Cargo.toml` / `Cargo.lock`
