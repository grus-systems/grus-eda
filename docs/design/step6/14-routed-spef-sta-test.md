# Ticket 14 — Our STA on the routed SPEF vs OpenSTA, gcd (review step 6, part 2)

## Context

Ticket 13 adds `gcd.rcx.spef` (true routed extraction: 3,544 coupling
capacitor entries, 1,037 resistors, `*NAME_MAP` throughout, `*R_UNIT 1
OHM`) and a `routed_spef` mode in `gcd.sta.reference.json`. Until now the
SPEF reader's coupling-capacitor, resistor-loop and name-map paths were
implemented but never exercised against OpenSTA (`docs/e3-sta.md`
Section 5, "Not scheduled"). This ticket runs our SPEF STA on the routed
SPEF and scores it against OpenSTA on the same inputs.

The existing harness is `compare_design_with_opensta` in
`crates/grus-core/src/sta/analysis.rs`; `Rc::Spef` hardcodes
`<design>.routed.spef` (the placement estimate) and every design uses
`<design>.placed.def`.

## Task

1. Add an `Rc` variant (e.g. `Rc::RoutedSpef`) that loads
   `<design>.rcx.spef` instead, leaving `Rc::Spef` untouched.
2. Add `test_gcd_routed_spef_matches_opensta` calling the harness with
   golden key `routed_spef` and tolerance `Tol::GCD_SPEF` (the same
   tolerance as the gcd placed-SPEF test: strict arrival/required/slack,
   slew 3%).
3. Measure first, then fix: run the test and record the failure table
   (per-field max diffs, WNS/TNS ours vs reference, failing endpoint
   count) in your job report **before** changing any reader or STA code.
   If it passes as-is, say so and change nothing else.
4. If it fails, diagnose before touching code. Known candidate causes, in
   the order to check them:
   - Coupling capacitors: our reader lumps a two-node `*CAP` entry to
     ground on the first node only (E3.3 spec, Stage 1 item 2). Compare
     per-net total capacitance ours vs the SPEF sums and state what
     OpenSTA's numbers imply about its rule. Do not read OpenSTA source
     (GPL); infer from measurements only.
   - Resistor loops / parallel resistors dropped by the BFS spanning
     tree (same spec item).
   - `*NAME_MAP` references in `*CONN` or node names that the reader
     resolves differently from the placed SPEFs (which barely use it).
   - `*R_UNIT 1 OHM` handling (the placed SPEFs use KOHM).
   Fix only what the measurements indict, in the reader or the SPEF
   parasitic builder, with the rule written into `docs/e3-sta.md`
   Section 3 (E3.3 Stage 1) in the same change.
5. Prove the new test can fail: corrupt one capacitance value in a copy
   of the SPEF (or an equivalent mutation you describe), show the test
   goes red, then restore.

## Files

- `crates/grus-core/src/sta/analysis.rs` (harness variant + test)
- `crates/grus-core/src/readers/spef.rs` (only if a measured defect requires it)
- `crates/grus-core/src/sta/parasitics.rs` (only if a measured defect requires it)
- `docs/e3-sta.md` (only the E3.3 rule text for a fix actually made)

## Acceptance

- The new test passes at `Tol::GCD_SPEF`, or the job stops and reports
  the measured failure table with a named root cause. **Do not widen any
  tolerance and do not `#[ignore]` the test** — that decision is the
  maintainer's, taken with the measurements in hand (threshold discipline:
  tolerances are never fitted to measurements).
- Full workspace test suite shows no regression against the baseline
  failing set; clippy clean.
- Bump the workspace patch version in the landing commit (functional
  change).
- Maintainer acceptance: re-run the full suite on the Intel host (PDK +
  generated inputs present), read the whole diff, verify the mutation
  claim, and record the comparison in `docs/results.md` (WNS/TNS, max
  per-field diffs, runtime, cold/warm cache stated honestly).

## Round 2 — maintainer feedback (2026-11-15)

Round 1 (a delegated job) added the
`Rc::RoutedSpef` variant and the test below, then correctly stopped as
blocked: its worktree has no PDK, so the measurement could not run. That
change is **not** in HEAD (the maintainer held it back so `dev` never
carries a knowingly-red test); recreate it identically as part of this
round: enum variant `RoutedSpef` loading `<design>.rcx.spef`, plus
`test_gcd_routed_spef_matches_opensta` with golden key `routed_spef` and
`Tol::GCD_SPEF`.

Maintainer measurement on the Intel host (PDK present), round-1 code:
the test fails **before any STA runs** — our SPEF reader cannot parse
the routed SPEF at all. Panic: `routed SPEF: SPEF line 671: NAME_MAP
entry `*PORTS` has no name`. Root cause, confirmed against the file
and the code: in `gcd.rcx.spef` the `*NAME_MAP` section (entries
`*3`..`*655`) is followed by a `*PORTS` section header; in
`crates/grus-core/src/readers/spef.rs` the `in_name_map` state is only
cleared by a later section arm, and the entry arm treats any line
starting with `*` as a NAME_MAP entry, so `*PORTS` hits the F6
"entry has no name" error.

Fix direction: a NAME_MAP entry is a first token of the form
`*<digits>`; any other line whose first token starts with `*` is a new
section header — it must end the NAME_MAP section and be handled as
that section (and the `*PORTS` body lines that follow must not be
misparsed). Keep the F6 strictness: a `*<digits>` entry with no name
token must still error.

Verification you can do locally this round: parsing needs no PDK. Add
spef.rs unit tests that (a) load the committed
`tests/designs/gcd/gcd.rcx.spef` with `load_spef` and assert the parse
succeeds with 269 nets and a spot-checked mapped name resolves
(e.g. net `clk`); (b) a synthetic NAME_MAP followed by `*PORTS` parses;
(c) a malformed `*123` entry without a name still errors. The numeric
STA comparison still cannot run in your worktree; the maintainer
re-runs `test_gcd_routed_spef_matches_opensta` on Intel after your fix
and reports the failure table back if the numbers then miss. Do not
widen tolerances.
