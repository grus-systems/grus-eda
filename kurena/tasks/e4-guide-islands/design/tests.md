# Tests

## New / updated unit fixtures (`parasitics.rs` test module)

- **`test_guide_island_dead_semantics`** (extend or adapt
  `test_guide_via_transition`, which already builds a disconnected `li1`
  stub island): assert
  - the dead node keeps its cap in `total_wire_cap_pf`,
  - `dead[pin_node]` pins return `0.0` from `elmore_ns` while reachable
    pins keep real delays,
  - `write_spef` emits no resistor bridging components, plus a
    `name name <cut-R>` self-loop for the unpaired stub,
  - round-trip through `parse_spef`/`spef_parasitics` reproduces the same
    dead/pin mapping (update `test_guide_parasitics_spef_round_trip` —
    it now exercises dead components too).
- **Self-loop fidelity**: fixture with a multi-node island (two cells on
  one layer, detached) → `*RES` keeps the internal real resistor between
  the island nodes and adds the self-loop; mirrors `gcd.guide.spef`
  `_132_` structure.
- **Reader parity**: feed a hand-written mini SPEF with a dead component +
  `*CONN` pin → `spef_parasitics` must return a tree with the pin marked
  dead instead of bailing; also un-bails OpenROAD's own `gcd.guide.spef`
  (`_057_`) — assert `spef_parasitics` on it now succeeds if cheap to add,
  else record as a recorded improvement.

## Existing gates that must stay green

- `test_gcd_guide_parasitics_match_openroad` /
  `test_picorv32_guide_parasitics_match_openroad` — e4-2 cap gate,
  `269/269` + `6744/6744` inside `max(5%, 1 fF)` (dead-node caps are still
  summed).
- `test_gcd_guide_matches_openroad` / `test_picorv32_guide_matches_openroad`
  — e4-1: weighted share ≥ 0.70, box delta ≤ 15% (split strips raise the
  count — assert and record).
- `test_guide_emit_deterministic`, `strips_merge_dedup_and_cover_corners`
  (update expected boxes for split strips / ordered emission),
  `test_guide_straight_strip_rc`, `test_guide_pin_pair_one_gcell`,
  `test_guide_skips`, `test_resolve_guide_nets`,
  `test_guide_parasitics_deterministic`.
- `test_write_spef_round_trip_parasitics` / `_sta` (placement path —
  `dead` all-false must not alter anything).

## e2e assertions (golden-driven, after step 9c regeneration)

- `test_<d>_guide_sta_matches_opensta` — re-tightened `Tol::*_GUIDE`
  floors from the measured residual.
- `test_<d>_guide_spef_engine_residual` — `GCD_SPEF` /
  `PICORV32_GUIDE_ENGINE` bands must hold without widening (if they move,
  the cause is named).
- `test_<d>_guide_sta_decomposition` — print-only four-row table feeding
  `docs/results.md`; parasitics-axis median must drop materially.
- Optional new assertion: self-loop census regression guard (e.g.
  post-fix ours self-loops ≤ measured value) — keep it measured, not a
  hardcoded target without evidence.

## Fail-proof (§7.7)

- Re-enable the `SPEF_STUB_RES_KOHM` bridge temporarily → decomposition
  parasitics axis must regress (red on the tightened floors), then revert.
- Drop `dead` marking in `elmore_ns` (return tau) → gate test red.
- Revert to layer-major `write_guides` sort → census test/read-back count
  returns to ~1,389 (red if a census guard exists).

## Verify commands

`cargo test --workspace`; `cargo clippy --workspace --all-targets --
-D warnings`; `cargo test --release -p grus-core sta` (the `sta-regression`
verify — requires the container + seeded picorv32 artifacts).
