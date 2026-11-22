use crate::interner::{
    CellId, LayerId, LibCellId, LibPinId, MacroId, MacroPinId, NetId, PinDir, PinId,
    StringInterner, SymbolId,
};
use crate::lib_db::LibDb;
use crate::tech_db::{RectDbu, TechDb};
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
    pub pin_io_layer: Vec<Option<LayerId>>, // DEF `+ LAYER` of primary I/O pins; None for instance pins
    /// Absolute `(layer, rect)` shapes of primary I/O ports, in PINS record
    /// order. Guide parasitics reproduce OpenROAD's bterm pin boxes from
    /// these; instance pins stay empty (their shapes live in `MacroPin`).
    pub pin_io_rects: Vec<Vec<(LayerId, RectDbu)>>,
    pub pin_is_driver: Vec<bool>,
    pub io_pins: Vec<PinId>, // indices of primary I/O ports (where pin_cells[p] == None)

    // --- Net SoA & CSR (indexed by NetId) ---
    pub net_names: Vec<SymbolId>,
    /// Raw NETS-section spelling per net (escape characters kept). OpenDB
    /// stores net names verbatim, so route-guide emission must reproduce
    /// this token for `read_guides` to match nets. Empty for non-DEF
    /// sources; then guide emission falls back to escaping `net_names`.
    pub net_def_spellings: Vec<SymbolId>,
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

    /// Finds the design pin of `cell` whose Liberty pin name is `sym` —
    /// Liberty names pins by `function`/`related_pin`/`when` text, not by
    /// the netlist's pin names.
    pub(crate) fn pin_by_lib_name(
        &self,
        lib: &LibDb,
        cell: CellId,
        sym: SymbolId,
    ) -> Option<PinId> {
        self.cell_pins(cell).iter().copied().find(|&p| {
            self.pin_lib_pins[p.0 as usize]
                .map(|lp| lib.pins[lp.0 as usize].name == sym)
                .unwrap_or(false)
        })
    }

    /// Computes the absolute coordinates (x, y) in DBU for a given pin under its cell's placement and orientation.
    pub fn pin_pos_dbu(&self, pin: PinId, tech_db: &TechDb) -> (i32, i32) {
        let p = pin.0 as usize;
        match self.pin_cells[p] {
            None => {
                // Primary I/O pin: absolute coordinates
                (self.pin_offset_x_dbu[p], self.pin_offset_y_dbu[p])
            }
            Some(cell_id) => {
                let c = cell_id.0 as usize;
                let cx = self.cell_x_dbu[c];
                let cy = self.cell_y_dbu[c];
                let orient = self.cell_orient[c];
                let macro_id = self.cell_macros[c];
                let mac = &tech_db.macros[macro_id.0 as usize];
                let w = mac.width_dbu;
                let h = mac.height_dbu;
                let ox = self.pin_offset_x_dbu[p];
                let oy = self.pin_offset_y_dbu[p];
                let (fx, fy) = match self.pin_macro_pins[p] {
                    Some(mp) => {
                        let mpin = &tech_db.macro_pins[mp.0 as usize];
                        (mpin.offset_x_frac, mpin.offset_y_frac)
                    }
                    None => (false, false),
                };
                let x = |base, off, frac| truncated_axis(base, off, frac, false);
                let mx = |base, off, frac| truncated_axis(base, off, frac, true);

                match orient {
                    Orient::N => (x(cx, ox, fx), x(cy, oy, fy)),
                    Orient::S => (mx(cx + w, ox, fx), mx(cy + h, oy, fy)),
                    Orient::W => (mx(cx + h, oy, fy), x(cy, ox, fx)),
                    Orient::E => (x(cx, oy, fy), mx(cy + w, ox, fx)),
                    Orient::FN => (mx(cx + w, ox, fx), x(cy, oy, fy)),
                    Orient::FS => (x(cx, ox, fx), mx(cy + h, oy, fy)),
                    Orient::FW => (x(cx, oy, fy), x(cy, ox, fx)),
                    Orient::FE => (mx(cx + h, oy, fy), mx(cy + w, ox, fx)),
                }
            }
        }
    }

    /// Computes the total Half-Perimeter Wirelength (HPWL) in DBU across all signal nets.
    pub fn compute_hpwl_dbu(&self, tech_db: &TechDb) -> i64 {
        let mut total_hpwl: i64 = 0;
        for net_idx in 0..self.num_nets() {
            let nid = NetId(net_idx as u32);
            let pins = self.net_pins(nid);
            if pins.len() <= 1 {
                continue;
            }
            let mut min_x = i32::MAX;
            let mut max_x = i32::MIN;
            let mut min_y = i32::MAX;
            let mut max_y = i32::MIN;

            for &pin in pins {
                let (px, py) = self.pin_pos_dbu(pin, tech_db);
                min_x = min_x.min(px);
                max_x = max_x.max(px);
                min_y = min_y.min(py);
                max_y = max_y.max(py);
            }

            if max_x >= min_x && max_y >= min_y {
                let dx = (max_x - min_x) as i64;
                let dy = (max_y - min_y) as i64;
                total_hpwl += dx + dy;
            }
        }
        total_hpwl
    }
}

/// One axis of an instance pin position, as OpenROAD's `dbITerm::getAvgXY` gives it:
/// the exact value is `base + c` (or `base - c` when `mirrored`), where the local mean
/// center `c` lies in `[off, off + 1)` and is fractional iff `frac`. OpenROAD truncates
/// toward zero, so the result is whichever of `floor` and `floor + 1` is nearer zero.
fn truncated_axis(base: i32, off: i32, frac: bool, mirrored: bool) -> i32 {
    if !frac {
        return if mirrored { base - off } else { base + off };
    }
    let floor = if mirrored { base - off - 1 } else { base + off };
    if floor < 0 { floor + 1 } else { floor }
}
