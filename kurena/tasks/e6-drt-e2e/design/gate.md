# e6-drt-e2e — integration gate (r3)

All additions land in `crates/grus-core/tests/drt.rs`, additive to
the existing emit/self-DRC/maze tests. `load(name)` already wires
placed DEF + guide + `<d>.drt.reference.json`; add the ours reference
JSON (`serde_json::Value`, same deserialize pattern as the other
goldens — fail loudly if absent, never skip).

## 1. `test_e2e_ours_reference_gcd`

Deserialize `gcd.drt.ours.reference.json` + reuse the taxonomy from
`gcd.drc.reference.json`.

- **`lef_expressible` completeness**: the map's key set equals the
  `classification` rule-id set filtered to `lef_expressible` (50 —
  derived from the committed taxonomy, not a constant). Then per
  rule `assert_eq!(entry.ours, entry.placed)` — equality against the
  clean baseline, not `== 0`, so a future placed-DEF drift fails
  honestly. `entry.inherited` asserted `true` alongside.
- **`magic_only` decomposition**: every row `assert!(row.inherited)`
  — ours findings must equal the placed baseline per rule. A row
  that can't match is named by the failure, not rebaselined.
- **`unclassified_fired`**: every `name` ∈ the pinned two-string
  allowlist (`{"Can't overlap those layers",
  "This layer can't abut or partially overlap between subcells"}`),
  and `assert_eq!(entry.ours, 0)` per entry. If implementation lands
  with a genuine residual, replace `0` with the pinned measured count
  and name the residual in `docs/results.md` — never silently.
- **`sanity_control` structural asserts**:
  `violated_rules_in_checked_deck == true`, `mutation` object
  present. `new_rules_vs_clean` non-empty **by construction** once
  the golden is clean (any fired rule on the mutation DEF is a new
  rule vs the all-zero baseline — keep the assert so a vacuous
  control still fails; at `4c5e2cb` it was empty because probe
  fragments joined the already-firing met2.2).
- **`ours_router` block**: `hull_masked > 0` on gcd (the mask fires,
  not vacuously green); `notch_fills` present and asserted `>= 0`
  (recorded, not gated — `self-spacing.md`); `unrouted` name list
  asserted against the pinned expectation — empty if the access work
  lands clean, else the exact `BTreeSet` residual list named in
  `docs/results.md`; `unrouted_kinds`/`unrouted_blockers` counts
  consistent with `unrouted_count`; every `stub_outside_cover`
  `cli_line` entry maps to a net in the `unrouted` list (the
  unattachable-pin census may not name a routed net).
- **`openroad` block**: `drt_report_source ==
  "not_applicable_native_router"` pinned; antenna counts recorded
  and printed, not gated (deltas vs baseline/delegated are the
  `results.md` table).
- **Viability floor**: `def_metrics.signal_nets_routed` tracks
  `signal_nets` minus the pinned unrouted count on gcd — a mask that
  starved the design to conns-only output fails here rather than
  passing vacuously on an empty route.
- Print the `delta` block and per-rule census for `results.md`
  transcription (census convention matches existing tests).

## 2. `test_e2e_ours_def_deterministic_gcd`

`emit_leg(gcd)` fresh → serialize routed DEF → `assert_eq!` bytes vs
committed `tests/designs/gcd/gcd.drt.ours.routed.def`. The committed
file IS the golden: the test proves `generate_drt_ours.sh`'s emit
path is byte-stable (the `--verify` arm demonstrates the same
property script-side; results.md records which arms ran). The DEF
committed at `4c5e2cb` is superseded — recommit the post-fix
emission in the same change as the fix it measures. Note: the emit
now includes `RECT` fill pieces (`self-spacing.md`) — the byte-equal
assert covers them like any other statement.

## 3. `#[ignore]` picorv32 leg

`test_e2e_ours_reference_picorv32` — same joins as (1) on
`picorv32.drt.ours.reference.json`; ignored for runtime, run
manually before committing its golden (E0: say what ran). Its
`unrouted`/`hull_masked`/`notch_fills` asserts take picorv32's own
pinned values (measured, not assumed).

## 4. Re-pins of existing assertions

- `test_emit_roundtrip_gcd` — **currently red** at `4c5e2cb`
  (`report.unrouted.is_empty()` with 73 names). The access work
  restores it to empty; if residuals remain, pin the measured name
  list (`BTreeSet` literal) — the named residual class, reviewed on
  change.
- `test_maze_census` classification asserts — unchanged unless new
  FailKinds/blocking classes appear; `hull_masked` census prints
  added.
- `test_emit_selfdrc_gcd` `findings_total == 0` and
  `area_residuals.is_empty()` — must still hold. The same-net notch
  class is outside self-DRC's foreign-spacing scope (recorded in
  `docs/e6-drt.md` §7) — if self-DRC *is* extended to catch it, the
  assert stays `== 0` post-closure.
- `test_maze_occupancy`, `test_maze_determinism*` — digests re-pin
  only to measured values; route changes under the fix/access
  rungs legitimately move digests (recorded, not "fixed" blindly).

## Fail-proof demonstration

Container-side: the RED first-contact measurement demonstrated the
deck fires nonzero on our DEF; the committed mutation DEF keeps the
arm live every run. Test-side: `sanity_control` asserts +
`hull_masked > 0` + the viability floor. Fix-side: `who5.tcl`'s
move-via probe is the demonstrated mechanism for the closure pass —
`docs/results.md` records it.

## Files

- `crates/grus-core/tests/drt.rs`
