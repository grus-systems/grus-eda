# drt-unrouted-analysis — Overview

Analysis-only E6 ticket. The final unrouted report already names
per-pin kinds (`NoPath` 2,225 / `Unattachable` 1,480 on picorv32,
`NoPath` 46 / `Unattachable` 52 on gcd) and wave-death postmortem
blockers (`l1Net` 1,194 dominant) — but neither answers the fix
question: *would this pin route if the other nets were not in the
way?* This ticket decomposes every pin in the hard-iteration
unrouted report into exactly one root-cause class — **geometric**
(fails even with foreign `Net`/`Pin` marks absent) vs
**negotiation** (routes with them absent) — with named subclasses,
run as a post-pass probe over `route_db.unrouted` on gcd and
picorv32.

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
- `Occupancy` + `premark_obs` (`maze.rs` ~741-816) — rasterizes
  `Obs` (cell OBS clipped vs pin rects + PDN `Obs(u32::MAX)`),
  `ObsPin` (instance-pin metal), `Pin` (io-port rects) via
  `rasterize_pin_marks` (~824-850).
- `NetTopo::new` (~1975-2046) — attach sets, `project_attach`
  (~884-929), `access_vias` (~1615+): the occupancy-snapshot
  half, built once pre-route against the premark-only occupancy.
- `route_one_net` (~2540-3107) — the full per-net attempt:
  seed → `extend_once` wave → extraction with `try_pin_paint`
  (~3188-3347) → `NetOutcome::{Routed, Failed{unrouted,
  stub_blocked}}`.
- `build_net_graph` (`graph.rs` ~1278) — per-net `NetBuild`
  carrying `graph` + `region_escape`/`escapes` census records
  (`graph.rs` ~86-137). `HullMask::new` (`graph.rs` ~481).
- `guide_union_connected` — test-local (`tests/drt.rs` ~2313)
  union-find over guide boxes, `max_layer=2` boundary; the
  pinned `met3_bridge`/`met2_connected` classifier.

## Key finding that shapes the probe

`OccMode::Disabled` **cannot** implement the probe. It skips
foreign `Net`/`Pin` checks only inside `hit_verdict`
(~986-988); the paint-side checks — `fill_blocked` (~3724),
`try_pin_paint`'s stub/via scans, `close_self_spacing`, the
min-area patch — hard-block on foreign `ObsPin`/`Net`/`Pin`
mode-blind ("emit never gets a soft pass"). The spec's probe
occupancy is therefore **physically rebuilt**: `premark_obs`
minus the io-port `Pin` arm — `Obs`/`ObsPin`/net-less
`Obs(u32::MAX)` retained, all `Pin` marks absent (equivalent to
"foreign `Net`/`Pin` absent": no `Net` marks exist in a fresh
occupancy; own `Pin` marks are free to their own net, so
dropping *all* `Pin` marks is behaviour-identical to dropping
only foreign ones — `probe.md` §1).

## Decisions

1. **Standalone post-pass, not inside `route_prepared`.** New
   `pub fn probe_unrouted(db, tech, tracks, join, opts, vias,
   route_db, threads) -> ProbeReport` in a new
   `src/drt/probe.rs`. `route_nets` never calls it — routed
   output unchanged *by construction* (`route_digest` and DEF
   goldens cannot move; verified anyway, `tests.md`).
2. **Per-net `NetTopo` rebuild against the probe occupancy.**
   The spec says "re-attempt each net's attach *and* search" —
   attach must re-run because foreign io-port `Pin` bands
   refused `project_attach`/`access_vias` candidates at topo
   time (`Net` marks could not exist then; `Pin` marks could).
   `NetAdj`/`route_one_net` rerun per net inside `drt_pool`.
3. **Classification is `(real FailKind × probe state)` driven.**
   Probe `Connected` → negotiation (sub-split: real
   `Unattachable` → attach-side, real `NoPath` → wave-side,
   evidence from `stub_outside_cover` / `u.blockers`).
   Probe-failed → geometric (sub-split by attach-evidence
   rescan and the guide-union class). Full tree in
   `taxonomy.md`.
4. **Residual classes are named, never bucketed.**
   `geom_disjoint` (fully disjoint guide union — ~0 expected on
   real GRT output), `neg_wave_obs_frontier`,
   `neg_wave_unattributed`, `residual_missing_pin`,
   `residual_attach_inconsistent`. Every `u.failed` entry gets
   exactly one class; per-pin counts sum to the pinned
   `unrouted_kinds` totals.
5. **The `unguided` io-port row is reproduced from `join` +
   `db`, not probed.** Golden-verified semantics: single-conn
   `( PIN <port> )`-only DEF nets absent from the guide —
   `join.absent_expected` ∩ io-pin-only ∩ 1-member. The test
   joins to `ours_router.unguided` in the committed golden
   (gcd 0, picorv32 67).
6. **Invocation is a test, not a CLI flag** — `## Files` scopes
   the change to `src/drt/` + `tests/drt.rs` + docs. The probe
   is opt-in by construction; the results entry records the
   exact `cargo test` command line (spec's "how it was
   invoked" clause).

## Risks

- **`NetTopo::new` rebuild cost** — per unrouted net it is the
  same order as one attach-pass net (`build_net_graph` +
  topo + `NetAdj` + one `route_one_net`, extraction closure
  included — the honest probe: a pin "connected" only through
  an illegal paint is not connected). picorv32 probe ≈ one
  hard-iteration's worth of work, parallelizable (no commits —
  shared `&Occupancy`), measured not estimated in the results
  entry.
- **Anchor equality is not tautological** — a `met3_bridge`-union
  net the probe rescues lands its pins in negotiation, so the
  geometric `met3_bridge` count ≤ the guide-union bridged
  count. The test asserts the containment + reconciliation
  identity; the results entry names any delta (spec's "or
  names the delta" arm).
- **`u.blockers` ↔ pin pairing is lossy** — blockers are pushed
  at most once per failed attachable pin in `pin_nodes` order;
  per-pin evidence is exact only when `blockers.len() ==
  #NoPath failed` (then 1:1), else net-level set
  (`taxonomy.md` §4).
- **Visibility plumbing** — `probe.rs` needs ~10 `pub(crate)`
  openings on `maze.rs` internals (`api.md` §2). Pure
  visibility widening, no behaviour change.

## Files

- `crates/grus-eda-core/src/drt/`
- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
