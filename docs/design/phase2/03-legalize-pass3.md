# Legalizer Pass 3: exact median selection and inner-scan audit

## Goal

`legalize_placement` on jpeg takes ~4.0 s; measured per round: Pass 1 ~80 ms,
Pass 2 ~355 ms, Pass 3 ~510–700 ms (x4 rounds). Pass 3 dominates. Its per-cell
inner loop collects every non-cell pin x of every incident net into
`other_xs_buf`, `sort_unstable`s it, and takes the median for one candidate
position. Replace the sort with `select_nth_unstable` (identical median
value), and audit the incident-net pin scans in Passes 2 and 3 for exact
constant-factor wins. **The placement output must stay bit-identical**:
`final_hpwl_dbu` unchanged on gcd, aes, picorv32, ibex, jpeg.

## Files
- crates/grus-core/src/placement/legalize.rs

## Interface

No public API change. `LegalizeResult` fields unchanged.

## Acceptance

- `cargo test --release -p grus-core placement::` green, including
  `test_jpeg_place_and_legalize` and the pass-1 equivalence tests.
- `grus-eda place` on all 5 floorplan DEFs: `final_hpwl_dbu` bit-identical to
  the `v0.1.52` recorded values (gcd 4,395,223; aes 349,495,091; picorv32
  212,135,994; ibex 418,367,448; jpeg 1,018,912,430) and `verify_legality`
  passes.
- jpeg `place` elapsed reported before/after; target legalize <= ~2.5 s.
- Prove the change can fail: perturb the median index or a span and show a
  test or the HPWL check goes red, then restore.

## Notes

- `select_nth_unstable(k)` yields the same element as
  `sort_unstable()[k]` for `k = len/2`; duplicates do not matter (only the
  value is used). This part is provably exact.
- Do NOT change the candidate set (`left_limit`, `right_limit`, snapped
  median), the evaluation order, or the accept rule (`<`, first best) — any
  of those changes the result and breaks bit-identity.
- Pass 2 scans all pins of merged incident nets per adjacent pair; Pass 3
  scans them per cell. Exact sub-linear schemes (per-net extrema caches) are
  **out of scope**: positions mutate inside the pass, so caches are stale and
  results would differ. Constant-factor only (fewer passes over the pins,
  avoiding re-`pin_pos` where geometry is unchanged, `select_nth_unstable`).
- Clock/reset nets have very high degree and are incident to most cells —
  they are the likely dominant cost; measure their share before optimizing.
- Temporary instrumentation must be removed before commit.
