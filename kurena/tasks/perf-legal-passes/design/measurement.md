# perf-legal-passes — measurement

## Protocol

Same machine (M4 mini), same flags as the v0.3.2 baseline, idle
machine (a background build inflated an earlier perf log — run
nothing else):

```sh
cargo build --release
REPS=5 sh kurena/tasks/perf-profile/measure.sh /tmp/perf-legal-passes.log
```

The committed script runs the full 38-cell matrix (7 base cells × 5
designs + gated `sta.spef_rcx` on gcd/picorv32 + gated `power.vcd` on
gcd). Run it whole on the
post-change binary — it is the honest "what was run" and the
`sta`/`power`/`congestion` cells double as a no-regression check (they
should be unchanged within noise; the ticket touches only
`legalize.rs`).

**Before**: two sources, both recorded:

- `place` cells: the pre-change binary's `place.<design>` lines from
  the step-0 log ([`steps.md`](steps.md) §0) — `measure.sh` has no
  cell filter, so step 0 runs the five `place` commands by hand with
  `GRUS_PHASE_TIMING=1` into `/tmp/perf-legal-before.log`, and the
  five `/tmp/<d>.out.def` files are copied aside for optional `cmp`.
- Everything else: the v0.3.2 baseline tabled column — legalizer code
  is byte-identical between v0.3.2 and this branch's base (v0.3.3 and
  v0.3.4 touched `sta/analysis.rs` only); state that in the entry.
  The `out.def` files from step 0 additionally give a same-session
  bit-comparison for passes 1/2 (see verification §6).

## Ceiling

Baseline `place` wall and phase medians (jpeg, v0.3.2):

| phase | ms |
|---|---|
| `place.gp.*` (density+wl+step+init) | 2,122 |
| `place.legal.pass1` | 421 |
| `place.legal.pass2` | 619 |
| `place.legal.pass3` | 783 |
| `place.legal.abacus` | 43 |
| `place` wall | 4,190 |

Plan ceiling (rank 3): ×1.3 → ~3.2 s. Design estimate: passes
1,823 → ~450–700 ms → jpeg `place` ~2.8–3.1 s. Report achieved vs the
×1.3 ceiling, not vs the estimate.

Named residuals expected for the entry: (1) `place.gp.*` untouched —
~51% of the new wall; (2) remaining pass cost — candidate enumeration
and moved-pin walks scale with per-cell net counts, not net degree;
(3) `place.legal.resync` + `pin_x` rebuild ×4 + `pre` + `verify` tails;
(4) `refine_extrema_rebuilds` — rescan count and its size class.

## Phase keys

Unchanged — `place.legal.pre` now includes the one-time extrema build
(it previously ran 4× inside pass1); `place.legal.pass1/2/3`,
`place.legal.resync`, `place.legal.abacus`, `place.legal.verify` all
still fire. No new keys.

## `docs/results.md` entry skeleton

```
### 2026-11-22 — perf-legal-passes: incremental extrema + argmin slide (v0.3.5)

`place.legal.pass1/2/3` keep their ordered serial structure but stop
rescanning incident nets: one shared `NetExtrema` cache is built once
and maintained exactly across all accepted moves (insert-into-top-2,
full rescan only when a moved pin held a maintained slot — N rebuilds
on jpeg); pass 2/3 evaluations cost O(moved pins) instead of O(net
degree); pass 3's median-of-other-pins candidate is replaced by the
exact site-grid argmin of the convex piecewise-linear span sum
(limits + snap_down/snap_up of each per-net kink); pass 1's band index
is rekeyed (width, x) so collection scans same-width entries only.
Design: `kurena/tasks/perf-legal-passes/design/`.

**Before/after wall** — median (min–max) ms, REPS=5, M4 mini:
| cell | before | after | speedup |
| `place` — gcd / picorv32 / aes / ibex / jpeg | … | … | … |

**Phase** (jpeg `place`, median ms): pass1 421 → …, pass2 619 → …,
pass3 783 → …, abacus 43 → …, resync …, pre …, verify ….

**HPWL per design** (final_hpwl dbu, before → after, Δ%):
gcd …; picorv32 …; aes …; ibex …; jpeg … — all within ±0.1% / <name
any regression + justification>; vs OpenROAD golden gaps … (≤5%).

**Achieved vs. ceiling**: plan ×1.3 → ~3.2 s; landed … s. Residual:
(1) gp.* untouched …, (2) pass cost now scales with per-cell net
counts …, (3) resync/pre/verify tail …, (4) rebuilds N …

**Equivalence**: verify_legality + E2 tests green; out-def cmp for
passes 1/2 <clean / divergent-explained>; determinism 2× jpeg +
picorv32 cmp-clean, RAYON_NUM_THREADS=1 cmp-clean; fail-proof
<mutations → red tests → restored>. `cargo test --workspace` (+n),
clippy -D warnings, `cargo test --release -p grus-core sta` (…).

**What was run**: `REPS=5 sh kurena/tasks/perf-profile/measure.sh
/tmp/perf-legal-passes.log` (full matrix / place subset — state
which); before = /tmp/perf-legal-before.log place cells + v0.3.2
tabled column (legalizer unchanged since v0.3.2).
```

## `docs/roadmap.md` N8 update

Mark `perf-legal-passes` done with the measured jpeg `place` number
and the pass table; note `place.gp.*` (~51% of `place` wall after this
ticket) as the dominant remaining `place` phase. Keep the
"candidates, not commitments" phrasing for the parser tickets.

## `docs/e2-placement.md` sync (AGENTS §7.8 — same change)

- §4 item 3's candidate description ("median x of connected pins")
  → the kink/argmin candidate set.
- §4 item 1: note the `(width, x)`-keyed band index.
- The `LegalizeResult` listing is stale already (missing
  `pass1_eval_fallbacks`): restore both telemetry fields while there.
- Note the shared incremental `NetExtrema` cache replacing per-round
  rebuilds and per-evaluation full scans.
