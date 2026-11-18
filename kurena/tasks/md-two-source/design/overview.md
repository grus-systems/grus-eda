# md-two-source — Overview

## Goal

Close the coverage gap recorded in `docs/path-delays.md` section 5 item 3
(and the skewed-clock part of item 1): today every `sta_exceptions` endpoint
has exactly one launch source, so the delay-domain merge's delay-winner vs
arrival-winner pairing (`delay_max` / `delay_launch_max` in
`PinTimingState`) is exercised only by the unit test
`test_delay_merge_tracks_the_delay_winner_launch`, never by OpenSTA golden
data.

Extend the fixture so `u_ff1/D` is driven by a `sky130_fd_sc_hd__mux2_1`
(`u_mux`) with `u_ff0/Q -> A0` and `u_ff2/Q -> A1`, give the select pin a new
input port `sel`, and put `u_ff2/CLK` on its own clock buffer `u_clkb2` so a
propagated clock lands at the three FF clock pins with different latencies.

## How the delay domain actually gets two sources (key design fact)

`run_sta_partitioned` partitions launch sources by **signature** — the
file-ordered rule indices whose `-from` covers the source
(`analysis.rs:3193-3210`). One eval run per class merges only that class's
sources. Consequences for scenario design:

- `set_max_delay ... -from [get_cells u_ff0]` and a second command
  `-from [get_cells u_ff2]` give the two FFs *different* signatures ->
  different classes -> each delay domain at `u_ff1/D` sees a single source.
  The endpoint composes two competing delay checks (correct, and covered
  already by the existing `pmd_multi` scenario once the DEF changes), but the
  per-pin merge of two delay candidates never runs.
- A **single** command covering both sources —
  `set_max_delay 1.0 -from [get_cells {u_ff0 u_ff2}] -to [get_cells u_ff1]` —
  gives both the same signature -> same eval class -> both launches propagate
  into `u_ff1/D` and the `delay_max`/`delay_launch_max` merge runs on golden
  data. Under `set_propagated_clock` the two launch arrivals differ, so this
  is the only scenario shape that can discriminate a broken launch pairing.
- For the same reason, the goal's `pm2_prop` (`-from u_ff0` alone under a
  propagated clock) does *not* by itself merge two covered sources — it
  checks delay-check composition against competing clock checks. The plan
  therefore adds **`pm2_both`** (propagated clock + one command covering both
  sources) to the goal's scenario list; that is the case where delay winner
  and arrival winner can diverge, which is the reason the fixture exists.
  `m2_both` likewise uses the one-command form (ideal clock: exercises the
  merge without skew).

## Expected asymmetry (from the committed Liberty)

Both engines see zero wire delay here (the test passes `parasitics = None`;
OpenSTA runs no `estimate_parasitics`), so placement coordinates do not
affect numbers — only cell delays do.

- Clock skew: `u_clkb` drives two `dfxtp_1` CLK pins (~0.0036 pF), `u_clkb2`
  drives one (~0.0018 pF) -> `u_clkb` is slower -> `u_ff0`/`u_ff1` launch
  **later**, `u_ff2` launches **earlier**. (The goal text says "u_ff2
  launches later than u_ff0"; per the load analysis the sign is probably the
  opposite — the golden decides. The mechanism is the same either way.)
- Data asymmetry: `mux2_1` pin capacitances are A1 0.001884 pF > A0
  0.001558 pF, and the A1->X arc tables are a few ps above A0->X, so the pure
  data delay via `u_ff2` is slightly larger.
- Divergence at `u_ff1/D` (max side) needs delay winner `u_ff2` while
  arrival winner `u_ff0`, i.e. `0 < delay(u_ff2 path) - delay(u_ff0 path) <
  launch(u_ff0) - launch(u_ff2)`. Whether the skew exceeds the data-delay
  difference is decided by the run — see `steps.md` for how to check and a
  contingency knob (an extra load on `c1`) if it does not.

## Side effects on the existing fixture (must be handled, not bugs)

- `u_ff1/D` gains launch sources `u_ff2` and `sel`, so endpoint reports take
  the min-slack candidate across sources: relaxed checks (multicycle shifts,
  loose delays, partial false paths) get masked by tighter unshifted
  competitors. Goldens regenerate, so values stay correct, but two
  `test_sta_exception_diagnostics` asserts hardcode single-source behaviour
  and must be updated (see `steps.md`).
- `sel` must be constrained in *every* scenario file (shared preamble), not
  just the new ones: left unconstrained it launches at ~0 and dominates every
  min-side report at `u_ff1/D`, masking the existing hold probes. With a
  small `-min` delay its hold slack stays positive and delay-check
  candidates (negative slacks) still win their reports.
- `u_ff2/Q` gains fanout (`u_mux/A1`); `u_ff2` becomes a real launch into
  `u_ff1/D`. Existing `pmd_multi` (two `set_max_delay` commands from `u_ff0`
  and `u_ff2`) and `md_nofrom` (universal `-from`) automatically become
  multi-source delay scenarios once the DEF changes — good extra coverage
  for free.

## Non-goals

- No production-code changes unless `test_sta_exceptions_match_opensta`
  shows a real divergence on the new scenarios (then fix the delay merge /
  launch pairing in `crates/grus-core/src/sta/analysis.rs`, per the goal).
- No tolerance changes, no new test files, no new dependency.
- A literal two-clock-domain `set_min_delay` anchoring fixture and the
  latch-borrow delay fixture stay open (path-delays.md section 5 item 1).

## Files

- `tests/designs/sta_exceptions/exceptions.def`
- `tests/designs/sta_exceptions/base.sdc`
- `tests/designs/sta_exceptions/base_prop.sdc`
- `tests/designs/sta_exceptions/*.sdc` (all existing scenario files gain the
  `sel` preamble lines; new `m2_max0.sdc`, `m2_max2.sdc`, `m2_both.sdc`,
  `m2_min2.sdc`, `pm2_prop.sdc`, `pm2_min2.sdc`, `pm2_both.sdc`)
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs` (diagnostics-test assert updates;
  production code only if a real divergence surfaces)
- `Cargo.toml` (patch bump 0.1.86 -> 0.1.87 with the functional change)
- `docs/results.md`
- `docs/path-delays.md`
- `docs/timing-exceptions.md` (stale fixture description, one-line fix)
