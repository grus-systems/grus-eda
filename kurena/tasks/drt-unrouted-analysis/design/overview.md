# drt-unrouted-analysis — Overview

> **Revision 3 (2026-11-28) — finish in place; the leg already
> ran.** The r2 executor died on a model rate limit at `04b11ca`,
> *after* the picorv32 probe leg had completed once
> (`/tmp/probe_picorv32.log`, 3403.67 s). Verified during r3
> planning on HEAD: `cargo test --workspace --profile verify`
> **green** (8 cli + 559 lib + 18 drc + 42 drt [4 ignored] + 2
> irdrop); `cargo clippy --workspace --all-targets -- -D
> warnings` **clean** (the r2 clippy FAIL was the `EdgeProbe`
> `private-interfaces` trip — already fixed by `04b11ca`). The
> leg FAILED two stale-pin asserts, both root-caused (not
> classifier bugs): `unrouted_nets` 1731 vs the v0.12.9-pinned
> 2016 — the picorv32 leg stayed deferred through
> `drt-guide-escape` and `drt-rayon` (set-diff vs the committed
> golden: 970 rescued / 685 newly unrouted); and
> `geom_met3_bridge` nets 32 vs the spec's 43 — the 43 was the
> 108-net-era *union* census; today's union census is 142 with
> the class count 32 after the probe-rescue split. **Remaining**
> (`steps.md`): re-pin `test_unrouted_probe_picorv32` to the
> measured values (union census and class count pinned
> separately), one detached re-run to prove it green (~57 min),
> the `results.md`/`e6-drt.md`/`status.md` amendment naming the
> deltas, verify re-run, commit. The technical design below
> stands as-built.

Analysis-only E6 ticket. The final unrouted report already names
per-pin kinds (`NoPath` 2,225 / `Unattachable` 1,480 on picorv32,
`NoPath` 46 / `Unattachable` 52 on gcd — note the current gcd leg
measures 93 pin-failures, a delta of 5 the results entry names)
and wave-death postmortem blockers (`l1Net` 1,194 dominant) — but
neither answers the fix question: *would this pin route if the
other nets were not in the way?* This ticket decomposes every pin
in the hard-iteration unrouted report into exactly one root-cause
class — **geometric** (fails even with foreign `Net`/`Pin` marks
absent) vs **negotiation** (routes with them absent) — with named
subclasses, run as a post-pass probe over `route_db.unrouted` on
gcd and picorv32.

## What exists (verified against code)

- `route_db.unrouted` (`Vec<UnroutedNet>`) — the final
  hard-iteration report, `failed: Vec<(PinId, FailKind)>` in
  `pin_nodes` order plus per-net `blockers: Vec<(u8, u8)>`
  (`(layer, kind)`, kind 0=`Obs` 1=`ObsPin` 2=`Net` 3=`Pin`,
  `maze.rs` ~134-139, attribution loop ~2753-2840).
- `route_db.stub_outside_cover` — the ever-failed paint census
  (per `(net, pin)`); a final `Unattachable` pin that got there
  via extraction-stage refusal (`try_pin_paint` exhaustion)
  appears here; a topo-stage `Unattachable` does not.
- `Occupancy` + `premark_obs`/`premark_marks` (`maze.rs` ~741-816)
  — rasterizes `Obs` (cell OBS clipped vs pin rects + PDN
  `Obs(u32::MAX)`), `ObsPin` (instance-pin metal), `Pin` (io-port
  rects, gated by `io_pin_marks`) via `rasterize_pin_marks`
  (~824-850).
- `NetTopo::new` (~1975-2046) — attach sets, `project_attach`
  (~884-929), `access_vias` (~1615+): the occupancy-snapshot
  half, rebuilt per probe net against the probe occupancy.
- `route_one_net` (~2540-3107) — the full per-net attempt:
  seed → `extend_once` wave → extraction with `try_pin_paint`
  (~3188-3347) → `NetOutcome::{Routed, Failed{unrouted,
  stub_blocked}}`.
- `build_net_graph` (`graph.rs` ~1278) — per-net `NetBuild`
  carrying `graph` + `region_escape`/`escapes` census records
  (`graph.rs` ~86-137). `HullMask::new` (`graph.rs` ~481).
- `graph::guide_union_connected` / `graph::guide_union` /
  `GuideUnion` (`graph.rs` ~1532+) — moved verbatim from
  `tests/drt.rs` (the pinned `met3_bridge`/`met2_connected`
  classifier), shared by `maze_census` and the probe.
