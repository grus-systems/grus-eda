# golden — `mba` AP2 test design

Extends `test_activity_defaults_mba_matches_opensta` (`power.rs`
`#[cfg(test)]`) — same `pin_net` map, strict/compare shape, and group
harness. New: sessions carry an optional VCD path, `propagated` enters
the strict set under the mapping rule, `EXPECTED_DIVERGENT` shrinks, and
a per-pin residual table names the annotated non-driver pins.

## Sessions

| session | `ActivityOverrides` | VCD |
|---|---|---|
| `s_none` | `::default()` | none |
| `s_input` | `input: {1e7, 0.4}` | none |
| `s_global` | `global: {1e7, 0.4}` | none |
| `s_global_density` | `global: {3e7, 0.35}` | none |
| `s_ports` | `::default()` | `mba_ports.vcd` |
| `s_ports_a2x` | `::default()` | `mba_ports_a2x.vcd` |
| `s_partial` | `::default()` | `mba_partial.vcd` |
| `s_full` | `::default()` | `mba_full.vcd` |
| `s_xz` | `::default()` | `mba_xz.vcd` — include iff it holds verbatim (x/z half-weight seeds are already in `SignalActivity`; drop to AP4 with a recorded note if not) |
| `s_partial2` | `::default()` | `mba_partial2.vcd` — **bonus**: the scoped `u_and/X` dump lands through `pin_signals`; include iff the whole session holds verbatim |

Not covered: `s_input_ports` (`-input_ports` per-pin seeds — no override
surface yet), `s_p40` (uncommitted SDC).

Cross-session assert — `s_partial` vs `s_ports`: the test pins
`s_partial`'s resolved per-net vectors **verbatim** against `s_ports`'s
(`prob_one`, `eff_toggles`, `origin` per net — `s_partial`'s only extra
is the dead flat `n_and` entry). This compares `eff_toggles` as raw
counts (density × `window_s`), which is valid **only because both
fixtures record the same window** — `mba_partial.vcd` and
`mba_ports.vcd` both run to `#2000`, so `duration_s`/`window_s` is
identical. If either fixture's duration ever changes, the assert must
switch to `eff_toggles / window_s`; note the assumption in a comment at
the assert site.

## Per-net resolution after AP2 (s_none shape)

18 nets: `clk` Clock; `a`,`b` Input/`Annotated`/`Global` per session;
`n_float` Input; `n_hi`,`n_lo`,`n_bh`,`n_bl` Constant;
`n_and`,`n_inv`,`n_or`,`y`,`n_flt` **Propagated**;
`q`,`n_q0`,`n_q0n`,`n_q1`,`n_c1d` **Default** (`u_df` Q output is the AP3
rule; the counter cone is tainted). `s_global*`: no `Propagated` exists
(Global beats it in the seed order — the pass only touches `Default`).

