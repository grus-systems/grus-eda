# TRACKS emission

## Grammar and placement

One statement per routing layer per axis, in
`tech.routing_layers` order, `X` before `Y` per layer — the exact
layout OpenROAD writes (`gcd.floorplan.def` lines 36–47):

```
TRACKS X 230 DO 200 STEP 460 LAYER li1 ;
TRACKS Y 170 DO 302 STEP 340 LAYER li1 ;
TRACKS X 170 DO 270 STEP 340 LAYER met1 ;
TRACKS Y 170 DO 302 STEP 340 LAYER met1 ;
...
```

Emitted between the `ROW` lines and `COMPONENTS` in
`write_def_full`. The `LAYER <name>...` clause lists the grid's
`layers` resolved through `tech_db.strings`
(`tech.routing_layers[lid.0].name`); when `layers` is empty the
bare form `TRACKS X s DO n STEP p ;` is emitted (it parses back
to an empty `layers` vec — same record, lossless). Layer names
go through `escape_def_name`. Grids with `num_tracks <= 0` are
skipped (a `DO 0` statement is meaningless).

## Source policy: verbatim-if-present, else derived

```
if db.tracks.is_empty():
    grids = drt::tracks::default_track_grids(tech_db, db.die_area)
else:
    grids = &db.tracks            // record order preserved
```

- **floorplan** path: `db.tracks` empty → derived (this is the
  point of the ticket).
- **cts** path: `db.tracks` populated from the input DEF →
  verbatim re-emit preserves foreign/custom grids byte-for-byte
  and makes write→parse lossless on the track model.

## The derivation — `default_track_grids(tech, die) -> Vec<TrackGrid>`

Correction during implementation: `RoutingLayer::pitch_dbu`/
`offset_dbu` are the *preferred-direction* pair only — sky130
`li1` declares a split `PITCH 0.46 0.34` / `OFFSET 0.23 0.17`
and OpenROAD emits the axis-correct values (`TRACKS Y 170 STEP
340 LAYER li1`). `RoutingLayer` gained per-axis fields
`pitch_{x,y}_dbu` / `offset_{x,y}_dbu` (single-value `PITCH`/
`OFFSET` fills both; the preferred pair is unchanged for all
existing consumers).

For each `RoutingLayer l` in `tech.routing_layers` order, emit
two grids (`is_vertical_tracks` true = `TRACKS X`, false =
`TRACKS Y`), each `layers: vec![lid]`, `start_dbu`/`step_dbu`
the axis-matched `offset_{x,y}_dbu`/`pitch_{x,y}_dbu`, and

```
w2   = l.width_dbu / 2                       (integer)
n    = if start - w2 >= die_lo
        then max(0, (die_hi - w2 - start) / step + 1)
        else 0
```

i.e. a track coordinate `c = offset + k·step` is kept iff the
min-width wire centred on it lies inside the die:
`die_lo + w2 <= c <= die_hi - w2`. Note `TRACKS` offsets/pitches
are absolute coordinates independent of `die.xl`; the lower-edge
check is symmetric with the verified upper-edge rule and only
matters for non-zero die origins (all OpenROAD inputs have
`xl=yl=0` — the lower bound is unverified vs the reference tool
and noted as such).

### Evidence for the `+ width/2` bound

`DO` is **not** "coordinates < die edge" and not
`floor(die/step)`. Measured against every OpenROAD-produced DEF
in `tests/designs/` (the `??` fixtures under `sta_edge/` /
`sta_clocks/` are hand-written and follow no rule — excluded):

| file / line | rule: coord < die | rule: coord + w/2 ≤ die |
|---|---|---|
| `gcd.placed` `TRACKS Y met4 DO 111` (die 102670) | 112 | **111** |
| `picorv32.placed` `TRACKS X met1 DO 1417` (die 481715) | **1417** | 1417 |
| `jpeg.floorplan` `TRACKS Y met1 DO 3020` (die 1026710) | **3020** | 3020 |
| `ibex.placed` `TRACKS X met1 DO 1640` (die 557795) | 1641 | **1640** |
| `aes.placed` `TRACKS Y met2 DO 1155` (die 531265) | **1155** | 1155 (pitch/2 would give 1154 — rules out pitch/2) |

With real LEF widths (DBU/1000: li1 170→w2 85, met1 140→70,
met2 140→70, met3 300→150, met4 300→150, met5 1600→800) the
`coord + w/2 ≤ die_hi` rule reproduces **every** OpenROAD `DO`
count — including the two cases where a coord with margin 90
(met4, w2 150) was dropped while a coord with margin 105
(met1, w2 70) was kept. Integer arithmetic on DBU; no f32.

Boundary equality (`c + w/2 == die_hi`) is unexercised by any
in-tree input; `<=` is chosen (a wire flush with the die edge is
legal).

## Relationship to `drt::tracks::layer_tracks`

`layer_tracks` merges LEF `tracks_{x,y}` (empty on sky130) with
the expansion of `db.tracks`. Emitting derived grids means a
round-trip through our own DEF reconstructs exactly the grid a
plain pitch/offset model expects — and, per the evidence above,
byte-equals the TRACKS block OpenROAD writes for the same
(tech, die). The derivation lives in `drt/tracks.rs` so the
emit-side producer and the consume-side expander share one file.
