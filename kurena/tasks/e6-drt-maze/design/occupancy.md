# e6-drt-maze — Occupancy and spacing-lite

## 1. Model

The committed geometry of a design — macro OBS, placed via
footprints, committed wires — is rasterized onto track lines so that
an edge's conflict check is a **single-line interval query**.

Per layer `l`, two maps (line coordinate → interval list):

- `lines_y[l]`: `BTreeMap<i32, Vec<Interval>>` — horizontal lines
  (`tracks_y` coords), queried by X-axis (horizontal) edges.
- `lines_x[l]`: `BTreeMap<i32, Vec<Interval>>` — vertical lines,
  queried by Y-axis edges.

`Interval { lo, hi, who }` is a *shape* extent along the line axis
(lo < hi, DBU) plus the occupant:

```rust
enum Occupant { Obs, Net(NetId) }   // via footprints record their owner Net
```

**Rasterization rule** — a rect `S` committed on layer `l` inserts
`[S.yl, S.yh]` into every `lines_x[l][x]` with
`x ∈ (S.xl − w/2 − s, S.xh + w/2 + s)` (strict band), and
`[S.xl, S.xh]` into every `lines_y[l][y]` with
`y ∈ (S.yl − w/2 − s, S.yh + w/2 + s)`, where `w =
layer.width_dbu`, `s = layer.min_spacing_dbu()`, `w/2` is integer
division (recorded; sky130 widths are even). Rects covering no track
line inside a band contribute nothing on that axis — correct, since
candidates only exist on lines.

- **Committed wire** on line `y_l`, span `[a,b]`: its shape rect is
  `[a − w/2, y_l − w/2, b + w/2, y_l + w/2]` — rasterized onto
  *both* axes' lines, so it also blocks perpendicular crossings and
  adjacent-line parallels (`|y − y_l| < w + s`; on sky130 only li1's
  pitch is below `w + s`, so the band is same-line-only on met
  layers — recorded, the model still handles it).
- **Via** at `(x,y)` on pair `k`: the default candidate's
  `ViaCandidate::rects` filtered to `DrcLayer::Routing`, shifted to
  `(x,y)`, rasterized on both layers. Cut-layer occupancy is not
  modeled (spacing-lite; recorded).
- **Macro OBS**: per placed cell (`{Placed,Fixed,Cover}` gate +
  `orient_rect` + translate — the `drc::geom` source-4 convention,
  replicated locally for OBS only), `obs_rects` and `pin_obs_rects`
  on routing layers, rasterized once at construction as `Obs`.
  Macro *pin* shapes and io-port rects of other nets are not marked
  (recorded simplification — pin-access DRC is the emit/e2e
  tickets' job).

## 2. Conflict check

A candidate edge on line `t` on layer `l` (span `[c,d]` track
coords, shape extent `[c − w/2, d + w/2]`):

```
hits = intervals on lines[l][t] overlapping [c − w/2 − s, d + w/2 + s]
```

— i.e. shape edge-distance `< s` along the line; the perpendicular
distance `< s` is already guaranteed because the committed shape was
rasterized onto `t` only within its `w/2 + s` band. Together this is
the rect-expand-by-`s`-and-intersect rule restricted to track
geometry — the spec's "interval + the LEF `SPACING` bound collides".

Then:

| hit class | soft iteration | last iteration (hard) |
|---|---|---|
| `Obs` | hard block | hard block |
| `Net(own)` | free | free |
| `Net(foreign)` | `+OCC_PENALTY × (1+hist) × #distinct foreign nets` | hard block |

`OccMode::Disabled` (the fail-proof knob) skips foreign-`Net`
checks entirely — `Obs` stays blocking (a fixed blockage is not
"occupancy") and collision recording is skipped.

## 3. What each clause of the spec maps to

- "a committed route marks its track intervals occupied" — the
  rasterized shape intervals above.
- "later nets see the occupancy as cost" — the soft-mode penalty
  (PathFinder-style: sharing is priced, not forbidden, while the
  iteration has slack to route around it).
- "a hard-block when the interval + SPACING collides" — `Obs` always;
  foreign nets on the final iteration, when remaining conflicts can
  no longer be deferred to another rip-up.
- Commit-time recording: in soft mode the foreign hits an edge
  actually takes are stored as provisional `Collision`s (deduped by
  `(a, b, layer, line, lo, hi)`); they drive the rip-up set per
  `search.md` §5.

## 4. Post-hoc collision census

`overlap_census(&[NetRoute], tech, tracks) -> Vec<Collision>`:
rebuilds every route's wire-shape intervals and via footprints into
the same per-line model (routes only, no OBS), sweeps each line's
sorted intervals, and reports each cross-net pair whose spacing rule
violates. Exact on the committed result — this is the `RouteDb`
collision list and the `results.md` number, and the fail-proof
compares it between `Enabled` and `Disabled` runs.

## 5. Cost notes (recorded, not optimized)

Rasterizing a wire inserts `O(span/pitch)` entries per axis;
per-line `Vec`s stay unsorted (query = linear scan of one line's
entries — short in practice on signal layers). picorv32-scale
estimates put total inserts in the same order as routed edges;
measure, record the runtime in `results.md`, and only then optimize.

## Files

- `crates/grus-core/src/drt/maze.rs`
