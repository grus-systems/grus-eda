# async-latch-delay — Overview

## Goal

Close the measurable half of `docs/path-delays.md` §5 item 1: does OpenSTA
honour `set_max_delay` / `set_min_delay` on the two endpoint kinds our delay
coverage counts in `unsupported_path_delay_endpoints` —

- **async-check pins**: recovery/removal checks on `RESET_B`-type pins
  (`ExceptionEndpointKind::Check { latch: false }` whose `RegisterCheck`
  carries `recovery_arc`/`removal_arc` instead of `setup_arc`/`hold_arc`),
  and
- **latch-D pins** (`Check { latch: true }`, borrow semantics).

Measure first, reconcile second: whatever OpenSTA applies gets implemented
and golden-covered at STRICT; whatever it ignores or rejects stays counted
with the measured evidence recorded. The literal two-clock-domain half of
item 1 stays open (multi-clock SDC unsupported); clock-gate enable
endpoints stay deferred and counted — they are out of this task's scope.

## What exists today

- `docs/path-delays.md` documents the measured FF/output-port delay-check
  semantics and marks the async/latch probe + two-clock fixture as open
  (§5 item 1, `docs/path-delays.md:488-492`).
- Delay composition lives in `run_sta_partitioned`
  (`analysis.rs:3194-3768`). The support gate is `delay_supported`
  (`analysis.rs:3426-3432`):
  - `OutputPort` → supported (edge-referenced formula, `mdo_*`/`pmo_*`),
  - `Check { latch: false }` → supported only when the check carries
    `setup_arc`/`hold_arc` — async recovery/removal pins fail this
    predicate and land in `unsupported_pd` (`analysis.rs:3433-3437`),
  - `Check { latch: true }` and `ClockGate` → `false`, counted.
- The composition code itself is already endpoint-arc generic: the max
  side reads `check.late_arc()` = `setup_arc.or(recovery_arc)` and
  `check.clk` (`analysis.rs:3456-3521`), the min side adds N to the
  pre-MCP hold required time (`analysis.rs:3536-3546`, which for an async
  pin is the removal required time). An "applied" verdict on async pins
  may reduce to widening that predicate — pending measurement.
- Fixtures with golden exist: `tests/designs/sta_edge/async_reset/`
  (`u_ff0/RESET_B` endpoint, ideal + propagated modes) and
  `tests/designs/sta_edge/latch_borrow/` (11 latch-D endpoints covering
  every sky130 latch shape — `u_lp/D` actively borrows, `u_ln/D` does
  not, `u_iso/D` sits exactly at borrow, plus four `RESET_B` latch-async
  endpoints). Both fixtures' `generate_ref.sh` scripts emit the shared
  endpoint JSON schema (`find_timing_paths` + PathEnd accessors), the
  same schema the delay-check scenarios need — PathEnds for delay checks
  report through the same accessors (proven by every committed `md_*`
  golden).
- Selector machinery is endpoint-kind agnostic: `-to [get_pins …]`,
  `-to [get_cells …]` (all endpoint pins of the instance — `get_cells
  u_ff0` covers `u_ff0/D` *and* `u_ff0/RESET_B`), `-to [all_registers]`
  (FF + latch cells), `-from [get_ports …]` (port-seed launches; the
  async pin's launch is the `rst_n` port seed, the latch-D launch is the
  `p_*` port seed).

## Verdict tree

Per endpoint kind (and per side, max/min, if they differ):

- **applied** → the reported `data_required_time`/`slack` move off the
  baseline. Decompose the required time into the candidate rules listed
  in `probe.md`, record the measured rule in `docs/results.md`, implement
  it in the exception model (`api.md`), add golden scenarios at STRICT.
- **ignored** → reported values byte-identical to the baseline. Keep
  `unsupported_path_delay_endpoints` counting the kind (our behaviour —
  keep the clock check — already matches OpenSTA); commit the scenario
  SDCs anyway so the evidence is reproducible; record the verdict.
- **errored** → `read_sdc`/eval raises inside OpenSTA. Keep the counted
  skip; record the error verbatim in `docs/results.md`; the scenario SDC
  stays committed for reproduction but produces no golden section
  (golden = `{"error": …}` only if the harness needs it — see
  `fixture.md`).

## Files

- `crates/grus-core/src/sta/analysis.rs` (only if a kind is applied)
- `tests/designs/sta_edge/async_reset/`
- `tests/designs/sta_edge/latch_borrow/`
- `docs/path-delays.md`
- `docs/results.md`
- `docs/timing-exceptions.md` (only if the skip surface changes —
  expected unchanged: its out-of-scope list names multicycle endpoint
  limits, not delay endpoint kinds)
- `Cargo.toml`
- `Cargo.lock`
