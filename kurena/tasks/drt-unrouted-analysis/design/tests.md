# drt-unrouted-analysis — Test pins

All pins land in `crates/grus-eda-core/tests/drt.rs` (the
census/classifier test home; spec `## Files`).

## 1. `test_unrouted_probe_gcd`

Runs `probe_unrouted` on the shared `gcd_leg()` route
(`route_db` borrowed — the leg is `&'static`).

Asserts:

- **Completeness** — `report.nets.len() == route.unrouted.len()`,
  same `net` order; `Σ pins.len() == Σ u.failed.len()` (98 on
  the pinned gcd leg); every `u.failed` `(pin, kind)` appears
  exactly once, same order.
- **Class sanity** — `class_counts` sums to 98; residual
  classes are present (counted) even at 0.
- **Anchors**:
  - `geom_met3_bridge`-carrying nets ⊆ the
    `guide_union_connected`-bridged unrouted nets, and
    `geom ∪ rescued == bridged` (the union classifier runs
    over `join` on the same net set — now the shared
    `graph::guide_union_connected`).
  - Every `u.failed` pin carrying `region_escape` lands in a
    named class; the `region_escape ∩ u.failed` split is
    printed (gcd pinned population: `_134_`/`_138_`).
  - `report.unguided` == `ours_router.unguided` from
    `gcd.drt.ours.reference.json` (∅ on gcd — the join is
    exercised, the picorv32 leg carries the count).
- **Output-unchanged**: `route_digest(&leg.route) ==
  GCD_ROUTE_DIGEST` after the probe ran (probe can't mutate
  `&RouteDb`; the pin documents it).
- Prints the full decomposition table + `probe_ms` (the
  results.md transcription source).

## 2. `test_unrouted_probe_picorv32` (`#[ignore]`d)

Same asserts on `leg_for("picorv32")`, plus:
`report.unguided.len() == 67` joined by name to the golden
`ours_router.unguided` list, and the met3-bridge split printed
(`geom_met3_bridge` nets vs bridged-union nets vs the pinned
138/48/43 census numbers — deltas go to `docs/results.md`,
not the assert). Run:

```
cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture
```

## 3. `test_unrouted_probe_determinism` (gcd leg)

Two `probe_unrouted` calls on the same leg produce equal
`ProbeReport`s — `par_iter`-order + pure classification.

## 4. Fixture arms (known-class injections)

`test_unrouted_probe_fixtures` — three arms on the existing
maze fixtures, each injecting a failure of a *known* class:

- **Arm A — `geom_disjoint`**: `test_maze_cases` case-4 route
  (n1's two disjoint met1 guide boxes). Probe → n1's `NoPath`
  pins classify `geom_disjoint`; n3's `u2/C` `Unattachable`
  → `geom_off_track`. Exact-class asserts — the fixture makes
  the cause unambiguous.
- **Arm B — `neg_wave`**: `test_maze_pin_collision` arm-B route
  (n1's only met1 wave path crosses io2's foreign `Pin` band;
  fails `NoPath` with `(met1, Pin)` blocker — already asserted
  there). Probe occupancy drops the `Pin` mark → n1 routes →
  every failed pin lands `neg_wave` with `(0, 3)`-kind
  evidence. The negotiation answer the blocker kind predicts —
  the classifier is checked against an independently pinned
  mechanism, not re-derived.
- **Arm C — `geom_obs_sealed`**: a fixture mutation — `blk`'s
  `OBS` rect injected over `u0/A`'s only attach window (same
  `maze_db`/`maze_guide` harness; the injection is the DEF
  fixture variant, not a committed change). Baseline arm
  routes; injected arm fails `Unattachable` and classifies
  `geom_obs_sealed` (or `geom_no_in_region` if the seal leaves
  no window — the exact class is asserted after the mechanism
  is confirmed on the fixture); reverted arm = baseline re-run
  routes clean. The injection/revert cycle is the
  prove-it-red arm: an *injected known-class failure lands in
  the expected class, then the injection is reverted* —
  recorded in `docs/results.md`.

## 5. Prove-it-red, second arm (classifier break)

Recorded run, not a committed test: temporarily swap one
mapping in the classifier (e.g. `GeomOffTrack → GeomPocket`)
→ `test_unrouted_probe_fixtures` arm A goes red naming the
wrong class → restore. E0 lesson 7 — proves the pins can
fail. The `results.md` entry records the break, the failure
text, and the revert.

## 6. Existing-pins check (must stay green, unchanged)

- `test_maze_census` — `GCD_ROUTE_DIGEST` unchanged.
- `test_e2e_ours_def_deterministic_gcd` /
  `test_e2e_ours_reference_{gcd,picorv32}` — routed DEF
  byte-identical (no emit path touched).
- `test_maze_pin_collision`, `test_maze_cases` — the fixture
  baselines the probe arms reuse; unchanged.
- `cargo test --workspace --profile verify` + clippy
  `-D warnings` (workflow verify commands).
