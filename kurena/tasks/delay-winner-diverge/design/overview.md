# delay-winner-diverge — Overview

## Goal

Close the diverging-winner half of `docs/path-delays.md` section 5 item 3:
make the `sta_exceptions` fixture produce a merged delay domain whose
`delay_max` winner is a *different source* than the ordinary max-arrival
winner at `u_ff1/D`, so OpenSTA golden data finally discriminates the
`delay_launch_max` pairing in `PinTimingState` (which launch arrival travels
with the delay winner).

Acceptance wants: delay-domain winner `u_ff0`, ordinary max-arrival winner
`u_ff2`, verifiable from `pm2_prop` vs `pm2_both` and
`m2_max0`/`m2_max2`/`m2_both`, all 72 scenarios STRICT.

## Correction to the goal's expected mechanics (important)

The goal predicts the two-line mux-pin swap alone makes "`u_ff0` own the
larger pure delay but launch later on `c1`... the arrival winner stays
`u_ff2`". That outcome is arithmetically impossible. Recomputed from the
committed golden (`exceptions.reference.json`):

- Pure path-delay gap between the mux inputs:
  `g = d(A1 path) - d(A0 path) ≈ +9.9 ps` in favour of the `A1` driver.
  Derivation: `pm2_prop.u_ff1/D.max_arr = launch_c1 + d_A0 = 0.717891` and
  `pm2_both.u_ff1/D.max_arr = launch_c2 + d_A1 = 0.719321`, with
  `launch_c1 - launch_c2 = s ≈ +8.5 ps` (c1 has two CLK loads, c2 one;
  measured via the `pm2_both` req anchor 1.013333 vs launch-referenced
  ~1.0048). So `g = 0.719321 - 0.717891 + s ≈ 9.9 ps`.
- The goal's "~1.4 ps delay difference" is `g - s` — the *arrival* gap
  (launch included), not the pure-delay gap.
- Today (`u_ff0->A0`, `u_ff2->A1`): `u_ff2` wins delay by `g` and arrival
  by `g - s ≈ 1.4 ps`. Both domains agree.
- After the swap alone (`u_ff0->A1`, `u_ff2->A0`): `u_ff0` wins delay by
  `g` and arrival by `s + g ≈ +18.4 ps` — the later-launching source gained
  delay, so it wins arrival *harder*. `u_ff2` cannot win the arrival race
  while holding both the smaller delay and the earlier launch.
- The goal's fallback ("add a clock load on `c1`") also pushes the wrong
  direction for the acceptance pairing: it makes `u_ff0`'s launch later
  still.

For the acceptance pairing — delay winner `u_ff0`, arrival winner `u_ff2` —
the requirements are:

1. `d(u_ff0 path) > d(u_ff2 path)` → the goal's net swap (`u_ff0` drives
   `A1`) is correct and stays.
2. `launch(u_ff2) - launch(u_ff0) > g ≈ 9.9 ps` → `u_ff2`'s clock must
   arrive **later** than `u_ff0`'s by more than ~10 ps. Today it arrives
   ~8.5 ps *earlier*, so `u_ff2`'s clock path needs ~18-20+ ps of added
   delay.

`fixture.md` prescribes the swap plus a `clkbuf` stage inserted on `u_ff2`'s
clock (`c2 -> u_clkb3 -> c3 -> u_ff2/CLK`), which adds one buffer delay
(tens of ps) — a deterministic overshoot, and overshoot is harmless: it
only widens the split margin.

## What the fixture then discriminates

At a delay-covered endpoint our engine reports
`arr = delay_launch_max[pol] + delay_max[pol]` (the delay winner's own
launch paired with its pure delay — `analysis.rs:3338-3344`). The competing
OpenSTA hypotheses the golden can now separate:

- **(b) winner-pairing (implemented)**: reported arr =
  `launch(delay_winner) + delay(delay_winner)` = `launch_c1 + d_A1`.
- **(a) covered-arrival max**: OpenSTA's path-delay check may instead
  report the max `launch + delay` over covered sources = the *arrival*
  winner's value = `launch_c3 + d_A0` — the delay check then never needs a
  separate delay domain.

Today these coincide (same winner), so the golden is blind. After the
fixture change they differ by `launch_c3 + d_A0 - (launch_c1 + d_A1)
= s' - g` (positive by construction), comfortably above the 5 ps absolute
tolerance. `pm2_prop` supplies `u_ff0`'s isolated delay-check arr
(`launch_c1 + d_A1`); `pm2_both` shows what the merged domain reports;
`base_prop` shows the ordinary arrival winner; `m2_*` pin the pure-delay
ordering under an ideal clock (same launch, so `m2_both` winner =
delay winner = `u_ff0`).

If the test goes red under hypothesis (a), the fix is the composition in
`run_sta_partitioned`'s delay-check block (report the covered-source
arrival max, not `delay_launch + delay_max`), not the test or tolerances —
see `steps.md`.

## Non-goals

- No new scenario files, no `generate_ref.sh` edits, no tolerance changes,
  no new dependencies.
- The CRPR-credit part of section 5 item 3, the propagated-clock
  port-launch anchor (item 4), the two-clock-domain `set_min_delay` /
  async-reset / latch-borrow fixtures (item 1) and output-port delay
  semantics (item 2) all stay open.
- `analysis.rs` production code is touched only if the regenerated golden
  exposes a real pairing/composition divergence; the stale test-comment
  literal (`4.562147`) is updated regardless.

## Files

- `tests/designs/sta_exceptions/exceptions.def`
- `tests/designs/sta_exceptions/exceptions.reference.json` (regenerated)
- `crates/grus-core/src/sta/analysis.rs` (stale comment literal; production
  fix only if the split exposes a pairing bug)
- `docs/results.md`
- `docs/path-delays.md`
- `docs/timing-exceptions.md` (fixture-topology line, one-line sync)
- `Cargo.toml` (patch bump in the same commit)
