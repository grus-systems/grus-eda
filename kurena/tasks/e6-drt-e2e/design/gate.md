# e6-drt-e2e — integration gate (r7, landed; head `5d1502b`)

> **r7 note** — `5d1502b` is refactor-only (`LineNodes`/`PaintLines`
> aliases + a clippy allow in `maze.rs`); no test or golden join
> changed. `test_e2e_ours_def_deterministic_gcd`'s byte-equality is
> the check that proves the rename has no functional delta.

All additions landed in `crates/grus-core/tests/drt.rs`, additive to
the existing emit/self-DRC/maze tests. A `load_ours_ref(name)` helper
deserializes `tests/designs/<d>/<d>.drt.ours.reference.json` as
`serde_json::Value` — same `require`'d pattern as `load_drt_ref`
(fail loudly if absent, never skip); `load_drc_ref(name)` reads the
`drc.reference.json` taxonomy the `lef_expressible` key-set joins
against.

## 0. Pinned-residual mechanism (landed — list empty)

Strict `ours == placed` stays the default for every `lef_expressible`
rule; residuals the ticket could not close would be **pinned, not
tolerated** — but the r5 attribution closed all 7 sites (X3 arm,
`met12-attribution.md`), so the shipped list is empty:

```rust
/// Measured Magic findings on our DEF that the ticket pinned rather
/// than closed … A pinned rule asserts `ours == pin && placed == 0` —
/// a residual is ours-only … Empty since the r5 met1.2 attribution
/// closed all 7 sites (`docs/results.md`) — a residual reappears only
/// by re-pinning it here with a named reason.
const PINNED_LEF_RESIDUALS: &[(&str, i64)] = &[];
```

The mechanism is unchanged — only the concrete `[("met1.2", 32)]`
example was superseded by the fix. For each of the 50
`lef_expressible` rules: pinned rule → `entry.ours == pinned &&
entry.placed == 0` (a residual is *ours-only* — if the placed
baseline ever fires the same rule the pin is wrong, not inherited);
unpinned → `entry.ours == entry.placed` (equality against the clean
baseline, so placed-DEF drift fails honestly). `entry.inherited` is
asserted `== (entry.ours == entry.placed)` alongside.

## 1. `test_e2e_ours_reference_gcd` (new)

Deserialize `gcd.drt.ours.reference.json` + reuse the taxonomy from
`gcd.drc.reference.json` (`golden["magic_rules_checked"]
["classification"]`, same access pattern as `tests/drc.rs`).

- **`lef_expressible` completeness**: the golden map's key set equals
  the `classification` set filtered to `lef_expressible` (50 —
  derived from the committed taxonomy, not a constant). Per rule the
  §0 join.
- **`magic_only` decomposition**: every row `assert!(row.inherited)`
  — ours findings equal the placed baseline per rule. A row that
  can't match is named by the failure, not rebaselined.
- **`unclassified_fired`**: every `name` ∈ the pinned two-string
  allowlist (`{"Can't overlap those layers", "This layer can't abut
  or partially overlap between subcells"}`) — post-closure the array
  is empty; a re-appearing unclassified name fails on the allowlist
  join, a new *kind* of name fails on the script's own abort (version
  lock).
- **`sanity_control` structural asserts**:
  `violated_rules_in_checked_deck == true`, `mutation` object
  present. `new_rules_vs_clean` non-empty **by construction** once
  the golden is clean-or-residual-only — minus the pinned rules (a
  probe fragment joining `met1.2` is legal: assert on the *unpinned*
  difference).
- **`ours_router` block**: `hull_masked > 0` on gcd (the mask fires,
  not vacuously green); `notch_fills` present `>= 0` (recorded, not
  gated); **`notch_residuals`/`hole_residuals` asserted equal to the
  number of `notch_residual`/`hole_residual` lines in `cli_lines`**
  (the census is the count of its own evidence lines — no residual
  can exist un-named); `unrouted_kinds`/`unrouted_blockers` counts
  consistent with `unrouted_count`; every `stub_outside_cover`
  `cli_lines` entry maps to a net in `unrouted`.
