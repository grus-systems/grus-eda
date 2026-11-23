# e6-drt-tracks — Access-point model (`drt/access.rs`)

## What an access point is (our model)

A **candidate attachment point**: a grid location on a routing layer
where a track line crosses a routed pin's shape. It is *not* a chosen
tap point (no track-assignment stage yet) and *not* a DRC-clean point
(no spacing/OBS pruning yet) — that claim boundary goes in
`docs/e6-drt.md`.

Per pin rect `R = [xl,xh]×[yl,yh]` on routing layer `l`:

- each `t ∈ tracks_x[l]` with `xl ≤ t ≤ xh` → `AccessPoint(t, mid_y)`
- each `t ∈ tracks_y[l]` with `yl ≤ t ≤ yh` → `AccessPoint(mid_x, t)`

where `mid_x = (xl + xh) / 2`, `mid_y = (yl + yh) / 2` — i32 division,
which truncates toward zero; die-internal coordinates are non-negative
so this is floor. Boundary **inclusive**: a track on the rect edge
touches the shape.

This is deliberately not TritonRoute's access-point algorithm (which
places APs on preferred-track × secondary-track intersections with
cost models). Our enumeration is a superset candidate list the later
maze router prunes. One point per track line per rect — not the
cross product.

## Types

```rust
pub struct AccessPoint {
    pub pin: PinId,
    pub layer: LayerId,
    pub x: i32,
    pub y: i32,
}

/// A member pin whose shapes intersected no track on any candidate
/// layer — a finding, never silently counted.
pub struct OffTrackPin {
    pub pin: PinId,
    /// `db.pin_nets[pin]` at census time.
    pub net: Option<NetId>,
    /// The absolute candidate rects that were checked (may be empty
    /// — see `no_shapes` for the no-geometry case).
    pub rects: Vec<(LayerId, RectDbu)>,
}

pub struct AccessReport {
    /// In emission order (below); deduplicated per pin.
    pub points: Vec<AccessPoint>,
    /// `points` count per routing layer — parallel to
    /// `tech.routing_layers`. The census evidence.
    pub per_layer: Vec<usize>,
    /// Pins with candidate shapes but zero access points.
    pub off_track: Vec<OffTrackPin>,
    /// Pins with no candidate shapes at all (unplaced cell, missing
    /// `pin_macro_pins` mapping, io pin without `+ LAYER` rects).
    pub no_shapes: Vec<PinId>,
}

pub fn access_points(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
) -> AccessReport;
```

## Pin shape enumeration (`pin_shapes`, private)

```rust
fn pin_shapes(db: &DesignDb, tech: &TechDb, p: PinId)
    -> Vec<(LayerId, RectDbu)>
```

- `db.pin_cells[p] == Some(c)`: empty unless `db.cell_status[c]` is
  `Placed | Fixed | Cover` (same gate as `drc::geom` source 4). Then
  `mp = db.pin_macro_pins[p]` — `None` → empty. Each `(lid, r)` in
  `tech.macro_pins[mp].rects` maps through
  `DesignDb::orient_rect(r, mac.width_dbu, mac.height_dbu,
  db.cell_orient[c])` then translates by `(cell_x_dbu[c],
  cell_y_dbu[c])` — verbatim the `place` closure in
  `drc/geom.rs` source 4. Reuse `orient_rect` directly; do **not**
  route through `drc::geom::assemble` (it flattens per-layer stores
  and loses the pin key).
- `db.pin_cells[p] == None` (io `BTERM`): `db.pin_io_rects[p]`, already
  absolute — the same source `drc::geom` source 5 reads.
- `+ SPECIAL` ports (`VGND`/`VPWR`) share `pin_cells == None` but are
  never members of `net_names` nets, so they are never visited —
  no special-case needed.

## Emission order (determinism contract)

Nets `0..db.num_nets()` ascending → member pins in `db.net_pins(net)`
order (driver first) → candidate layers ascending by `LayerId`
(`BTreeMap<LayerId, Vec<RectDbu>>` over the pin's shapes, rects kept
in file order per layer) → per rect: `tracks_x` hits ascending, then
`tracks_y` hits ascending.

Per-pin dedup: two overlapping rects of one pin on one layer can
produce the same `(layer, x, y)`; dedup `(layer, x, y)` within the pin
before pushing (collect into a `BTreeSet`/sorted scan — the per-pin
set is tiny). Determinism follows from fixed iteration order plus
sorted track lists; a double-run assert in the integration test pins
it with nonzero evidence.

A pin lands in `off_track` when `pin_shapes` was nonempty but produced
zero points; in `no_shapes` when `pin_shapes` was empty. Both are
gate-blocking on real designs.

`per_layer[layer.0] += 1` per emitted point — computed after dedup.

## Files

- `crates/grus-core/src/drt/access.rs`
