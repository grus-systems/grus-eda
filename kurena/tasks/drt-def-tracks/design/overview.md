# drt-def-tracks — overview

Emit `TRACKS` and `GCELLGRID` statements from `write_def_full`
(`crates/grus-core/src/readers/def.rs`) so a self-contained
`floorplan → place → route → drt` chain can re-derive the track
model and GCell grid from our own DEF text instead of requiring
the LEF (and a side-channel grid convention) at every consumer.

## Current state

`write_def_full` emits `VERSION/DIVIDERCHAR/BUSBITCHARS/DESIGN/
UNITS/DIEAREA/ROW/COMPONENTS/PINS/NETS` and nothing else — its
doc comment (`def.rs` ~1927) says TRACKS/GCELLGRID/VIAS/
SPECIALNETS are omitted. Two callers:

- `floorplan` (`main.rs:1021`): `db` comes from a netlist
  (`parse_yosys_json` / `parse_verilog`), so `db.tracks`,
  `db.gcell_grids` are empty. TRACKS emission here is purely a
  **derivation** from `TechDb` + die.
- `cts` (`main.rs:1852`): `db` comes from `parse_def`, so
  `db.tracks`/`db.gcell_grids`/`db.def_vias` are populated when
  the input DEF carried them. Emission here is a **pass-through**
  of the parsed records.

`write_routed_def` (`drt/emit.rs`) does not derive anything: it
splices a canonical `NETS` section into the placed-DEF template,
so TRACKS/GCELLGRID/VIAS survive only because the template had
them. The shared derivation this task adds is what fills the gap
when the template path is unavailable (`write_def_full`).

## Decisions (details in the per-aspect files)

1. **TRACKS**: emit `db.tracks` verbatim when non-empty;
   otherwise derive per-layer grids via a new
   `drt::tracks::default_track_grids(tech, die)`. The derived
   `start/step/DO` reproduces OpenROAD's emitted values exactly —
   verified against every OpenROAD-produced DEF in the tree
   (gcd, aes, ibex, jpeg, picorv32: 4 utilisation sweeps +
   placed/cts/routed/orient variants). Rule: `start = offset_dbu`,
   `step = pitch_dbu`,
   `DO = floor((die_hi − offset − width_dbu/2) / step) + 1`,
   i.e. the last track is the highest coordinate whose
   centered minimum-width wire still fits inside the die.
   See `tracks.md` for the evidence table.

2. **GCELLGRID**: emit `db.gcell_grids` verbatim when non-empty;
   otherwise derive the grid the congestion model actually
   assumes (`GCellGrid::compute_default_grid`): origin = die
   lower-left, `step = 15 × met2 pitch` (6900 DBU on sky130),
   `nx = floor(die_w / step)`, `ny = floor(die_h / step)`.
   **`DO` = column count, matching OpenROAD's convention** — its
   DEFs write `DO n` where `n` equals `llength getGridX` and the
   FastRoute column count (gcd `DO 13` ↔ 13 columns, proven by
   the exact `GRT-0053` met1 raw match `3624 = 12 × 302`).
   This requires fixing `GCellGrid::build`'s read-side
   interpretation (`num_x = num_boundaries − 1` → `num_x =
   num_cells`), which is today an unexercised latent off-by-one
   on every OpenROAD DEF that carries GCELLGRID. See
   `gcellgrid.md`.

3. **VIAS**: non-goal (the spec allows this). `write_def_full`
   emits no generated-via records elsewhere, and `db.def_vias`
   cannot faithfully round-trip: the `+ VIARULE` rule name is
   discarded at parse (`def.rs` ~590 keeps the flag, not the
   name) and inline `+ RECT`/`+ POLYGON` records store nothing
   by design. Emitting `VIAS` with fabricated rule names would
   corrupt consumers that resolve the rule against the LEF.
   A conns-only placed DEF needs no `VIAS` section.

4. **SPECIALNETS**: still omitted — untouched, out of scope.

## Output layout

OpenROAD order, matching `gcd.floorplan.def` / `gcd.routed.def`:

```
... DIEAREA ...
ROW ...
TRACKS X ... / TRACKS Y ...        (per routing layer, X then Y)
GCELLGRID X ... ;
GCELLGRID Y ... ;
COMPONENTS ...
```

## Risks / notes

- `gcd.cts.ours.def` is a committed `write_def_full` artifact
  asserted byte-equal by `test_gcd_cts_ours_def_deterministic` —
  it must be regenerated via `tests/designs/generate_cts_ours.sh
  gcd` (host-side release binary + PDK) and committed.
- The `GCELLGRID DO` semantic change alters what `parse_def` +
  `GCellGrid::build` reconstructs for **any** DEF carrying
  GCELLGRID (today: n−1 columns; after: n columns). No test
  exercises that path today; on OpenROAD DEFs the new reading is
  the correct one. Documented in `docs/results.md` and the
  commit message.
- Strict DEF 5.8 grammar says `GCELLGRID DO` = grid *lines*
  (columns+1). OpenROAD deviates (writes columns). We follow the
  reference tool; a spec-strict foreign file would now read one
  column too many — acceptable, all repo inputs are OpenROAD's
  or ours.