`origin_counts` per session is a good verbatim assert — `s_none`:
`{clock 1, constant 4, input 3, propagated 5, default 5}` (count it in
the test once, then pin one representative session's map).

## Strict / mapping / divergent rules

Golden `source` → rule:

- `clock`, `global`, `constant`, `input` — ours must be the same label,
  `{d,P}` strict (unchanged rule; instance pins share the net's value).
- `vcd`, `saif`, `user` — strict `{d,P}` + `annotated` **only when the
  pin resolves its net** (the pin is the net's driver: io-pin driver, or
  the cell output pin of `inst/leaf` keys). A golden annotated pin that
  is *not* its net's resolving pin is a per-pin annotation residual —
  collected into `PER_PIN_RESIDUAL: &[(session, pin)]`, asserted
  verbatim; AP4 scope. Expected content: `[("s_full", "y")]` — port `y`
  holds `vcd 3e6/0.25` while net `y`'s driver `u_xor/X` reads
  `propagated 6e6/0.47`; our per-net value is the driver's.
- `propagated` — strict `{d,P}`. Origin label under the recorded
  mapping: `Propagated` on comb-derived nets; `Annotated`/`Input`
  accepted where the pin is a non-driver pin on a *seeded* net (OpenSTA
  re-derives it per-pin: `u_and/A`, `u_and/B`, `u_inv/A`, `u_xor/A`,
  `u_xor/B`, `u_df/D` on nets `a`/`b`; `u_and/A`+`u_and/B` on `s_partial2`
  via the input default). `Default` ⇒ divergent.
- `unknown` — divergent (AP3).

`EXPECTED_DIVERGENT` — verbatim, **13 pins** (the spec's `u_df/Q` covers
net `q`; its output-port pin `qd` shares the residual — flag in the
commit/results):

```rust
static EXPECTED_DIVERGENT: &[&str] = &[
    "q0", "q1", "qd", "u_df/Q",
    "u_f0/D", "u_f0/Q", "u_i0/A", "u_i0/Y",
    "u_f1/D", "u_f1/Q", "u_x1/A", "u_x1/B", "u_x1/X",
];
```

Same set every strict session; `s_global*` expects ∅. A new divergent
pin or a moved set fails loudly — AP3 shrinks it next.

Density compare unchanged (`rel ≤ 1e-9`, `abs ≥ 1.0` floor); `prob`
`abs ≤ 1e-9`; sources under the mapping above.

## Group columns

- `s_global`, `s_global_density`: unchanged asserted bounds (internal/
  switching 0.5%, leakage 1% + 1e-14, total 0.3%) — no `Propagated` nets
  exist, so these sessions pin the AP1 behavior.
- The propagation sessions (`s_none`, `s_input`, `s_ports`,
  `s_ports_a2x`, `s_partial`, `s_full`, `s_xz`): compute and **record**
  the new residuals in `docs/results.md` — remaining divergence is the
  AP3 set: net `q` (`Default` seed vs the golden Q=D copy — coincidentally
  equal to the input seed in `s_none`, off by the annotated rate in
  `s_ports`/`s_full`) plus the four `unknown`-cone nets contributing
  `5e6·window` switching+internal where the golden is 0. Assert the
  totals at the measured residual bounds **if they come out clean**;
  the accepted minimum is the recorded delta table (kept `eprintln`
  shape from AP1). Decision and measured numbers go in the entry.

## Moved goldens

- `mb.power.json`: regenerated via the recorded CLI command — expected
  `git diff` **empty** (nets `a`,`q`,`y` stay `Annotated` — now through
  `pin_signals` `_d_/Q`,`_i_/Y` — and `clk` `Clock`; identical values,
  identical `activity_origin_counts`).
- `mb.saif.reference.json`, `mbw.reference.json`, all other
  `*.reference.json`: `git diff` empty — asserted in the results entry.
  `mbw`'s net `z` stays `Annotated` via `u_t/Z`; `mb`'s net `y` via
  `_i_/Y`. Any move is a bug — stop and bisect the matcher.
- `mba.activity.reference.json`: input, never written.

## Fail-proof (record both in the results entry)

- Disable the propagation pass (early-return) → `s_ports` strict reds on
  a `Propagated` pin (`u_and/X` reads `default`/`5e6` vs golden
  `propagated`/`4.05e6`); restore.
- Corrupt one propagated golden entry (`s_ports` `u_and/X` density) in
  `mba.activity.reference.json` → red at the density bound; `git checkout
  --` restore, green.
- (Already covered by AP1: seed pass off → seeded-class red.)

## Files

- `crates/grus-core/src/power.rs` — `test_activity_defaults_mba_matches_opensta`
  extension (VCD session column, `propagated` handling + mapping rules,
  `PER_PIN_RESIDUAL`, 13-pin `EXPECTED_DIVERGENT`, `origin_counts` pin,
  group bounds for propagation sessions)
- `tests/designs/microbench/mba.activity.reference.json` — read-only
  golden input (fail-proof corrupts then restores it byte-for-byte)
- `tests/designs/microbench/mba_*.vcd` — existing session fixtures,
  read-only
- `tests/designs/microbench/mb.power.json` — regenerated; asserted
  byte-identical (`git diff` empty)
- `docs/results.md` — measured residuals, divergent-set note, moved-golden
  record, fail-proof record
