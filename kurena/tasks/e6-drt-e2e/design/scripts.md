# e6-drt-e2e — measurement script fixes

`tests/designs/check_drt_ours.sh` and `generate_drt_ours.sh` are
committed (`20ad784`) and largely complete — fix the two flagged
issues in place; do not rewrite.

## `check_drt_ours.sh`

### 1. Pin the unclassified-name allowlist (step E)

Today `count_by_id` collects every rendered `per_rule` name the
committed taxonomy can't classify into `unclassified_fired`
unconditionally (lines ~817–845). First contact fired exactly two
names — Magic's built-in hierarchy-boundary messages with no
tech-file rule id:

- `Can't overlap those layers` (drcOverlapCookie, tag 2)
- `This layer can't abut or partially overlap between subcells`
  (drcSubcellCookie, tag 3)

Fix: declare the allowlist verbatim near `count_by_id`, e.g.

```python
ALLOWED_UNCLASSIFIED = {
    "Can't overlap those layers",
    "This layer can't abut or partially overlap between subcells",
}
```

then after building `unclassified_names`:

```python
new_uncl = sorted(set(unclassified_names) - ALLOWED_UNCLASSIFIED)
if new_uncl:
    fail(f"unclassified Magic rule names outside the pinned "
         f"allowlist: {new_uncl}")
```

The two allowed names stay recorded with `{ours, placed,
routed_ref}` counts — they are data (the gate asserts `ours == 0`
post-fix; see `gate.md`). Any *new* unclassified name is a deck
change or a new violation class — abort loudly rather than growing
the set silently (E0-review fail-proof convention). Verify the
verbatim spelling against the first-run transcript before pinning —
the strings are Magic `drcWhyCreate` literals, case-sensitive.

### 2. Strip the absolute-path line from `ours_router.cli_lines`

The emit log's `wrote routed DEF to "<abs path>"` line carries the
checkout path into the golden — it breaks byte-reproduction on any
other checkout. The `cli_lines` filter (line ~920) currently keeps
it via `s.startswith("wrote routed DEF")`.

Fix: drop that prefix from the keep-list (the DEF path is already
provenance'd in `input_def`) and add a guard — after building
`cli`, fail if any retained line contains an absolute path:

```python
if any(re.search(r"(?<![A-Za-z_.])/", ln) for ln in cli):
    fail("cli_lines carries an absolute path — golden would not "
         "byte-reproduce")
```

A line-level path guard catches any *future* path-bearing line
slipping into the keep-list.

### 3. Non-changes (verified, keep)

- `drc catchup` before counting — already present (E7 lesson).
- `image_id` from `DRC_IMAGE_ID` — recorded (line ~992).
- `count_flat` stays as emitted — documented insensitive in
  `golden.md`/`docs/e6-drt.md`; the sanity control at lines ~938–957
  already uses the right signal (`per_rule` names +
  `violated_in_deck`).
- `{ours, placed, routed_ref, inherited}` object form in
  `lef_expressible` and `magic_only` — `golden.md` documents it as
  shipped.

## `generate_drt_ours.sh`

No changes needed: `--verify` already re-routes and `cmp`s the DEFs
(`OURS-DETERMINISTIC <sha>`, exit 1 on diff), the emit log is tee'd
for `ours_router`, and `.gitignore` already whitelists
`gcd.drt.ours.routed.def` while keeping picorv32's ignored. Both
`<d>.drt.ours.reference.json` are committable (`git check-ignore`
clean on both paths).

## Regeneration flow (post-fix)

Inside the same container/image-id discipline as `generate_drt_ref.sh`:

```
generate_drt_ours.sh gcd --verify        # emits OURS-SHA + OURS-DETERMINISTIC
check_drt_ours.sh gcd                    # steps A–E → gcd.drt.ours.reference.json
generate_drt_ours.sh picorv32 --verify
check_drt_ours.sh picorv32
```

Commit `gcd.drt.ours.routed.def` + both reference JSONs once the
gate is green. Rerun the exact committed scripts and `git diff` the
goldens before committing (E0 lesson 1).

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh` (no change expected)
- `tests/designs/{gcd,picorv32}/<d>.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
