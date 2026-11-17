use crate::interner::{LibCellId, LibPinId, PinDir, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingSense {
    PositiveUnate,
    NegativeUnate,
    #[default]
    NonUnate,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingType {
    #[default]
    Combinational,
    CombinationalRise,
    CombinationalFall,
    RisingEdge,  // CLK -> Q (rising clock)
    FallingEdge, // CLK -> Q (falling clock)
    SetupRising, // D vs rising CLK
    HoldRising,  // D vs rising CLK
    SetupFalling,
    HoldFalling,
    Preset,
    Clear,
    RemovalRising,
    RecoveryRising,
    RemovalFalling,
    RecoveryFalling,
    Other,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TableAxisVar {
    InputNetTransition,
    TotalOutputNetCapacitance,
    ConstrainedPinTransition,
    RelatedPinTransition,
    #[default]
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NldmTable {
    pub var1: TableAxisVar,
    pub var2: TableAxisVar,
    pub index_1: Vec<f32>, // length M
    pub index_2: Vec<f32>, // length N
    pub values: Vec<f32>,  // row-major length M * N: values[i * N + j]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibTimingArc {
    pub related_pin: LibPinId,
    pub target_pin: LibPinId,
    pub timing_sense: TimingSense,
    pub timing_type: TimingType,
    pub cell_rise: Option<NldmTable>,
    pub cell_fall: Option<NldmTable>,
    pub rise_transition: Option<NldmTable>,
    pub fall_transition: Option<NldmTable>,
    pub rise_constraint: Option<NldmTable>,
    pub fall_constraint: Option<NldmTable>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibPin {
    pub name: SymbolId,
    pub dir: PinDir,
    pub cap_rise: f32, // pF
    pub cap_fall: f32, // pF
    pub cap_nom: f32,  // pF
    pub is_clock: bool,
    pub max_cap: Option<f32>,
    pub max_transition: Option<f32>,
    /// Boolean expression of an output pin (`function : "(A&B)"`), interned. `None` when
    /// the Liberty file does not declare one; clock-gate classification needs it because
    /// timing arcs alone cannot tell AND from OR.
    pub function: Option<SymbolId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibCell {
    pub name: SymbolId,
    pub area: f32,
    pub is_sequential: bool, // true if ff() block present
    pub is_latch: bool,      // true if latch() block present
    pub pin_offset: u32,
    pub pin_count: u16,
    pub arc_offset: u32,
    pub arc_count: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibDb {
    pub library_name: String,
    pub time_unit_ns: f32, // multiplier to convert file time -> ns
    pub cap_unit_pf: f32,  // multiplier to convert file cap -> pF
    pub strings: crate::interner::StringInterner,
    pub cells: Vec<LibCell>,
    pub pins: Vec<LibPin>,
    pub arcs: Vec<LibTimingArc>,
    #[serde(skip)]
    pub cell_by_name: HashMap<SymbolId, LibCellId>,
}

impl Default for LibDb {
    fn default() -> Self {
        Self {
            library_name: String::new(),
            // Unity multipliers: a table value is already in the file's own unit
            // until `time_unit`/`capacitive_load_unit` rescales it.
            time_unit_ns: 1.0,
            cap_unit_pf: 1.0,
            strings: crate::interner::StringInterner::new(),
            cells: Vec::new(),
            pins: Vec::new(),
            arcs: Vec::new(),
            cell_by_name: HashMap::new(),
        }
    }
}

impl LibDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild_cell_by_name(&mut self) {
        self.cell_by_name.clear();
        self.cell_by_name.reserve(self.cells.len());
        for (i, cell) in self.cells.iter().enumerate() {
            self.cell_by_name.insert(cell.name, LibCellId(i as u16));
        }
    }

    pub fn cell_pins(&self, cell: &LibCell) -> &[LibPin] {
        let start = cell.pin_offset as usize;
        let end = start + cell.pin_count as usize;
        &self.pins[start..end]
    }

    pub fn cell_arcs(&self, cell: &LibCell) -> &[LibTimingArc] {
        let start = cell.arc_offset as usize;
        let end = start + cell.arc_count as usize;
        &self.arcs[start..end]
    }
}
