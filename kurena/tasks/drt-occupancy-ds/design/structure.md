# structure — dense track-indexed occupancy

Replace the `BTreeMap<i32, Line>` keying with dense `Vec<Line>`
indexed by track position. Everything else about the model —
`Occupant`, `Interval`, `Line { ivs, max_hi }`, `SKIP_CHUNK`, the
`w/2 + s` band math, `hit_verdict` classes — is unchanged.

## 1. Type changes

```rust
struct Occupancy {
    lines_x: Vec<Vec<Line>>,   // [layer][track index into coords_x]
    lines_y: Vec<Vec<Line>>,   // [layer][track index into coords_y]
    /// Sorted line coordinates per axis — the `LayerTracks` lists,
    /// owned so `scan_rect`/`rasterize` windows and `Hit.line`
    /// resolution need no `tracks` parameter.
    coords_x: Vec<Vec<i32>>,
    coords_y: Vec<Vec<i32>>,
    halfw: Vec<i32>,
    spc: Vec<i32>,
    stats: ScanStats,           // Cell<u64> fields — measurement.md
}
```

`Mark` gains the index the rip-up log actually needs:

```rust
struct Mark {
    x_axis: bool,
    layer: u8,
    line_idx: u32,   // was `line: i32` (coordinate)
    iv: Interval,
}
```

`Hit` is unchanged — `line: i32` stays a **coordinate**; every
consumer (`Collision.line`, `edge_collisions`, census joins) wants
the coordinate.

## 2. Constructor

```rust
fn new(tech: &TechDb, tracks: &[LayerTracks]) -> Self
```

`coords_{x,y}[l] = tracks[l].tracks_{x,y}.clone()`;
`lines_{x,y}[l] = vec![Line::default(); len]` (`Line` is
`Clone + Default`; an untouched line is two empty `Vec`s — no
allocation, memory = 48 B/track slot). Parallel to
`tech.routing_layers` exactly as before: `coords.len() ==
lines.len() == tech.routing_layers.len()`, and the existing
`lid.0 as usize >= occ.lines_x.len()` layer-count guards keep
working verbatim.

