# drt-unrouted-census — Downstream consumers

The emit log is consumed by two committed parsers plus the golden
`cli_lines` evidence list. This ticket's print change must not
break any of them, and should leave the evidence path *better*
without forcing a golden regeneration.

## 1. `check_drt_ours.sh` step-E (`cli_lines`)

Keep-list is prefix-based over the emit log:

```
s.startswith("== drt emit") or s.startswith("guided=")
or s.lstrip().startswith("unrouted ")
or s.lstrip().startswith("unrouted_kinds ")
or s.lstrip().startswith("unrouted_blockers ")
or s.lstrip().startswith("area_residual ")
or s.lstrip().startswith("notch_residual ")
or s.lstrip().startswith("hole_residual ")
or s.lstrip().startswith("stub_outside_cover ")
or s.startswith("  rule=") or re.match(r"^    \S+ \[", s)
or s.startswith("self_drc")
```

Two facts drive the format decision (`format.md` §Constraints):
`^\s+unrouted (\S+)$` extracts the `unrouted` field, and `fail`
lines match no prefix today (silently dropped from `cli_lines`).

**Change**: add `or s.lstrip().startswith("fail ")` to the
keep-list so the next golden regeneration captures per-pin
evidence. Consequences:

- Committed goldens (`gcd`/`picorv32.drt.ours.reference.json`) are
  unchanged — they are generation-time snapshots; no test asserts
  `fail` lines exist, and existing joins (`cli_unrouted ==
  unrouted`, `stub_lines ≥ Unattachable`, residual line counts)
  are prefix-scoped and unaffected.
- The step-E absolute-path guard (`cli_lines` must carry no
  absolute-path token) is `re.search(r"(?<![A-Za-z_.])/", ln)` —
  fires on any `/` whose preceding char is not a letter, dot, or
  underscore. `inst/pin` labels contain `/`, so the inst-name
  character immediately before it must be in `[A-Za-z_.]`.
  Digit-ending insts (`u0/A`) or DEF-escaped names would trip —
  same exposure the landed `stub_outside_cover <net> <inst>/<pin>`
  lines already carry. gcd/picorv32 insts are `_NNNN_`-style or
  identifier-style (all end `_`/letter), so today's committed
  emit logs are clean; verify on the gcd run in `steps.md` step 4
  (`grep -nE '(?<![A-Za-z_.])/'` over the emit log's `fail`/stub
  lines) and record it in `results.md`.
- No `ours_router` schema change: a `fail_lines` count is
  derivable from `cli_lines` — keep the JSON surface stable.

## 2. `recount_unrouted.py`

Its `cli_unrouted` parse (`parts[0] == "unrouted" and len(parts)
== 2`) is compatible with the nested-lines format by construction.
Changes:

- Extend the Semantics docstring: `fail` lines = per-pin final
  `FailKind`, `region_escape` suffix = graph-build provenance;
  reiterate `u.failed` final vs `stub_outside_cover` ever-failed.
- Optional join (tolerant of committed goldens lacking `fail`
  lines): when `fail` lines are present, assert
  `#fail lines == sum(unrouted_kinds.values())` and that every
  `fail` line's net is in the unrouted set — cheap consistency
  proof for regenerated goldens. Absent → skip silently (old
  goldens stay consumable).

## 3. `tests/drt.rs` gates

No change required: `ours_reference_gate` reads the committed
golden's fields only; `test_maze_census`'s `!u.failed.is_empty()`
assert and the digest determinism asserts already cover the
invariants this print relies on. The `fail`-line joins, once
goldens carry them, are the recount script's job — not a new test
here (keeps the diff print-path only).

## 4. `docs/e6-drt.md` §8

The CLI paragraph (currently describes `unrouted <name>` + census
lines) gains one sentence for the `fail` lines and the
final-vs-ever-failed note — E0 lesson 8: keep docs in step with
the code in the same change.

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/recount_unrouted.py`
- `docs/e6-drt.md`
