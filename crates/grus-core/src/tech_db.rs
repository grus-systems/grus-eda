use crate::interner::{LayerId, MacroId, PinDir, StringInterner, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerDir {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutingLayer {
    pub name: SymbolId,
    pub dir: LayerDir,
    pub pitch_dbu: i32,
    pub width_dbu: i32,
    pub offset_dbu: i32,
    pub res_per_sq: f32,      // Ohms per square
    pub cap_per_sq_um: f32,   // pF per um^2
    pub edge_cap_per_um: f32, // pF per um
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

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroDef {
    pub name: SymbolId,
    pub class: MacroClass,
    pub width_dbu: i32,
    pub height_dbu: i32,
    pub pin_offset: u32,
    pub pin_count: u16,
    pub obs_rects: Vec<(LayerId, RectDbu)>,
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