Every construction site has `tracks` in scope:
`route_nets_with_progress` (~L3824), `overlap_census` (~L4051),
`pin_overlap_census` (~L4108), and test helpers (`toy_occ`,
`Occupancy::new(&tech)` at ~L4455/4571 — pass `toy_tracks()` /
the fixture's track list).

## 3. Per-function equivalence

### `rasterize(&mut self, layer, r, who, log)`

The `tracks` parameter drops (coords are owned). The band window
is the identical `partition_point` pair on `coords_x[l]` /
`coords_y[l]` (currently run on `tx`/`ty`):

```rust
let a = cx.partition_point(|&t| t <= r.xl - w2 - s);
let b = cx.partition_point(|&t| t <  r.xh + w2 + s);
for i in a..b {
    Self::insert(&mut self.lines_x[l][i], iv);
    // log: Mark { x_axis: true, layer, line_idx: i as u32, iv }
}
```

Same index range `a..b` as the current `tx[a..b]` iteration;
`entry(t).or_default()` → direct slot index. Deterministic
insertion order preserved (ascending track index = ascending
coordinate, since coords are sorted).

### `unmark(&mut self, m)`

`map.get_mut(&m.line)` → `self.lines_{x|y}[m.layer][m.line_idx]` —
no lookup at all. The `iv` equality find + `remove` +
`rebuild_from` are unchanged. The `Option` fallback on a missing
line disappears: a logged mark always points at a live slot
(indices never move; the slot's `ivs` may be shorter after other
unmarks, so keep the `position(..)` find, keep the early-return
arm as defense only at this boundary — it costs nothing).

### `scan_wire(&self, axis, layer, ti: u32, c, d, out)`

`t: i32` (coordinate) → `ti: u32` (track index). Body:

```rust
let (lines, coords) = match axis {
    Axis::X => (&self.lines_y[l], &self.coords_y[l]),
    Axis::Y => (&self.lines_x[l], &self.coords_x[l]),
};
let line = &lines[ti as usize];
let t = coords[ti as usize];       // for Hit.line
// …identical chunked ivs scan…
```

`map.get(&t) → lines[ti]` is the whole change: `None` (no marks)
and empty-`Line` are the same answer — zero hits. `Hit { line: t,
x_axis: matches!(axis, Axis::Y), .. }` identical, including the
axis→map flip (X-axis wires scan `lines_y`).

### `scan_rect(&self, layer, r, out)`

`map.range(re.lo − w2 − s ..= re.hi + w2 + s)` over keys becomes a
two-`partition_point` window on the owned coord list, then a slice
walk:

```rust
// for lines_y (query axis X):
let a = cy.partition_point(|&t| t <  re.yl - w2 - s);
let b = cy.partition_point(|&t| t <= re.yh + w2 + s);
for (ti, line) in self.lines_y[l][a..b].iter().enumerate() {
    let t = cy[a + ti];            // Hit.line
    // …identical chunked ivs scan + 2-D perp_* filter…
}
```

Bounds check: `range(lo..=hi)` yields keys `k` with `lo ≤ k ≤ hi`;
`coords[a..b]` with `a = pp(≥ lo)`, `b = pp(> hi)` is the same set,
in the same ascending order. Lines in-window but mark-free are
`Line::default()` — the chunk loop is a no-op, identical result.

### `insert`, `rebuild_from`, `hit_bbox`, `Hit`

Untouched. `insert` keeps its "after existing equal-`lo` entries"
rule — the determinism argument still holds.

## 4. Why this pick over the goal's other options

- **Raster grid (bitset/owner-per-cell)** — rejected: every hit must
  carry `iv` (`who`, `perp_*`) and the exact overlap `[lo, hi]` for
  `Collision` records, `exempt_obs`, and `hit_bbox` repair
  decomposition. A grid stores "occupied" — the interval payload
  must live somewhere anyway, so the grid is extra state, not a
  replacement. Track-resolution cells == the lines we already have.
- **Interval tree per layer** — rejected: marks are pre-decomposed
  onto lines (the rasterization), so queries are 1-D on one line's
  sorted `ivs`. A 2-D tree answers a harder problem and pays node
  chases again — the thing being removed.
- **Keep `BTreeMap`, add a coord→`Line` `HashMap` side index** —
  rejected: still a hash lookup per edge; duplicate bookkeeping;
  `Vec` index is strictly better and simpler.
- **Dense `Vec<Line>` (chosen)** — the key domain is already dense
  (`0..n_tracks`); marks can only exist on track lines, so no sparse
  waste beyond the `Line` header (48 B). `scan_wire`/`rasterize`/
  `unmark` become O(1) slot access; `scan_rect` becomes two binary
  searches + contiguous slice walk.

## 5. Call-site index availability (why `ti` costs nothing)

| call site | query line | index already held |
|---|---|---|
| `edge_cost` ~L950 X-arm | `tracks_y[na.iy]` | `na.iy` |
| `edge_cost` ~L950 Y-arm | `tracks_x[na.ix]` | `na.ix` |
| `edge_collisions` ~L1116 | same pattern | `na.iy` / `na.ix` |
| `scan_stub` ~L1035 | `nx` on `Axis::Y` | `g.nodes[n].ix` |
| `scan_stub` ~L1041 | `ny` on `Axis::X` | `g.nodes[n].iy` |
| `patch_component` ~L3614 | `line_at(line)` | `line` (it IS the index) |

`scan_stub` reads `n`'s `ix`/`iy` directly instead of re-resolving
coords — `node_xy` (~L826) proves `nx = tracks_x[n.ix]` etc., so
index ↔ coord correspondence is by construction.

The `(line, c, d)` coord tuples at ~L932–943 and ~L1104–1115 keep
computing `c`/`d` (coordinate extents — needed) but `line` becomes
unneeded for the scan on the query axis; keep the coord only where
still consumed (`paint` rect in `edge_cost`, `Hit.line` is now
internal). Audit each arm — drop a `line` binding only if nothing
else reads it, else leave the computation (a single `Vec` index).

Test call sites pass coordinates today (`scan_wire(Axis::X, 0,
1000, …)` on `toy_tracks()` = index 1): update each literal to the
track index — mechanical, no compat wrapper (no half-shims).

## 6. Direct field accesses outside the scan fns

- `occ.lines_x.len()` layer guards (~L662/692/699/735/3406) —
  unchanged semantics (`Vec<Vec<Line>>` `.len()` = layer count).
- `overlap_census` ~L4057–4088 and `pin_overlap_census`
  ~L4115–4152 iterate `for (&t, line) in map`. Rewrite as
  `for (t, line) in occ.coords_x[l].iter().zip(&occ.lines_x[l])`
  (and `_y` arm): ascending-coordinate order identical to BTreeMap
  iteration, so `Collision` emission order is unchanged — census
  output stays byte-identical.

## 7. Determinism invariants (must hold — listed for the reviewer)

- Ascending track-index iteration == ascending coordinate iteration
  (coords sorted-unique by `layer_tracks` construction).
- `insert` stable position among equal `lo` (kept).
- Mark log order = insertion order across `a..b` then the `lines_y`
  pass (kept — `commit_route` logs in the same shape order).
- `Hit` construction identical field-for-field (`line` resolved
  through `coords[ti]` — the same coordinate that keyed the map).
- `Hit.lo/hi` = `iv.lo.max(slo)`/`iv.hi.min(shi)` — unchanged.
