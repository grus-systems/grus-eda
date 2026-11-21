# async-arc-probe — Overview

## Goal

A measure-first ticket for roadmap R6 (asynchronous arcs) plus a stale-roadmap
correction: R3 (propagated-clock hold) and R4 (clock gating) already have STRICT
goldens and are only missing their DONE markers. No propagation model is
implemented unless the probe proves OpenSTA propagates `Clear`/`Preset` arrivals
— the prior measurement says it does not.

## Why a new fixture (the goal's "check first" verdict)

`sta_edge/async_reset` (`dfrtp_1`) and `sta_edge/async_fall` (`dfrtn_1`) already
exist and pass STRICT, and `docs/results.md` 2026-11-17 ("Roadmap R6") records
that `find_timing_paths -from rst_n -through u_ff0/Q` is empty and
`report_arrival u_ff0/Q` shows only clock-derived arrival. That measurement
cannot distinguish "no arrival through the async arc" from "arrival present",
because `u_ff0/Q` also carries a `CLK -> Q` arrival: if OpenSTA *did* merge an
async-arc arrival into Q, the reported value would still look clock-derived.
The new fixture `sta_edge/async_arcs` carries flops whose `CLK` pin is driven
by a port with no `create_clock`, so the *only* way `Q`/`Q_N` (and the
downstream `u_d*/D` endpoint) can carry an arrival is through the `Clear`/
`Preset` arc. That is the "endpoint only reachable via an async-arc path"
the goal asks about.

The existing fixtures also cover only `RESET_B`-clear cells. The census below
shows `SET_B`-only and dual-pin cells exist and are uncovered.

## Liberty census (sky130_fd_sc_hd__tt_025C_1v80, measured 2026-11-21)

Counted per cell family (drive-strength suffix stripped), from the pinned
sky130A lib under `~/.ciel`:

- `recovery_rising` + `removal_rising` (12): `dfbbp`, `dfrbp`, `dfrtp`, `dfsbp`,
  `dfstp`, `dlrbn`, `dlrtn`, `sdfbbp`, `sdfrbp`, `sdfrtp`, `sdfsbp`, `sdfstp`
- `recovery_falling` + `removal_falling` (6): `dfbbn`, `dfrtn`, `dlrbp`,
  `dlrtp`, `sdfbbn`, `sdfrtn`
- `clear` arcs (16): `dfbb[n/p]`, `dfrbp`, `dfrt[n/p]`, `dfsbp`, `dlrb[n/p]`,
  `dlrt[n/p]`, `sdfbb[n/p]`, `sdfrbp`, `sdfrt[n/p]`, `sdfsbp`
- `preset` arcs (12): `dfbb[n/p]`, `dfrbp`, `dfsbp`, `dfstp`, `dlrb[n/p]`,
  `sdfbb[n/p]`, `sdfrbp`, `sdfsbp`, `sdfstp`
- `non_seq_setup_rising` + `non_seq_hold_rising` (4): `dfbb[n/p]`, `sdfbb[n/p]`
  (the `SET_B`/`RESET_B` pair checked against each other)

Notes that shape the fixture:

- There is no `dfrbn`/`dfsbn`: falling-check flops are `dfrtn`, `dfbbn`,
  `sdfrtn`, `sdfbbn`.
- `dfrbp` has only `RESET_B` yet carries both `clear` (`RESET_B -> Q`) and
  `preset` (`RESET_B -> Q_N`); `dfsbp` mirrors it on `SET_B`.
- `dfstp` is set-only: `preset` arc + rising recovery/removal on `SET_B`.
- Falling-check latches are the *positive* ones (`dlrbp`/`dlrtp` — their gate
  closes on the falling edge); rising-check latches are `dlrbn`/`dlrtn`.
- `sdf*` cells are scan supersets (extra `SCD`/`SCE` pins) of the base shapes.

## Coverage already locked by STRICT goldens

| cell shape | check edge | fixture |
|---|---|---|
| `dfrtp` (reset, posedge) | rising | `async_reset` (ideal+propagated, 7 SDCs) |
| `dfrtn` (reset, negedge) | falling | `async_fall`, `async_fall_chain`, `clock_gate` |
| `dlrtn`/`dlrbn` (neg latch) | rising | `latch_borrow` (RESET_B endpoints) |
| `dlrtp`/`dlrbp` (pos latch) | falling | `latch_borrow` (RESET_B endpoints) |

