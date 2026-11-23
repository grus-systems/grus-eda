# e6-drt-tracks — Implementation steps

One commit per the ticket (~one logical unit is larger than the usual
50–150 line guidance; if it grows past ~400 lines, split into two
commits: 1–3 parse+model, then 4–7 census+docs). Order:

1. **`RoutingLayer` track fields** (`tech_db.rs`): `tracks_x`,
   `tracks_y` with `#[serde(default)]`, plus the transient
   `LefTrackRecord` + `TechDb::lef_track_records` (`serde(skip)`).
2. **LEF `TRACKS` parse** (`lef.rs`): top-level dispatch arm, record
   collection, end-of-file expansion into the layer lists; unit tests
   from `tests.md` (expand, multi-layer, bare form, before-UNITS,
   unknown-layer error, malformed error).
3. **`drt` module** (`drt.rs`, `drt/tracks.rs`, `lib.rs`):
   `Axis`, `LayerTracks`, `TrackGap`, `layer_tracks`, `track_gaps`;
   merge/gap unit tests.
4. **Access model** (`drt/access.rs`): `AccessPoint`, `OffTrackPin`,
   `AccessReport`, `access_points`, `pin_shapes`; rect-level unit test.
5. **Integration census** (`tests/drt.rs`): `load` helper,
   `test_track_census`, `test_access_census`,
   `test_access_off_track_fixture`, `test_access_failproof`.
   `picorv32.placed.def` must exist (generate via
   `tests/designs/picorv32/generate_ref.sh` if absent — container run,
   mention it before running).
6. **Docs**: `docs/e6-drt.md` (model + claim boundary), dated
   `docs/results.md` entry with the actual printed census,
   `status.md` top line, `roadmap.md` progress line, `README.md`
   reading-order link.
7. **Version**: `Cargo.toml` `0.12.0 → 0.12.1` + `Cargo.lock` in the
   functional commit.
8. **Verify**: `cargo test --workspace`;
   `cargo clippy --workspace --all-targets -- -D warnings`. Record
   what ran (and the picorv32 generation step if it ran) in
   `docs/results.md`.

## Sequencing notes

- Steps 1–2 are one logical change (fields + parse); keep together.
- The fixture test (step 5) does not depend on real designs — write it
  before the census tests if debugging ordering helps.
- Fail-proof is part of `test_access_failproof`, not a separate run —
  the mutate/restore lives inside one test.

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/`
- `crates/grus-core/src/lib.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `docs/README.md`
- `Cargo.toml`
- `Cargo.lock`
