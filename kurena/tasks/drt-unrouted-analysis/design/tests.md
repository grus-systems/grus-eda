# drt-unrouted-analysis — Test pins (r2: as-built)

All pins landed in `crates/grus-eda-core/tests/drt.rs`. **Verified
in r2 planning**: dev profile `cargo test -p grus-eda-core --test
drt unrouted_probe` → 5 passed, 1 ignored, 0 failed.

Shared helpers (as-built names):

- `probe_report_lines(name, rep, db)` — prints the decomposition
  table (`class_counts`/`net_counts`), per-net pin classes, the
  `bridged_union_nets / geom_met3_bridge_pins / rescued_pins`
  anchor line, and the `region_escape ∩ u.failed` per-class
  split. The `docs/results.md` transcription source.
- `assert_probe_coverage(rep, db, r)` — the invariant bundle
  every leg asserts: `rep.nets` mirrors `r.unrouted` positionally
  (same net order, same `u.failed` pins/kinds/order),
  `class_counts` sums to `Σ u.failed.len()`, both residual
  buckets are 0 (the assert prints the offending net names),
  `geom_met3_bridge` nets ⊆ `union == Bridged` nets, and no
  bridged-union net carries a non-`geom_met3_bridge` geometric
  pin.
- `probe_find(rep, db, net, pin)` — named-pin row lookup.
- Fixtures: `SEAL_LEF` + `maze_tech_seal()` (arm C's `CELLD` OBS
  seal / `CELLE` no-OBS baseline), `maze_db_custom`,
  `MAZE_DEF_TAIL`, `MAZE_PIN_TAIL`.

## 1. `test_unrouted_probe_gcd` — green (dev + release recorded)

Runs `probe_unrouted` on the shared `gcd_leg()`. Asserts (all
passing, transcribed into `docs/results.md`):

- `assert_probe_coverage` bundle.
- `rep.nets.len() == 80`, `Σ pins == 93` (the current leg's
  unrouted census — the older 98 doc total is a named delta).
- `rep.unguided` name-equal to golden `ours_router.unguided`
  (∅ on gcd).
- `class_counts["neg_attach_stub"] == 54`,
  `class_counts["neg_wave"] == 35`,
  `class_counts["geom_pocket"] == 4` — the measured
  decomposition, pinned.
- `route_digest(&leg.route) == GCD_ROUTE_DIGEST` after the probe
  ran (output-unchanged pin).

## 2. `test_unrouted_probe_picorv32` (`#[ignore]`d) — **pending**

Same bundle on `leg_for("picorv32")`, plus `nets == 2016`,
`net_counts()["geom_met3_bridge"] == 43`, `unguided` name-equal
to the golden (67). Run (detached — see `steps.md` §1):

```
cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture
```

**Assert risk, flagged in r2**: `geom_met3_bridge == 43` pins the
*class* count, but the pinned 43 is the *union* census. If any
bridged-union net is probe-rescued the class count drops below
43 — a finding, not a bug. The decision rule (`steps.md` §4):
re-pin the assert on the union count
(`rep.nets.filter(union == Bridged).count() == 43`) and record
the class split in the results entry. Only touch the assert if
the leg actually fails there; a green run needs no change.

## 3. `test_unrouted_probe_deterministic_gcd` — green

Two `probe_unrouted` calls on the same leg produce equal
`nets`/`unguided` (`elapsed` excluded — wall time).

## 4. Fixture arms (known-class injections) — all green

Separate tests as built (one per arm), each on the maze
fixtures:

- **`test_unrouted_probe_geom_disjoint`** (arm A): case-4
  disjoint guide route — n1 `u0/A` `NoPath` → `GeomDisjoint`;
  n3 `u2/C` `Unattachable` → `GeomOffTrack` with
  `region_escape` set.
- **`test_unrouted_probe_pin_collision`** (arm B): the
  `maze_pin_collision` hard arm — io2's foreign `Pin` band on
  n1's wave path; probe drops it → n1 routes, `u0/A` →
  `NegWave` with `wave_evidence == Some((0, 3))` (`(met1, Pin)`
  — negotiation predicted by the independently pinned blocker
  kind).
- **`test_unrouted_probe_geom_obs_sealed`** (arm C — the
  committed inject/revert arm): baseline `CELLE` (no OBS) routes
  clean; injected `CELLD` OBS seal over `u2/C`'s attach window →
  `Unattachable` → `GeomObsSealed` + `region_escape`. The
  inject/revert cycle inside one test is the prove-it-red arm —
  recorded in `docs/results.md`.

## 5. Prove-it-red, second arm (classifier break) — recorded

Arm D, recorded not committed: `GeomOffTrack → GeomPocket` in
the probe-`Unattachable` rescan arm → arm A red at
`tests/drt.rs:2821` (`left: GeomPocket, right: GeomOffTrack`) →
reverted. The cited line is verified consistent with the current
file; `steps.md` §3 has the optional on-tree re-confirmation.

## 6. Existing-pins check (in the verify lane)

- `test_maze_census` — `GCD_ROUTE_DIGEST` unchanged.
- `test_e2e_ours_def_deterministic_gcd` /
  `test_e2e_ours_reference_{gcd,picorv32}` — routed DEF
  byte-identical.
- `test_maze_pin_collision`, `test_maze_cases` — the fixture
  baselines the probe arms reuse; unchanged.
- `cargo test --workspace --profile verify` + clippy
  `-D warnings` — **the r1 lanes never ran; `steps.md` §3 is the
  real run**.
