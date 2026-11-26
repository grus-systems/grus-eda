# e6-drt-tracks — Test design

Two levels: parser unit tests inside `readers/lef.rs` (synthetic LEF
strings, the established `#[cfg(test)]` convention) and the
integration census `crates/grus-core/tests/drt.rs` over real designs.

## Unit tests — `lef.rs` `TRACKS` parse

Fixture style follows the existing `DRC_LEF` inline-string pattern.
One base LEF (`UNITS`, `SITE`, `LAYER met1 ROUTING`, `LAYER met2
ROUTING`, a `MACRO`/`PIN` to keep it realistic but minimal).

- `test_tracks_xy_expand` — `TRACKS X 0.17 DO 3 STEP 0.34 LAYER met1 ;`
  → `met1.tracks_x == [170, 510, 850]` (dbu 1000). `TRACKS Y 0.23 DO 2
  STEP 0.46 LAYER met1 ;` → `tracks_y == [230, 690]`.
- `test_tracks_multi_layer_and_bare` — one record naming both layers;
  one bare record (no `LAYER`) → lands on every routing layer.
- `test_tracks_dedup_and_sort` — overlapping records merge sorted,
  deduped.
- `test_tracks_before_units` — `TRACKS` ahead of `UNITS DATABASE
  MICRONS 2000` still expands at 2000 DBU/µm (deferred expansion).
- `test_tracks_unknown_layer_errors` — `LAYER foo` → `Err` containing
  `TRACKS` and `foo`.
- `test_tracks_malformed_errors` — missing `DO`/`STEP`, non-numeric
  start, `MASK`/`SAMEMASK` token → `Err`.

## Unit tests — `drt/tracks.rs` / `drt/access.rs`

- `test_layer_tracks_merges_def_grids` — synthetic `DesignDb` (two
  `TrackGrid`s, one axis each, one overlapping coord) + synthetic
  `TechDb` with LEF coords → merged sorted/deduped lists.
- `test_track_gaps` — a `Horizontal` layer with empty `tracks_y` → one
  `TrackGap { layer, axis: Y }`; populated → empty.
- `test_access_points_rect` — rect `[100,300]×[50,250]` on met1,
  `tracks_x=[100,170,510]`, `tracks_y=[230]` → exactly three points:
  `(100,150)` (edge track `t == xl`, boundary inclusive),
  `(170,150)`, `(200,230)` (`mid_x = (100+300)/2 = 200`). 510 and any
  out-of-span track contribute nothing.

## Integration — `tests/drt.rs`

Helpers duplicated per the `tests/drc.rs` convention (`pdk_root`,
`sky130_hd`, `design_dir`, `require` — loud panic on missing inputs).

```rust
fn load(name: &str) -> (TechDb, DesignDb, Vec<LayerTracks>, AccessReport)
```

loads `sky130_fd_sc_hd__nom.tlef` + `sky130_fd_sc_hd.lef` via
`load_tech_db`, parses `<name>.placed.def` via `parse_def`, runs
`layer_tracks` + `access_points`. (`parse_def` — not `parse_def_bytes`
— matches `tests/drc.rs::load`.)

### `test_track_census`

Per design (`gcd`, `picorv32`):

- print `layer dir tracks_x.len tracks_y.len` per routing layer —
  the per-axis evidence for `docs/results.md`.
- every routing layer has `tracks_x` and `tracks_y` nonempty (both
  DEFs declare both axes on all six layers — verified by inspection;
  if a design lacked one, assert only the preferred axis and name the
  missing one).
- `track_gaps` returns `[]` — print any entries before the assert.
- sky130 honesty line: print that `RoutingLayer.tracks_*` (LEF
  source) is empty on every layer — the DEF merge is what supplies the
  grid. No assert that they are nonempty (they are not).

### `test_access_census`

Per design:

- `off_track.is_empty()` and `no_shapes.is_empty()` — print each
  finding named (`cell/pin` for instance pins via `cell_names[c]` +
  `pin_names[p]`; port name for io pins) before asserting, so a
  regression names its pins.
- explicit member coverage: count `points` per `pin` and assert every
  pin iterated through `net_pins` over `0..num_nets()` appears —
  this, not just `off_track` emptiness, is the §7.5 per-element
  check.
- per-layer census printed (`per_layer` with layer names); assert
  total `> 0` and at least the pin-bearing layers (`li1`, `met1`,
  plus the io layers) nonzero — record actual values in
  `docs/results.md`.
- **Determinism (§7.7 nonzero evidence)**: `access_points` twice →
  `assert_eq!(r1.points, r2.points)` and
  `assert!(!r1.points.is_empty())`.

### `test_access_off_track_fixture`

Inline LEF (met1 + `TRACKS`) + inline DEF (ROWS, COMPONENTS, one
`CELL` instance `+ PLACED`, one io `PIN` with `+ LAYER`, one signal
`NETS` net joining them; DEF `TRACKS` records on met1 both axes).
The macro pin rect sits between tracks → `off_track` contains exactly
that pin, named; the io rect covers tracks → it has APs. Asserts:

- `off_track` names the shifted pin (`pin_names` + owning cell).
- `points` is nonempty (the io pin's) — the finding is named, not
  "everything failed".
- `no_shapes` empty.

### `test_access_failproof` (AGENTS §7.7)

On `gcd`: build `tracks = layer_tracks(...)`, run census →
`off_track` empty. Then for every routing layer clear the
preferred-axis list (`tracks_y` for `Horizontal`, `tracks_x` for
`Vertical`) and rerun → `off_track` **nonempty** (the census goes
red: print count + first few names). Restore by rebuilding
`layer_tracks` → `off_track` empty again. Same pattern as the
`met5.1`-drop fail-proof in `tests/drc.rs`.

## Checks

`cargo test --workspace` and `cargo clippy --workspace --all-targets
-- -D warnings`. `picorv32.placed.def` must exist (gitignored;
`tests/designs/picorv32/generate_ref.sh` regenerates — `require`
panics loudly otherwise).

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/drt/tracks.rs`
- `crates/grus-core/src/drt/access.rs`
