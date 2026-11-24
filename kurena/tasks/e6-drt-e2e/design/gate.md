# e6-drt-e2e — integration gate

All additions land in `crates/grus-core/tests/drt.rs`, additive to
the existing emit/self-DRC/maze tests. `load(name)` already wires
placed DEF + guide + `<d>.drt.reference.json`; add the ours
reference JSON (`serde_json::Value`, same deserialize pattern as
the other goldens — fail loudly if absent, never skip).

## 1. `test_e2e_ours_reference_gcd`

Deserialize `gcd.drt.ours.reference.json` + reuse the taxonomy from
`gcd.drc.reference.json`.

- **`lef_expressible` completeness**: the map's key set equals the
  `classification` rule-id set filtered to `lef_expressible` (50 —
  derived from the committed taxonomy, not a constant). Then per
  rule `assert_eq!(entry.ours, entry.placed)` — equality against
  the clean baseline, not `== 0`, so a future placed-DEF drift
  fails honestly. `entry.inherited` asserted `true` alongside.
- **`magic_only` decomposition**: every row `assert!(row.inherited)`
  — ours findings must equal the placed baseline per rule. A row
  that can't match is named by the failure, not rebaselined.
- **`unclassified_fired`**: every `name` ∈ the pinned two-string
  allowlist (`{"Can't overlap those layers",
  "This layer can't abut or partially overlap between subcells"}`),
  and `assert_eq!(entry.ours, 0)` per entry. If implementation
  lands with a genuine residual, replace `0` with the pinned
  measured count and name the residual in `docs/results.md` —
  never silently.
- **`sanity_control` structural asserts**:
  `violated_rules_in_checked_deck == true`,
  `new_rules_vs_clean` non-empty, `mutation` object present —
  proves the deck flags injected violations on our DEF (the
  fail-proof arm; E0 lesson 7).
- **`openroad` block**: `drt_report_source ==
  "not_applicable_native_router"` pinned; antenna counts recorded
  and printed, not gated (deltas vs baseline/delegated are the
  `results.md` table).
- **Viability floor**: `def_metrics.signal_nets_routed > 0` on gcd —
  a mask that starved the design to conns-only output fails here
  rather than passing vacuously on an empty route.
- Print the `delta` block and per-rule census for `results.md`
  transcription (census convention matches existing tests).

## 2. `test_e2e_ours_def_deterministic_gcd`

`emit_leg(gcd)` fresh → serialize routed DEF → `assert_eq!` bytes vs
committed `tests/designs/gcd/gcd.drt.ours.routed.def`. The committed
file IS the golden: the test proves `generate_drt_ours.sh`'s emit
path is byte-stable (the `--verify` arm demonstrates the same
property script-side; results.md records which arms ran).

## 3. `#[ignore]` picorv32 leg

`test_e2e_ours_reference_picorv32` — same joins as (1) on
`picorv32.drt.ours.reference.json`; ignored for runtime, run
manually before committing its golden (E0: say what ran).

## 4. Re-pins of existing assertions

The hull mask changes routes → several existing asserts re-pin
against measured values (named, not erased):

- `test_emit_roundtrip_gcd` / emit census asserts:
  `unrouted.is_empty()` → if the mask strands pins, pin the measured
  unrouted *name list* (a `BTreeSet` literal) — the named residual
  class, reviewed on change.
- `test_maze_census` classification asserts — unchanged unless new
  FailKinds appear; `hull_masked` census prints added.
- `test_emit_selfdrc_gcd` `findings_total == 0` — must still hold;
  a nonzero self-DRC post-fix is its own signal.
- `test_maze_occupancy` — digest comparison, still fine.

## Fail-proof demonstration

Container-side: the RED first-run measurement already demonstrated
the deck fires nonzero on our old DEF — recorded in results.md as
the gate's proven red arm. Test-side: `sanity_control` asserts +
a census assert that `hull_masked > 0` on gcd (the mask fires, not
vacuously green).

## Files

- `crates/grus-core/tests/drt.rs`
