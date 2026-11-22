# async-latch-delay — Remaining steps (review-2 fixup)

The implementation landed as `e530643` + `76ae0c4` + `a356baf` and is
STRICT-green (`docs/results.md` "path delays on async-check and latch-D
endpoints (v0.1.95)"). The second human review rejected on one
docs-only point: three reachable-but-ungoldened delay-check semantics
must be recorded so the coverage claim is honest per AGENTS.md §7.4.
Everything below is that delta — do not touch code, fixtures, goldens,
`docs/timing-exceptions.md` (the skip surface did not change), or the
version (`0.1.95` already shipped the functional change).

## Step 1 — `docs/path-delays.md` §5 known-gaps note

Append the "Known gaps" block from `known-gaps.md` ("Proposed wording —
`docs/path-delays.md` §5") directly after the `latch-borrow delay
fixture` resolved bullet, before `Still open:`. Keep the file's ~66-col
wrap and bullet style; the `Still open` list (two-clock-domain fixture)
is unchanged.

## Step 2 — `docs/results.md` `### Not run` mirror

Append the three one-line bullets from `known-gaps.md` ("Proposed
wording — `docs/results.md`") to the `### Not run` list of the
"2026-11-19 — path delays on async-check and latch-D endpoints
(v0.1.95)" entry, after the existing edge0/two-clock/clock-gate items.

## Step 3 — Verify & commit

- Docs-only change: `cargo test -p grus-core sta_edge` (cheap) and
  `cargo clippy --workspace --all-targets -- -D warnings` to confirm
  nothing regressed; the full workspace suite was green at `a356baf`
  and is not affected by doc edits.
- One `docs:`-type commit (the kurena pipeline commits as
  `kurena: implement async-latch-delay`), explicit path staging of the
  two doc files plus the design dir, agent trailer. No version bump —
  docs-only, and `0.1.95` already shipped with the functional change.

## Prior fixups (executed — kept for the record)

- Review 1 (docstring sign): `test_sta_edge_latch_borrow_delay_max`'s
  docstring claimed `pulse + t_setup`; corrected to `pulse - t_setup`
  in `76ae0c4`, with an `edge0_pol` contested-parity comment and the
  residual negative-control probe outputs committed in `a356baf`.

## Full procedure (executed — kept for the record)

Step 0 (fixtures + committed `generate_ref.sh` regen in the container),
Step 1 (`api.md` implementation), Step 2 (11 STRICT `compare_sta_edge`
tests + fail-proof), Step 3 (`docs/results.md` entry), Step 4
(`docs/path-delays.md` §1/§3/§5 + `0.1.95` bump) all ran in `e530643`;
the regenerated goldens agreed with `probe.md`'s record. If a future
change invalidates the fixture, rerun those steps in that order —
evidence first (`goal.md` "Measure first"), implementation second.
