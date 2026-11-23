# e6-drt-maze — Occupancy and spacing-lite

Matches the implementation at `cf34607` (code identical to the
`c1832dc` rewrite) — the §5 diagnosis resolved as C1 + C2 and the L3
skip index landed; the rules below are final for this ticket.

## 1. Model

Committed geometry — macro OBS, pin shapes, via footprints,
committed wire/stub rects — is rasterized onto track lines so an
edge's conflict check is a per-line interval query.

Per routing layer `l`, two maps (line coordinate → `Line`):

- `lines_y[l]: BTreeMap<i32, Line>` — horizontal lines (`tracks_y`
  coords), queried by `Axis::X` edges and rect scans.
- `lines_x[l]` — vertical lines, queried by `Axis::Y` edges.

```rust
struct Interval { lo: i32, hi: i32, perp_lo: i32, perp_hi: i32,
                  who: Occupant }
enum  Occupant { Obs(u32 /* cell idx; u32::MAX = no exempting host */),
                 Net(u32 /* NetId.0 */),
                 Pin(u32 /* NetId.0 */) }
struct Line { ivs: Vec<Interval>,  // lo-sorted
              max_hi: Vec<i32> }   // max(hi) per SKIP_CHUNK=16 chunk
```

`perp_*` stores the marked shape's extent on the line's perpendicular
axis so rect candidates (via footprints) and the post-hoc census run
the exact `R' ∩ S` 2-D test, not just the band approximation a wire
candidate gets for free.

**Rasterization** (`Occupancy::rasterize`): rect `r` on layer `l`
inserts `[r.yl, r.yh]` into every `lines_x[l][x]` with
`x ∈ (r.xl − w/2 − s, r.xh + w/2 + s)` (strict band) and
`[r.xl, r.xh]` into `lines_y[l][y]` likewise, with
`w/2 = width_dbu/2` (integer division — sky130 widths are even) and
`s = min_spacing_dbu()`. Each insertion is optionally logged as a
`Mark` — the rip-up journal (`unmark` frees it). A rect covering no
track line inside a band contributes nothing on that axis — correct,
since candidates only exist on lines.

**Skip index**: `ivs` stays `lo`-sorted (insert via
`partition_point`, equal-`lo` goes after existing entries so
insertion order is deterministic); `max_hi` is rebuilt eagerly from
the dirty chunk onward (`rebuild_from`). Scans skip a chunk when
`max_hi ≤ qlo` and stop at `lo ≥ qhi` — the hit *set* is identical
to a linear scan; the hit *order* may differ and both consumers are
order-insensitive.

**Pre-mark** (`premark_obs`, once at `route_nets` start):

- Every placed cell's (`{Placed,Fixed,Cover}` gate + `orient_rect`
  + translate — the `drc::geom` source-4 convention) `obs_rects_raw`
  on routing layers → `Obs(cell_idx)`. The **unmerged** set: the
  `merge_connected_obs` bbox fills the decap/edge gaps pins need.
- Every instance pin's shapes → `Pin(own_net)` — a pin's metal is
  free to its own net, soft-priced for foreign nets, and **never a
  hard block** (rip-up cannot move a pin; charging it as congestion
  churns the queue without clearing the mark; a foreign wire landing
  on it is a pin-access finding for the emit/e2e DRC). Pins without
  a net get `Obs(u32::MAX)` — immovable blockage with no exempting
  host.
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
  on both axes, using the stored perpendicular extents. A collision
  sliver containing no track line on either axis is the recorded
  miss class (spacing-lite).
- `scan_stub`: a relaxed via's implied stub wire between the hang
  node and the true via position is committed metal — scanned like
  a wire on both layers before the footprint check.

`hit_verdict(hits, host_cells, own, hist, hard, mode)`:

| hit class | soft iteration | last iteration (hard) |
|---|---|---|
| `Obs(cell)`, `cell ∈ host_cells` | invisible | invisible |
| `Obs(cell)`, `cell ∉ host_cells` | hard block | hard block |
| `Obs(u32::MAX)` | hard block | hard block |
| `Net(own)` | free | free |
| `Net(foreign)` | `+(OCC_PENALTY + hist·HIST_STEP)` per distinct foreign net | hard block |
| `Pin(own)` | free | free |
| `Pin(foreign)` | same penalty, pooled with `Net` in the distinct set | **soft-priced — never a block** |

**Host-cell carve (final rule, C1)**: `host_cells` is the sorted,
deduped set of cells hosting the candidate net's member pins. An
`Obs(cell)` hit is invisible whenever `cell ∈ host_cells` — the
rasterized form of OpenROAD's "access shapes ignore own-instance
OBS", applied unconditionally. Measured: the earlier access-halo
carve left the approach edge blocked before it reached the halo —
that was 47/48 of the gcd `NoPath`s. Recorded looseness: the net may
cross the host cell's OBS anywhere, not just near the pin —
connectivity-level acceptable; the emit/e2e DRC owns the real check.

`OccMode::Disabled` (the fail-proof knob) skips foreign-`Net`/`Pin`
checks and collision recording entirely; `Obs` stays blocking — a
fixed blockage is not "occupancy".

## 3. Spec mapping

- "a committed route marks its track intervals occupied" — the
  rasterized shape intervals (wires + stubs + via footprints).
- "later nets see the occupancy as cost" — soft-mode penalty
  (PathFinder-style sharing priced, not forbidden).
- "a hard-block when the interval + SPACING collides" — `Obs`
  always; foreign `Net` on the final iteration. `Pin` stays
  soft-only (recorded deviation — a pin cannot move under rip-up).
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

Measured on gcd: Enabled → 0 collisions; Disabled → 6,429 — the
fail-proof's nonzero evidence that occupancy constrains.

## 5. Resolved diagnosis record

The 48-net gcd `NoPath` residual at `2c3c713` resolved as:

- **C1 — unconditional host carve** (landed): host `Obs` walled the
  pocket approach outside the old halo — the deep-inner-pin pattern
  on AOI/OAI cells (`a21oi`/`a22o`/`a221o`/`a32o`/`o2bb2a`/`a21boi`,
  the `_131_`/`_132_` pin groups).
- **C2 — `Pin` occupant class** (landed): pin metal can seal a
  pocket mouth under hard mode and cannot be ripped up, so it must
  never hard-block.
- **C3 — relaxed-via widening / bigger spare layer**: *not needed* —
  gcd closed at `unrouted=0` without it; picorv32 decides whether it
  resurfaces (a residual there is named per-pin, `tests.md`).
- **H4 — stuck-loop**: ruled out by the bounded backtrack; the
  rewrite's measured runtimes confirm "slow, not stuck".

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/tech_db.rs`
