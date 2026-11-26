# drt-def-tracks: emit TRACKS / GCELLGRID (and VIAS) from write_def_full

External review (Codex usage session, 2026-11-24): our
`place`/`floorplan`/`cts` DEF output carries no TRACKS, VIAS, or
GCELLGRID, so a self-contained `place → route → drt` chain cannot
re-derive the track model / gcell grid from our own text — every
consumer must hold the LEF. TRACKS is machine-derivable from
tech_db (the same derivation `drt::tracks`/`emit` already uses for
the routed DEF) and GCELLGRID from the die; emit them.

## Scope

1. `write_def_full` writes `TRACKS X/Y` lines per routing layer
   (same track origin/pitch the routed-DEF emitter writes — share
   the derivation, don't fork it) between DIEAREA/ROW and
   COMPONENTS, matching DEF grammar and the routed file's layout.
2. `GCELLGRID X/Y` from the die: pick the grid the congestion model
   actually assumes (the default 15 tracks/GCell consumers fall
   back to when the statement is absent is only safe if our own
   model agrees — record the chosen value and why in the commit).
3. `VIAS` only if the emitted file can legally route (a placed DEF
   has no nets geometry — emit VIAS only when the writer already
   emits generated-via records elsewhere; otherwise name it a
   non-goal in the commit).
4. Re-parse: our own DEF reader must round-trip everything we emit
   (the emitted TRACKS/VIAS/GCELLGRID parse without error and the
   parsed track model equals what we wrote).

## Files

- `crates/grus-core/src/readers/def.rs` (write_def_full emit)
- `crates/grus-core/src/drt/tracks.rs` (share the derivation if it
  isn't already reusable)
- tests asserting the emitted headers parse + round-trip
- `docs/results.md` dated entry (what an emitted header block looks
  like on gcd; note the downstream chain it unblocks)
