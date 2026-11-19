# async-latch-delay — Implementation shape

> **Status**: implemented in `e530643` as described below and
> STRICT-green against the committed goldens. The only outstanding delta
> is the docstring sign fix at `analysis.rs:7754` (see `steps.md`).

All changes live in `crates/grus-core/src/sta/analysis.rs`. The aborted run
left a draft implementing the recorded rules end-to-end; it is gone, but the
shape below is what it did and remains the plan — to be written fresh and
verified against the committed-script goldens, never trusted on its own.
Line references are to the current file.

## 1. `dly` arrival extremes (the new plumbing)

`PinTimingState` (~line 94) gains `dly_max: [f32; 2]` / `dly_min: [f32; 2]`
— arrival extremes that re-base a latch open-edge launch to the delay edge,
merged independently of which source wins `arr` (same discipline as
`slew`):

- `PinTimingState::merge` (~line 133, currently 9 args) takes
  `dly_max`/`dly_min` arguments; every call site updated (port seeds pass
  `arr`, unreachable passes ±∞, `sink_state_with_wire` ~1573 adds the same
  net delay as `arrival`, `propagate_clock` ~1814 / `prepare_sta`
  clock-state merges pass the edge latency).
- `eval_launch` (a closure inside `prepare_sta`, ~line 2450) gains a
  `delay_edge: bool` — `true` only for `graph.latch_opens` (clock-to-Q
  open-edge launches): `dly = arr − clk_state.arrival[edge_pol]` strips the
  clock-edge term so the launch reports at the delay edge. FF/port launches
  pass `false` → `dly = arr`. `LaunchPair` (~2146) carries the two new
  fields through `LaunchSeed` (~2169) into `eval_sta`'s merges.
- Transparent `D -> Q` borrow propagation (`eval_sta` ~2701 reads
  `graph.latch_borrow`) caps `src.dly_max` through the same borrow `cap()`
  as `arrival_max`; `dly_min` through a borrow arc contributes nothing
  (`f32::INFINITY`, matching the early side's clock-to-Q-only launch rule).
- `Worst` (~3771) gains `dly`; `collect_endpoints` (~3957) fills it from
  the state; cross-class `pick_worst_polarity`/best-slack merge carries it
  inside `Worst` untouched.

## 2. `disabled_borrow` (measured D→Q silencing)

`eval_sta` (~2602) gains a `disabled_borrow: &HashSet<(u32, u32)>` of
`(D_pin, Q_pin)` borrow arcs. `run_sta_partitioned` (~3194) builds it per
signature class: for every `ResolvedRule::Delay` with `kind ==
PathDelayKind::Max` in the signature, each `graph.latch_borrow` key whose D
pin is covered by the rule's `to` (universal or `contains`) is disabled for
the whole class — uncovered endpoints included (measured). Inside the eval
loop, a disabled borrow arc merges slew only (±∞ arrivals) and `continue`s.
`run_sta_detail`'s non-partitioned call (~2125: `eval_sta(&prepared, None,
…)`) passes `&HashSet::new()` — no delay rules ever reach it (the
partitioned path owns `path_delays`, gated at ~2123).

## 3. Gate: `delay_supported`

`Check { latch: false }` predicate (~3428: `c.setup_arc.is_some() ||
c.hold_arc.is_some()`) widens to `Check { .. }` with
`c.late_arc().is_some() || c.early_arc().is_some()` — covers FF-D, async
recovery/removal (`late_arc = recovery_arc`, `early_arc = removal_arc`,
lines 84-89), and latch-D. `ClockGate` stays `false` → still counted in
`unsupported_path_delay_endpoints`.

## 4. Req composition per endpoint kind

- **FF-D & async-check, max**: existing block (~3457-3521) unchanged in
  shape — `req = capture_min[check.edge] − unc_setup − t_late + N + crpr`
  — except `arr`/`slack` now read `w.dly` (a through-latch launch re-bases;
  identical when `dly == arr`). The recovery table flows through
  `late_arc` + `check.clk` with no new branch.
- **Latch-D, max**: `req = w.dly.clamp(base, base + (close − open −
  t_setup).max(0))` where `base = N + clk_state.arrival_min[edge0_pol] −
  setup_uncertainty` — **sign correction** (golden regen): the bound is
  `pulse − t_setup`, not `pulse + t_setup`; OpenSTA's
  `report_checks` prints the setup term negated ("library setup time
  -0.18"), which is what misled the first record. `open`/`close` are
  `arrival_min[1 − check.edge]` / `arrival_min[check.edge]` (+period when
  it wraps). Reports `arr = w.dly`, `slack = req − w.dly`.
- **Latch-D, min**: dedicated branch — `req = N +
  clk_state.arrival_max[edge0_pol] + t_hold + hold_uncertainty` (delay
  edge anchored); `arr = w.dly`, `slack = w.dly − req`.
- **FF-D & async-check, min**: unchanged generic `req += N` on the
  pre-MCP hold/removal required time (~3537), plus `arr = w.dly`.
- **OutputPort**: unchanged formula, `arr = w.dly` for uniformity.
- `edge0_pol(clk)`: first edge reaching the clock pin, decoded from
  `clock_parity` (parity of the pin's RISE state — `0` ⇒ the pin edge at
  nominal 0 is RISE, `1` ⇒ inverted net ⇒ FALL); contested/absent parity
  falls back to RISE.

## 5. What to scrutinize before trusting the model

- **`dly` as an independent extreme**: merged like slew, it can pick the
  `dly` of a different source than the `arr` winner. If a golden `arr`
  mismatches, the alternative model is `dly`-of-the-`arr`-winner — decide
  by OpenSTA, not by fit.
- **`disabled_borrow` breadth**: class-wide silencing off one `-to`
  selector is the bold claim. `delay_allregs` (every latch D covered) and
  `delay_max` (only `u_lp/D`) are the discriminators — watch `u_lp`-
  downstream endpoints' `arr`.
- **`edge0_pol` fallback**: `async_fall`'s `dfrtn` (inverting clock) has
  no committed delay scenario — the fallback path is exercised only if a
  negative-parity clock pin meets a delay check; note the gap in
  `results.md` rather than invent coverage.
- **Existing-golden regression**: `arr = w.dly` is a no-op unless a
  covered path launches through a latch open edge. `sta_exceptions` has
  no latches, so `md_*`/`pm*`/`m2_*`/`pmp_*`/`pml_*`/`pmd_*` goldens
  should be byte-identical — any diff there is a bug, not a tolerance.

## 6. Tests & fail-proof

- `fixture.md` §3's `compare_sta_edge` calls, all `Tol::STRICT`.
- `unsupported_path_delay_endpoints` shrinks to clock-gate-only on
  `delay_allregs`/`delay_cell` — assert the count if a cheap hook exists,
  else note it in `results.md`.
- Fail-proof (AGENTS.md §7.7): (a) revert `delay_supported` to
  FF-only → the new scenarios must go red on the async/latch endpoints;
  (b) substitute `w.arr` for `w.dly` in the latch compose → `delay_max`
  must go red on `u_lp/D`. Restore after each.
- `merge_skip_reports` (~4436) merges the counter by `max` — unchanged.
