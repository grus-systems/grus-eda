# golden — `mba` strict/expected-divergence test design

Test: `test_activity_defaults_mba_matches_opensta`, `power.rs` `#[cfg(test)]`
(extends `mb`'s pattern). Loads `mba.def`, `mba.sdc`, `mba.activity.reference.json`
(the committed OpenSTA dump, 12 sessions × 43 pins). Calls the public
`resolve_activity` for per-pin checks and `estimate_power` for group columns —
both paths share the fill pass by construction.

## Session → overrides

| session | `ActivityOverrides` | golden knob |
|---|---|---|
| `s_none` | `::default()` | none |
| `s_input` | `input: Some({1e7, 0.4})` | `-input -activity 0.2 -duty 0.4` |
| `s_global` | `global: Some({1e7, 0.4})` | `-global -activity 0.2 -duty 0.4` |
| `s_global_density` | `global: Some({3e7, 0.35})` | `-global -density 0.03 -duty 0.35` |

Knob values are passed as resolved rates (measured conversions recorded in
`docs/results.md`); `s_p40` is not covered — its SDC was `/tmp`-only.

## Pin → net map (test helper)

Golden keys are *pin* names; our model is per-net. Build
`HashMap<String, NetId>` from `io_pins` (port name → `pin_nets`) plus
`cell_name + "/" + lib_pin_name` per `cell_pins` entry (`pin_lib_pins` →
`lib.pins[..].name`). All 43 golden keys must resolve or the test fails.

## Per-session expected resolution (18 nets)

| net | driver | `s_none` | `s_input` | `s_global` | `s_global_density` |
|---|---|---|---|---|---|
| `clk` | PIN `clk` | clock 1e8/0.5 | clock | clock | clock |
| `a`, `b` | PIN | input 5e6/0.5 | input 1e7/0.4 | global 1e7/0.4 | global 3e7/0.35 |
| `n_float` | (undriven, `u_flt/A`) | input 5e6/0.5 | input 1e7/0.4 | global | global |
| `n_hi`, `n_lo` | `u_c/HI`/`LO` | constant 0/0 | constant | constant | constant |
| `n_bh`, `n_bl` | `u_bh/X`, `u_bl/X` | constant | constant | constant | constant |
| `n_and` | `u_and/X` | default 5e6/0.5 | default 1e7/0.4 | global | global |
| `n_inv` | `u_inv/Y` | default | default | global | global |
| `n_or` | `u_or/X` | default | default | global | global |
| `y` | `u_xor/X` | default | default | global | global |
| `q` | `u_df/Q` | default | default | global | global |
| `n_q0`, `n_q1` | `u_f0/Q`, `u_f1/Q` | default | default | global | global |
| `n_q0n` | `u_i0/Y` | default | default | global | global |
| `n_c1d` | `u_x1/X` | default | default | global | global |
| `n_flt` | `u_flt/X` | default | default | global | global |

Density compare = `resolved.eff_toggles / window_s` vs golden `density`,
`rel ≤ 1e-9` (f64 construction noise; `f_clk/10` and `2·f_clk` are exact at
period 20). `prob` compare `abs ≤ 1e-9`. `source` = origin label vs golden
`source`, exact.

## Strict vs expected-divergent pins

Strict-source set (our model reproduces these in AP1):
`{"clock", "global", "input", "constant", "vcd", "saif", "user"}`.

Divergent-source set: `{"propagated", "sequential", "unknown"}`.

- `s_global`, `s_global_density`: **all 43 pins strict** — global 31,
  clock 4 (`clk`, `u_df/CLK`, `u_f0/CLK`, `u_f1/CLK`), constant 8
  (`u_c/HI`, `u_c/LO`, `u_bh/A`, `u_bh/X`, `u_bl/A`, `u_bl/X`, `yh`, `yl`).
- `s_none`, `s_input`: 15 pins strict (the same clock/constant/input set —
  `a`, `b`, `u_flt/A` read `input`), **28 pins in the committed
  expected-divergence table** — a `static EXPECTED_DIVERGENT: &[&str]` in
  the test, the exact pin set whose golden source is `propagated`/`unknown`
  (shared: same pin set in both sessions):
  - `unknown` (11): `q0`, `q1`, `u_f0/D`, `u_f0/Q`, `u_i0/A`, `u_i0/Y`,
    `u_f1/D`, `u_f1/Q`, `u_x1/A`, `u_x1/B`, `u_x1/X`
  - `propagated` (17): `y`, `yc`, `qd`, `u_and/A`, `u_and/B`, `u_and/X`,
    `u_inv/A`, `u_inv/Y`, `u_or/A`, `u_or/B`, `u_or/X`, `u_xor/A`,
    `u_xor/B`, `u_xor/X`, `u_df/D`, `u_df/Q`, `u_flt/X`

Assertion shape per golden pin: golden source strict → assert
{density, prob, source} match; golden source divergent → assert pin ∈
`EXPECTED_DIVERGENT`. Then assert the whole divergent-source pin set
equals the table exactly — a *new* divergence (e.g. a pin whose golden
source is seeded but which we resolve wrong) fails as a strict mismatch,
and an AP2/AP3 landing is a deliberate table shrink, not a silent green.

Note: `y` is a port pin on net `y` (driven by `u_xor/X`) — per-pin OpenSTA
labels it `propagated`; our per-net `Default` puts it in the table. Same
for `yc`/`qd` ports. No special-casing needed.

## Group columns

- `s_global`, `s_global_density`: `estimate_power` totals asserted at the
  established N7 bounds — internal 2%, switching 5%, total 1%, leakage
  25% + 1.5e-12 floor — **tightened if the honest measured residual is
  smaller** (record the actual deltas and the chosen bound in the results
  entry; target the mbw-style 0.1% only if measured).
- `s_none`, `s_input`: computed but **not asserted** — the `Default`
  interim fill on the 28 divergence-table pins makes the columns
  deliberately divergent vs OpenSTA's propagated model (notably: ours
  adds `n_q*`/`n_c1d` switching+internal at 5e6 where the golden's
  `unknown` cones are 0; `y`/`yc`/`n_or` densities differ). The measured
  deltas are recorded in `docs/results.md`; AP2 converts these sessions
  to strict bounds.

## Moved goldens

- `mb.power.json`: regenerate via the recorded CLI command
  (`grus-eda power --lib … --tlef … --lef … --def mb.def --sdc mb.sdc
  --vcd mb.vcd --json`). Expected diff: `diagnostics` gains
  `activity_origin_counts` (`{"annotated": 4}`) and nothing else — the
  count-representation decision keeps annotated arithmetic bit-identical.
  Any watt-column move is a bug; `git diff` is the proof, recorded.
- `mb.saif.reference.json`, `mbw.reference.json`, `mbw.power.json`,
  `mba.activity.reference.json`: untouched; `git diff` empty and asserted
  in the results entry (mb/mbw fully annotated ⇒ byte-identical output).
- `mba.partial.vcd` / `mba_partial` sessions and `s_p40`: not exercised
  by the test (scoped match is AP4; the 40 ns SDC is uncommitted).

## Fail-proof (recorded in results entry)

- Disable the seed pass (all nets `Default`) → `s_global`/`s_none`
  seeded-class assertions go red; restore.
- Corrupt one density inside the committed `mba.activity.reference.json`
  `s_global` block → red; restore via `git checkout --`.
- With the fill re-enabled: assert `resolve_activity` on `mb`'s VCD
  (all four nets `Annotated`) — the "leftover fills something" path is
  what the s_* strict pins already pin down; the mb run proves the
  `Required` annotation path is unchanged.
