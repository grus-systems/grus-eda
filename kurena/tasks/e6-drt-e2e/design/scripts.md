# e6-drt-e2e — script deltas (r3)

Both scripts are committed on dev and stay — r3 changes are
surgical, matching the "fix, don't rewrite" directive. The emit
binary and check script are version-locked (the census-line contract
below): they land in the same change.

## `tests/designs/check_drt_ours.sh`

Landed at `4c5e2cb` (already in the WIP golden): `unrouted_kinds` /
`unrouted_blockers` objects (non-empty `unrouted` without kinds is
`fail`, not `null`), `cli_lines` absolute-path guard, `unrouted` /
`stub_outside_cover` / `area_residual` keep-list, pinned
`ALLOWED_UNCLASSIFIED` two-name + abort, `sanity_control` block,
`hull_masked` field, `[design]` arg defaulting to gcd.

r3 delta — one parse + one field:

- The `guided=` census line gains `notch_fills=<n>` (appended after
  `hull_masked=` — the anchored regex keeps matching the prefix;
  extend it anyway so the golden parses, not just survives).
  `ours_router.notch_fills: int` added to `golden.md`'s schema.
  An emit log without the token → `fail` (version-locked), same
  rule as `unrouted_kinds`.
- `def_metrics`: if the wire-segment parser counts routed pieces,
  teach it `RECT ( dx dy dx dy )` statements exist and are paint
  (perimeter paint, not wire-length contributions and not vias) —
  delegated's reference DEF already emits them, so the baseline
  parser should already treat them; verify the ours-side parser does
  the same rather than double-counting.
- No change to the pinned allowlist, sanity control, or Magic
  invocation blocks.

## `tests/designs/generate_drt_ours.sh`

No functional delta — it just runs `grus-eda drt`. The emit's new
geometry (`RECT` fills, wider `access_vias` candidates) flows through
it unchanged; the `--verify` double-emit arm still covers
determinism.

## `crates/grus-eda/src/main.rs` (emit side of the contract)

- `guided=` census line: append `notch_fills=<total>` (sum over
  routed nets' `NetRoute.fills.len()`).
- `stub_outside_cover` comment updated: "pins that got no legal
  paint — the Unattachable census" (r3 semantics; the "should be
  empty" framing is gone — `pin-access.md`).
- No other print changes: `unrouted_kinds`, `unrouted_blockers`,
  `stub_outside_cover`, `area_residual` lines all keep their current
  formats (the script's keep-list regexes already match them).

## Probe scripts (no commit obligation)

`tests/designs/gcd/runs/drt-ours/who1.tcl`–`who5.tcl` +
`who1-who5.log`, `region.mag`, `gcd.whofix.def` — the root-cause
probes (region anatomy, move-via causality) live in the gitignored
runs dir. `docs/results.md` names them so the experiment is
reproducible; they are not committed and the check script does not
reference them.

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh`
- `crates/grus-eda/src/main.rs`
