# Legalizer Pass 3: exact median selection and inner-scan audit

## Goal

`legalize_placement` on jpeg takes ~4.0 s; measured per round: Pass 1 ~80 ms,
Pass 2 ~355 ms, Pass 3 ~510–700 ms (x4 rounds). Pass 3 dominates. Its per-cell
inner loop collects every non-cell pin x of every incident net into
`other_xs_buf`, `sort_unstable`s it, and takes the median for one candidate
position. Replace the sort with `select_nth_unstable` (identical median
value), and audit the incident-net pin scans in Passes 2 and 3 for exact
wins. **The placement output must stay bit-identical**:
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
- jpeg `place` elapsed reported before/after.
- Prove the change can fail (`AGENTS.md` §7 rule 7): perturb the median index
  or a span and show a test or the HPWL check goes red, then restore.
- `cargo clippy --all-targets -- -D warnings` clean.

## Notes

- `select_nth_unstable(k)` yields the same element as
  `sort_unstable()[k]` for `k = len/2`; duplicates do not matter (only the
  value is used). This part is provably exact.
- Do NOT change the candidate set (`left_limit`, `right_limit`, snapped
  median), the evaluation order, or the accept rule (`<`, first best) — any
  of those changes the result and breaks bit-identity.
- Exact incremental caches are allowed and precedented: Pass 1 already
  maintains `net_ext: Vec<NetExtrema>` across in-pass mutations — rebuilt only
  for the accepted swap's incident nets (`build_net_extrema_fast`), which is
  what keeps it bit-identical. The same pattern applies to Pass 2/3 if a
  per-net x-extrema (and, for Pass 3's median, a per-net ordered structure)
  cache can be updated on each accepted move at lower total cost than the
  current full pin scans. Any such cache must be updated for every mutation
  before the next evaluation, exactly as Pass 1 does; a stale read is a bug,
  not a tolerance.
- Clock/reset nets have very high degree and are incident to most cells —
  they are the likely dominant cost; measure their share before optimizing.
- Target: legalize <= ~2.5 s on jpeg with the constant-factor changes alone;
  if exact incremental caching is needed to get there, implement it in this
  ticket only if it stays bit-identical — otherwise record the measured floor
  and leave the algorithmic change as a follow-up decision.
- Temporary instrumentation must be removed before commit.
