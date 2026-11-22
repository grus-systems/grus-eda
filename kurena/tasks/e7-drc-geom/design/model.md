# e7-drc-geom — Data model

Type-level design for routed-geometry storage and via rectangles. All
types derive `Clone`/`Debug`/`PartialEq`/`Serialize`/`Deserialize`;
new `Vec` fields carry `#[serde(default)]`.

## `tech_db.rs` — LEF via rects + cut-layer width

```rust
/// Which coordinate space a `ViaLayerRect` lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViaRectLayer {
    Routing(LayerId),
    /// Index into `TechDb::cut_layers`.
    Cut(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaLayerRect {
    pub layer: ViaRectLayer,
    /// Via-local coordinates, centered on the via origin (DBU).
    pub rect: RectDbu,
}

pub struct ViaDef {
    // existing: name, lower, upper, cut, cut_count
    /// Every `RECT` in the LEF `VIA` block, group order, min/max
    /// normalized, µm → DBU by the same `.round()` pin rects use.
    /// Parallel cut rects are all kept (mcon arrays have >1).
    #[serde(default)]
    pub rects: Vec<ViaLayerRect>,
}

pub struct CutLayer {
    // existing: name, above
    /// `WIDTH` in DBU, 0 when the LEF omits it.
    #[serde(default)]
    pub width_dbu: i32,
}
```

`lef.rs` `parse_via` already collects per-`LAYER` groups as
`(name_sym, Option<LayerId>, Option<cut_idx>, rect_count)`; extend the
tuple to carry `Vec<RectDbu>` and flatten into `rects`. `width_um` is
already parsed for every layer — record it on `CutLayer` the same way
routing layers do (µm × `dbu_per_micron`, rounded). The stale
`ViaDef` doc comment ("cut/bottom/top RECT geometry is not stored")
goes away.

## `design_db.rs` — via instances + signal-net geometry

```rust
/// How a `PdnVia`'s shapes are derived at expansion time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ViaGeom {
    /// LEF `VIA` — index into `TechDb::via_defs`; `rects` verbatim.
    Lef(u32),
    /// DEF `VIAS + VIARULE` generated via — rects from params.
    Generated(GeneratedViaGeom),
}

/// DEF `VIAS` generated-via parameters (`+ CUTSIZE` etc.), retained at
/// parse. `enc_*` are symmetric overhangs per OpenDB semantics:
/// `(botX, botY, topX, topY)` — see def-grammar.md for the arithmetic
/// check against `via2_3_1600_480_1_5_320_320`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratedViaGeom {
    pub cut_w_dbu: i32,
    pub cut_h_dbu: i32,
    pub rows: u32,
    pub cols: u32,
    pub cut_spacing_x_dbu: i32,
    pub cut_spacing_y_dbu: i32,
    pub enc_bot_x_dbu: i32,
    pub enc_bot_y_dbu: i32,
    pub enc_top_x_dbu: i32,
    pub enc_top_y_dbu: i32,
}

pub struct PdnVia {
    // existing: name, lower, upper, cut, cut_count, x_dbu, y_dbu
    pub geom: ViaGeom,
}

/// Routed shapes of one `NETS` net — same shape vocabulary `PdnNet`
/// proved for SPECIALNETS.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetGeom {
    /// Two-point segments + `RECT` stubs, half-width-expanded at parse
    /// by the layer's LEF `WIDTH`. Statement order.
    pub wire_rects: Vec<(LayerId, RectDbu)>,
    /// Via instances, path order.
    pub vias: Vec<PdnVia>,
}
```

`DesignDb` gains:

```rust
/// Per-net routed geometry from `NETS`, parallel to `net_names`.
/// Invariant I7: `len() ∈ {0, num_nets()}` — `0` only for non-DEF
/// sources (verilog/yosys) that never populate it.
#[serde(default)]
pub net_geoms: Vec<NetGeom>,
```

plus `net_geom(net) -> Option<&NetGeom>` and a
`signal_wire_rects()` flat iterator mirroring `special_wire_rects()`.
`create_net` keeps I7 by pushing `NetGeom::default()` whenever
`net_geoms.len() == net_names.len()` — the exact `net_def_spellings`
pattern.

**Type reuse notes:** `PdnVia` is now the shared DEF via-instance
record (NETS + SPECIALNETS); the name is kept — renaming it
`ViaInst`-style touches `pdn.rs`/`pdn/mesh.rs`/`def.rs` for zero
behavior gain. `PdnVia.geom` is a required field — `resolve_def_via`
becomes the only constructor, so the field can't be forgotten.
`Generated` params are u32/i32 already validated in `add_def_via`.

## `resolve_def_via` signature

Now returns the full stack + geometry:

```rust
fn resolve_def_via(
    tech_db: &TechDb, def_vias: &DefViaMap,
    name: &str, net_name: &str, line: usize,
) -> Result<(LayerId, LayerId, SymbolId, u32, ViaGeom)>
```

Resolution order unchanged — `def_vias` first, then
`tech_db.via_by_name` → `via_defs[i]` (`ViaGeom::Lef(i)`); `Inline` and
unknown names bail as today; `Fixed(sym)` resolves through LEF and gets
`Lef(i)` too. `DefVia::Generated` grows a `geom: GeneratedViaGeom`
field populated by `add_def_via`; the `(lo, hi)` layer sort is kept.

The SPECIALNETS call site in `def.rs` (`parse_special_wire`) stores the
`geom` on the `PdnVia` it emits; `pdn.rs` consumes those records and
needs only the stack, unchanged. `drc::geom` reads `geom` at expansion.
This makes "same code path as `pdn.rs`" literal: NETS and SPECIALNETS
vias are resolved by one function into one record type.

## serde / compat

- `PdnVia.geom` and `ViaDef.rects`/`CutLayer.width_dbu` are additive;
  `DesignDb`/`TechDb` aren't persisted to disk, so no migration needed
  — `#[serde(default)]` only where an empty value is meaningful
  (`rects`, `width_dbu`, `net_geoms`).
- Public API additions + a new required `PdnVia` field → breaking for
  literal constructors → workspace version **minor** bump
  (`0.8.0 → 0.9.0`) in the same commit as the model change.

## Files

- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/pdn.rs`
- `Cargo.toml`
