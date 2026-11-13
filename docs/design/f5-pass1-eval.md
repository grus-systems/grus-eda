# F5 follow-up 2: exact fast evaluation for legalizer pass 1 swaps

## 1. Measured cost (Mac mini, 2026-11-14; see `f5-pass1-speed.md` Section 6)

With the spatial index in place, pass 1 per round on picorv32 is ~3 ms of
candidate collection and ~115 ms of swap evaluations: ~27k evaluations, each
computing `net_hpwl_dbu` (a full scan of every pin of every incident net)
twice, ~26M pin visits per round. High-fanout nets dominate — a trial swap
of any flip-flop re-scans the clock net end to end. Legalization total is
~1,000 ms; the non-pass-1 share (Abacus ~132 ms, passes 2/3 ~142 ms, resync
~36 ms) bounds what this fix can win: pass 1 at ~15 ms/round gives a total
of roughly 550 ms.

## 2. Change

Evaluate pass 1 trial swaps against a per-net extrema cache instead of full
pin scans. All arithmetic stays integer; every computed HPWL is exactly the
value `net_hpwl_dbu` returns, so accept/reject decisions — and the placed
output — are unchanged.

- **Cache**, built at the start of each pass 1 round from committed
  positions (one pass over all pins of all nets; passes 2/3 move cells
  between rounds, so the cache is not carried across rounds). Per net:
  pin count, and per axis (x, y) the two smallest and two largest pin
  positions, each with the pin it belongs to (top-2 by value over distinct
  pins; ties keep both pins).
- **Before sum**: the cached `(max_x - min_x) + (max_y - min_y)` per
  incident net (0 for nets with at most 1 pin), summed as today. No scan.
- **After sum**: write the trial positions to `design_db` exactly as today.
  Per incident net, adjust the cached extrema: the moved pins are the pins
  of the two swapped cells on that net. Per axis and direction, the
  extremum over the unmoved pins is the first of (top-1, top-2) whose pin
  is not moved; if both are moved and other pins remain, fall back to
  today's full scan of that net (`net_hpwl_dbu`, trial positions already in
  `design_db`). If no pins remain unmoved, the extrema are those of the
  moved pins alone. The after-extrema are then the min/max of the unmoved
  extrema and the moved pins' trial positions (read via `pin_pos_dbu`,
  which sees the trial positions and orientations). Combine per axis as
  `net_hpwl_dbu` does.
- **Accept**: rebuild the cache entries of the pair's incident nets from
  the committed positions (a full scan of those nets only). **Reject**:
  revert the positions as today; the cache was never touched by the trial.
- The incident-net list construction (Vec + dedup) is unchanged in this
  ticket, as are the index, the passes 2/3, the resync, the round count
  and all thresholds.

Exactness of the removal step: for one axis and direction, the extremum
over remaining pins is the first not-moved entry of the top-2, because
every other pin is >= top-2 (for min; symmetric for max). It fails to
determine the extremum only when both top-2 pins moved and a third pin
remains — exactly the fallback condition. Ties are safe because the top-2
holds two distinct pins even at equal values.

## 3. Acceptance

- Output identity: `place` on picorv32 and gcd (release, Mac mini) byte-
  identical to the same pre-F5 baselines (final HPWL 207,078,763 /
  4,401,009 dbu).
- Speed: legalization elapsed on picorv32 at most 650 ms; maintainer phase
  timers show pass 1 evaluation at most 30 ms per round.
- A unit test that can fail: a differential test (no PDK) that builds
  random small designs (multiple pins per cell on a net, both swapped
  cells on one net, IO pins, ties at extrema, 2-pin nets) and asserts the
  cached evaluation's before/after sums equal the full-scan sums for every
  trial swap, including configurations that force the fallback. Plus a
  targeted case where both top-2 minima of a net belong to the swapped
  cells and a third pin remains (fallback path), asserting the exact sum.
- Full test suite green on the mini; clippy clean; version bump;
  `docs/results.md` updated with before/after.

## 4. Risks and worst case

- Worst case is adversarial (every evaluation falls back): cost is today's
  evaluation plus the per-round cache build (one pass over all pins,
  ~40k pin positions on picorv32 — sub-millisecond to a few ms) and the
  accept-path rebuilds (281 accepts; incident nets rescanned once each).
  No asymptotic regression.
- The cache is pass 1 round state only; nothing else reads it. Passes 2/3
  keep full scans in this ticket.
- Small designs (gcd) may see no gain; identity, not speed, is their
  acceptance criterion.
