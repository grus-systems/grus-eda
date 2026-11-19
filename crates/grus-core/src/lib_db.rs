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
    /// Minimum clock-pulse width at the related pin (`min_pulse_width`): an
    /// intra-cell check, never a data-propagation arc. Evaluated as an STA
    /// endpoint family since v0.1.93 — see `TimingGraph::pulse_width_checks`.
    MinPulseWidth,
    /// Setup/hold checks on non-sequential pins (`non_seq_*`): like the D-pin
    /// checks but without a `CLK -> Q` launch, deferred from the endpoint model.
    NonSeqSetupRising,
    NonSeqSetupFalling,
    NonSeqHoldRising,
    NonSeqHoldFalling,
    /// Tristate/bus-hold timing (`three_state_enable`/`three_state_disable`).
    ThreeStateEnable,
    ThreeStateDisable,
    /// Value-preservation checks (`preserve`).
    Preserve,
    /// Any `timing_type` string outside the recognised set.
    Other,
}

impl TimingType {
    /// Liberty `timing_type` names that are recognised check arcs: they constrain a
    /// pin (non-sequential setup/hold, tristate, preserve, async clear/preset)
    /// rather than propagate data, so the endpoint model defers them into
    /// `SkipReport::recognized_check_arcs` instead of dropping them as unknown.
    /// `min_pulse_width` left this list when it became an evaluated endpoint
    /// family in v0.1.93.
    pub fn is_deferred_check(self) -> bool {
        matches!(
            self,
            Self::NonSeqSetupRising
                | Self::NonSeqSetupFalling
                | Self::NonSeqHoldRising
                | Self::NonSeqHoldFalling
                | Self::ThreeStateEnable
                | Self::ThreeStateDisable
                | Self::Preserve
                | Self::Clear
                | Self::Preset
        )
    }
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
    /// `internal_power` blocks: energy tables evaluated on pin transitions.
    pub internal_power: Vec<LibPinInternalPower>,
}

/// One `internal_power` block inside a pin. `related_pin` names the cell-local pin
/// whose transition triggers the power (`None` = the pin's own transition, the
/// input-pin self-power form). `rise`/`fall` select the trigger edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibPinInternalPower {
    pub related_pin: Option<SymbolId>,
    /// Raw `when` state expression (only ebu*/einv* cells use it); `None` = unconditional.
    pub when: Option<SymbolId>,
    pub rise: Option<NldmTable>,
    pub fall: Option<NldmTable>,
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
    /// `cell_leakage_power` in watts: the Liberty-provided average over all
    /// `leakage_power` `when` states. `None` when the cell declares neither.
    pub leakage_power_w: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibDb {
    pub library_name: String,
    pub time_unit_ns: f32, // multiplier to convert file time -> ns
    pub cap_unit_pf: f32,  // multiplier to convert file cap -> pF
    /// Multiplier to convert file leakage power values -> W.
    pub leakage_power_unit_w: f32,
    /// `default_cell_leakage_power` converted to W; used when a cell declares none.
    pub default_cell_leakage_power_w: f32,
    /// `nom_voltage` in volts; 0 when undeclared (power estimation reports it).
    pub nominal_voltage_v: f32,
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
            leakage_power_unit_w: 1e-9, // 1 nW; Liberty overrides via leakage_power_unit
            default_cell_leakage_power_w: 0.0,
            nominal_voltage_v: 0.0,
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
