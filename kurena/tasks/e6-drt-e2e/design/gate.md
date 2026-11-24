# e6-drt-e2e — integration gate (r4)

All additions land in `crates/grus-core/tests/drt.rs`, additive to
the existing emit/self-DRC/maze tests. `load(name)` already wires
placed DEF + guide + `<d>.drt.reference.json`; add the ours reference
JSON (`serde_json::Value`, same deserialize pattern as the other
goldens — fail loudly if absent, never skip).

## Pinned-residual mechanism (new in r4)

The strict `ours == placed` equality stays the default for every
`lef_expressible` rule; residuals the ticket could not close are
**pinned, not tolerated**:

```rust
/// Measured Magic findings on our DEF with a named site each —
/// `residuals.md` §5. Entry: (rule, ours_count). Every entry's site
/// is named in the comment AND in `docs/results.md`; an entry that
/// no longer reproduces fails the join, an unexplained nonzero rule
/// fails the strict assert — a residual can only move by re-pinning
/// with a named reason.
const PINNED_LEF_RESIDUALS: &[(&str, i64)] = &[
    // e.g. ("met1.2", N), // _099_ undroppable notch — gap fully
                           // foreign-blocked; site in results.md
];
```

For each of the 50 `lef_expressible` rules: if the rule is in
`PINNED_LEF_RESIDUALS` assert `entry.ours == pinned && entry.placed
== 0` (a residual is *ours-only* — if the placed baseline ever fires
the same rule the pin is wrong, not inherited); otherwise assert
`entry.ours == entry.placed` (equality against the clean baseline,
not `== 0`, so placed-DEF drift fails honestly). `entry.inherited`
is asserted `== (entry.ours == entry.placed)` alongside.

## 1. `test_e2e_ours_reference_gcd`

Deserialize `gcd.drt.ours.reference.json` + reuse the taxonomy from
`gcd.drc.reference.json`.

- **`lef_expressible` completeness**: the map's key set equals the
  `classification` rule-id set filtered to `lef_expressible` (50 —
  derived from the committed taxonomy, not a constant). Per rule the
  `PINNED_LEF_RESIDUALS` join above.
- **`magic_only` decomposition**: every row `assert!(row.inherited)`
  — ours findings must equal the placed baseline per rule. A row
  that can't match is named by the failure, not rebaselined.
- **`unclassified_fired`**: every `name` ∈ the pinned two-string
  allowlist (`{"Can't overlap those layers",
  "This layer can't abut or partially overlap between subcells"}`),
  and `assert_eq!(entry.ours, 0)` per entry (or the pinned measured
  count if a genuine residual lands — named in `docs/results.md`).
- **`sanity_control` structural asserts**:
  `violated_rules_in_checked_deck == true`, `mutation` object
  present. `new_rules_vs_clean` non-empty **by construction** once
  the golden is clean-or-residual-only (a probe fragment can still
  join a pinned-residual rule — then `new_rules_vs_clean` is the
  difference set minus the pinned rules; keep the assert on the
  *unpinned* difference).
- **`ours_router` block**: `hull_masked > 0` on gcd (the mask fires,
  not vacuously green); `notch_fills` present `>= 0` (recorded, not
  gated — and post-dedup it counts distinct rects);
  **`notch_residuals`/`hole_residuals` counts asserted equal to the
  number of `notch_residual`/`hole_residual` lines in `cli_lines`**
  (the census is the count of its own evidence lines — a residual
  can never exist un-named); `unrouted` name list asserted against
  the pinned `BTreeSet` residual list (§4 below — reviewed on
  change); `unrouted_kinds`/`unrouted_blockers` counts consistent
  with `unrouted_count`; every `stub_outside_cover` `cli_line`
  entry maps to a net in the `unrouted` list.
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
committed at `b372d5e` is superseded — recommit the post-fix
emission in the same change as the fix it measures. The `RECT`
fill pieces (now deduplicated) are covered by the byte-equal assert
like any other statement.

## 3. `#[ignore]` picorv32 leg — deferred (r4 memo)

`test_e2e_ours_reference_picorv32` — same joins as (1) on
`picorv32.drt.ours.reference.json`. **The leg is deferred per the
r4 memo**: the test is written (or updated for the new
`notch_residuals`/`hole_residuals` fields and `PINNED` mechanism)
but stays `#[ignore]`d, and `docs/results.md` records the deferral
explicitly. No picorv32 golden commits this revision — a manual
`--ignored` run without the file fails loudly, which is the honest
state.

## 4. Re-pins of existing assertions (measured at b372d5e)

- `test_emit_roundtrip_gcd` — **currently red**
  (`report.unrouted.is_empty()` with 81 names). Per the memo the 81
  are the named connectivity residual: pin
  `report.unrouted` to the measured `BTreeSet` literal (the 81 names
  from the emit log — mechanical pin, reviewed on change) with a
  comment pointing at `docs/results.md`. If the `residuals.md`
  §1–§3 fixes change the set, re-pin to the new measured list.
- `test_emit_selfdrc_gcd` — **currently red** (`firing.is_empty()`
  with `met1.2:1`, `met1.7:1`). Both findings are real geometry the
  `residuals.md` §1–§3 fixes address: prefer the assert returning to
  `firing.is_empty()` post-fix; if a named residual survives (e.g.
  a hole the void-fill couldn't legally cover), pin the measured
  finding list — same named-residual contract as the Magic gate.
  `area_residuals.is_empty()` unchanged.
- `test_maze_census` classification asserts — unchanged unless new
  FailKinds/blocking classes appear; `hull_masked`/`notch_fills`
  census prints added.
- `test_maze_occupancy`, `test_maze_determinism*`,
  `test_emit_determinism` — structural, no pinned digests; the
  digest prints move legitimately (recorded, not "fixed" blindly).

## Fail-proof demonstration

Container-side: the RED first-contact (~3025) and WIP (847, 514)
measurements are the demonstrated nonzero paths; the committed
mutation DEF keeps the arm live every run. Test-side:
`sanity_control` asserts + `hull_masked > 0` + the viability floor +
the strict non-pinned `ours == placed` join (a vacuous all-pin gate
is impossible — `PINNED_LEF_RESIDUALS` is finite and named). Fix-side:
`who5.tcl`'s move-via probe is the demonstrated mechanism for the
closure pass — `docs/results.md` records it; the §1–§3 fixes each
carry a fixture or a fail-then-pass measurement.

## Files

- `crates/grus-core/tests/drt.rs`
