# e6-drt-maze — Occupancy and spacing-lite

Matches the implementation at `a52d771`.

## 1. Model

Committed geometry — macro OBS, pin shapes, via footprints,
committed wire/stub rects — is rasterized onto track lines so an
edge's conflict check is a single-line interval query.

Per routing layer `l`, two maps (line coordinate → interval list):

- `lines_y[l]: BTreeMap<i32, Vec<Interval>>` — horizontal lines
  (`tracks_y` coords), queried by `Axis::X` edges and rect scans.
- `lines_x[l]` — vertical lines, queried by `Axis::Y` edges.

```rust
struct Interval { lo: i32, hi: i32, perp_lo: i32, perp_hi: i32, who: Occupant }
enum  Occupant { Obs, Net(u32 /* NetId.0 */) }
```

`perp_*` stores the marked shape's extent on the line's
perpendicular axis so rect candidates (via footprints) and the
post-hoc census run the exact `R' ∩ S` 2-D test, not just the band
approximation a wire candidate gets for free.

**Rasterization** (`Occupancy::rasterize`): rect `r` on layer `l`
inserts `[r.yl, r.yh]` into every `lines_x[l][x]` with
`x ∈ (r.xl − w/2 − s, r.xh + w/2 + s)` (strict band) and
`[r.xl, r.xh]` into every `lines_y[l][y]` likewise, with
`w/2 = width_dbu/2` (integer division — sky130 widths are even) and
`s = min_spacing_dbu()`. Each insertion is optionally logged as a
`Mark` — the rip-up journal (`unmark` frees it). A rect covering no
track line inside a band contributes nothing on that axis — correct,
since candidates only exist on lines.

**Pre-mark** (`premark_obs`, once at `route_nets` start):

- Every placed cell's (`{Placed,Fixed,Cover}` gate + `orient_rect`
  + translate — the `drc::geom` source-4 convention, OBS rects only)
  `obs_rects` on routing layers → `Obs`.
- Every instance pin's shapes → `Net(own_net)` — a pin's metal
  blocks foreign nets but is free to its own net so routing can
  reach its own attach nodes.
- io-port rects are not marked (recorded simplification — pin-access
  DRC is the emit/e2e tickets' job).

**Commit** (`commit_route`): merged wire rects (`± w/2`), relaxed-via
stub wires, and via footprints (the default candidate's routing
rects shifted to the placement) → `Net(net)`, logged for rip-up.
Cut-layer occupancy not modeled (spacing-lite).

## 2. Conflict check

- `scan_wire(axis, l, t, c, d)`: candidate on line `t` spanning
  track coords `[c,d]` — shape extent `[c − w/2, d + w/2]`, hits are
  the line's intervals overlapping `[c − w/2 − s, d + w/2 + s]` —
  shape edge-distance `< s` along the line; perpendicular `< s` is
  guaranteed by the band the mark was rasterized into.
- `scan_rect(l, r)`: via footprints — expand `r` by `s` and test
  true 2-D intersection against the marks inside the `w/2 + s` band
  on both axes, using the stored perpendicular extents.

`hit_verdict(hits, own_pins, own, hist, hard, mode)` classifies:

| hit class | soft iteration | last iteration (hard) |
|---|---|---|
| `Obs` (uncarved) | hard block | hard block |
| `Obs` (carved) | invisible | invisible |
| `Net(own)` | free | free |
| `Net(foreign)` | `+(OCC_PENALTY + hist·HIST_STEP) × #distinct foreign` | hard block |

`OccMode::Disabled` (the fail-proof knob) skips foreign-`Net`
checks and collision recording entirely; `Obs` stays blocking — a
fixed blockage is not "occupancy".

**Pin-carve exemption**: an `Obs` mark is invisible to the candidate
net when `hit_rect(mark)` 2-D touches any rect of the net's own pins
on the same layer — the rasterized form of "a net may touch the
blockage its own pin already sits on". Without it, stdcell li1 OBS
jumpers (which cover the whole cell including pin rects) wall in
every li1 pin — this was the measured cause of the first NoPath
flood. Recorded side effect: the whole OBS rect of a cell hosting
the net's pin becomes transparent to that net.

## 3. Spec mapping

- "a committed route marks its track intervals occupied" — the
  rasterized shape intervals (wires + stubs + via footprints).
- "later nets see the occupancy as cost" — soft-mode penalty
  (PathFinder-style sharing priced, not forbidden).
- "a hard-block when the interval + SPACING collides" — `Obs`
  always; foreign nets on the final iteration.
- Commit-time recording: in soft mode the foreign hits an edge took
  are stored as provisional `Collision`s deduped on
  `(a, b, layer, line, lo, hi)` with `a < b`; they drive the rip-up
  set (`search.md` §6).

## 4. Post-hoc collision census

`overlap_census(routes, tech, tracks, vias)`: rebuilds every route's
wire rects and via footprints into a fresh `Occupancy` (routes only,
no OBS), sweeps each line's sorted intervals, reports each cross-net
pair whose `R + s`-intersect rule violates — checking both the
along-line gap and the perpendicular gap (`pgap ≥ s` clears it).
Exact on the committed result — the `RouteDb.collisions` list and
the fail-proof's measuring stick.

## 5. Cost notes

Per-line `Vec`s stay unsorted (linear scan per query — short in
practice). Relaxed-via stub wires add marks but keep `O(span/pitch)`
per axis. If the picorv32 census runs hot, the first lever is
hoisting `relaxed_vias` out of `route_one_net` (per-iteration
recompute is the current, simpler choice); record whatever was
needed, do not change semantics silently.

## Files

- `crates/grus-core/src/drt/maze.rs`
