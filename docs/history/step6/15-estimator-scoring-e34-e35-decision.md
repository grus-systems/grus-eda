# Ticket 15 — Score both estimators against routed truth; record the E3.4/E3.5 decision (review step 6, part 3)

## Context

Review `docs/history/review-2026-11-11.md` Section 3, item 2: `estimate_parasitics
-placement` is itself an estimate; matching OpenROAD's Steiner tree makes
us agree with an estimator, not with reality. With a routed SPEF (ticket
13) both estimators can be scored against the truth, and "if the exact
Steiner tree is as close to routed timing as OpenROAD's tree, E3.4 can be
closed as 'different, not worse'."

Open items being decided (`docs/e3-sta.md` Section 5):

- **E3.4** — our placement tree (greedy MST + L-flip) differs from
  OpenROAD's on nets of 4+ pins; an exact solver (`build_precise_rsmt_points`)
  exists but is never longer than OpenROAD's and is kept out of E1.
  picorv32 placement RC is `#[ignore]`d at 9% arrival error.
- **E3.5** — the DMP driver model's `RD_SCALE = 4.5` is fitted, not
  derived; it is the named cause of the loosened picorv32 SPEF tolerances
  and of the aes/jpeg/ibex wire-mode `#[ignore]`s from step 4.

## Task (measurement code + numbers; the decision text is the maintainer's)

1. Add a measurement test (or extend the existing Steiner measurement
   test) that, on gcd, reports endpoint timing error **against the
   `routed_spef` OpenSTA golden** (arrival and slack, max path) for each
   configuration below. Print a table with `--nocapture`: per
   configuration, the median / p95 / max absolute arrival error over the
   53 endpoints, the WNS and TNS error, and the count of endpoints
   outside the strict criterion `max(5 ps, 1%)`.
   - (a) ours, placement RC, current greedy tree (the shipped path);
   - (b) ours, placement RC, exact tree (`build_precise_rsmt_points`) —
     wire it behind a test-only switch; do not change the shipped
     default and do not touch E1;
   - (c) ours, placed SPEF (OpenROAD's tree and RC, our STA/DMP);
   - (d) OpenSTA placed SPEF vs OpenSTA routed SPEF — a pure
     golden-vs-golden comparison from `gcd.sta.reference.json`, no STA
     run; this isolates the estimator difference from our STA entirely.
2. From ticket 14's record, tabulate our SPEF-mode residuals on the
   routed SPEF vs on the placed SPEF (same DMP model, different
   parasitics): if the residuals are the same size, the fitted driver
   model — not the parasitics source — is the dominant wire-mode error.
3. Licence premise check (review Section 3, item 1): inside the pinned
   container, locate and read **only the licence file(s)** that ship with
   OpenROAD and OpenSTA (e.g. `LICENSE`/`COPYING` in their install
   trees). Report which licence governs each. **Do not open any source
   file.** This only establishes whether reading OpenROAD's tree-building
   rule would even be permitted by the project's GPL rule; acting on it
   is a separate user decision, out of scope here.
4. Report all numbers in the job report, unrounded, with the exact
   commands run. Do not write the decision yourself.

## Files

- `crates/grus-core/src/sta/analysis.rs` and/or
  `crates/grus-core/src/sta/parasitics.rs` (measurement code only)
- No docs files: the maintainer writes the decision into
  `docs/e3-sta.md` Section 5, `docs/results.md` and `docs/status.md`
  from your numbers.

## Acceptance

- The measurement runs on gcd and its table is reproducible by the
  maintainer on the Intel host; configurations (a)–(d) are all present.
- No shipped behaviour changes: full suite green with the same pass/ignore
  counts as ticket 14 left, clippy clean; the exact-tree switch is
  test-only.
- Maintainer decision record (after acceptance), one of:
  - E3.4 closed as "different, not worse" — if (b) and (a) sit within
    (d)'s error band against routed truth; or kept open with the
    quantified gap and the licence finding attached.
  - E3.5 kept as scheduled work with the residual evidence, or closed,
    or re-scoped — with the routed-vs-placed residual table as evidence.
  - The aes/jpeg/ibex wire-mode sunsets are revisited in light of the
    record (they stay `#[ignore]`d unless the evidence says otherwise;
    no tolerance is widened either way).
