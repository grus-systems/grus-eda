# multi-launch-port — Overview

## Goal

Close `docs/path-delays.md` section 5 item 2: `y` has exactly one launch
source (`u_ff1/Q`), so the covered-candidate winner rule at an output-port
delay check is exercised only per-transition, never per-source. Add a
second `sky130_fd_sc_hd__mux2_1` (`u_mux2`) driving a new output port `y2`
from `u_ff0/Q` and `u_ff2/Q`, then golden the merged delay check.

## Why the merged rule actually merges (key design fact)

`run_sta_partitioned` partitions launch sources by signature — the
file-ordered rule indices whose `-from` covers the source
(`analysis.rs:3092-3103`). One eval run per class; the composition loop
then keeps, per (class, endpoint, transition), the worst surviving
candidate (`analysis.rs:3129-3330`). Consequences:

- A **single** `set_max_delay N -from [get_cells {u_ff0 u_ff2}] -to
  [get_ports y2]` puts `u_ff0` and `u_ff2` in one signature class: both
  launches propagate into `y2` in one eval run, and the delay check
  reports the worst covered candidate. That is the merge under test
  (`ml_both`, `pml_both`).
- Two separate commands (`-from u_ff0`, `-from u_ff2`) split the sources
  into two classes — two competing delay checks, never a merge. That
  shape is still wanted: `ml_max0`/`ml_max2` decompose *which* candidate
  `ml_both` reports (each shows its source's delay check in isolation).
- `sel` is a third launch at `y2` (see below) but is **not** covered by
  the `-from {u_ff0 u_ff2}` rules, so it keeps its ordinary clock check
  and only competes on slack.

## Predicted winners (from the committed golden — verify after regen)

Mirroring `u_mux`'s post-swap orientation (`u_ff0 -> A1`, `u_ff2 -> A0`)
reproduces the FF-side diverging-winner geometry at the port:

- `u_ff0` holds the larger pure delay — at `u_ff1/D` the `A1`-input path
  beat the `A0` path by ~13.5 ps (`m2_both` 1.277188 vs `m2_max2`
  1.263656). Same pin-capacitance asymmetry applies to `u_mux2`, so under
  the ideal clock `u_ff0` is the predicted `ml_both` winner:
  `ml_both.y2.max_arr` should equal `ml_max0.y2.max_arr` and exceed
  `ml_max2.y2.max_arr` by ~13.5 ps — above the 5 ps abs tolerance.
- Under `set_propagated_clock`, `u_ff2`'s launch lands ~69 ps after
  `u_ff0`'s (extra `u_clkb3` stage), so `u_ff2` is the predicted
  `pml_both` winner: `pml_both.y2.max_arr` should equal
  `base_prop.y2.max_arr` (the ordinary arrival winner), ~55 ps above a
  wrong delay-winner pairing — the port-side analog of the `pm2_both`
  falsification.
- `ml_all` (`-to [all_outputs]`) covers `y` and `y2` with a universal
  `-from`; `ml_all`'s `y` row should match `mdo_max1`'s `y` row
  (`set_max_delay 1.0 -to [get_ports y]`) — a free cross-check in the
  regen diff.

All three predictions are checked against the regenerated golden before
running the test; if any fails, the fixture needs adjustment, not the
code (see `steps.md`).

## Min-side masking constraint (drives the `set_min_delay` N choice)

`sel` shares `u_mux2/S` (per the goal), so it reaches `y2` and keeps an
uncovered ordinary check. On the min side the delay check must be the
*reported* candidate or the merge is unexercised: the golden collapses
each side to the min-slack path per endpoint. Numbers: ordinary
`req_min(y2) = latency - delay_min + hold_unc = 0.7`; `sel`'s
`arr_min(y2) ≈ 0.6 + 0.1 + 0.1 + S->X ≈ 0.9` → `sel` min slack ≈ +0.2.
The delay check's `req_min = N - delay_min + hold_unc = N`, so a covered
slack ≈ `arr_ff_min - N` with `arr_ff_min ≈ 0.9`. `N = 0.5` would give
+0.4 > +0.2 — `sel` would win the report and the merged min rule would be
invisible. `fixture.md` therefore uses **`set_min_delay 1.0`**
(`ml_min`/`pml_min`): covered slack ≈ −0.1, clearly below `sel`'s and a
real hold violation (the same shape that made the `m2_min*`/`md_min*`
delay checks visible). Implementation must verify from the golden that
`ml_min`'s reported `y2` min candidate is the delay check; if masked,
raise `N` and regenerate.

## Side effects on the existing fixture (expected, not bugs)

- `d0`/`d2`/`sel` each gain a sink (`u_mux2` pins), adding pin
  capacitance: every scenario whose report touches `u_ff0/Q`/`u_ff2/Q`/`sel`
  nets shifts by a few ps — most `u_ff1/D` rows and any `sel`-launched
  candidates. Expected; all values come from the regen.
- Every scenario gains a `y2` endpoint row (`y2` becomes a real checked
  endpoint via the preamble `set_output_delay` added to every `.sdc`).
- `test_sta_exception_diagnostics` counts are unaffected:
  `fp_to_y` (`-to [get_ports y]`) still removes exactly `y`
  (`false_path_endpoints == 1`), `fp_from_din` still removes `u_ff0/D` and
  `u_ff2/D` (`== 2`), `exc_diag`'s unresolved count (`== 4`) is unchanged.
  Its winner-req literal `2.550057` is data-path independent but `t_setup`
  sees the new `D`-pin slew — verify the assert still holds post-regen
  and update the literal if it drifted (precedent: `4.562147 ->
  4.550057`).

## Non-goals

- No production-code changes unless the regenerated golden shows a real
  divergence at `y2` (then fix the `OutputPort` composition in
  `crates/grus-core/src/sta/analysis.rs` — the covered-candidate arrival
  and edge-referenced required times — or record a measured
  fixture/model limit in `docs/results.md`). Test/doc-comment updates are
  expected regardless.
- No tolerance changes: STRICT rel 1 % / abs 5 ps stays.
- No new test files, no new dependencies.
- The other section-5 items stay open: two-clock-domain `set_min_delay`
  anchoring, async-reset/latch-borrow delay fixtures, CRPR credit on
  delay checks, propagated-clock input-port launch anchor.

## Files

- `tests/designs/sta_exceptions/exceptions.def`
- `tests/designs/sta_exceptions/*.sdc` (all 97 existing files gain the
  `y2` preamble lines; new `ml_both.sdc`, `ml_max0.sdc`, `ml_max2.sdc`,
  `ml_all.sdc`, `ml_min.sdc`, `pml_both.sdc`, `pml_min.sdc`)
- `tests/designs/sta_exceptions/generate_ref.sh`
- `tests/designs/sta_exceptions/exceptions.reference.json`
- `crates/grus-core/src/sta/analysis.rs` (doc-comment refresh; possible
  `exc_diag` literal; production code only on a real divergence)
- `docs/path-delays.md`
- `docs/results.md`
- `docs/timing-exceptions.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
