# golden — `mba` AP3 test design

Extends `test_activity_defaults_mba_matches_opensta` (`power.rs`
`#[cfg(test)]`) to **all 12 committed sessions** — same `pin_net` map,
strict/compare shape, and group harness. `EXPECTED_DIVERGENT` goes
empty; the `unknown` golden source becomes a strict compare; two
sessions (`s_input_ports`, `s_p40`) enter coverage.

## Sessions — 12

| session | `ActivityOverrides` | VCD | SDC |
|---|---|---|---|
| `s_none` | `::default()` | none | `mba.sdc` |
| `s_input` | `input: {1e7, 0.4}` | none | `mba.sdc` |
| `s_input_ports` | `input_ports: {1e7, 0.4}` | none | `mba.sdc` — new |
| `s_global` | `global: {1e7, 0.4}` | none | `mba.sdc` |
| `s_global_density` | `global: {3e7, 0.35}` | none | `mba.sdc` |
| `s_ports` | `::default()` | `mba_ports.vcd` | `mba.sdc` |
| `s_ports_a2x` | `::default()` | `mba_ports_a2x.vcd` | `mba.sdc` |
| `s_partial` | `::default()` | `mba_partial.vcd` | `mba.sdc` |
| `s_partial2` | `::default()` | `mba_partial2.vcd` | `mba.sdc` |
| `s_full` | `::default()` | `mba_full.vcd` | `mba.sdc` |
| `s_xz` | `::default()` | `mba_xz.vcd` | `mba.sdc` |
| `s_p40` | `::default()` | none | `mba_p40.sdc` — new fixture |

`mba_p40.sdc` is committed — verbatim the 5-line `/tmp/mba40.sdc`
`generate_activity_ref.sh` writes inline (`create_clock -period 40.0`
only change). `s_p40` recomputes `sdc`/`pin_slews` with it (f_clk
`2.5e7`, input seed `2.5e6`, clock `5e7`).

## Per-net resolution after AP3 (`s_none` shape)

18 nets: `clk` Clock; `a`,`b`,`n_float` Input; `n_hi`,`n_lo`,`n_bh`,
`n_bl` Constant; `n_and`,`n_inv`,`n_or`,`y`,`n_flt` Propagated (comb);
`q` **Propagated** (seq Q=D copy of `a`); `n_q0`,`n_q0n`,`n_q1`,
`n_c1d` **Unknown** `{0,0}` (feedback cycles — no producer ever
resolves). `s_none` `origin_counts` pin moves to
`{clock 1, constant 4, input 3, propagated 6, unknown 4}` —
`default` disappears (post-conversion `Default` is an impossible
origin).

`driven_nets_without_activity` on `s_none` moves 5 → 4 (the four
unknown nets, all cell-driven — recorded in `results.md`; `q` leaves
the count).

## Strict / mapping / divergent rules

Golden `source` → rule (changes **bold** relative to AP2):

- `clock`, `global`, `constant`, `input` — unchanged strict `{d,P}` +
  same label.
- `vcd`, `saif`, `user` — strict `{d,P}` + `annotated` only when the
  pin resolves its net (driver); else `PER_PIN_RESIDUAL` —
  unchanged, still `[("s_full", "y")]`. `s_input_ports`'s `a`/`b` are
  `user` on port drivers → resolve → strict `annotated`.
- `propagated` — strict `{d,P}`; label `Propagated` on comb- and
  seq-derived nets, `Annotated`/`Input` accepted on non-driver pins of
  seeded nets — unchanged mapping; `u_df/Q`/`qd` now land inside it
  instead of the divergent set. In every session net `q` reads
  `Propagated` (in `s_full` the `q` dump entry is a flat internal net
  name → dead → `Default` → seq copy; no session annotates net `q`).
- **`unknown`** — **moved to strict**: `{d≤bound, P≤1e-9}` + origin
  label `unknown`. The 11 pins (`q0`, `q1`, `u_f0/D`, `u_f0/Q`,
  `u_i0/A`, `u_i0/Y`, `u_f1/D`, `u_f1/Q`, `u_x1/A`, `u_x1/B`,
  `u_x1/X`) hold verbatim in every non-global session.

`EXPECTED_DIVERGENT` → `static EXPECTED_DIVERGENT: &[&str] = &[];` and
the `s_global*` special case collapses — every session's expected set
is empty. A new divergent pin fails loudly.

Named discriminators the verbatim compare pins (no extra asserts
needed — call them out in the entry): `u_df/Q`/`qd` track `a` exactly,
including `s_ports_a2x`'s `6e6/0.55` (the a2x density doubling — the
S1 discriminator) and `s_xz`'s `0.40` (x-degraded seed).

Density/prob bounds unchanged (rel ≤ 1e-9, abs-floor 1.0; prob abs
≤ 1e-9).

## Diagnostics asserts (unchanged expectations)

- `unresolvable_functions` == 0 on all sessions (the seq-expression
  residual is *not* this counter — api.md §3).
- `dead_annotation_entries`: `s_partial` 1, `s_full` 15, else 0.
- `s_partial` vs `s_ports` verbatim per-net equality — unchanged
  (same-window comment retained).

## Group columns — asserted bounds

The `s_global*` `check` harness generalizes to **all 12 sessions**.
Measured-first rule: record the achieved residuals per session/column
in `docs/results.md`, then assert at the honest bound — expected to
land at or inside the established `s_global*` bounds (internal/
switching ≤ 0.5%, leakage ≤ 1% + 1e-14, total ≤ 0.3%), since the
surviving AP2 residual *was* the sequential cone. If a session exceeds
those bounds, keep the measured bound, name the root cause, record
both — do not silently widen.

## Moved goldens

- `mb.power.json` — regenerated; expected `git diff` **empty** (mb's
  `q` stays `Annotated` via `_d_/Q`; no `Default` net exists on mb).
- `mb.saif.reference.json`, `mbw.reference.json`, all other
  `*.reference.json` — `git diff` empty, asserted in the entry. `mbw`
  has no sequential cells; nothing else produces `Default`/`Unknown`
  diffs.
- `mba.activity.reference.json` — input, never written.
- `mba.none.openroad.txt` — untouched (generation artifact).

## Fail-proof (record both in the results entry)

- Sequential copy disabled (seq producers dropped from `propagate`) →
  `s_ports` strict reds on `u_df/Q` (origin `unknown`/density `0` vs
  golden `propagated`/`3e6`); the 11 unknown pins still match — the
  discriminating pins are the seq columns. Restore, green.
- Corrupt one sequential golden entry (`s_ports` `u_df/Q` density
  `3e6 → 4e6`) → strict red at the density bound; `git checkout --`
  restore, green.
- (AP1/AP2 proofs stand: seed pass off → seeded-class red; comb
  propagation off → `n_and` red.)

## Files

- `crates/grus-core/src/power.rs` — session table → 12 (adds
  `s_input_ports`, `s_p40` SDC column), `unknown` strict arm,
  `EXPECTED_DIVERGENT` empty + collapse of the `s_global*` case,
  `origin_counts` pin update, group `check` for all sessions
- `tests/designs/microbench/mba_p40.sdc` — new committed fixture
  (verbatim the generator's `/tmp/mba40.sdc`)
- `tests/designs/microbench/mba.activity.reference.json` — read-only
  golden input
- `tests/designs/microbench/mb.power.json` — regenerated; asserted
  byte-identical
- `docs/results.md` — residual tables, census, moved-golden record,
  fail-proof record
