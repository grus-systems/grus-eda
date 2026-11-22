# perf-slewonly-power — measurement

## Protocol

Same machine (M4 mini), same flags as the v0.3.3 baseline, idle
machine (the first perf-par-slew log was inflated by a background
build — run nothing else):

```sh
cargo build --release
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-slewonly-power.log
```

The committed script runs the full 45-cell matrix. Run it whole —
the `sta` cells double as a no-regression check on the shared
`prepare_sta`/`eval_seed_state` refactor, and the run is the honest
"what was run" for `docs/results.md`. If only the `power` cells are
rerun for time reasons, record exactly which cells and why
(AGENTS §7.9).

Before = v0.3.3 numbers. Where the perf-par-slew entry already tabled
them (all `power` placement-RC cells), reuse the tabled column and say
so; for cells it did not table (`power` zero-wire, `power --vcd` gcd),
measure the pre-change v0.3.3 binary in the same session — step 0 of
[`steps.md`](steps.md) builds it and runs `measure.sh` before any edit —
and record the mixed provenance (AGENTS §7.9).

## Ceiling

v0.3.3 jpeg `power` placement RC: 2170 ms wall; `power.slews` envelope
1824 ms = `sta.prepare.clock.slew` 901 + `sta.eval.walk` 830 + ~90
prepare/seeds.

Slew path removes (all-ideal SDC): the whole clock block (~901+),
endpoint collection, `sta.assemble`, arrival bookkeeping inside the
walk. It keeps the prep tail and a slew replay ≈ `sta.eval.walk`
minus arrival merge arithmetic (same `driver_response` calls — the
dominant term — so expect ~0.8–0.95× of 830, not a fraction).

Ceiling estimate: `power.slews` → ~0.85–1.0 s → jpeg `power`
~1.2–1.35 s — consistent with the plan's ~1.3 s residual. Gains on
the other designs scale with their `clock.slew` share (aes ~200,
picorv32 ~130, ibex ~75, gcd ~12 ms post-par-slew).

Named residuals for the results entry: (1) the slew replay itself —
`sta.slew.walk` is the new dominant `power` phase; it does the same
`driver_response` lookups as `eval.walk`, so further gains are rank-4
work (finer level granularity / NLDM-DMP table caching), not this
ticket; (2) `power.cells` (~63 ms jpeg) + `power.activity.resolve`
(~37) + DEF/SDC loads — the post-slew floor; (3) on `any_propagated`
SDCs the clock block remains by necessity.

## Phase keys moved

Record in the results entry, not silently:

- `sta.prepare.clock.slew` / `sta.prepare.clock.walk`: absent under
  `power.slews` on the committed (all-ideal) SDCs; still fire for
  `any_propagated` and under `sta` as before.
- `sta.prepare.clock.seed`: once (ideal seeding) instead of twice.
- `sta.eval.seeds`/`sta.eval.walk`/`sta.eval.endpoints`,
  `sta.assemble`, `sta.partition.*`: gone under `power`.
- New: `sta.slew.seeds` (seed replay), `sta.slew.walk` (level walk);
  `power.slews` envelope unchanged in name, smaller in value.

## `docs/results.md` entry skeleton

```
### 2026-11-22 — perf-slewonly-power: slew-only sta_pin_slews (v0.3.4)

`power` no longer buys `eval_sta`'s arrival walk, endpoint collection
or the propagate_clock block for pin slews: `sta_pin_slews` runs the
shared prepare (clock block re-gated on `any_propagated` — pulse-width
checks never fed slew), the shared seed state, and a slew+reached-only
replay of the eval legs; `latch_borrow` edges off clock nets fall back
to the eval tail (arrival-coupled window; ibex has one — a
`_21829_/Q` data-latch borrow — and takes the fallback).
Design: `kurena/tasks/perf-slewonly-power/design/`.

**Before/after wall** — median (min–max) ms, REPS=5, M4 mini, same
flags; before = v0.3.3 (tabled column where the par-slew entry
provides it, same-session pre-change run otherwise):

| cell | before | after | speedup |
|---|---|---|---|
| `power` placement RC — gcd | 40 | … | |
| `power` placement RC — picorv32 | 350 | … | |
| `power` placement RC — aes | 490 | … | |
| `power` placement RC — ibex | 440 | … | |
| `power` placement RC — jpeg | 2170 | … | |
| `power` zero-wire — jpeg | … | … | |
| `power --vcd` gcd | … | … | |

**Phase** (`power --parasitics placement`, jpeg, median ms):
`power.slews` 1824 → …; nested `sta.prepare.clock.slew` 901 → absent,
`sta.eval.walk` 830 → `sta.slew.walk` ….

**Achieved vs plan**: plan estimated power → ~1.3 s (rank 2); landed
…. Residual: (1) `sta.slew.walk` …, (2) `power.cells`/activity/loads
…, (3) `any_propagated` keeps the clock block.

**Parity/equivalence**: slew-only `Vec<PinSlew>` == full-run eval state
`Vec<PinSlew>` field-wise on gcd×3 modes, picorv32×2, aes, ibex, jpeg,
`*.propagated.sdc`, microbench, an exceptions fixture, and the
`latch_borrow` fixture (via the fallback); all power goldens and
`tests/designs/**` clean; fail-proof: <mutations> → <red tests> →
restored. `cargo test --workspace` (+n), `cargo clippy … -D warnings`,
`cargo test --release -p grus-core sta` (…, 2 ignored) green.

**What was run**: `REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-slewonly-power.log` (full matrix / power-cell subset —
state which; raw log not committed, script is).
```

## `docs/roadmap.md` N8 update

`perf-slewonly-power` → done with the measured jpeg `power` number;
note `sta.slew.walk`/`sta.eval.walk` as the remaining co-dominant
phases feeding rank 4, and that `power.slews` no longer inherits
`sta.*` eval phases (update the "inherits" phrasing if present).
