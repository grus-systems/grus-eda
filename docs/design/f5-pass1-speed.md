# F5 follow-up: speed up refinement pass 1 of the legalizer

## 1. Measurement (Mac mini, 2026-11-13, `docs/results.md` F5 entry)

Legalization on picorv32 (6,709 movable cells, 173 rows) takes 1,009 ms, of
which refinement pass 1 (equal-width swaps) is 691 ms. Per-round counters for
pass 1, measured with temporary instrumentation (not committed):

| round | pairs visited | width pass | HPWL evaluations | swaps accepted |
|---|---|---|---|---|
| 0 | 21,837,151 | 3,762,532 | 26,035 | 223 |
| 1 | 22,315,063 | 3,827,989 | 26,796 | 56 |
| 2 | 22,496,721 | 3,854,741 | 26,979 | 2 |
| 3 | 22,501,986 | 3,855,761 | 26,988 | 0 |

The scan visits all `m (m-1) / 2` pairs per round to find about 27k pairs that
pass both filters (same width; `|dy| <= 4 * site_height`; `|dx| <= 25,000 dbu`)
and are actually evaluated. The cost is the scan itself, not the evaluations:
roughly 160 ms per round of pair visits against roughly 13 ms of HPWL work.

## 2. Change

Replace the all-pairs scan in pass 1 with a spatial index, keeping the pass's
semantics — including the exact sequence of evaluated pairs and therefore the
exact output — unchanged:

- At the start of each pass 1 round, build one list per distinct sub-row y
  (the y values cells can sit on), each list holding the movable cells on that
  y, sorted by x (ties by movable-cell index). Bands are addressed through the
  sorted array of distinct y values, so a query for `|dy| <= 4 * site_height`
  is a contiguous range of bands, found by binary search. No assumption that
  rows are evenly pitched.
- For cell `i` (in `movable_cells` order, as today), collect candidate partners
  from the bands in range whose x lies in `[x_i - 25_000, x_i + 25_000]`
  (binary search per band), keeping only cells after `i` in `movable_cells`
  order with the same width. Sort the candidates by `movable_cells` index and
  visit them in that order, re-checking all three filters against the live
  positions in `design_db` exactly as today's inner loop does, and evaluating,
  accepting (with the same `break`) and reverting exactly as today.
- On an accepted swap, update the index before continuing: the two cells
  exchange positions, so remove each from its band list and insert it into the
  other's band at the sorted position. (If both are in the same band, remove
  both first, then insert both.)
- Passes 2 and 3 move cells, so the index is rebuilt at every round; it is not
  maintained across passes.

Why the output is unchanged: today's inner loop visits partners in
`movable_cells` order and evaluates exactly those passing the three filters on
live positions. The index contains every movable cell at its live position
(it is rebuilt per round and updated on every accepted swap, the only mutation
inside pass 1), so the candidate set for `i` is a superset of the pairs
passing the filters; the live re-check then admits exactly the same pairs, in
the same order, with the same break-on-accept. The evaluations and the
sequence of accepted swaps are therefore identical.

Out of scope: Abacus insertion (132 ms, the next-largest item), passes 2/3,
the resync, the round count (4), and every threshold (25,000 dbu, 4 site
heights, equal width).

## 3. Acceptance

- Output identity: `place` on picorv32 and gcd (floorplan DEFs, release,
  Mac mini) produces a placed DEF byte-identical to the pre-change baseline
  (maintainer holds the baselines and their md5s; final HPWL 207,078,763 dbu
  on picorv32 and 4,401,009 dbu on gcd are necessary but not sufficient).
- Speed: legalization elapsed on picorv32 at most 450 ms (from 1,009 ms);
  pass 1's share measured with the maintainer's phase timers at most 150 ms.
- Full test suite green on the mini; `cargo clippy -- -D warnings` clean.
- A test that can fail: a unit test on a synthetic layout where a beneficial
  swap partner sits more than half the design away in `movable_cells` order
  (so an index that only looked at neighbouring indices would miss it), plus
  the existing placement tests unchanged.
- `docs/results.md` entry with before/after timings and the identity check;
  version bump (functional change).

## 4. Risks

- y values that are not a sub-row y: after Abacus every movable cell sits on a
  sub-row, and passes 2/3 keep cells on sub-rows (swaps exchange positions,
  sliding keeps y), so at pass 1 round starts every movable y is a band y.
  The implementation must still not panic if a y is missing from the band map
  (treat as a bug: fall back to including the cell in a catch-all band or
  error out — the ticket fixes the choice after the review).
- Duplicate x within a band and cells with identical (x, y) after swaps:
  ordering by (x, movable index) keeps the index deterministic; candidacy
  does not depend on intra-band order beyond the x-range search.

## 5. Review resolutions (external review, 2026-11-13)

The full review text is not included in this repository. Verdict: adopt with changes.
The reviewer confirmed the exactness argument, including that passes 2/3 and
the resync never change a cell's y. Adopted:

- **Missing-y handling (Q2, S2):** a round-start y that is not a band y is
  unreachable (Abacus writes sub-row y; pass 1 swaps exchange positions, so y
  stays a sub-row y; passes 2/3 keep y). Enforce it with a hard `assert!` at
  index build plus a comment stating the invariant — no silent catch-all band,
  which could drop pairs and quietly change the output.
- **Index update order (Q4, S2):** on an accepted swap, `design_db` already
  holds the new positions; remove both cells from their band lists before
  inserting either (covers the same-band case), then insert each at its
  sorted position in the other's band.
- **Implementation details (Q1/Q3, S3):** precompute a
  `cell -> movable_cells index` array (no HashMap in the hot loop); band
  entries carry `(x, movable index, cell)`; collect candidates into one
  reusable scratch Vec cleared per cell; sort candidates with
  `sort_unstable` by movable index.
