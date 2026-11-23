# e6-drt-maze — Occupancy and spacing-lite

Matches the implementation at `2c3c713`, plus the diagnosis-driven
change candidates in §5.

## 1. Model

Committed geometry — macro OBS, pin shapes, via footprints,
committed wire/stub rects — is rasterized onto track lines so an
edge's conflict check is a per-line interval query.

Per routing layer `l`, two maps (line coordinate → interval list):

- `lines_y[l]: BTreeMap<i32, Vec<Interval>>` — horizontal lines
  (`tracks_y` coords), queried by `Axis::X` edges and rect scans.
- `lines_x[l]` — vertical lines, queried by `Axis::Y` edges.

```rust
struct Interval { lo: i32, hi: i32, perp_lo: i32, perp_hi: i32,
                  who: Occupant }
enum  Occupant { Obs(u32 /* cell idx; u32::MAX = no exempting host */),
                 Net(u32 /* NetId.0 */) }
```

`perp_*` stores the marked shape's extent on the line's perpendicular
axis so rect candidates (via footprints) and the post-hoc census run
the exact `R' ∩ S` 2-D test, not just the band approximation a wire
candidate gets for free.

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
  + translate — the `drc::geom` source-4 convention) `obs_rects_raw`
  on routing layers → `Obs(cell_idx)`. The **unmerged** set: the
  `merge_connected_obs` bbox fills the decap/edge gaps pins need.
- Every instance pin's shapes → `Net(own_net)` — a pin's metal
  blocks foreign nets but is free to its own net so routing can
  reach its own attach nodes. Pins without a net get
  `Obs(u32::MAX)` (never carved — no exempting host).
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

`hit_verdict(hits, own_pins, cand, own, hist, hard, mode)`:

| hit class | soft iteration | last iteration (hard) |
|---|---|---|
| `Obs(cell)`, uncarved | hard block | hard block |
| `Obs(cell)`, carved | invisible | invisible |
| `Net(own)` | free | free |
| `Net(foreign)` | `+(OCC_PENALTY + hist·HIST_STEP) × #distinct foreign` | hard block |

`OccMode::Disabled` (the fail-proof knob) skips foreign-`Net`
checks and collision recording entirely; `Obs` stays blocking — a
fixed blockage is not "occupancy".

**Pin-carve exemption (current rule)**: an `Obs(cell)` hit is
invisible when the *candidate rect* `cr` (the edge/via's own shape
on that layer) overlaps the access halo — pin rect expanded by
`w/2` + the layer's widest track gap — of one of the candidate net's
own pins **hosted by that same cell**. The halo reaches every node
`project_attach` can emit and the unmodeled access stub a projected
node implies. Neighboring cells' OBS stays blocking whatever the
distance.

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

## 5. Routability-gap candidates (diagnosis decides — `steps.md` §2)

The measured gcd residual is 48 `NoPath` nets, almost all deep inner
pins on AOI/OAI cells. Candidates, in expected-hit order:

- **C1 — unconditional host carve.** If the probe shows host-cell
  `Obs` hits blocking approach edges whose candidate rect has not
  yet reached the halo, widen the exemption: `Obs(cell)` is
  invisible to the candidate net whenever the net has a member pin
  hosted by `cell`. This matches OpenROAD's "access shapes ignore
  own-instance OBS" directly (the halo approximation was the
  intermediate step). Recorded looseness: the net may cross the host
  cell's OBS anywhere, not just near the pin — acceptable at
  connectivity level; the emit/e2e DRC owns the real check.
- **C2 — pin-metal occupant class.** If foreign pin marks wall the
  pocket (hard-mode block), split pin marks into `Occupant::Pin(net)`
  — exempt for own net, soft penalty for foreign, never hard-block
  (or stop marking pin shapes entirely). Pin-pin shorts become a
  post-check finding, recorded in `docs/e6-drt.md`.
- **C3 — spare-layer / relaxed-via widening.** If the pocket mouth
  is sealed on every covered layer, or the nearest upper line holds
  no in-region node, widen `relaxed_vias` to probe the next-nearest
  lines (bounded, e.g. ±2) and/or reconsider `spare_above`.
- **C4 — none of the above**: the probe names the real blocker class
  and the doc records it (the `unclassified` assert stays strict —
  classes are named, not excused).

Each fix is applied minimally and recorded in `docs/e6-drt.md` §7 —
no silent semantic changes (AGENTS §2).

## 6. Performance levers (`perf.md`)

The per-line `Vec<Interval>` linear scan is the prime suspect for
the superlinear blowup. Lever L3 replaces each list with a
`lo`-sorted structure plus a skip index (chunked `max_hi` summaries
rebuilt lazily per line on mutation) so `scan_wire`/`scan_rect`
touch only intervals in the query window. Hit *sets* are unchanged;
hit *order* may change — both consumers (`hit_verdict`,
`edge_collisions`) are order-insensitive, so output bytes are
preserved. Record whichever structure lands.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
