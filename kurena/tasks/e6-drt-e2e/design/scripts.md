# e6-drt-e2e — script deltas (r4)

> **r7 status — still no script delta at `5d1502b`.** The checkpoint
> is `maze.rs`-internal (alias refactor); neither script moved.
>
> **r6 status — COMPLETE, no script delta at `0d113ba`.** The owed
> fresh `check_drt_ours.sh gcd` run executed on the regenerated
> committed DEF and emitted the committed golden
> (`input_def_sha_prefix=635f19a9aae86a44`); `generate_drt_ours.sh
> gcd --verify` demonstrated byte-stability (`OURS-DETERMINISTIC
> 635f19a9…`). `who6.tcl`/`who7.tcl` joined the gitignored probe set.
>
> **r5 status — LANDED at `fba5f85`, no further delta.** The census
> contract below is what the 20:07 transcripts carry
> (`notch_residuals`/`hole_residuals` tokens + fields + evidence
> keep-list). `generate_drt_ours.sh` stays unchanged. One fresh
> `check_drt_ours.sh gcd` run is owed regardless — the checkpoint
> run's step-E output was never persisted (`steps.md` §B); further
> reruns only if `met12-attribution.md`'s fix arm fires.

Both scripts are committed on dev and stay — changes are surgical,
matching the "fix, don't rewrite" directive. The emit binary and
check script are version-locked (the census-line contract below):
they land in the same change.

## `tests/designs/check_drt_ours.sh`

Landed at `4c5e2cb` + `b372d5e` (already in the emit contract):
`unrouted_kinds` / `unrouted_blockers` objects (non-empty `unrouted`
without kinds is `fail`, not `null`), `cli_lines` absolute-path
guard, `unrouted` / `stub_outside_cover` / `area_residual` /
`notch_residual` keep-list, pinned `ALLOWED_UNCLASSIFIED` two-name +
abort, `sanity_control` block, `hull_masked` + `notch_fills` fields
(`guided=` regex requires `notch_fills=` — version-locked),
`[design]` arg defaulting to gcd.

r4 delta — census tokens, residual fields, evidence keep-list:

- `guided=` gains `notch_residuals=<n>` and (with `residuals.md` §3)
  `hole_residuals=<n>`, appended after `notch_fills=` — the anchored
  regex is extended for both tokens; an emit log without them →
  `fail` (version lock, same rule as `notch_fills`). Golden fields
  `ours_router.notch_residuals` / `ours_router.hole_residuals` per
  `golden.md`.
- `hole_residual` added to the `cli_lines` keep-list (r4 residual
  class — `residuals.md` §3).
- Keep-list gains the self-DRC detail lines (`  rule=<id> family=…`
  + `    <rule-id> [bbox] …`) so `cli_lines` carries the emit's own
  site evidence alongside `self_drc findings_total=` — the gate and
  `docs/results.md` cross-name residuals from it (`residuals.md` §5).
- `def_metrics`: verify the wire-segment parser treats `RECT
  ( dx dy dx dy )` records as paint, not wire-length or vias — the
  delegated reference DEF already emits them, so the ours-side
  parser should match; fix only if it double-counts.
- No change to the pinned allowlist, sanity control, or Magic
  invocation blocks.

## `tests/designs/generate_drt_ours.sh`

No functional delta — it just runs `grus-eda drt`. The emit's new
geometry (`RECT` fills — deduplicated, void fills) flows through it
unchanged; the `--verify` double-emit arm still covers determinism.

## `crates/grus-eda/src/main.rs` (emit side of the contract)

- `guided=` census line: append `notch_residuals=<n>`
  (`route_db.notch_residuals.len()`) and `hole_residuals=<n>` once
  `route_db.hole_residuals` exists (`residuals.md` §3) — same
  version-lock: both sides land together.
- `  hole_residual <net> <layer> area=… bound=… region=…` print per
  unfillable under-bound void — the `area_residual` line convention.
- `notch_fills=` semantics unchanged (sum over routed nets'
  `NetRoute.fills.len()`) — post-dedup it is the true distinct-rect
  count.
- No other print changes: `unrouted_kinds`, `unrouted_blockers`,
  `notch_residual`, `stub_outside_cover`, `area_residual`,
  `self_drc` lines keep their current formats.

## Probe scripts (no commit obligation)

`tests/designs/gcd/runs/drt-ours/who1.tcl`–`who5.tcl` +
`who1-who5.log`, `region.mag`, `gcd.whofix.def` — the r3 root-cause
probes live in the gitignored runs dir. `docs/results.md` names them
so the experiment is reproducible; they are not committed and the
check script does not reference them.

## Files

- `tests/designs/check_drt_ours.sh`
- `tests/designs/generate_drt_ours.sh`
- `crates/grus-eda/src/main.rs`
