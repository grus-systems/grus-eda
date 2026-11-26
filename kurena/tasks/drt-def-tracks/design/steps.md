# Implementation steps

Single logical change, one commit. Ordered so each step compiles
on its own:

1. **`drt::tracks::default_track_grids`** — new pub function in
   `drt/tracks.rs` + its unit tests (`tests.md`). Pure addition.
2. **GCELLGRID read-side fix** — rename `num_boundaries` →
   `num_cells` in `GCellGridDef`, store it in the `def.rs` parse
   arm, `num_x = num_cells.max(1)` in `GCellGrid::build`, update
   the doc lines in `def.rs`/`design_db.rs`/`architecture.md`.
   Add the `DO 13 → nx 13` test (red before the fix, green after).
3. **`write_def_full` emission** — TRACKS + GCELLGRID blocks
   between ROWs and COMPONENTS per `tracks.md`/`gcellgrid.md`;
   update the function doc comment (VIAS non-goal stated).
   Round-trip + determinism tests.
4. **Regenerate `gcd.cts.ours.def`** —
   `tests/designs/generate_cts_ours.sh gcd` (release build,
   PDK at `~/.ciel`); commit the artifact. Expect diff = added
   header lines only.
5. **Docs + version** — `docs/results.md` dated entry;
   fix now-stale prose in `docs/e0-readers.md` (writer bullet),
   `docs/roadmap.md` ~line 427 ("TRACKS/GCELLGRID/VIAS/SPECIALNETS
   are omitted"), `docs/e6-drt.md` ~line 631 (the write_def_full
   rejection rationale stays valid via PDN/SPECIALNETS but must
   stop claiming TRACKS is dropped). `Cargo.toml` `0.12.7 →
   0.12.8`. Run `cargo test --workspace` and clippy `-D warnings`.

## Files

- `crates/grus-core/src/drt/tracks.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/lef.rs` (per-axis `PITCH`/`OFFSET` retention)
- `crates/grus-core/src/tech_db.rs` (`pitch_{x,y}_dbu`/`offset_{x,y}_dbu` fields)
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/congestion/gcell.rs`
- `crates/grus-core/tests/`
- `docs/results.md`
- `docs/e0-readers.md`
- `docs/roadmap.md`
- `docs/e6-drt.md`
- `docs/architecture.md`
- `tests/designs/gcd/gcd.cts.ours.def`
- `Cargo.toml`
