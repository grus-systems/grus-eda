# Tests

## New unit fixtures

- **Bend via pairing** (`guide.rs` tests, on `mini_tech`/`mini_design`):
  a routed net with an H run and a V run meeting at a non-pin cell →
  assert `boxes` contains consecutive `met1`/`met2` 1×1 boxes at the
  bend. Mirror check in `parasitics.rs`: feed the emitted boxes through
  `guide_net_tree` → the `met1` and `met2` nodes sit in one component
  (pin on the `met1`-only side gets a real Elmore delay, not 0/lumped).
- **Port rung stack** (`guide.rs` tests): pin cell with access `met3`
  bound to a `met1` strip → stub group emits contiguous ascending
  `[met1, met2, met3]` zls at the cell, adjacent to the strip wire.
  Parasitics side: `(P,met3)` reachable → pin not dead → no
  `dead *P` → `lumped` not forced by `dead_port`.
- **End-cell order** (`group_after`): `high` non-empty at a strip end →
  `low` descending then `high` ascending — parasitics check that
  `(P,met3)` is live and the `met1`-rung tail convention still yields
  the expected self-loop set.
- **Dead-`*P` census gate** (`parasitics.rs` tests): run
  `estimate_guide_parasitics` on the emitted guides of each design
  (load `.placed.def`, `route_design`, `build_guides` — the
  `guide_caps_match` pattern), count pins with
  `pin_cells[p].is_none() && dead[pin_node]`; print the per-net list,
  assert the count equals the post-fix measured bound (exact count —
  `build_guides` is deterministic; update the constant deliberately).
  Same assert on `spef_parasitics` of the emitted `<d>.guide.ours.spef`
  so the gate covers the written artifact.
- **Island census diagnostic** (print-only, `--nocapture`): multi-node
  dead-component count on (i) our boxes, (ii) our boxes parsed from the
  emitted guide, (iii) `estimate_guide_parasitics` on the **reference**
  `parse_guides(<d>.guide)` — an apples-to-apples baseline inside our
  own engine; the named residual class prints per net.

## Existing gates that must stay green

- `test_gcd_guide_matches_openroad` / `test_picorv32_guide_matches_openroad`
  — e4-1 weighted share ≥ 0.70, box delta ≤ 15% (record new delta).
- `test_gcd_guide_parasitics_match_openroad` /
  `test_picorv32_guide_parasitics_match_openroad` — e4-2 `*D_NET` gate
  269/269 + 6744/6744 (added boxes are zero-length; totals must be
  byte-identical in the per-net diff).
- `test_guide_emit_deterministic`, `strips_merge_dedup_and_cover_corners`
  (update expected boxes for the new stub groups / bend pairs),
  `test_guide_straight_strip_rc`, `test_guide_via_transition`,
  `test_guide_island_dead_semantics`, `test_guide_pin_pair_one_gcell`,
  `test_guide_skips`, `test_resolve_guide_nets`,
  `test_guide_parasitics_spef_round_trip`,
  `test_guide_parasitics_deterministic`,
  `test_spef_parasitics_accepts_openroad_guide_spef`.
- `test_<d>_guide_spef_engine_residual` — `GCD_SPEF` /
  `PICORV32_GUIDE_ENGINE` bands; a move here is an engine/lumped-model
  regression, not a topology artifact (the new live `*P`s shrink the
  lumped-fallback population — expected movement, verify per endpoint).

## e2e assertions (after step-9c regeneration)

- `test_<d>_guide_sta_matches_opensta` — re-measured `Tol::*_GUIDE`
  floors (tighten to the new extremes).
- `test_<d>_guide_sta_decomposition` — print-only table feeding
  `docs/results.md`; gcd topology max < 250.8 ps and picorv32 <
  626.66 ps, or the residual re-attributed.

## Fail-proof (§7.7)

- Disable bend-pair emission → island census / dead-`*P` gate goes red
  (multi-node islands return; gcd `_012_`-class dead components back).
- Emit the `high` rung stack descending (or revert to `stub_extra` for
  access ≥ S) → `resp_val`-class dead `*P` returns → census gate red.
- Restore both, rerun, committed goldens byte-identical to HEAD.

## Verify commands

`cargo test --workspace`; `cargo clippy --workspace --all-targets --
-D warnings`; `cargo test --release -p grus-core sta` (`sta-regression`
— requires container + seeded picorv32 artifacts).
