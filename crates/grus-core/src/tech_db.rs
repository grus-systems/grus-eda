use crate::interner::{LayerId, MacroId, PinDir, StringInterner, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerDir {
    Horizontal,
    Vertical,
}

/// One `SPACINGTABLE` `WIDTH w s` row: shapes wider than `width_dbu`
/// require `spacing_dbu` edge clearance (strict `>` — the commented
/// `SPACING ... RANGE 3.001 100` form reads 3.001µm, i.e. >3000 DBU).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpacingTableRow {
    pub width_dbu: i32,
    pub spacing_dbu: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutingLayer {
    pub name: SymbolId,
    pub dir: LayerDir,
    /// Preferred-direction pitch: the x `PITCH` for a `Vertical`
    /// layer, the y `PITCH` for a `Horizontal` one.
    pub pitch_dbu: i32,
    pub width_dbu: i32,
    /// Preferred-direction `OFFSET` (same axis rule as `pitch_dbu`).
    pub offset_dbu: i32,
    /// Per-axis LEF `PITCH`/`OFFSET` (`PITCH <x> [<y>]`, y defaulting
    /// to x). `TRACKS` grid derivation needs the axis-correct value —
    /// sky130 li1 splits 460/340 and 230/170.
    #[serde(default)]
    pub pitch_x_dbu: i32,
    #[serde(default)]
    pub pitch_y_dbu: i32,
    #[serde(default)]
    pub offset_x_dbu: i32,
    #[serde(default)]
    pub offset_y_dbu: i32,
    /// LEF `TRACKS X` coordinates, expanded and sorted. Empty when the
    /// tech LEF declares none (sky130: always empty — the DEF supplies
    /// the grid; `drt::tracks::layer_tracks` merges both sources).
    #[serde(default)]
    pub tracks_x: Vec<i32>,
    /// LEF `TRACKS Y` coordinates, same convention.
    #[serde(default)]
    pub tracks_y: Vec<i32>,
    pub res_per_sq: f32,      // Ohms per square
    pub cap_per_sq_um: f32,   // pF per um^2
    pub edge_cap_per_um: f32, // pF per um
    /// Plain `SPACING <v>` in DBU; 0 when the layer is table-driven.
    #[serde(default)]
    pub spacing_dbu: i32,
    /// `SPACINGTABLE` `WIDTH` rows in file order (ascending threshold in
    /// sky130; the helpers do not rely on order). The PRL column is not
    /// modeled — selecting by width alone can only over-require.
    #[serde(default)]
    pub spacing_table: Vec<SpacingTableRow>,
    /// LEF `AREA` — minimum merged-polygon area, DBU². 0 = undeclared.
    #[serde(default)]
    pub area_dbu2: i64,
    /// LEF `MINENCLOSEDAREA` — minimum hole area, DBU². 0 = undeclared.
    #[serde(default)]
    pub min_enclosed_area_dbu2: i64,
}

impl RoutingLayer {
    /// Minimum same-layer spacing: plain `SPACING` when present (LEF
    /// precedence), else the smallest-width-threshold table row's
    /// spacing. 0 = no LEF spacing data (rule skipped, mirroring
    /// `width_dbu == 0`).
    pub fn min_spacing_dbu(&self) -> i32 {
        if self.spacing_dbu != 0 {
            return self.spacing_dbu;
        }
        self.spacing_table
            .iter()
            .min_by_key(|r| r.width_dbu)
            .map(|r| r.spacing_dbu)
            .unwrap_or(0)
    }

    /// The wide-metal row — the `WIDTH` row with the largest threshold —
    /// as `(width_threshold_dbu, spacing_dbu)`. `None` when the table has
    /// a single row or is absent.
    pub fn wide_spacing(&self) -> Option<(i32, i32)> {
        if self.spacing_table.len() < 2 {
            return None;
        }
        self.spacing_table
            .iter()
            .max_by_key(|r| r.width_dbu)
            .map(|r| (r.width_dbu, r.spacing_dbu))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroClass {
    Core,
    CoreTie,
    CoreWellTap,
    CoreSpacer,
    Block,
    Pad,
    EndCap,
    Other,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RectDbu {
    pub xl: i32,
    pub yl: i32,
    pub xh: i32,
    pub yh: i32,
}

impl RectDbu {
    pub fn new(xl: i32, yl: i32, xh: i32, yh: i32) -> Self {
        Self { xl, yl, xh, yh }
    }

    pub fn width(&self) -> i32 {
        self.xh - self.xl
    }

    pub fn height(&self) -> i32 {
        self.yh - self.yl
    }

    pub fn area(&self) -> i64 {
        self.width().max(0) as i64 * self.height().max(0) as i64
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroPin {
    pub name: SymbolId,
    pub dir: PinDir,
    pub is_power: bool,    // true if USE POWER or USE GROUND
    pub offset_x_dbu: i32, // floor of the mean rect center, relative to macro origin
    pub offset_y_dbu: i32,
    // The mean center is not an integer. OpenROAD truncates after applying the
    // orientation, so a mirrored axis must round up instead (docs/e0-readers.md).
    pub offset_x_frac: bool,
    pub offset_y_frac: bool,
    pub layer: Option<LayerId>,
    /// Routing-layer pin shapes in file order, macro coordinates. Guide
    /// parasitics need the per-rect middles (OpenROAD's on-grid vote) and
    /// the last rect's lower-left corner (its pin position).
    #[serde(default)]
    pub rects: Vec<(LayerId, RectDbu)>,
}

/// A LEF `TYPE CUT` layer. `above` is the routing layer directly below it in
/// LEF stack order — the index `via_res_ohm_above` is keyed on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutLayer {
    pub name: SymbolId,
    pub above: LayerId,
    /// `WIDTH` in DBU, 0 when the LEF omits it.
    #[serde(default)]
    pub width_dbu: i32,
    /// Plain `SPACING <v>` in DBU, 0 when the LEF omits it.
    #[serde(default)]
    pub spacing_dbu: i32,
    /// LEF `ENCLOSURE BELOW o1 o2` — overhang pair the routing layer
    /// below the cut must provide. [0,0] = undeclared / zero.
    #[serde(default)]
    pub enc_below_dbu: [i32; 2],
    /// LEF `ENCLOSURE ABOVE o1 o2` — same for the routing layer above.
    #[serde(default)]
    pub enc_above_dbu: [i32; 2],
}

/// Which coordinate space a `ViaLayerRect` lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViaRectLayer {
    Routing(LayerId),
    /// Index into `TechDb::cut_layers`.
    Cut(u32),
}

/// One `RECT` of a LEF `VIA` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaLayerRect {
    pub layer: ViaRectLayer,
    /// Via-local coordinates, centered on the via origin (DBU).
    pub rect: RectDbu,
}

/// A LEF `VIA` block: the layer triple + parallel cut count the PDN model
/// needs, plus every `RECT` in group order for the DRC shape model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViaDef {
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut-layer name; resistance via `cut_layer_by_name → cut_layers[..].above`.
    pub cut: SymbolId,
    /// Parallel cuts — the total `RECT` count across the `VIA`'s cut-layer
    /// groups (DEF-generated vias carry the per-instance count in `+ ROWCOL`).
    pub cut_count: u32,
    /// Every `RECT` in the `VIA` block, group order, min/max normalized,
    /// µm → DBU by the same `.round()` pin rects use. Parallel cut rects
    /// are all kept (mcon arrays have >1).
    #[serde(default)]
    pub rects: Vec<ViaLayerRect>,
}

/// One LEF `TRACKS` record, kept in declared µm units until
/// `parse_lef_bytes` finishes: `TRACKS` may precede the `UNITS` scale
/// and the `LAYER` statement it names, so expansion is deferred the
/// same way `cut_res_above` is.
#[derive(Clone, Debug, PartialEq)]
pub struct LefTrackRecord {
    /// true = `TRACKS X` (vertical lines / x-coords), false = `TRACKS Y`.
    pub is_x: bool,
    pub start_um: f32,
    pub count: u32,
    pub step_um: f32,
    /// `None` = bare form (applies to every routing layer).
    pub layers: Option<Vec<String>>,
    /// Source line, for unknown-layer / expansion error messages.
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroDef {
    pub name: SymbolId,
    pub class: MacroClass,
    pub width_dbu: i32,
    pub height_dbu: i32,
    pub pin_offset: u32,
    pub pin_count: u16,
    pub obs_rects: Vec<(LayerId, RectDbu)>,
    /// `OBS` rectangles before `merge_connected_obs`. The merged set
    /// inflates chains of touching rects into their bounding box —
    /// acceptable for gcell blockage density and the DRC source-4
    /// convention, but it fills the gaps decap/edge cells leave
    /// between cap fingers and seals neighboring pin pockets, so the
    /// maze router marks this unmerged set instead.
    #[serde(default)]
    pub obs_rects_raw: Vec<(LayerId, RectDbu)>,
    pub pin_obs_rects: Vec<(LayerId, RectDbu)>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct TechDb {
    pub dbu_per_micron: i32,
    pub site_name: String,
    pub site_width_dbu: i32,
    pub site_height_dbu: i32,
    pub strings: StringInterner,
    pub routing_layers: Vec<RoutingLayer>,
    /// Via resistance (ohms) of the cut layer above each routing layer —
    /// `via_res_ohm_above[k]` is the cut layer between routing layers `k`
    /// and `k+1` in LEF stack order, 0 when none exists (top layer). Guide
    /// parasitics need it; OpenROAD reads it off `dbTechLayer::getUpperLayer`.
    #[serde(default)]
    pub via_res_ohm_above: Vec<f32>,
    /// Transient parse state: (routing layer index below, RESISTANCE ohms)
    /// per CUT layer, finalized into `via_res_ohm_above` by `load_tech_db`.
    #[serde(skip)]
    pub cut_res_above: Vec<(u8, f32)>,
    /// Transient parse state: raw `TRACKS` records in file order,
    /// expanded into `RoutingLayer::tracks_{x,y}` at the end of each
    /// `parse_lef_bytes` call.
    #[serde(skip)]
    pub lef_track_records: Vec<LefTrackRecord>,
    /// CUT layers in LEF order (`licon`, which sits below every routing
    /// layer, is not recorded — it has no `above`).
    #[serde(default)]
    pub cut_layers: Vec<CutLayer>,
    /// LEF `VIA` definitions by name.
    #[serde(default)]
    pub via_defs: Vec<ViaDef>,
    pub macros: Vec<MacroDef>,
    pub macro_pins: Vec<MacroPin>,
    #[serde(skip)]
    pub layer_by_name: HashMap<SymbolId, LayerId>,
    /// Cut-layer name → `cut_layers` index; populated during `parse_layer`
    /// and rebuilt by `rebuild_maps`.
    #[serde(skip)]
    pub cut_layer_by_name: HashMap<SymbolId, u32>,
    /// LEF `VIA` name → `via_defs` index; same lifecycle as `layer_by_name`.
    #[serde(skip)]
    pub via_by_name: HashMap<SymbolId, u32>,
    #[serde(skip)]
    pub macro_by_name: HashMap<SymbolId, MacroId>,
}

impl TechDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild_maps(&mut self) {
        self.strings.rebuild_map();
        self.layer_by_name.clear();
        self.layer_by_name.reserve(self.routing_layers.len());
        for (i, layer) in self.routing_layers.iter().enumerate() {
            self.layer_by_name.insert(layer.name, LayerId(i as u8));
        }

        self.macro_by_name.clear();
        self.macro_by_name.reserve(self.macros.len());
        for (i, mac) in self.macros.iter().enumerate() {
            self.macro_by_name.insert(mac.name, MacroId(i as u16));
        }

        self.cut_layer_by_name.clear();
        self.cut_layer_by_name.reserve(self.cut_layers.len());
        for (i, cut) in self.cut_layers.iter().enumerate() {
            self.cut_layer_by_name.insert(cut.name, i as u32);
        }

        self.via_by_name.clear();
        self.via_by_name.reserve(self.via_defs.len());
        for (i, via) in self.via_defs.iter().enumerate() {
            self.via_by_name.insert(via.name, i as u32);
        }
    }

    /// Per-instance resistance of a resolved via: the cut layer's
    /// `via_res_ohm_above` divided by the parallel cut count. `None` when the
    /// cut layer is unknown or carries no RESISTANCE.
    pub fn via_res_ohm(&self, cut: SymbolId, cut_count: u32) -> Option<f32> {
        let idx = *self.cut_layer_by_name.get(&cut)? as usize;
        let res = *self
            .via_res_ohm_above
            .get(self.cut_layers[idx].above.0 as usize)?;
        (res != 0.0).then_some(res / cut_count.max(1) as f32)
    }

    pub fn macro_pins(&self, mac: &MacroDef) -> &[MacroPin] {
        let start = mac.pin_offset as usize;
        let end = start + mac.pin_count as usize;
        &self.macro_pins[start..end]
    }
}
