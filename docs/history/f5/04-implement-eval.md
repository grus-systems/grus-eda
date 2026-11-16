# Ticket 04 (F5): extrema cache for pass 1 swap evaluation

## Goal

Implement `docs/history/f5-pass1-eval.md` including the review resolutions
in its Section 5 (external review),
on top of the spatial index from ticket 02 (already in the tree as
uncommitted changes the maintainer applied — work with the current state
of `legalize.rs`, where pass 1 is `equal_width_swap_pass`).

In short: build a per-net extrema cache at each pass 1 round start
(top-2 pin positions per axis and direction, with the owning pin); take
the before sum from the cache; write trial positions as today and take
the after sum by adjusting the cached extrema over the moved pins, with
a full `net_hpwl_dbu` scan of a net only when both top-2 pins of a
direction moved and an unmoved pin remains; on acceptance rebuild the
cache entries of the pair's incident nets; count fallback evaluations
into `LegalizeResult.pass1_eval_fallbacks` (new field, total over the 4
rounds; CLI output unchanged).

## Files

- `crates/grus-core/src/placement/legalize.rs` (cache, evaluation,
  `LegalizeResult` field, tests)
- `docs/results.md` (extend the F5 follow-up entry with the eval change
  and this machine's test results; leave the mini-acceptance items
  listed as remaining)
- `Cargo.toml` / `Cargo.lock` (version bump 0.1.23 → 0.1.24)

## Interface

`legalize_placement` keeps its signature; `LegalizeResult` gains the one
field above. The index, passes 2/3, the resync, the round count, all
thresholds and the incident-net list construction are untouched.
`net_hpwl_dbu` stays as the fallback and the reference for tests.

## Acceptance

Run by you (no PDK on this machine; PDK tests fail on missing files —
expected, report as such):

- [ ] Differential unit test (no PDK): random small designs — cells with
      several pins on one net, both swapped cells on the same net, IO
      pins, ties at extrema (including a 3+-way tie), 2-pin nets, a net
      whose pins are all on the swapped cells — asserting the cached
      evaluation's before/after sums equal the full-scan
      (`net_hpwl_dbu`) sums for every trial swap, and at least one
      configuration that forces the fallback (both top-2 minima on the
      swapped cells, third pin unmoved) asserting the exact sum and
      that the fallback counter increments. Prove the test can fail
      (e.g. make the removal step ignore the second extremum, see red,
      restore) and say so in the job report.
- [ ] The two pass 1 tests from ticket 02 and the rest of the PDK-free
      placement tests pass; name them in the report.
- [ ] `cargo clippy -- -D warnings` and `cargo build --release` pass.

Run by the maintainer on the Mac mini from your patch:

- `place` on picorv32 and gcd byte-identical to the pre-F5 baselines.
- Legalization on picorv32 at most 650 ms; pass 1 evaluation at most
  30 ms per round by phase timers; fallback count reported.
- Full `cargo test --release` green.

## Notes

- Exactness is the point of this ticket: every HPWL the evaluation
  produces must equal `net_hpwl_dbu` on the same positions. If a case
  seems to need an approximation, stop and report it instead.
- Do not commit; the maintainer commits after the mini acceptance run.
  (Tickets 02 and 04 land as separate commits; the maintainer handles
  the split.)