Uncovered flop variants: `dfstp` (set-only), `dfbbp`/`dfbbn` (dual async pins
+ `non_seq_*`), `dfrbp`/`dfsbp` (single pin driving both arc types), `sdf*`
scan cells (same check shapes as base). The new fixture covers `dfstp`,
`dfbbp`, `dfbbn`, and `dfrbp` (as a propagation discriminator); `sdf*` is the
same check shape plus scan pins and stays a family-equivalence note in
`results.md`.

## Our side today (predicted — record actuals)

- `Clear`/`Preset` arcs hit the `t.is_deferred_check()` arm in `build_graph`
  (`analysis.rs:1240`) and count in `SkipReport::recognized_check_arcs` on
  every cell kind — never `latch_arcs_dropped` (that arm is later, line 1276).
  They are `is_clean`-exempt (`analysis.rs:486-489`), so `--strict` ignores
  them.
- `recovery/removal` rising and falling are collected into `RegisterCheck`
  (`analysis.rs:1214-1225`) and evaluated — covered by the fixtures above.
- `non_seq_*` on `dfbb*` counts in `recognized_check_arcs`; whether OpenSTA
  reports those pins as endpoints is a probe question (Q4) — if it does, the
  endpoint-count comparison diverges and that is recorded, not hidden.
- An unclocked `u_ar`: its `LaunchArc` never fires, so `Q` is unreached and
  `u_ds/D` lands in `skipped_endpoints` with reason `no_path`
  (`related = u_ar/Q`); `u_ar/D` and `u_ar/RESET_B` become `incomplete_check`
  or `no_clock`. `is_clean` counts both, so `--strict` exits non-zero on this
  fixture — expected and recorded.
- Pulse-width rows: every clocked flop's clock pin (`u_ff0/CLK`, `u_fp/CLK`,
  `u_fbp/CLK`, `u_fbn/CLK_N`, `u_ff2/CLK`, `u_d*/CLK`) sits on net `c1`, so the
  golden should carry `pulse_width` rows (collector exists in
  `min_pulse_width/generate_ref.sh`); `u_a*/CLK` on the unclocked `aclk` net
  emits none (OpenSTA parity is part of the golden).

## Decision structure

- **Branch A (expected): OpenSTA does not propagate `Clear`/`Preset`.** Then
  R6's "endpoints reached through a Clear/Preset arc" clause is vacuous and R6
  closes as *already matching* — the new fixture's check-variant coverage
  (`dfstp`/`dfbbp`/`dfbbn`, plus unconstrained-endpoint parity) is the evidence
  delta. STRICT `compare_sta_edge` tests are added iff our endpoint set equals
  the golden; a `non_seq` divergence (Q4) is recorded as a named R6 residual,
  not silently absorbed.
- **Branch B: OpenSTA propagates them.** Then R6 stays open and this ticket's
  `results.md` entry carries the measured arrival rules (which arc, which
  transitions, launch clock requirement) plus a sequenced plan in
  `docs/roadmap.md`: Clear/Preset become propagation arcs with an async launch
  model — follow-on ticket, not this one.

R3 and R4 move to DONE regardless of the branch — their evidence is already
committed (`docs-update.md`).

## Files

- `tests/designs/sta_edge/async_arcs/` (new: DEF, `ideal.sdc`,
  `propagated.sdc`, `generate_ref.sh`, `async_arcs.reference.json`,
  `probe_async.txt`)
- `crates/grus-core/src/sta/analysis.rs` (two `compare_sta_edge` tests —
  Branch A only, and only when the endpoint sets match)
- `docs/results.md` (dated probe entry, verbatim)
- `docs/roadmap.md` (R3/R4 DONE, R6 disposition)
- `tests/designs/README.md` (fixture entry)
- `Cargo.toml`, `Cargo.lock` (`0.2.9 -> 0.2.10`)
- `kurena/tasks/async-arc-probe/` (spec + these design docs)
