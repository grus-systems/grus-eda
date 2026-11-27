# drt-unrouted-analysis — Steps

HEAD is `dev` post-`v0.16.0` (`drt-thread-param`). One logical
feature — land it as two commits (implementation+pins, then
docs/measurement) or one if the measurement run folds in
cleanly; keep the `Cargo.toml` patch bump (0.16.0 → 0.16.1)
in the functional commit per the versioning rule.

## 1. `graph.rs` — shared union classifier

Move `guide_union_connected` from `tests/drt.rs` (~2313) into
`graph.rs` as `pub fn guide_union_connected(boxes: &[GuideBox],
max_layer: u8) -> bool`, plus `pub enum GuideUnion {Bridged,
Met2Connected, Disjoint, GuideAbsent}` and
`pub fn guide_union(boxes: &[GuideBox]) -> GuideUnion`
encoding the pinned semantics verbatim:
`!conn(≤2)&&conn(all) → Bridged`; `conn(≤2) → Met2Connected`;
`else → Disjoint`; `boxes.is_empty() → GuideAbsent` (the
classifier's `kept.is_empty() → false` arm lands a
box-empty net in `Disjoint` today — keep `GuideAbsent`
distinct so the "0 guide-absent" invariant stays assertable
rather than collapsing into `Disjoint`; check the test
helper's exact arm before deleting it). Update
`tests/drt.rs` call sites (`maze_census`'s bridged/pocket
block ~2510-2539) to the shared function and delete the local
copy — semantics must be a verbatim move, the pinned
`met3_bridge` count depends on it.

## 2. `maze.rs` — `pub(crate)` openings + premark split

- `premark_obs` → internal `premark_marks(db, tech, occ,
  io_pin_marks: bool)`; `premark_obs` remains as the `true`
  wrapper. `rasterize_pin_marks` gains `io_pin_marks: bool`
  (the `Occupant::Pin` arm ~841-842 is skipped when false);
  update the `pin_overlap_census` call site (~4883) with
  `true`.
- Widen to `pub(crate)`: `Occupancy` + `new`, `scan_wire`,
  `scan_rect`, `Occupant`, `Hit`, `hit_bbox` (if needed),
  `NetTopo::new`, `NetAdj::new`, `route_one_net`, `NetOutcome`,
  `scan_stub`, `fill_blocked`, `node_xy`, `covered_range`.
  Field-level access only where the rescan touches them
  (`h.iv.who`, `h.layer`).

No behaviour change; `cargo test -p grus-eda-core --lib` and
the drt suite stay green after this step alone.

## 3. `src/drt/probe.rs` — the post-pass

Per `api.md`: `RootCause`, `ProbePin`, `ProbeNet`,
`GuideUnion` re-export or local, `ProbeReport` +
`class_counts`/`net_counts`, `probe_unrouted`.

Order of work inside `probe_unrouted`:

1. `HullMask::new`, `Occupancy::new` +
   `premark_marks(.., false)` (probe occupancy — shared).
2. `net_idx` map `NetId → join.nets` index; `pool =
   pool::n(threads)` (`src/drt/pool.rs`); `pool.install` +
   `unrouted.par_iter().map_init(Vec::new(), …)` collecting
   `ProbeNet`s in `unrouted` order.
3. Per net: `build_net_graph` → `NetTopo::new` (probe occ) →
   `NetAdj::new` → `route_one_net(hist=0, hard=false,
   OccMode::Enabled)` → classify `u.failed` per the
   `taxonomy.md` tree, filling `ProbePin` rows (attach-evidence
   rescan per `probe.md` §3 only on probe-`Unattachable`
   pins; `wave_evidence` per §4's pairing rule; `union` via
   `guide_union`).
4. `unguided` per `probe.md` §5; `elapsed` measured.

## 4. `tests/drt.rs` — the pins (`tests.md`)

`test_unrouted_probe_gcd`, `test_unrouted_probe_determinism`,
`test_unrouted_probe_fixtures` (arms A–C),
`test_unrouted_probe_picorv32` `#[ignore]`d. Update the
`guide_union_connected` call sites from step 1 in the same
hunk range.

## 5. Verify + red arms

```
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_gcd -- --nocapture
cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture
```

Then the two prove-it-red arms (`tests.md` §4 arm C
inject/revert — the test itself IS the recorded arm; §5
classifier-break — run once, record the red text, restore).

## 6. Docs + commit

`docs/results.md` entry per `docs.md` §1 (tables transcribed
from the test prints, probe_ms measured, anchors reconciled,
target list ordered, red-arm record, what-ran);
`docs/e6-drt.md` §7 subsection; `docs/status.md` line;
`Cargo.toml` 0.16.1.

```
feat: Add root-cause decomposition of the unrouted population

- drt/probe.rs: probe_unrouted post-pass — re-attempts each
  finally-unrouted net's attach + search against a premark
  occupancy without foreign Net/Pin marks (Obs/ObsPin
  retained), classifying every u.failed pin into mutually
  exclusive geometric vs negotiation causes
- maze.rs: premark gains an io_pin_marks switch; internals
  widened to pub(crate) for the probe module
- graph.rs: guide_union_connected moved here (shared by the
  pinned met3_bridge classifier and the probe)
- tests/drt.rs: gcd/picorv32 census pins, fixture
  known-class arms, determinism; unrouted totals and the
  met3_bridge/region_escape/unguided anchors asserted
- routed output unchanged: post-pass only, route_digest and
  DEF goldens byte-identical (pinned)
```

## Contingencies

- **picorv32 probe_ms is huge** — probes parallelize where
  the hard arm is serial, so this should stay well under one
  iteration; if not, the entry records the measured cost and
  the `#[ignore]`d leg stays the opt-in path (never loosen
  the report into estimates).
- **A residual class fires non-zero** — that IS a finding:
  name it in the results entry with the net/pin list, do not
  fold it away or re-map it silently.
- **Anchors don't reproduce** — the spec's escape hatch is
  "name the delta": record the measured split and the
  mechanism hypothesis; do not bend the classifier to hit a
  number.
- **`probe_attached` arms** (real `Unattachable` → probe
  `NoPath`) — expected small; if it dominates an attach
  class, the evidence flag is what keeps the story honest in
  the results table.

## Files

- `crates/grus-eda-core/src/drt/graph.rs`
- `crates/grus-eda-core/src/drt/maze.rs`
- `crates/grus-eda-core/src/drt/probe.rs`
- `crates/grus-eda-core/src/drt.rs`
- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