- `drt::probe` (`probe.rs`) — `probe_unrouted` post-pass,
  `RootCause` (13 classes), `ProbePin`/`ProbeNet`/`ProbeReport`,
  `ProbeState`, `AttachEvidence`/`attach_evidence` rescan.

## Key finding that shapes the probe

`OccMode::Disabled` **cannot** implement the probe. It skips
foreign `Net`/`Pin` checks only inside `hit_verdict`
(~986-988); the paint-side checks — `fill_blocked` (~3724),
`try_pin_paint`'s stub/via scans, `close_self_spacing`, the
min-area patch — hard-block on foreign `ObsPin`/`Net`/`Pin`
mode-blind ("emit never gets a soft pass"). The spec's probe
occupancy is therefore **physically rebuilt**: `premark_marks(..,
io_pin_marks: false)` — `Obs`/`ObsPin`/net-less `Obs(u32::MAX)`
retained, all `Pin` marks absent (equivalent to "foreign
`Net`/`Pin` absent": no `Net` marks exist in a fresh occupancy;
own `Pin` marks are free to their own net, so dropping *all*
`Pin` marks is behaviour-identical to dropping only foreign ones
— `probe.md` §1).

## Decisions (all landed in `ed0a8d7`)

1. **Standalone post-pass, not inside `route_prepared`.**
   `pub fn probe_unrouted(db, tech, tracks, join, opts, vias,
   route_db, threads) -> ProbeReport` in `src/drt/probe.rs`.
   `route_nets` never calls it — routed output unchanged *by
   construction* (`route_digest` and DEF goldens cannot move;
   verified anyway, `tests.md`).
2. **Per-net `NetTopo` rebuild against the probe occupancy.**
   Foreign io-port `Pin` bands refused
   `project_attach`/`access_vias` candidates at topo time;
   `NetAdj`/`route_one_net` rerun per net inside `drt_pool`.
3. **Classification is `(real FailKind × probe state)` driven.**
   Probe `Connected` → negotiation (sub-split: real
   `Unattachable` → attach-side, real `NoPath` → wave-side,
   evidence from `stub_outside_cover` / `u.blockers`).
   Probe-failed → geometric (sub-split by attach-evidence
   rescan and the guide-union class). Full tree in
   `taxonomy.md`.
4. **Residual classes are named, never bucketed.**
   `geom_disjoint`, `neg_wave_obs_frontier`,
   `neg_wave_unattributed`, `residual_missing_pin`,
   `residual_attach_inconsistent`. Every `u.failed` entry gets
   exactly one class; per-pin counts sum to the unrouted census
   (asserted: 93 on gcd).
5. **The `unguided` io-port row is reproduced from `join` +
   `db`, not probed.** `join.absent_expected` ∩ io-pin-only ∩
   1-member — joined by name to `ours_router.unguided` in the
   committed golden (gcd 0 asserted green; picorv32 67 pending
   the leg).
6. **Invocation is a test, not a CLI flag** — `## Files` scopes
   the change to `src/drt/` + `tests/drt.rs` + docs. The probe
   is opt-in by construction; the results entry records the
   exact `cargo test` command line.

## Risks (updated for r3 — the leg ran; risks resolved or measured)

- **picorv32 leg runtime — resolved.** Measured 3403.67 s ≈
  57 min total (route ~53 min + probe ~3.7 min), inside
  `timeout_sec = 7200`; the test's "~135 min" comment is stale.
  Still run the re-run detached (cheap insurance).
- **`geom_met3_bridge == 43` assert — failed as flagged, root
  cause found.** Measured: union census 142, class count
  32 nets / 466 pins. Resolution: pin union census and class
  count separately (`steps.md` §1); results entry names the
  43 (108-net era) → 138 (2016 era) → 142 → 32 lineage.
- **`unrouted_nets == 2016` assert — stale pin.** The route's
  own report is 1731; the spec's 2016 predates
  `drt-guide-escape` + `drt-rayon` (picorv32 legs deferred at
  both). Set-diff vs the committed golden names the churn:
  970 rescued / 685 newly unrouted.
- **`u.blockers` ↔ pin pairing is lossy** — blockers are pushed
  at most once per failed attachable pin in `pin_nodes` order;
  per-pin evidence is exact only when `blockers.len() ==
  #NoPath failed` (then 1:1), else net-level set
  (`taxonomy.md` §4). Implemented as `wave_evidence: Option`;
  `neg_wave_unattributed` measured 49 pins — the named
  uninstrumented-divergence class.
- **Residual assertions — measured.** Both residual buckets 0
  on picorv32 (`assert_probe_coverage` ran before the panicked
  assert, so coverage + containment are proven on the r1 log).

## Files

- `crates/grus-eda-core/src/drt/`
- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
