# e6-drt-tracks — Track model

## LEF `TRACKS` parse (`readers/lef.rs`, `tech_db.rs`)

Grammar accepted (top level of a LEF file, anywhere a top-level
statement may appear — the existing dispatch chain already treats
statement order as open):

```
TRACKS {X|Y} start DO n STEP s [LAYER l]+ ;
TRACKS {X|Y} start DO n STEP s ;          -- bare form, all routing layers
```

- `X` = vertical track lines (x-coordinates), `Y` = horizontal
  (y-coordinates) — the same convention `TrackGrid::is_vertical_tracks`
  documents.
- `LAYER` may be followed by one or more layer names (DEF's reader
  already accepts the plural form). Each name must resolve to a
  declared `TYPE ROUTING` layer at expansion time; anything else
  (`MASK`, `SAMEMASK`, non-numeric fields) is malformed → `bail!` with
  the `LEF line N:` prefix, per parser convention.
- Unknown layer name → `bail!` (spec: "unknown-layer references and
  malformed records error out"). This is stricter than the DEF reader,
  which drops unknown TRACKS layers silently — record that asymmetry
  in a comment; do not change the DEF side in this ticket.

### Deferred expansion

`TRACKS` may precede `UNITS` (the µm→DBU scale) and the `LAYER`
statement it names. Mirror the `cut_res_above` pattern:

```rust
// tech_db.rs — transient parse state, serde(skip)
pub struct LefTrackRecord {
    pub is_x: bool,
    pub start_um: f32,
    pub count: u32,
    pub step_um: f32,
    /// None = bare form (all routing layers).
    pub layers: Option<Vec<String>>,
}
```

`TechDb::lef_track_records: Vec<LefTrackRecord>` collects records in
`parse_lef_bytes`. At the **end of each `parse_lef_bytes` call** (before
`Ok(report)`), drain the buffer and expand:

```
x0_dbu = (start_um * dbu).round() as i32
s_dbu  = (step_um  * dbu).round() as i32
coords[k] = x0_dbu + k * s_dbu,  k in 0..count
```

Rounding the two declared quantities then expanding matches how
`pitch_dbu`/`offset_dbu` are derived — document the convention in
`docs/e6-drt.md`, not the alternative (per-point rounding of
`x0 + k*s`). For each record, resolve `layers` (or all routing layers
when `None`) through `interner.get` + `tech_db.layer_by_name`; append
coords into `RoutingLayer.tracks_x`/`tracks_y`, then `sort_unstable` +
`dedup` per layer per axis.

A TRACKS record in file 1 (tlef) naming a layer only declared in file
2 (cell lef) errors at file 1's expansion — acceptable: LEF should not
forward-reference across files, and sky130 has no TRACKS at all.

### `RoutingLayer` fields

```rust
/// LEF `TRACKS` coordinates, expanded and sorted. Empty when the tech
/// LEF declares none (sky130: always empty — DEF supplies the grid).
#[serde(default)]
pub tracks_x: Vec<i32>,
#[serde(default)]
pub tracks_y: Vec<i32>,
```

`#[serde(default)]` matches the established convention for added
fields (TechDb is serialized). Field order in the struct: after
`offset_dbu`, before the analog params — keep the doc comment.

## Effective per-design grid (`drt/tracks.rs`)

```rust
pub enum Axis { X, Y }

pub struct LayerTracks {
    /// Sorted unique DBU coordinates — vertical-line (X) tracks.
    pub tracks_x: Vec<i32>,
    /// Sorted unique DBU coordinates — horizontal-line (Y) tracks.
    pub tracks_y: Vec<i32>,
}

/// A routing layer whose preferred-direction track list is empty.
/// Named, never synthesized from PITCH/OFFSET.
pub struct TrackGap {
    pub layer: LayerId,
    pub axis: Axis,
}

/// Union of the LEF-declared lists on `RoutingLayer` and the expansion
/// of every `DesignDb::tracks` grid. Parallel to `tech.routing_layers`.
pub fn layer_tracks(db: &DesignDb, tech: &TechDb) -> Vec<LayerTracks>;

/// Preferred-axis coverage check over `layer_tracks`' output.
pub fn track_gaps(tech: &TechDb, tracks: &[LayerTracks]) -> Vec<TrackGap>;
```

Merge rule in `layer_tracks`:

1. Seed layer `i` with `tech.routing_layers[i].tracks_{x,y}` (clone).
2. For each `tg` in `db.tracks`: `coords[k] = tg.start_dbu + k *
   tg.step_dbu` for `k in 0..tg.num_tracks`; for each `lid` in
   `tg.layers`, push into `tracks_x` when `tg.is_vertical_tracks`,
   else `tracks_y`. A `TrackGrid` with empty `layers` contributes
   nothing (DEF bare form dropped by the reader — note, don't fix).
3. `sort_unstable` + `dedup` each list.

`track_gaps`: `dir == Horizontal` requires nonempty `tracks_y`;
`Vertical` requires `tracks_x`. (Horizontal wires ride horizontal
lines — the `TRACKS Y` list; the direction-to-axis flip is the one
fact a reader most easily gets backwards, so say it in the comment.)

sky130 check: `DIRECTION` gives li1/met1/met3/met5 horizontal,
met2/met4 vertical; the placed DEFs supply both axes on all six
routing layers, so `track_gaps` must return `[]` on gcd/picorv32.

## Module wiring

`src/drt.rs` holds only `pub mod tracks; pub mod access;` plus the
module doc line; `src/lib.rs` gains `pub mod drt;` in alphabetical
position (between `drc` and `floorplan`).

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/tracks.rs`
- `crates/grus-core/src/lib.rs`
