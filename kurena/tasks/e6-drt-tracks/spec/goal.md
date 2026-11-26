# e6-drt-tracks: track model + pin access points

E6 ticket 2 (first implementation ticket; `e6-drt-ref` landed the
reference). Builds the on-grid substrate the maze router needs:
which tracks exist per layer, and where each routed pin can legally
attach to them.

## Scope

1. **LEF `TRACKS` retention** — parse `TRACKS X x0 DO n STEP s LAYER
   l` (and the `Y` form) into `TechDb`: per routing-layer, per-axis
   track coordinate lists (`x0 + k*s`, k < n). Both `TRACKS` and
   `UNITS` ordering are already handled by the LEF reader; the record
   may appear anywhere a routing-layer statement may. Unknown-layer
   references and malformed records error out per existing parser
   convention.
2. **Track grid model** — for each routing layer expose `tracks_x`
   / `tracks_y` (sorted DBU) plus the layer's preferred routing
   direction from the LEF `DIRECTION` statement (already retained if
   parsed — check `RoutingLayer` first). A layer with no `TRACKS`
   record for its preferred axis is a named gap, not a guess.
3. **Access-point generation** — for every signal-net member pin
   (placed `MacroPin` shapes from `drc::geom`'s five-source assembly;
   io `BTERM` ports too): intersect each pin rect on each candidate
   pin layer with the layer's tracks; a track line crossing the rect
   yields an access point at (track coord, rect span). Emit
   (layer, x, y, pin) tuples deterministically (pin order = member
   order, then layer order, then track order).
4. **Census + gate** — integration test: every member pin of every
   signal net on gcd and picorv32 has ≥1 access point on at least
   one layer; print per-layer access-point counts (real coverage per
   AGENTS §7.4). Pins whose shapes lie entirely off-track are named
   in the report, not silently zero — sky130 pins should not produce
   this, so any such pin is a finding worth naming.

## Non-goals

- Maze routing, via insertion, DEF route emission (later tickets).
- Track assignment (TritonRoute's TA stage) — the model enumerates
  candidate access points; choosing among them is later.
- DRC-aware pruning of access points (spacing to macro OBS is a
  router concern, not the model's).

## Files

- `crates/grus-core/src/readers/lef.rs` (`TRACKS` parse)
- `crates/grus-core/src/tech_db.rs` (`RoutingLayer`/`TechDb` track fields)
- `crates/grus-core/src/drt.rs` + `drt/tracks.rs` + `drt/access.rs` (new)
- `crates/grus-core/tests/drt.rs` (new — census gate)
- `docs/e6-drt.md` (new — model doc + claim boundary)
- `docs/results.md` (dated entry)
- `docs/status.md`, `docs/roadmap.md` (progress line)
- `Cargo.toml`/`Cargo.lock` (version bump per surface)
