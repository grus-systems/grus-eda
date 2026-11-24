# e6-drt-emit — Test design

All emit tests live in `crates/grus-core/tests/drt.rs` in the emit
section after `mod maze` (sibling scope, shared helpers `emit_leg`,
`self_drc`, `print_emit_census`, `emit_gate`, `tmp_def`,
`placed_def`, `geom_totals`). Emitted DEFs land in
`std::env::temp_dir()` per-process paths — generated artifacts,
never committed (the committed `<d>.drt.ours.routed.def` is
`e6-drt-e2e`'s artifact).

## Landed test list (v0.12.5 — all green on gcd; picorv32 leg red as classified)

- `test_emit_roundtrip_gcd`, `test_emit_selfdrc_gcd`,
  `test_emit_fail_proof` (both arms), `test_emit_determinism`,
  `test_emit_unrouted_fixture`, `test_emit_via_default_consistency`,
  `test_emit_selfdrc_picorv32` (**`#[ignore]`d** — red at 42 min-area
  findings; the patch is the fix, never an assert loosening).

## This revision — the min-area patch fixtures

The patch function is private to `maze.rs`, so its fixtures live in
`maze.rs`'s `mod tests` next to `toy_tech`/`toy_tracks`/
`square_graph`. `toy_tech`'s single `Horizontal` layer
(`width_dbu=140`, `spacing_dbu=140`, tracks `[0,1000,2000,3000]`²)
gets an `area_dbu2` bound via a parameter — either extend
`toy_tech(bound)` or add a `toy_tech_area` sibling; the existing
callers pass `0` (no `AREA` → pass skips, preserving every current
assert).

- `test_min_area_patch_fills` — hand-built `NetRoute`: one wire
  `x∈[0,1000] @ y=0` on layer 0 → rect `[-70,-70,1070,70]`,
  area 159,600. Bound 250,000: the `NetGraph` line
  `nodes (0,0),(1,0),(2,0),(3,0)` puts candidate nodes at x=0 and
  x=1000; the first candidate's +x walk accumulates `(0,0)→(2,0)`
  → one `WireSeg` `(l,0,0)-(2000,0)` spanning the existing wire
  plus the extension → patched union 299,600 ≥ bound. Assert the
  appended `WireSeg` is exactly `(l,0,0)-(2000,0)`,
  `area_patches == 1`, `residuals` empty, and `occ` now reports
  own-net hits on the extension line (the patch metal is
  committed).
- `test_min_area_patch_pin_merges` — same wire plus a member pin's
  `pin_io_rects` rect touching it: the component's measured area
  includes the pin metal (the checker's plane merge), so a bound
  just over `wire+pin` patches a smaller span than the no-pin
  case. Pins the component model's pin inclusion.
- `test_min_area_patch_occupancy_blocked` — foreign marks cover the
  +x extension span and an `Obs(u32::MAX)` mark covers −x → both
  directions reject → `residuals` gains exactly one entry
  (`layer=0`, `area_dbu2=159,600`, `bound=250,000`, region = the
  component bbox), `route.wires` unchanged, `area_patches == 0`.
  This is the fail-proof arm: the mechanism *can* decline.
- `test_min_area_patch_region_bound` — the wire ends at the last
  covered node on its line (graph carries only `(0,0),(1,0)`) →
  no in-region step exists → residual, wires unchanged. Pins
  decision 9 (patch never leaves the guide region).
- `test_min_area_patch_in_route_nets` — through-path: a `route_nets`
  call on the hand-built `square_graph`/`db` pattern with a bound
  the routed wire cannot meet alone → `route_db.area_patches == 1`,
  `area_residuals` empty, `route.wires` contains the extension.
  Pins the post-loop hook, not just the function.

Non-vacuity inside the fixtures: before patching, assert the
component *is* sub-bound (`union_area` < bound) — a fixture that
starts legal proves nothing (AGENTS §7.7).

## Emit-suite additions (tests/drt.rs)

- `emit_gate` prints `area_patches=<n> area_residuals=<r>` beside the
  routed/unrouted line and one `area_residual …` line per residual —
  both designs' detached/in-suite logs carry the census verbatim.
- `test_emit_selfdrc_gcd` asserts `route_db.area_residuals` is empty
  (gcd's gate was already 0 — a residual appearing there is a patch
  regression to investigate, and `area_patches` prints whatever it
  measures; do not pin a count).
- `test_emit_determinism` — unchanged mechanism; `PartialEq` now
  covers the two new `RouteDb` fields for free. Assert
  `route_digest` stays `0x48593d441074229b` on gcd *if* the patch is
  a no-op there — verify what the fixture reports first; a digest
  shift means gcd gained patches → investigate, then record.

## Hard constraint — the `#[ignore]` boundary does not move

`test_emit_selfdrc_picorv32` (and the picorv32 maze companions)
stays ignored; no new picorv32-input leg may be added to the
default set. The patch fixtures are toy-scale; the picorv32 proof
is exactly one detached `--ignored` re-run (steps.md).

## Existing tests that must stay green

- `test_maze_*` — the patch mutates `route.wires` post-loop; if any
  pinned digest/count shifts on gcd, that is a finding to
  investigate (patch should be a no-op on a clean gcd route), not
  a silent re-pin.
- `test_via_defaults`, `test_routed_def_net_geoms_reparse`, the
  `tests/drc.rs` `run_all` control — untouched paths.
- `test_maze_occupancy` — `OccMode::Disabled` still paints-but-
  ignores marks; the patch still runs and still rejects foreign
  hits, so disabled-mode routes can only gain *fewer* legal patch
  spots — collision census unchanged in kind.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
