use crate::interner::{
    CellId, LayerId, LibCellId, LibPinId, MacroId, MacroPinId, NetId, PinDir, PinId,
    StringInterner, SymbolId,
};
use crate::tech_db::RectDbu;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementStatus {
    Unplaced,
    Placed,
    Fixed,
    Cover,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orient {
    N,
    S,
    E,
    W,
    FN,
    FS,
    FE,
    FW,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacementRow {
    pub row_name: SymbolId,
    pub site_name: SymbolId,
    pub orig_x_dbu: i32,
    pub orig_y_dbu: i32,
    pub orient: Orient,
    pub num_x: i32,
    pub step_x_dbu: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackGrid {
    pub is_vertical_tracks: bool, // true = X tracks (vertical lines), false = Y tracks (horizontal lines)
    pub start_dbu: i32,
    pub num_tracks: i32,
    pub step_dbu: i32,
    pub layers: Vec<LayerId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GCellGridDef {
    pub is_x: bool, // true = GCELLGRID X, false = GCELLGRID Y
    pub start_dbu: i32,
    pub num_boundaries: i32,
    pub step_dbu: i32,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct DesignDb {
    pub design_name: String,
    pub strings: StringInterner,
    pub dbu_per_micron: i32,
    pub die_area: RectDbu,
    pub rows: Vec<PlacementRow>,
    pub tracks: Vec<TrackGrid>,
    pub gcell_grids: Vec<GCellGridDef>,
    pub special_wire_rects: Vec<(LayerId, RectDbu)>, // PDN stripes/rails from SPECIALNETS

    // --- Cell SoA (indexed by CellId) ---
    pub cell_names: Vec<SymbolId>,
    pub cell_macros: Vec<MacroId>,
    pub cell_lib_ids: Vec<Option<LibCellId>>,
    pub cell_status: Vec<PlacementStatus>,
    pub cell_x_dbu: Vec<i32>,
    pub cell_y_dbu: Vec<i32>,
    pub cell_orient: Vec<Orient>,
    pub cell_pin_offsets: Vec<u32>, // length num_cells + 1
    pub cell_pin_ids: Vec<PinId>,

    // --- Pin SoA (indexed by PinId; includes instance pins and primary I/O ports) ---
    pub pin_names: Vec<SymbolId>, // port name for primary I/O, or macro pin name for instance pin
    pub pin_dirs: Vec<PinDir>,
    pub pin_cells: Vec<Option<CellId>>, // None for primary I/O port
    pub pin_macro_pins: Vec<Option<MacroPinId>>,
    pub pin_lib_pins: Vec<Option<LibPinId>>,
    pub pin_nets: Vec<Option<NetId>>,
    pub pin_offset_x_dbu: Vec<i32>, // relative to cell (or absolute for I/O port)
    pub pin_offset_y_dbu: Vec<i32>,
    pub pin_is_driver: Vec<bool>,
    pub io_pins: Vec<PinId>, // indices of primary I/O ports (where pin_cells[p] == None)

    // --- Net SoA & CSR (indexed by NetId) ---
    pub net_names: Vec<SymbolId>,
    pub net_pin_offsets: Vec<u32>, // length num_nets + 1
    pub net_pin_ids: Vec<PinId>, // driver pin (if any) is always placed first at net_pin_offsets[net]
}

impl DesignDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn num_cells(&self) -> usize {
        self.cell_names.len()
    }

    pub fn num_pins(&self) -> usize {
        self.pin_names.len()
    }

    pub fn num_nets(&self) -> usize {
        self.net_names.len()
    }

    pub fn num_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn cell_pins(&self, cell: CellId) -> &[PinId] {
        let idx = cell.0 as usize;
        let start = self.cell_pin_offsets[idx] as usize;
        let end = self.cell_pin_offsets[idx + 1] as usize;
        &self.cell_pin_ids[start..end]
    }

    pub fn net_pins(&self, net: NetId) -> &[PinId] {
        let idx = net.0 as usize;
        let start = self.net_pin_offsets[idx] as usize;
        let end = self.net_pin_offsets[idx + 1] as usize;
        &self.net_pin_ids[start..end]
    }

    pub fn net_driver(&self, net: NetId) -> Option<PinId> {
        let pins = self.net_pins(net);
        pins.first()
            .copied()
            .filter(|&first_pin| self.pin_is_driver[first_pin.0 as usize])
    }
}
