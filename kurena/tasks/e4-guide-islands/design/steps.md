# Implementation steps

Order matters: measure the baseline first, then land the model change and
its gates, then the guide ordering, then re-measure everything.

1. **Stub census (baseline).** Run both `check_guide_readback.sh` on the
   existing `runs/guide-check/<d>.ours.guide` (or regenerate via
   `generate_guide_ours_spef.sh`); extract self-loop counts by R value /
   layer for our guide and for the reference SPEFs. Write the table into
   `docs/results.md` work-in-progress notes — the dominant class must be
   named from data, not assumed.

2. **`NetRcTree` dead components** (`sta/parasitics.rs`):
   - Add `dead: Vec<bool>` + `self_loops: Vec<(u32, f32)>` to
     `NetRcTree`; default-filled in `build_net_tree`.
   - `RouteGraph`: record `(node, kohm)` for `a == b` via-segments instead
     of dropping them.
   - `guide_net_tree` BFS tail: replace the `SPEF_STUB_RES_KOHM` bridge
     with deterministic dead-root BFS per remaining component.
   - `elmore_ns`: dead-node pins → `0.0`; guard `parent[v] != v`.
   - `pi_model`: verify no changes needed beyond the dead-root sentinel
     (moments can't cross a self-parented root).
   - `spef_net_tree`: same dead-subtree construction for unreached nodes;
     remove the `not connected to the driver` bail.
   - `write_spef`: skip `parent[v] == v` roots in `*RES`, append
     self-loops; `*CAP`/`*D_NET` unchanged.
   - Unit fixtures per `tests.md`; `cargo test -p grus-core guide` →
     e4-2 gates still 100%.

3. **Guide ordering** (`congestion/guide.rs`):
   - Split strip runs at pin GCells of the net (bounded splits).
   - Replace the `BTreeSet` emission with the ordered stream of
     `stub-pairing.md` §algorithm (start/interior/end/uncovered cases).
   - `write_guides`: preserve builder order, dedup first-occurrence.
   - Update `docs/e4-guides.md` §7 (ordering is now semantic — box order
     drives `updateVias` pairing).
   - `cargo test -p grus-core guide::` → e4-1 gates green; record the new
     box delta.

4. **Regenerate ours-side artifacts** (release build):
   `generate_guide_ours_spef.sh gcd` + `generate_guide_ours_spef.sh
   picorv32` → `<d>.guide.ours.spef` + `runs/guide-check/<d>.ours.guide`.

5. **Container re-measurement** (pinned image):
   - `check_guide_readback.sh` on both new guides → after-census numbers.
   - `generate_ref.sh` step 9c on both designs → new
     `<d>.guide.sta.reference.json`; `git diff` all non-guide goldens must
     be empty.
   - Commit `gcd.guide.ours.spef` + `gcd.guide.sta.reference.json`
     (< 1 MB); picorv32 files stay gitignored.

6. **Decomposition + verdict**: `cargo test -p grus-core
   guide_sta_decomposition -- --nocapture` both designs; update
   `Tol::GCD_GUIDE` / `Tol::PICORV32_GUIDE` to the measured residual
   (tighten only); write the `docs/results.md` entry (before/after census,
   decomposition table, attribution, probe conditions, what was run);
   update `docs/e4-guides.md` §10–11 and `docs/status.md` E4 status
   honestly.

7. **Verify + version**: `cargo test --workspace`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo test --release
   -p grus-core sta` (`sta-regression` verify); bump
   `[workspace.package] version` patch in the same commit.

## Files

- `crates/grus-core/src/sta/parasitics.rs`
- `crates/grus-core/src/congestion/guide.rs`
- `crates/grus-core/src/sta/analysis.rs`
- `tests/designs/gcd/gcd.guide.ours.spef`
- `tests/designs/gcd/gcd.guide.sta.reference.json`
- `tests/designs/picorv32/`
- `docs/e4-guides.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
