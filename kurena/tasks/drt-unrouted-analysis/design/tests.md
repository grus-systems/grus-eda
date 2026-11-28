# drt-unrouted-analysis — Test pins (r3: as-built)

All pins landed in `crates/grus-eda-core/tests/drt.rs`. **Verified
in r3 planning**: `cargo test --workspace --profile verify` green
(42 drt tests, 4 ignored); `cargo clippy --workspace --all-targets
-- -D warnings` clean. The picorv32 leg ran once — measured table
in `steps.md` §0, re-pin spec in §2 below.

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

## 2. `test_unrouted_probe_picorv32` (`#[ignore]`d) — re-pin per `steps.md` §1

Same bundle on `leg_for("picorv32")`. The r1 run completed
(3403.67 s, `/tmp/probe_picorv32.r1.log`) and failed two
stale-pin asserts — measured values, all reproduced through
`assert_probe_coverage` (which ran green before the panic):

- `rep.nets.len() == 1731` (was `== 2016` — the v0.12.9 pin;
  the route's own report moved under `drt-guide-escape` +
  `drt-rayon` with the picorv32 leg deferred: 970 rescued /
  685 newly unrouted vs the committed golden list).
- `Σ n.pins.len() == 3083`.
- Union census pinned separately from the class count per the
  r2 decision rule: `union == Bridged` nets `== 142`, and
  `net_counts()["geom_met3_bridge"] == 32` (the spec's 43 was
  the 108-net-era union census — lineage in `steps.md` §0).
- Full `class_counts` pin: `neg_attach_stub 1230`, `neg_wave
  1206`, `geom_met3_bridge 466`, `geom_pocket 73`,
  `neg_wave_obs_frontier 59`, `neg_wave_unattributed 49`,
  every other class 0.
- `unguided` name-equality + `== 67` — asserts unchanged; the
  r1 run panicked before evaluating them, so the re-run is
  their first real check.

Run (detached, ~57 min measured — the "~135 min" comment is
stale; `steps.md` §2):

```
cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture
```

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
  `-D warnings` — **green on HEAD** (r3 planning ran both; re-run
  after the §2 re-pin per `steps.md` §4).
