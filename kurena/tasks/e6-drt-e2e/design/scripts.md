# e6-drt-e2e — measurement script state

`tests/designs/check_drt_ours.sh` and `generate_drt_ours.sh` were
committed in `20ad784`; both flagged issues were fixed in `d8d0c6c`.
This file records what landed and the one remaining log-format
dependency.

## Landed at `d8d0c6c` (`check_drt_ours.sh`)

1. **Unclassified-name allowlist (step E)** — `ALLOWED_UNCLASSIFIED`
   pins Magic's two built-in hierarchy-boundary literals
   (`Can't overlap those layers`,
   `This layer can't abut or partially overlap between subcells`);
   assembly aborts on any *new* unclassified name. The two allowed
   names stay recorded with `{ours, placed, routed_ref}` counts (the
   gate asserts `ours == 0` post-fix).
2. **`cli_lines` byte-reproduction** — `wrote routed DEF` dropped
   from the keep-list (absolute checkout path, redundant with
   `input_def`) and a guard fails assembly if any retained line
   carries an absolute path. `stub_outside_cover ` added to the
   keep-list. `hull_masked` parsed from the census line into
   `ours_router`.

Verified keepers (unchanged): `drc catchup` before counting,
`image_id` from `DRC_IMAGE_ID`, `count_flat` recorded-not-asserted,
the `{ours, placed, routed_ref, inherited}` object form, the step-D
mutation sanity control keyed on `per_rule` names.

## Remaining — only if the emit log gains lines

`pin-access.md` step 0 adds `unrouted_kinds NoPath=<n>
Unattachable=<n>` to the emit census. If it ships:

- keep-list: add `s.lstrip().startswith("unrouted_kinds ")`;
- `ours_router` gains `"unrouted_kinds": {"NoPath": n,
  "Unattachable": n}` (parsed from the line, `fail` if the census
  line is present but the kinds line is missing — the emit binary
  and script stay version-locked);
- the `unrouted <net>` line format must **not** change — the
  `^\s+unrouted (\S+)$` regex and the name list downstream depend on
  it. Per-pin detail, if added, goes on `unrouted_pin` lines with
  their own keep-list prefix and golden field — never appended to
  the `unrouted` line.

## `generate_drt_ours.sh`

No changes expected: `--verify` already re-routes and `cmp`s
(`OURS-DETERMINISTIC <sha>`, exit 1 on diff), the emit log is tee'd
for `ours_router`, and `.gitignore` whitelists `gcd.drt.ours.routed.def`
while keeping picorv32's ignored.

## Regeneration flow (post-pin-access)

Inside the same container/image-id discipline as `generate_drt_ref.sh`:

```
generate_drt_ours.sh gcd --verify        # OURS-SHA + OURS-DETERMINISTIC
check_drt_ours.sh gcd                    # steps A–E → gcd.drt.ours.reference.json
generate_drt_ours.sh picorv32            # detached multi-hour leg
check_drt_ours.sh picorv32
```

Commit `gcd.drt.ours.routed.def` + both reference JSONs once the gate
is green; `git diff` the goldens against a fresh rerun of the exact
committed scripts first (E0 lesson 1). The intermediate
`check_drt_ours.sh gcd` run on the checkpoint DEF (steps.md step 2)
is measurement-only — its JSON is *not* committed.

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh` (no change expected)
- `tests/designs/{gcd,picorv32}/<d>.drt.ours.reference.json`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
