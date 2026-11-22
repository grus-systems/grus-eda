# perf-legal-passes — algorithmic speedup of `place.legal.pass1/2/3`: overview

`place.legal.pass1/2/3` is the largest non-parser serial block.
Measured v0.3.2 baseline (`docs/results.md` 2026-11-22 perf-profile
entry; the legalizer code path is unchanged through v0.3.4 — perf-par-slew
and perf-slewonly-power touched `sta/analysis.rs` only): jpeg `place`
4,190 ms wall; `place.legal.pass1` 421 + `pass2` 619 + `pass3` 783 =
1,823 ms ≈ 44% of the cell (`place.legal.abacus` 43, resync/pre/verify
small). Plan rank 3 ceiling: ×~1.3 on `place` wall → ~3.2 s
(`kurena/tasks/perf-profile/design/plan.md`).

## Where the time goes (code-read cost model)

All three passes are serial, ordered, trial/evaluate/accept over live
positions — **no rayon** (acceptance). The dominant term in every pass
is the same: each trial evaluation rescans every pin of every incident
net, so a move touching a high-fanout net (clock/reset/scan-enable —
incident to most cells on jpeg, 46,568 nets) costs O(net degree) per
*evaluation*:

- **pass 2** (`legalize.rs` `sub_rows` adjacent-swap loop): per pair,
  `merge_sorted_nets` + a full `net_pins` scan per incident net —
  ~60k pairs/round × Σ incident degree.
- **pass 3** (single-cell slide): per cell, a full pin scan of every
  incident net to fill `other_xs_buf` (median candidate) and
  `net_spans_buf` — ~60k cells/round × Σ incident degree.
- **pass 1** (`equal_width_swap_pass_fast`): evaluations are already
  cached (`evaluate_trial_swap_ro` + `NetExtrema`), but it still pays
  (a) a full-net `build_net_extrema` sweep at the start of **every**
  round (46k nets × 4) and (b) a full rescan of **every** incident net
  on **every** accepted swap — including the clock net when a flop at
  its edge moves; and (c) candidate collection scans every same-band
  entry in the ±25,000 DBU x-window regardless of width.

## What lands

Three mechanisms, all exact w.r.t. the evaluated objective (see
[`algorithm.md`](algorithm.md) for the update rule and its exactness
argument):

1. **One shared, incrementally maintained `Vec<NetExtrema>`** for the
   whole refinement block. Built once after Abacus positions are
   committed (inside `place.legal.pre`), then kept exact across every
   accepted move in passes 1–3 and across all four rounds — never
   rebuilt per round. Update rule per moved pin per axis: if the pin
   occupies a maintained top-2 slot, rebuild that net's extrema by full
   rescan (rare — ~4 slots vs. net degree); else insert the new value
   into the top-2. This deletes pass 1's per-round build *and* its
   accept-time incident-net rescans (today: full rescan of every
   incident net on every accept; new: rescan only nets where a moved
   pin was actually extremal).
2. **Pass 2/3 evaluations become O(moved pins)** via surviving-extremum
   reads + moved-pin positions — the same trick pass 1 already uses —
   with a full rescan of that net only as fallback when both top-2
   pins of a direction are moved. No change to pair/cell visit order,
   candidate moves, or the strict `<` accept rule.
3. **Pass 3 candidate set: median → exact argmin.** The objective
   `Σ_net (o_hi.max(c_hi+dx) − o_lo.min(c_lo+dx))` is a convex
   piecewise-linear function of `dx` with two kinks per net
   (`dx = o_lo − c_lo`, `dx = o_hi − c_hi`). Candidates become
   `{left_limit, right_limit}` plus `snap_down`/`snap_up` of each kink
   position — an exact site-grid argmin that weakly dominates the old
   3-point sample (limits + median) per cell. `other_xs_buf` and its
   O(Σ degree) median scan disappear.
4. **Pass 1 band index rekeyed `(width, x, mi, cell)`**: candidate
   collection scans only same-width in-window entries — a strict
   subset scan; the evaluated set and order are unchanged (candidates
   are still sorted by `mi`, all live filters still applied).

Expected shape: pass 1/2 stay bit-identical to the v0.3.4 output by
construction (same evaluated values, same order, same accepts); pass 3
may legitimately diverge through its larger, dominating candidate set.
Combined estimate: 1,823 → ~450–700 ms → jpeg `place` ~2.8–3.1 s,
against the ×1.3 ceiling of ~3.2 s.

## Equivalence bar (from `spec/goal.md` — deliberately not bit-identity)

- `verify_legality` passes on every committed design's output (gated by
  the existing `test_*_place_and_legalize` tests);
- per-design legalized **HPWL not worse** than the v0.3.4 serial
  baseline on gcd/picorv32/aes/ibex/jpeg — before/after deltas recorded;
  a regression >0.1% must be named and justified in `docs/results.md`;
  any design pushed outside its 5% HPWL-vs-OpenROAD golden tolerance is
  a hard fail (the E2 tests already assert this);
- deterministic: identical output across repeated runs — recorded;
- all committed E2 goldens green;
- passes stay serial — the only `par_iter` is the one-time extrema
  build (pre-existing, deterministic per-net collect, not an
  ordered-move pass).

## Invariants

- `GRUS_PHASE_TIMING` keys unchanged: `place.legal.pre`,
  `place.legal.pass1`, `place.legal.resync`, `place.legal.pass2`,
  `place.legal.pass3`, `place.legal.verify`, `place.legal.abacus` all
  still fire. The one-time extrema build folds into `place.legal.pre`.
- Pass semantics unchanged: 4 rounds; pass-1 filters (equal width,
  ±4 site heights, ±25,000 DBU, `mi2 > i`, break-on-accept); pass-2
  exact-fit gate (`new_xa + wa <= xb + wb`); pass-3 non-overlap slide
  inside `[left_limit, right_limit]`; strict `<` accept everywhere.
- `LegalizeResult` gains `refine_extrema_rebuilds` (rescan count for the
  results entry's residual analysis); `pass1_eval_fallbacks` keeps its
  meaning. CLI output unchanged.
- Out of scope: Abacus, GP, round count, thresholds, `rayon` on the
  passes, `sta`/`power`/`congestion`.

## Documents

- [`algorithm.md`](algorithm.md) — shared extrema cache, the incremental
  update rule and its exactness argument, per-pass specs.
- [`verification.md`](verification.md) — differential tests, fail-proof
  mutations, determinism runs, HPWL gate.
- [`measurement.md`](measurement.md) — protocol, ceiling, `results.md`
  skeleton, roadmap update.
- [`steps.md`](steps.md) — implementation order and gates.

## Files

- `crates/grus-core/src/placement/legalize.rs`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/e2-placement.md`
- `kurena/tasks/perf-legal-passes/design/`
- `Cargo.toml`
- `Cargo.lock`
