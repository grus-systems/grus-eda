# e6-drt-e2e-picorv32 — steps (r2, docs recount)

r1 steps A–D landed (commits `8496158` + `9788e53`, v0.12.9).
This r2 plan is the diffreview-mandated docs correction — no
re-emit, no golden regen, no `.rs`/`.sh` behavioral change.

## R1. Recount script

`tests/designs/recount_unrouted.py` per `recount.md` §5. Run:

```
python3 tests/designs/recount_unrouted.py picorv32
python3 tests/designs/recount_unrouted.py gcd        # sanity leg
```

Check its output against `overview.md`'s verified table — the
values were reproduced during planning, so any divergence is a
script bug to fix, not a rebaseline.

## R2. Doc corrections

Per `docs.md`:

1. `docs/results.md` — rewrite the census paragraph (marked
   correction + method record; numbers transcribe the script
   output).
2. `docs/e6-drt.md` — rejoin the `unclassified` reflow break in
   the §9 "Test gate" paragraph.
3. `docs/status.md` — read-confirm the aggregate-only census;
   no edit expected.
4. This file — the "Post-landing notes" section below is the
   memo-requested annotation of the stale r1 item 4.

## R3. Gates + commit

`cargo test --workspace` and `cargo clippy --workspace
--all-targets -- -D warnings` (untouched code, but the
acceptance/verify contract wants the runs — record the verdict
line in the results.md entry's tail). Single `docs:` commit:
script + results.md/e6-drt.md edits + this annotation.
Conventional-commit English; trailer per AGENTS §5 (agent
`Co-Authored-By`; the diffreviewer gets `Reviewed-By` per the
global review rule).

## Post-landing notes (stale r1 checklist item 4 — the memo item)

r1 "Verify / diffreview checklist" item 4 read: "`check_drt_ours.sh`
delta = the `unrouted_kinds` fail arm only; `drt.rs` delta =
un-ignore + doc comment + per-design pins + optional ignored
determinism test; no emit-path or router changes; `.gitignore`
delta = the DEF exception line at most". It is **stale relative to
what landed** — the real diff additionally carried:

- step-E's new `unguided` derivation + per-member verifier
  (io-pin-only + guide-absent) in `check_drt_ours.sh`, plus the
  conn-whitespace fix the first build's `fail` exposed;
- `gcd.drt.ours.reference.json` regenerated with `unguided: []`;
- `drt.rs` `unguided`/viability-join changes and two picorv32
  `PINNED_LEF_RESIDUALS` entries (`li.3`=19, `met1.2`=146);
- no `.gitignore` line (the 4.9 MB DEF stayed gitignored, so no
  determinism byte-compare test — the `--verify` arm instead).

Recorded so later audits diff against landed state, not the r1
plan. Item 4's spirit (no emit-path/router changes, golden joins
pinned) did hold.

## Verify / diffreview checklist (r2)

0. `python3 tests/designs/recount_unrouted.py picorv32` —
   reproduces `overview.md`'s table exactly; `gcd` leg exits 0.
1. `cargo test --workspace` — green; same test census as the
   landed state (the recount adds no tests).
2. `cargo clippy --workspace --all-targets -- -D warnings` —
   clean.
3. Diff audit: only `tests/designs/recount_unrouted.py` (new),
   `docs/results.md`, `docs/e6-drt.md`, this file. No `Cargo.*`,
   no `.rs`, no `.sh`, no golden changes, no `.gitignore`.
4. `docs/results.md` recount numbers == script output verbatim;
   the wrong claims ("1,820 = Unattachable class", "all 196
   met2-only", "met3-bridge does not appear") are gone — grep
   the entry for `1,820`, `196`, `met3` to confirm the rewrite
   reads as designed.
5. `git diff` on the goldens is empty — the correction must not
   touch `*.reference.json`.

## Files

- `tests/designs/recount_unrouted.py` (new)
- `docs/results.md`
- `docs/e6-drt.md`
- `kurena/tasks/e6-drt-e2e-picorv32/design/steps.md` (this file)
