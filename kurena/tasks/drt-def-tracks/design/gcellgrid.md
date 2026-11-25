# GCELLGRID emission

## The chosen grid

Emit the grid `GCellGrid::build` already assumes when the DEF is
silent — `compute_default_grid` (`congestion/gcell.rs`):

```
step  = 15 × routing_layers[2].pitch_dbu     // met2; 6900 DBU on sky130
origin_x = die.xl,  origin_y = die.yl
nx = max(1, floor((die.xh - die.xl) / step))
ny = max(1, floor((die.yh - die.yl) / step))
```

Rationale to record in the commit message: emitting *this* grid
is the only statement whose presence is guaranteed to reproduce
the grid every consumer (`congestion`, `route`, `drt` guides,
guide parasitics) already uses on the same input — the "default
15 tracks/GCell" fallback becomes identical whether or not the
statement is read back. Any other density would silently change
the assumed model.

Output (gcd: die 91950×102670 → `DO 13`/`DO 14`, identical to
what OpenROAD writes into `gcd.routed.def` lines 48–49):

```
GCELLGRID X 0 DO 13 STEP 6900 ;
GCELLGRID Y 0 DO 14 STEP 6900 ;
```

## `DO` semantics: columns, not lines

Two readings exist:

- **lefdefref (strict DEF 5.8)**: `DO` = number of grid *lines*
  → `DO n` yields n−1 columns. Our reader encodes this today:
  `num_x = (x_grid.num_boundaries - 1).max(1)` (`gcell.rs:121`).
- **OpenROAD (the reference tool)**: `DO` = column count.
  Evidence:
  - `gcd.routed.def` writes `DO 13` / `DO 14`; `picorv32.routed.def`
    writes `DO 69` / `DO 71` — each equals `floor(die_span/6900)`,
    the FastRoute column count. For gcd the 13-column claim is
    pinned exactly: `GRT-0053` met1 raw = 3624 = `(nx−1) × 302`
    ⇒ nx = 13 while the file says `DO 13`.
  - `picorv32.gcell_usage.txt` header `# nx 55` came from
    `llength [$ggrid getGridX]` on the util65 die
    (floor(380265/6900) = 55); our `nx == dump.nx` assertion
    passes, so `getGridX` returns one entry per column (the
    column's x-start; the die edge is the implicit last
    boundary). `DO n` → n entries → n columns.

**Decision**: follow OpenROAD — `DO` = column count, and fix the
read side:

- `GCellGridDef::num_boundaries` → rename `num_cells` (field is
  only built in `def.rs` and read in `gcell.rs`; `DesignDb` is
  not persisted to disk so the rename is safe).
- `GCellGrid::build`: `num_x = num_cells.max(1)` (drop the `−1`).
- Update the parse comment at `def.rs:1685` and the struct doc
  in `docs/architecture.md` (~line 438).

This fixes a latent bug: today, parsing OpenROAD's own
`gcd.routed.def` yields a 12-column grid where the reference
tool's is 13. No test exercises the `gcell_grids` path today —
all congestion tests run on placed DEFs without GCELLGRID —
so the change is zero-regression on covered behaviour.

**Rejected alternative**: emit spec-strict `DO nx+1` (lines) and
leave the reader alone. It round-trips correctly through *our*
reader but writes a statement OpenROAD would read as `nx+1`
columns — the emitted grid would then disagree with the model
our congestion stage assumed, which is precisely the failure the
goal warns about ("only safe if our own model agrees").

## Source policy: verbatim-if-present, else derived

Same rule as TRACKS:

```
if db.gcell_grids.is_empty():
    emit  GCELLGRID X <ox> DO <nx> STEP <s> ;
          GCELLGRID Y <oy> DO <ny> STEP <s> ;
else:
    emit each stored GCellGridDef verbatim
          GCELLGRID {X|Y} <start_dbu> DO <num_cells> STEP <step_dbu> ;
```

A partial input (X without Y) is passed through as-is; `build`
already falls back to the default for a missing axis.

To derive, expose the default-grid computation: make
`GCellGrid::compute_default_grid` `pub` (or factor the six-tuple
into a named `pub fn default_grid_geometry`). `write_def_full`
calls it with `gcell_tracks_override: None`.

## Determinism

All-integer fields, fixed emission order (X then Y), layer order
from `tech.routing_layers`, stored order for verbatim records —
byte-identical output for identical input, same as the rest of
`write_def_full`.
