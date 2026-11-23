# e7-drc-e2e — integration gate

All changes land in `crates/grus-core/tests/drc.rs`, additive to the
existing per-family tests (which stay — they name per-family
candidates the aggregate report does not print). `load()` already
returns `{tech, design, drc, golden}` for both designs; the new tests
add `design.strings` use only.

## 1. `test_subset_coverage_complete`

The acceptance "name join complete" check, exact in both directions:

- `let report = run_all(&l.drc, &l.tech, &l.design.strings);`
- `assert_eq!(report.rules.len(), IMPLEMENTED_RULES)` (50).
- Build `BTreeSet` of `outcome.rule_name`; build the golden set =
  `classification` keys with value `"lef_expressible"`. Assert set
  equality — every `lef_expressible` name is implemented **and**
  nothing outside the class is implemented. Runs on both designs (the
  goldens' classification sets are identical, but the test doesn't
  assume it).
- Pin the deck constants against the golden so the CLI's fixed line
  can't drift: `DECK_STATEMENTS == names.len()` (278),
  `IMPLEMENTED_STATEMENTS == names.iter().filter(|n| cls[n] ==
  "lef_expressible").count()` (55), `MAGIC_ONLY_STATEMENTS` (223)
  likewise, `DECK_UNIQUE_NAMES == cls.len()` (239).
- Distinctness: the 50 `rule` ids are unique (a `BTreeSet` of
  `outcome.rule` has len 50) — the delta join key must not collide.

## 2. `test_subset_gate_matches_golden`

Per-rule equality vs `magic_per_rule` on both designs:

- `run_all` → `magic_deltas(&report, &l.golden)`.
- For every delta: `assert_eq!(d.ours, d.magic as usize)` — written as
  equality, not `== 0`, so a golden that grows an implemented-rule
  entry fails honestly rather than being silently re-baselined.
  Today every implemented rule is absent from `magic_per_rule` →
  expected 0 → this is the "all zero" gate.
- Print the per-rule census (`rule ours magic`) for `results.md`,
  matching the census convention of the existing tests.
- Assert `unclaimed` is exactly the golden's current three
  `magic_only` keys (not just "nonempty") — if Magic ever reports an
  implemented rule the delta above catches it; if a *new* unclaimed
  rule appears the test names it rather than growing the list
  silently. Assertion form: `BTreeSet` of keys equality against the
  golden's `magic_per_rule` key set minus claimed — i.e. simply
  `unclaimed.len() == magic_per_rule.len()` and every unclaimed key
  exists in `magic_per_rule`. Since claimed = 0 today, `unclaimed`
  is `magic_per_rule` verbatim; assert keys match the golden object.

## 3. met2 mutation parity at report level

Extend `test_spacing_mutation_met2_probe` (keeps every existing
assertion): after the current pair-level checks, run
`run_all(&drc, &tech, &db.strings)` on the mutated design and assert:

- Exactly one outcome has findings, `rule == "met2.2"`.
- The `sanity_control.magic_per_rule` golden entry for met2.2 is
  present and nonzero — Magic flagged the same rule on the same input
  shape. **Count units differ and the test says so**: Magic's 3 is
  merged error regions, ours is rect pairs — assert `ours > 0`, never
  `ours == 3`. The doc comment records this explicitly (same as the
  existing test's note).
- Control (unmutated DEF → `run_all` all findings empty) rides the
  existing control block — swap its `check_spacing` call for
  `run_all` so the full pipeline is exercised once.

## 4. What is *not* tested here

- CLI invocation itself — no `assert_cmd`/binary test; the CLI run on
  both designs is executed once during implementation and its output
  recorded in `docs/results.md` (the established convention — every
  prior CLI arm was verified the same way). The gate covers the
  checker's logic; the arm is thin glue.
- No new fixture files — mutation stays in-memory DEF text.

## Verify

`cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings` must pass
(workflow.toml verify commands). Fail-proof per AGENTS §7.7: the
coverage test is self-checking (a rule dropped from any table turns
it red immediately); demonstrate once during implementation by
commenting out one rule row.
