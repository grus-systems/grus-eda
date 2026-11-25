# Tests

All assertions run on committed inputs; nothing skips silently.

## Unit tests — `drt::tracks` (`tracks.rs` test module)

- `test_default_track_grids_counts`: synthetic 2-layer tech +
  die. Assert `start/step/layers/is_vertical_tracks` and `DO`
  for both axes; include a boundary pair: a candidate coord with
  `c + w/2 == die_hi` is **in**cluded, `c + w/2 == die_hi + 1`
  excluded; a layer whose `offset + w/2 > die_hi` emits no grid
  for that axis.
- `test_default_track_grids_sky130_gcd`: real tech LEF +
  `RectDbu(0,0,91950,102670)` → the 12 grids equal
  `gcd.placed.def`'s parsed `db.tracks` field-for-field (this is
  the derivation-fidelity proof: derived == what OpenROAD
  emitted).

## Round-trip — `readers/def.rs` tests

- `test_write_def_full_tracks_verbatim`: parse `gcd.placed.def`,
  `write_def_full` to temp, re-parse → `db2.tracks ==
  db.tracks` and `db2.gcell_grids` empty-derived present (gcd's
  placed DEF has no GCELLGRID). Optionally diff the emitted
  TRACKS block text against the input's 12 lines.
- `test_write_def_full_tracks_derived`: same, but clear
  `db.tracks` first → emitted file's TRACKS lines are
  byte-identical to the verbatim case (derived reproduces
  OpenROAD's values on gcd).
- `test_write_def_full_gcellgrid_rebuilds_default`: on the
  emitted file, `GCellGrid::build(db2, tech, None, None)` yields
  `(origin, w, h, nx, ny)` equal to `compute_default_grid` on
  the stripped db — gcd: origin 0/0, tile 6900, 13×14.
- `test_gcellgrid_do_is_column_count`: a DEF text with
  `GCELLGRID X 0 DO 13 STEP 6900` builds `nx == 13`
  (pins the read-side convention change; would fail before).
- `test_write_def_full_deterministic`: write the same db to two
  temp files → byte-equal. (The cts golden test also pins this;
  keep the cheap unit version.)

## Golden regeneration

- `cargo test -p grus-core` will flag
  `test_gcd_cts_ours_def_deterministic` red until
  `tests/designs/generate_cts_ours.sh gcd` is re-run and
  `gcd.cts.ours.def` committed — expected: the file now carries
  the TRACKS + GCELLGRID block (diff should show only the added
  lines; `gcd.cts.ours.report.json` unchanged).

## Red-check (prove the tests can fail)

- Comment out the TRACKS emit → `tracks_derived`/`verbatim`
  tests fail.
- Emit `DO = floor(die/step)` (dropping `w/2`) → the derived test
  fails on `gcd` met4 Y (`112` vs `111`) and picorv32-style
  margins.
- Keep `num_x = num_cells - 1` → `gcellgrid_rebuilds_default`
  fails (`nx 12` vs `13`).

## `docs/results.md` dated entry

Date, what was run (`cargo test --workspace` result count), the
emitted header block on gcd verbatim, the two conventions
(evidence pointer to this design dir), note that it unblocks a
self-contained `floorplan → place → route → drt` chain, and the
`gcd.cts.ours.def` regeneration note.
