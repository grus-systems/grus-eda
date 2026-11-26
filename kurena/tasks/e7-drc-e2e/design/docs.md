# e7-drc-e2e — documentation updates

All repository text in English; docs move in step with the code in
the same change (AGENTS §7.8).

## `docs/e7-drc.md`

Append ticket 5 (`e7-drc-e2e`) after §15, continuing numbering:

- **§16 `grus-eda drc`** — args (`--tlef/--lef/--def/--reference/
  --json`, no `--lib`), the run pipeline (load → assemble →
  `run_all`), stdout layout (per-rule census + findings + claim
  boundary + deltas), JSON schema pointer to `drc::report` types.
- **§17 The claim boundary, machine-readable** — the 55/278/223
  split restated as shipped constants (`IMPLEMENTED_*`,
  `DECK_STATEMENTS`, `MAGIC_ONLY_STATEMENTS` in `drc::report`,
  provenance = `magic_rules_checked`); `claim_boundary()` text; why
  `--reference` re-derives rather than restates.
- **§18 Count units** — ours = rect/pair/component findings;
  Magic's `magic_per_rule` = merged error regions. Equal at zero;
  the met2 control shows they need not coincide when nonzero
  (Magic 3 regions vs our pair count). A `drc listall` bbox-level
  comparison is the compatible next step; findings emit `[xl,yl,xh,
  yh]` DBU per shape for exactly that.
- Update the §7 family table note: the subset is now also reachable
  through `grus-eda drc` and gated as a whole — not just per family.

## `docs/results.md`

New dated entry (`e7-drc-e2e`, `v0.12.0`):

- Our command: the two `grus-eda drc` invocations on gcd and
  picorv32 (with `--reference`), plus `cargo test --test drc`.
- Per-rule outcome table — 50 rows is long; emit per-family
  subtotals (rules, candidates, findings per family per design)
  plus the full 50-row table only if it stays readable. Decision:
  compact family table + the claim-boundary line verbatim + the
  unclaimed list (the three `magic_only` keys and their counts),
  with a pointer to the test census for the per-rule rows.
- Deltas: all 50 rules `ours == magic` (0 = 0); `magic_total`
  recorded as context with the not-comparable note.
- Mutation parity: the met2 probe line — Magic `met2.2` count 3
  (merged regions), ours nonzero pairs — the unit note.
- Runtimes: load/assemble/check ms from the CLI runs on both
  designs, honestly labeled (debug vs release — run `--release`
  for the recorded numbers, state which).
- Fail-proof record: which test was broken how and observed red
  (§7.7) — e.g. drop one rule row → coverage test names it.

## `docs/status.md`

- `Last updated:` line → `v0.12.0` `e7-drc-e2e` summary (subset
  checker complete + `grus-eda drc` CLI; per-rule gate vs
  `magic_per_rule` all-zero on both designs; claim boundary printed
  on every run).
- E7 row: `Current state` → subset checker **complete** — 50/50
  `lef_expressible` rules implemented and gated as a set, CLI +
  JSON report landed; `Known gaps` → `magic_only` (223 statements)
  permanently out of scope; count units are findings not merged
  regions (bbox-level `drc listall` compare is the next step);
  antenna not covered.

## `docs/roadmap.md`

E7 progress line under the existing `e7-drc-enc-area` entry:
`e7-drc-e2e` landed (`v0.12.0`) — `grus-eda drc` CLI, `drc::report`
unified subset report, full-set gate vs `magic_per_rule`, honest
claim-boundary output; E7 stage gate passed.

## `README.md` / `tests/designs/README.md`

Unchanged — no new fixtures, and the README stage table deliberately
lists only E0–E3 (the DRC entry in "Not goals" refers to sign-off
DRC, which this subset checker does not claim). Note this decision
in the commit so it is visible.
