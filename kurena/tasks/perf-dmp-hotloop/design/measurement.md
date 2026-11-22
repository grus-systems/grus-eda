# perf-dmp-hotloop — measurement protocol and results entry

## Protocol

Same machine (M4 mini), idle, same session — per the established
convention (`perf-parsers`, `perf-legal-passes` entries):

```sh
# before — pre-change tree
cargo build --release
cp target/release/grus-eda /tmp/grus-eda.before
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-dmp-before.log
# also emits /tmp/{jpeg,ibex,aes,gcd,picorv32}.sta.json via the sta cells —
# copy the placement-RC ones aside for the cmp gate (steps §0 runs them
# by hand since all sta cells share one --json path).

# after — implementation tree
cargo build --release
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-dmp-after.log
```

The full matrix is the honest "what was run"; the
`place`/`congestion`/zero-wire cells double as the no-regression
control (untouched paths, should sit within noise).

## Ceiling

Measured-share bound, not a guess: the walk phases
(`sta.prepare.clock.slew`, `sta.prepare.clock.walk`,
`sta.eval.walk`, `sta.slew.walk`/`power.slews`) shrink by at most
`R` × (their DMP fraction) where `R` is the counted exact-repeat
share. Report achieved vs that bound. Reference numbers at v0.3.6
(jpeg): `sta.placement` wall 1,320 ms; `sta.eval.walk` ≈ 850 ms;
`power.placement` 720 ms; `sta.slew.walk` ≈ 771 ms; effective
`rayon` k ≈ 5.4/10 — memoization removes work, it does not add
parallelism, so a high hit rate also relieves the memory-bound
saturation.

Expected residuals to name in the entry:

- unique (non-repeat) `driver_response` calls — the solve cost
  that remains;
- `rc_crossing` Brent share — recorded, unfixable under
  bit-identity ([`counts.md`](counts.md));
- `sta.prepare.clock.walk` and `driver_input_slew` — small
  serial tails;
- `power`: everything outside `power.slews` (activity resolve,
  cells, report).

## Phase keys

No new `GRUS_PHASE_TIMING` keys. If a memo-hit counter is wanted
in the log line it goes into the results entry from the
`#[cfg(test)]` stats or a one-off print — not a new phase.

## `docs/results.md` entry skeleton

```
### 2026-11-22 — perf-dmp-hotloop: <mechanism name> (v0.3.7)

Counts that picked the mechanism (ephemeral counters, jpeg
`sta`/`power` placement RC, reverted after recording):
- driver_response calls: <total> (pi <pi>, lumped <l>); unique
  exact keys <u> → exact-repeat share <R>% (per-walk table:
  clock.slew <…>, clock.walk <…>, eval.walk <…>, slew.walk <…>,
  seeds <…>)
- intra-call table() evals: <total>, repeated <r> (<share>%)
- rc_crossing: <total>, closed-form <c> (<share>%), brent <b>,
  bisect-fallback <f>; pi_waveform_slew fast-path <…>
- eval_2d total <…>

Implemented: <mechanism> — <one line>.

**Phase before/after** — median (min–max) ms, REPS=5, same M4 mini:
| phase | cell | before | after | speedup |
| sta.prepare.clock.slew | jpeg sta.placement | … | … | … |
| sta.eval.walk | jpeg sta.placement | … | … | … |
| sta.slew.walk / power.slews | jpeg power.placement | … | … | … |

**Cell wall**: sta.placement.jpeg <b> → <a>, sta.spef.jpeg …,
power.placement.jpeg …; controls (place.*, congestion.*, zerowire)
within noise.

**Achieved vs residual**: repeat share <R>% removed <…>; residual —
<unique solves + brent share + serial tails named>.

**Equivalence**: `cmp`-clean --json jpeg/ibex placement-RC
pre/post; RAYON_NUM_THREADS 1/4/10 cmp-clean jpeg; fail-proof —
<mutation> → <test/cmp> red → restored. `cargo test --workspace`
(<n>), clippy -D warnings clean, `cargo test --release -p
grus-core sta` (<n>).

**What was run**: …
```

## `docs/roadmap.md` N8 update

Append `perf-dmp-hotloop` to the landed-ticket list in §N8 with
the achieved jpeg numbers; re-state what is now dominant (the
residual solve set + the next phase on the ranked list).