- **`openroad` block**: `drt_report_source ==
  "not_applicable_native_router"` pinned; antenna counts recorded and
  printed, not gated.
- **Viability floor**: `def_metrics.signal_nets_routed` ==
  `ours_router.routed` == `signal_nets_in_def − unrouted_count` — a
  mask that starved the design fails here rather than passing
  vacuously.
- Print the `delta` block + per-rule census for `results.md`
  transcription.

## 2. `test_e2e_ours_def_deterministic_gcd` (new)

`emit_leg("gcd")` fresh → `write_routed_def` to a tmp path →
`assert_eq!` file bytes vs committed
`tests/designs/gcd/gcd.drt.ours.routed.def`. The committed file IS
the golden: byte-equality proves `generate_drt_ours.sh`'s emit path
deterministic (the `--verify` arm demonstrates the same property
script-side; `results.md` records which arms ran). The attribution's
X3 arm did re-emit — the committed DEF (`635f19a9aae86a44`) and this
test updated in the same change as the fix.

## 3. `test_e2e_ours_reference_picorv32` — `#[ignore]`d, deferred

Same joins as §1 on `picorv32.drt.ours.reference.json`. **The leg is
deferred per the r4/r5 memo**: the test is written so the schema
joins are ready when the leg runs, stays `#[ignore]`d, and
`docs/results.md` records the deferral. No picorv32 golden commits
this revision — a manual `--ignored` run without the file fails
loudly, which is the honest state.

## 4. Re-pins + new unit fixtures (landed)

- `test_emit_roundtrip_gcd` — re-pinned as designed: the emptiness
  assert is gone; `report.unrouted` as `BTreeSet` joins against
  `ours_router.unrouted` from the committed reference JSON — the
  golden is the pinned list (no literal in source; a drift names the
  differing nets in the failure). Landed at 81 after the X3 fix.
- `test_emit_selfdrc_gcd` — green and tightened: `findings_total=0`
  plus `notch_residuals`/`hole_residuals` asserted empty (the new
  assert the fix made possible). **Asserts stay strict** — a
  re-appearing finding is a regression to chase, never a re-pin.
- **maze.rs closure coverage** — the two designed fixtures landed:
  - `test_closure_reject_droppable`: a walled notch gap (`Net(9)`
    rasterized over it) whose prongs are `ProjStub(0)`/`Access(1)` →
    `ClosureResult::Reject { pin: 0, src: ProjStub(0) }` — smallest
    `Ord` first.
  - `test_closure_notch_residual`: undroppable `Tree`/`PinShape`
    prongs over the same walled gap → `Done` with
    `residuals.notches == [NotchResidual { a, b, .. }]` — the
    census-evidence arm (`ClosureResult` gained `Debug` for the
    `panic!("…{res:?}")` messages).
- `test_maze_census`, `test_maze_occupancy`, `test_maze_determinism*`,
  `test_emit_determinism` — structural, no pinned digests; digest
  prints move legitimately (recorded, not "fixed" blindly).

## Fail-proof demonstration

Container-side: the RED first-contact (~3025), WIP (847, 514) and
post-closure (317, then **0** post-fix) measurements are the
demonstrated nonzero paths; the committed mutation DEF keeps the
sanity arm live every run (`new_rules_vs_clean = [met2.2]` on the
committed golden).
Test-side: `sanity_control` asserts + `hull_masked > 0` + the
viability floor + strict unpinned `ours == placed` (a vacuous
all-pin gate is impossible — `PINNED_LEF_RESIDUALS` is finite and
named) + the two new fixtures' fail arms (blocked-gap fixtures prove
the fill admission is what fails, not the test's own setup).

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/maze.rs` (two unit fixtures only, unless
  `met12-attribution.md` §4 lands an X3/X4 fix)
