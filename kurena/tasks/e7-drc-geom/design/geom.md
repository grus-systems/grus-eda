# e7-drc-geom — `drc::geom`: layer shapes + spatial index

`crates/grus-core/src/drc.rs` holds the shared types and re-exports
(`pub mod geom; pub mod width;` — same layout as `pdn.rs`/`pdn/mesh.rs`);
`src/drc/geom.rs` implements assembly.

## Types

```rust
/// A checkable layer: routing stack or cut stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrcLayer {
    Routing(LayerId),
    /// Index into `TechDb::cut_layers`.
    Cut(u32),
}

/// Which DB source produced the shape — debugging + fixture tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeSrc {
    SignalWire,   // NETS wire segment / RECT stub
    SpecialWire,  // SPECIALNETS wire segment
    Via,          // via-instance-derived (cut + metal)
    MacroPin,     // placed instance's macro PIN port rect
    MacroObs,     // placed instance's OBS / pin OBS rect
    IoPort,       // top-level PINS `+ LAYER` rect
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeRect {
    pub rect: RectDbu,
    pub src: ShapeSrc,
    /// Interned net name for wire/via shapes (diagnostics).
    pub net: Option<SymbolId>,
}

/// Shapes on one layer, insertion order + a sorted index into them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerShapes {
    pub rects: Vec<ShapeRect>,
    /// `rects` indices sorted by `rect.xl` (stable — `sort_by_key`).
    pub by_x: Vec<u32>,
}

pub struct DrcDesign {
    /// Indexed by `LayerId` (same order as `routing_layers`).
    pub routing: Vec<LayerShapes>,
    /// Indexed by `cut_layers` position.
    pub cut: Vec<LayerShapes>,
}
```

`LayerShapes::iter_overlapping(probe) -> impl Iterator<Item =
&ShapeRect>`: binary-search the first `by_x` entry whose `rect.xh >
probe.xl`, walk forward while `rect.xl < probe.xh`, yield on
y-overlap. Same strict-overlap convention as `pdn.rs`/gcell
(`RectDbu::intersects`: touching edges are not overlapping). This is
the query the future spacing family drives; width only needs
`rects.iter()`.

## `assemble(db, tech) -> DrcDesign`

Emission order inside a layer is **source-tagged, deterministic** —
inputs are iterated in fixed DB order so two assembles produce equal
`Vec<ShapeRect>`:

1. `db.net_geoms` in order → `wire_rects` → `(Routing(lid), SignalWire, net_names[n])`.
2. `db.pdn_nets` in order → `wire_rects` → `(Routing(lid), SpecialWire, pdn_net.name)`.
3. Every `PdnVia` in `net_geoms` then `pdn_nets` → expand per
   `ViaGeom` (below) → `(Routing|Cut, Via, net)`.
4. Every placed `CellId` in order: macro lookup → pin shapes from
   `MacroPin.rects` (`src = MacroPin`) and obstructions from
   `macro.obs_rects` **plus** `macro.pin_obs_rects` (both
   `src = MacroObs` — pin OBS is OBS, not pin metal; if a later rule
   needs them distinct, split the tag then). All transformed by
   `DesignDb::orient_rect(r, mac.w, mac.h, cell_orient[c])` then
   translated by `(cell_x_dbu[c], cell_y_dbu[c])` — the same rect
   transform `congestion/gcell.rs` and `pdn.rs` share. `Unplaced`
   cells contribute nothing (their position is 0,0 garbage; pdn.rs
   already conditions pin attach on `Placed`/`Fixed` — same gate).
5. `pin_io_rects` for `pin_cells[p] == None` (top-level PINS `+ LAYER`
   rects, already absolute) → `(Routing(lid), IoPort, net)`. These are
   real shapes Magic checks; the spec lists four sources — this is a
   recorded fifth, cheap and honest.

Cut-layer rects land only from source 3. Routing-layer `LayerId`s
index `routing`; `Cut(i)` indexes `cut` (`licon` is absent from
`cut_layers` — a `ViaRectLayer::Cut` on a dropped cut layer can't
occur because `parse_via` only records `cut_idx` for resolved layers;
an unresolvable cut layer in LEF already errors at parse).

## Via expansion arithmetic

`expand_via(via: &PdnVia, tech) -> Vec<(DrcLayer, RectDbu)>`:

- `ViaGeom::Lef(i)` → `tech.via_defs[i].rects`, each translated by
  `(x_dbu, y_dbu)`. Order = LEF group order.
- `ViaGeom::Generated(g)` →
  - cut array centered at `(x,y)`: `aw = cols*cut_w + (cols-1)*sp_x`,
    `ah = rows*cut_h + (rows-1)*sp_y`; cut `(i,j)` lower-left at
    `(x - aw/2 + j*(cut_w+sp_x), y - ah/2 + i*(cut_h+sp_y))` —
    truncating integer division (odd `aw`/`ah` don't occur in the
    committed DEFs; recorded).
  - lower metal: `x - aw/2 - enc_bot_x → x + aw/2 + enc_bot_x`,
    `y - ah/2 - enc_bot_y → y + ah/2 + enc_bot_y` (the extra `aw/2`
    residue on odd widths lands on the high side; documented).
  - upper metal: same with `enc_top_*`.
  - layer indices: `lower`/`upper` from `PdnVia`, cut index from
    `tech.cut_layer_by_name(cut_sym)` → `bail!` if absent.
  - Sanity check baked into the unit test (def-grammar.md):
    `via2_3_1600_480_1_5_320_320` at origin must emit lower metal
    exactly `(-800, -240, 800, 240)` = the `1600×480` in its name.
    This verifies the OpenDB `ENCLOSURE (botX, botY, topX, topY)`
    reading on one committed record — that is the coverage claim, not
    "all DEF producers".

`cut_count` stays a field (PDN + census); `Generated` could recompute
it but `rows*cols` round-trip is kept for a `debug_assert` in tests.

## Orientation coverage claim

`orient_rect` arms N/S/FN/FS are exercised by real designs; W/E/FW/FE
by fixtures only (`gcd.orient.def` proved the E0 bug class — the
synthetic fixture in `tests.md` asserts exact W and FE OBS coords).
Do **not** claim all-8 coverage in docs beyond this.

## Determinism

`assemble` iterates `Vec`s in order — no hash maps. `by_x` uses
`sort_by_key` (stable) → `LayerShapes` is `PartialEq`-comparable and
the round-trip test asserts `assemble(db1) == assemble(db2)` on
twice-parsed DEFs.

## Files

- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/lib.rs`
