# low-sweep — doc and `results.md` fixes

Each item gives the current text (locate by content; line numbers drift)
and the replacement. `R#` sections carry their `overview.md` D-id where
the mapping is not one-to-one.

## R1 — perf-profile entry: 45 cells → 38 cells

`docs/results.md`, the `perf-profile baseline` entry's "What was run"
paragraph. Current:

```
REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-measure.log` (45 cells × 5 reps = 225 runs; `sta.spef_rcx`
and `power.vcd` cells gated on committed artifacts — aes/ibex/jpeg have
no `*.rcx.spef`, only gcd has a committed VCD).
```

→ `(38 cells × 5 reps = 190 runs — the script's 7 base cells × 5 designs,
`sta.spef_rcx` gated on committed artifacts (fires on gcd + picorv32;
aes/ibex/jpeg have no `*.rcx.spef`), `power.vcd` gated likewise (gcd
only — the committed VCD)).`

Note: `picorv32.rcx.spef` is gitignored-but-present (generated); the gate
does fire on it — the count is 38, not 37.

## R2 — perf-par-slew entry: matrix count + gcd timer-floor marks

"full 45-cell matrix on the v0.3.3 release binary" → "full 38-cell
matrix on the v0.3.3 release binary".

In the same entry's wall table, the three gcd rows claim `×1.25` on
`50 → 40` medians — inside one `/usr/bin/time` 10 ms quantum (the
baseline entry's own caveat, L5705). Change the three gcd `speedup`
cells (`sta` placement RC, `sta` routed SPEF, `power` placement RC)
from `×1.25` to `~noise`, matching the rcx gcd row's existing mark, and
append one sentence after the table: "gcd wall rows sit at the
`/usr/bin/time` 10 ms quantum — marked `~noise` like the rcx row."

## R3 — perf-slewonly-power entry: gcd `×4.0` caveat (swept in)

Same rule as R2, not named on the card but identical: `power` placement
RC — gcd `40 (30–40) → 10 (10–10) | ×4.0`. The drop is real (phase
timers: `power.slews` 23.5 → 7.3) but the ×4.0 magnitude is
quantum-bound. Keep the cell; extend the existing note sentence under
the table (the "Zero-wire gcd stays ~10 ms…" line) with: "the gcd
placement-RC `×4.0` carries the same 10 ms-quantum caveat — the
direction is phase-timer-backed, the magnitude is approximate."

## R4 — perf-parsers entry: ≤50 ms precision note

The phase table's ≤50 ms rows (`sta.spef.read` aes 29→21, picorv32
23→17, ibex 44→32) and the DEF tail (aes `sta.load.def` 12→10, ibex
16→13, picorv32 10→8, "uniform ~×1.2") are `Instant`-timed phase
numbers, not `/usr/bin/time` wall numbers — the 10 ms quantum does not
apply, and the recorded rep ranges are non-overlapping (verified against
the recorded logs: aes 29.1–31.4 → 20.8–23.0; picorv32 22.5–23.7 →
16.8–19.1; ibex 43.0–45.5 → 31.4–37.0). Marking them "noise" would
misstate the data. Fix: one line after the phase table — "Sub-50 ms
phase rows are `Instant`-timed, not the `/usr/bin/time` quantum the
wall cells carry; before/after rep ranges are non-overlapping, so the
speedups are measured but small-magnitude. The DEF-parse tail (≤3 ms
deltas) reads direction-only at 5 reps. The gcd `sta.spef.read` row is
below rep noise and stays unmarked (`—`)."

## R5 — perf-parsers entry + design docs: `unit_scale`, `split_ascii`, `u32 NAME_MAP`

`docs/results.md` (~L6093): "`fast-float2` for SPEF
`*D_NET`/`*CAP`/`*RES`/unit numerics" → "`fast-float2` for SPEF
`*D_NET`/`*CAP`/`*RES` numerics (`unit_scale` keeps std `parse` — once
per `*_UNIT` header line, cold)". `spef.rs:42` uses `value.parse()` —
the doc overclaims.

`docs/results.md` (~L6096): current "`split_ascii_whitespace` note:
inside a SPEF token, non-ASCII whitespace now stays part of the word
instead of splitting — such input still fails at the same line (numeric
parse or field count), never silently; all committed SPEF and error
tests are ASCII." → keep the first clause, then: "a merged token still
hits the same-line rejects when the count lands on a wrong arity, but it
can also produce a *valid-shaped* entry whose node name carries the
non-ASCII byte (`*CAP 1 a\u{a0}b 0.5` parses as a ground cap on the
merged name instead of erroring) — a format-invalid input surfacing as
a dangling node name that matches nothing else in the file, not a
silent corruption of a real net; all committed SPEF and error tests are
ASCII." (Same correction to `perf-parsers/design/spef.md` S3's note.)

`perf-parsers/design/spef.md` S4 tail: "The `unit_scale` parse
(line 29) can share it — cold anyway." → "The `unit_scale` parse keeps
std `parse` — once per `*_UNIT` header line, it never shows in a
profile."

`perf-parsers/design/steps.md` (~L35): "S2's `u32` NAME_MAP + `Cow`
`resolve` first" → "S2's `*N` digit-slice NAME_MAP key + `Cow` `resolve`
first". (`overview.md`/`measurement.md`/`spef.md` already say
digit-slice — steps.md is the only stale spot.)

## R6 — perf-dmp-hotloop entry: ranges, `clock.walk` note, ceiling math

Replace the `**Phases**` table (median-only) with the skeleton's
`median (min–max) ms` form. Ranges recomputed from the recorded logs
(`/tmp/perf-dmp-{before,after}.log`, per-rep `phase` lines, n=5 each):

| phase | cell | before | after | Δ |
|---|---|---|---|---|
| `sta.eval.walk` | jpeg `sta.placement` | 547 (539–569) | 463 (449–465) | −84 |
| `sta.prepare.clock.slew` | jpeg `sta.placement` | 580 (568–597) | 481 (468–488) | −98 |
| `sta.eval.walk` | jpeg `sta.spef` | 541 (538–552) | 446 (446–448) | −94 |
| `sta.prepare.clock.slew` | jpeg `sta.spef` | 563 (556–572) | 466 (462–498) | −97 |
| `sta.slew.walk` | jpeg `power.placement` | 543 (528–548) | 448 (445–458) | −95 |
| `power.slews` | jpeg `power.placement` | 569 (554–575) | 473 (469–483) | −96 |
| `sta.eval.walk` | ibex `sta.placement` | 160 (151–176) | 123 (123–125) | −37 |
| `sta.prepare.clock.slew` | ibex `sta.placement` | 50 (48–53) | 39 (39–40) | −10 |
| `power.slews` | ibex `power.placement` | 165 (163–166) | 133 (132–133) | −32 |
| `sta.eval.walk` | aes `sta.placement` | 123 (122–129) | 106 (103–107) | −16 |
| `sta.prepare.clock.slew` | aes `sta.placement` | 127 (124–134) | 108 (108–110) | −19 |
| `power.slews` | aes `power.placement` | 131 (127–136) | 109 (108–112) | −22 |
| `power.slews` | picorv32 `power.placement` | 90 (88–91) | 73 (72–74) | −18 |

Add after the table: "`sta.prepare.clock.walk` does not appear — it
fires zero times on ideal-clock cells (the `any_propagated ||
pulse_width_checks` gate; the absent line is data, same convention as
the baseline table). Where it fires it is a small serial tail: jpeg
`sta.placement` 7.9 (6.3–8.0) → 6.1 (5.9–7.4) ms."

In "Achieved vs. plan", replace the first sentence's prose with the
arithmetic: the counted bound is the intra-call repeat share R = 21.6%
of `table(c)` evals inside the walks (`eval_2d` is ~all of `table` —
49.9M/49.9M — so the bound applies to the DMP fraction of each walk
phase); measured phase deltas land at −13%…−23% — jpeg walks
−15…−17.5% (`eval.walk` 547→463 = −15.4%, `clock.slew` 580→481 =
−17.0%, `slew.walk` 543→448 = −17.5%), ibex `eval.walk` −23%, aes
−13.3% — consistent with the bound plus `eval_2d`'s per-call setup
(index clamps, bilinear weights) removed on every hit, which is why
some rows exceed R. Keep the existing named residuals list.

## R7 — AP3 results entry: `!state_var` claim

`docs/results.md` (~L5289): "`function == state_var` copies `P`;
`== state_var_n` or `!state_var` copies `1 - P`" → "`function ==
state_var` copies `P`; `== state_var_n` copies `1 - P`". Append in the
same parenthetical: "`function` is interned verbatim (never parsed), so
the match is exact-name — a `!IQ`-style function would not equal
`state_var_n` and lands `Unknown` (unclassifiable, never a silent
guess); the sky130 census shows every sequential output's function is
exactly `IQ` or `IQ_N`, so exact match is complete on the committed
PDK." (Code: `activity.rs` ~L280 — `func == Some(seq.state_var)` /
`func == seq.state_var_n` only; `pin.function` is interned raw,
`liberty.rs` ~L1043.)

## R8 — `phases.md` attribution drift (overview D6)

`kurena/tasks/perf-profile/design/phases.md`:

- `place.legal.abacus` row (~L85): wraps text "Step 1–3: obstacle
  subrows, coordinate conversion, cluster insertion, verify" → drop
  ", verify" — `verify_legality` runs under its own
  `place.legal.verify` scope opened after `drop(t_abacus)`
  (`legalize.rs` ~L1309–1312).
- `sta.partition.compose` row (~L38): "post-class endpoint composition +
  merged pulse eval" → "post-class endpoint composition" — the merged
  pulse eval runs after `drop(t_comp)` under its own
  `sta.eval.endpoints.pulse` scope (`analysis.rs` ~L5632–5638); the
  `sta.eval.endpoints.pulse` row already names the partitioned tail.

## R9 — "full 45-cell matrix" in design docs (overview D3)

- `perf-slewonly-power/design/measurement.md` L14 and
  `perf-legal-passes/design/measurement.md` L14: "The committed script
  runs the full 45-cell matrix." → "The committed script runs the full
  38-cell matrix (7 base cells × 5 designs + gated `sta.spef_rcx` on
  gcd/picorv32 + gated `power.vcd` on gcd)."

## R10 — `census.md` reproduce script (overview D8)

`ap3-sequential-activity/design/census.md` (~L54–72): the Python heredoc
is quoted (`<<'EOF'`) and the path is inside a single-quoted Python
string — `$PDK_ROOT` never expands. Replace
`lib = open('$PDK_ROOT/sky130A/…').read()` with:

```python
import os
pdk = os.environ.get('PDK_ROOT', os.path.expanduser('~/.ciel'))
lib = open(os.path.join(pdk, 'sky130A/libs.ref/sky130_fd_sc_hd/lib/'
                        'sky130_fd_sc_hd__tt_025C_1v80.lib')).read()
```

The `~/.ciel` fallback matches `measure.sh`'s
`PDK="${PDK_ROOT:-$HOME/.ciel}"`. Also fix the prose path mention above
the script (L4) to name the env var + default.
