# API / code changes

All in `grus-core`. `write_def_full`'s signature is unchanged.

## `crates/grus-core/src/drt/tracks.rs` — new function

```rust
/// DEF `TRACKS` grids for a (tech, die) pair when the design DB
/// carries none — the emit-side inverse of `layer_tracks`
/// expansion. One grid per routing layer per axis (X then Y),
/// `start = offset_dbu`, `step = pitch_dbu`, count = the track
/// coordinates whose centred min-width wire fits inside the die:
/// `die_lo + w/2 <= start + k·step <= die_hi - w/2`.
/// Reproduces OpenROAD's emitted DO counts exactly (see
/// kurena/tasks/drt-def-tracks/design/tracks.md for evidence).
/// Grids with zero fitting tracks are omitted.
pub fn default_track_grids(tech: &TechDb, die: RectDbu) -> Vec<TrackGrid>
```

Layer id for each grid comes from the routing-layer index:
`layers: vec![LayerId(i as u8)]`.

## `crates/grus-core/src/design_db.rs`

- `GCellGridDef::num_boundaries` → `num_cells: i32` with a
  corrected doc comment (`DO` = gcell column/row count in the
  OpenROAD convention this codebase follows).

## `crates/grus-core/src/readers/def.rs`

- `GCELLGRID` parse arm (~line 1685): store into `num_cells`;
  fix the comment to name the convention.
- `write_def_full` (~line 1939): after the `ROW` loop and before
  `COMPONENTS`, emit:
  1. TRACKS — `db.tracks` verbatim if non-empty, else
     `drt::tracks::default_track_grids(tech_db, db.die_area)`.
     Format:
     `TRACKS {X|Y} <start_dbu> DO <num_tracks> STEP <step_dbu>
       [LAYER <name>...] ;`
     (`is_vertical_tracks` → `X`; `layers` resolved via
     `tech_db.strings.resolve(tech.routing_layers[lid.0].name)`
     through `escape_def_name`; skip `num_tracks <= 0`).
  2. GCELLGRID — `db.gcell_grids` verbatim if non-empty, else
     the default grid. Format:
     `GCELLGRID {X|Y} <start_dbu> DO <num_cells> STEP <step_dbu> ;`
- Update the `write_def_full` doc comment: TRACKS and GCELLGRID
  are now emitted; VIAS/SPECIALNETS remain omitted — name VIAS a
  non-goal with the one-line reason (`+ VIARULE` rule name not
  retained at parse; inline `+ RECT`/`+ POLYGON` records store
  nothing, so a faithful section cannot be rebuilt).

Note: `readers::def` gains a dependency on `drt::tracks` — the
reverse edge already exists (`drt::emit` uses
`readers::def::escape_def_name`); intra-crate, no cycle issue.

## `crates/grus-core/src/congestion/gcell.rs`

- `build`: `num_x = x_grid.num_cells.max(1)` / same for Y
  (replaces the `− 1`). One-line comment noting `DO` = column
  count (OpenROAD `getGridX` convention).
- Make the default-grid tuple reachable for emission — minimal
  change: `pub fn compute_default_grid(...)`. If the bare
  six-tuple reads badly, factor it into a small named struct or
  `pub fn default_grid_geometry(db, tech) -> (i32,i32,i32,i32,
  usize,usize)`; implementer's choice, keep it one call site.

## Versioning

Functional change → `[workspace.package] version`
`0.12.7 → 0.12.8` in the same commit.

## Commit message contents (required by acceptance)

- `feat:` subject; body states: verbatim-vs-derived policy, the
  `DO = floor((die − offset − w/2)/step) + 1` rule and that it was
  verified against every OpenROAD DEF in-tree, the GCELLGRID
  density choice (15 tracks/GCell = `compute_default_grid`,
  emitted as `DO` = column count) and *why*, the read-side
  `num_cells` fix and its rationale, and VIAS named a non-goal.
