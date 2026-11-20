use super::dmp::{PiModel, driver_waveform_slew, solve_dmp};
use super::nldm::{eval_nldm_constraint, eval_nldm_delay_or_slew};
use super::parasitics::NetParasitics;
use super::sdc::{
    CellSel, Clock, DERATE_CELL, DERATE_CHECK, DERATE_NET, ExceptionKind, ObjSel, PathDelayKind,
    Sdc,
};
use crate::design_db::DesignDb;
use crate::interner::{CellId, LibPinId, NetId, PinDir, PinId};
use crate::lib_db::{LibCell, LibDb, LibPin, NldmTable, TimingSense, TimingType};
use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const RISE: usize = 0;
pub const FALL: usize = 1;

/// Converts the table's delay-vs-load slope into the driver resistance of the DMP ramp model.
/// Fitted (`3.01 x delay_slope`, CV 12% across shielded `picorv32` arcs; `docs/history/e35/01-rd-and-wave-slew.md`
/// Section 5) and paired with `driver_waveform_slew`; the single-pole value `1/ln 2` is much worse.
const RD_SCALE: f64 = 3.0;

const WIRE_SLEW_FACTOR: f32 = std::f32::consts::LN_2 * 2.0;

/// Combinational arc between two pins of one cell instance.
#[derive(Clone, Copy, Debug)]
struct ArcEdge {
    src: PinId,
    dst: PinId,
    lib_arc: u32,
}

/// `CLK -> Q` arc of a flip-flop, the launch of every register-to-register path.
/// `edge` is the clock transition that launches Q (RISE for `rising_edge`, FALL for
/// `falling_edge` cells such as `dfrtn`).
#[derive(Clone, Copy, Debug)]
struct LaunchArc {
    clk: PinId,
    q: PinId,
    lib_arc: u32,
    edge: usize,
}

/// Enable-to-output arc of a latch: the output launches on the opening clock edge
/// (FALL for `dlxtn`-style `GATE_N` enables, RISE for `dlxtp` `GATE`). Data already
/// waiting when the latch opens propagates through this clock-to-Q arc.
#[derive(Clone, Copy, Debug)]
struct LatchOpen {
    enable: PinId,
    q: PinId,
    lib_arc: u32,
    open_pol: usize,
}

/// A transparent latch's borrowing window, keyed to `D -> Q` combinational arcs
/// through `latch_borrow`. `open_pol` is the clock transition the latch opens on —
/// the opposite of its setup check's clock edge.
#[derive(Clone, Copy, Debug)]
struct LatchInfo {
    enable: PinId,
    open_pol: usize,
    /// Setup arc of the latch's data pin: the borrow cap is the closing edge minus
    /// this arc's per-transition constraint.
    setup_arc: u32,
}

/// Timing checks of one constrained pin. D pins carry setup and hold; asynchronous set/reset pins
/// carry recovery and removal, which take the same role (late and early bound) against the same clock.
/// `latch` marks a latch D pin, whose setup arc closes the latch (see `collect_endpoints`).
/// `edge` is the clock transition the check fires on: RISE for `*Rising` check types, FALL for
/// `*Falling` (inverting-clock flip-flops such as `dfrtn`).
#[derive(Clone, Copy, Debug, Default)]
struct RegisterCheck {
    clk: Option<PinId>,
    latch: bool,
    edge: usize,
    setup_arc: Option<u32>,
    hold_arc: Option<u32>,
    recovery_arc: Option<u32>,
    removal_arc: Option<u32>,
}

impl RegisterCheck {
    fn late_arc(&self) -> Option<u32> {
        self.setup_arc.or(self.recovery_arc)
    }

    fn early_arc(&self) -> Option<u32> {
        self.hold_arc.or(self.removal_arc)
    }
}

#[derive(Clone, Copy, Debug)]
struct PinTimingState {
    arrival_max: [f32; 2],
    arrival_min: [f32; 2],
    /// Arrival extremes referenced to the delay edge (`set_max_delay`/
    /// `set_min_delay` checks report these as their data arrival). Identical
    /// to `arrival_*` for port and flop launches; a launch through a latch's
    /// open edge re-bases to the delay edge by stripping the opening-edge
    /// clock arrival (measured: `delay_allregs`'s `u_f0/D` reports the
    /// clock-to-Q delay, not the open-edge wait). Merged independently of
    /// which source wins `arrival_*`, the same discipline as `slew_*`.
    dly_max: [f32; 2],
    dly_min: [f32; 2],
    slew_max: [f32; 2],
    slew_min: [f32; 2],
    /// Bitmask of the launch-edge parities (bit `p` = clock-edge index parity `p`)
    /// that produced `arrival_max` / `arrival_min` at this pin and transition; 0 while
    /// unreached. Setup/hold required times shift by a period depending on the launch
    /// parity, so the extremes must keep every parity that ties them: equal arrivals
    /// from different clock-edge parities keep both bits and the endpoint resolves the
    /// worst slack across them.
    par_max: [u8; 2],
    par_min: [u8; 2],
    /// Bitmask of the CRPR launch tags (bits into `StaPrepared::crpr_tags`) that
    /// produced the corresponding arrival extreme; same winner-replace/tie-union
    /// discipline as `par_*`. OpenSTA credits a path the launch/capture clock
    /// spread at their last shared point, so the credit must know which launch
    /// won each extreme — the merged state alone does not remember.
    crpr_max: [u8; 2],
    crpr_min: [u8; 2],
    /// Bitmask of the launch clock-domain tags (`Sdc::clocks` row indices) that
    /// produced `arrival_max` / `arrival_min` at this pin and transition; same
    /// winner-replace/tie-union discipline as `par_*`. An endpoint check may be
    /// evaluated only when the mask is exactly the capture domain's bit —
    /// anything else is a cross-domain contribution mc-xdomain must pair.
    dom_max: [u8; 2],
    dom_min: [u8; 2],
}

impl PinTimingState {
    const UNREACHED: Self = Self {
        arrival_max: [f32::NEG_INFINITY; 2],
        arrival_min: [f32::INFINITY; 2],
        dly_max: [f32::NEG_INFINITY; 2],
        dly_min: [f32::INFINITY; 2],
        slew_max: [f32::NEG_INFINITY; 2],
        slew_min: [f32::INFINITY; 2],
        par_max: [0; 2],
        par_min: [0; 2],
        crpr_max: [0; 2],
        crpr_min: [0; 2],
        dom_max: [0; 2],
        dom_min: [0; 2],
    };

    fn reached(&self, pol: usize) -> bool {
        self.arrival_max[pol] > f32::NEG_INFINITY
    }

    #[allow(clippy::too_many_arguments)]
    fn merge(
        &mut self,
        pol: usize,
        arr_max: f32,
        arr_min: f32,
        dly_max: f32,
        dly_min: f32,
        slew_max: f32,
        slew_min: f32,
        par_max: u8,
        par_min: u8,
        crpr_max: u8,
        crpr_min: u8,
        dom_max: u8,
        dom_min: u8,
    ) {
        // Slews are merged independently of which arc wins the arrival (OpenSTA GraphDelayCalc).
        // The parity and tag arguments are masks: a strictly better arrival replaces
        // the mask, an equal one unions it so endpoint checks see every tying launch.
        if arr_max > self.arrival_max[pol] {
            self.arrival_max[pol] = arr_max;
            self.par_max[pol] = par_max;
            self.crpr_max[pol] = crpr_max;
            self.dom_max[pol] = dom_max;
        } else if arr_max == self.arrival_max[pol] {
            self.par_max[pol] |= par_max;
            self.crpr_max[pol] |= crpr_max;
            self.dom_max[pol] |= dom_max;
        }
        if arr_min < self.arrival_min[pol] {
            self.arrival_min[pol] = arr_min;
            self.par_min[pol] = par_min;
            self.crpr_min[pol] = crpr_min;
            self.dom_min[pol] = dom_min;
        } else if arr_min == self.arrival_min[pol] {
            self.par_min[pol] |= par_min;
            self.crpr_min[pol] |= crpr_min;
            self.dom_min[pol] |= dom_min;
        }
        self.dly_max[pol] = self.dly_max[pol].max(dly_max);
        self.dly_min[pol] = self.dly_min[pol].min(dly_min);
        self.slew_max[pol] = self.slew_max[pol].max(slew_max);
        self.slew_min[pol] = self.slew_min[pol].min(slew_min);
    }

    /// Element-wise worst-of merge across exception-signature runs: the union
    /// of per-class arrivals is the unfiltered arrival, and slews are identical
    /// across runs because every source merges them unfiltered.
    fn merge_all(&mut self, o: &Self) {
        for pol in [RISE, FALL] {
            self.arrival_max[pol] = self.arrival_max[pol].max(o.arrival_max[pol]);
            self.arrival_min[pol] = self.arrival_min[pol].min(o.arrival_min[pol]);
            self.dly_max[pol] = self.dly_max[pol].max(o.dly_max[pol]);
            self.dly_min[pol] = self.dly_min[pol].min(o.dly_min[pol]);
            self.slew_max[pol] = self.slew_max[pol].max(o.slew_max[pol]);
            self.slew_min[pol] = self.slew_min[pol].min(o.slew_min[pol]);
            self.par_max[pol] |= o.par_max[pol];
            self.par_min[pol] |= o.par_min[pol];
            self.crpr_max[pol] |= o.crpr_max[pol];
            self.crpr_min[pol] |= o.crpr_min[pol];
            self.dom_max[pol] |= o.dom_max[pol];
            self.dom_min[pol] |= o.dom_min[pol];
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EndpointReport {
    pub pin: String,
    pub max_arr_ns: f32,
    pub max_req_ns: f32,
    pub max_slack_ns: f32,
    pub max_slew_ns: f32,
    pub min_arr_ns: f32,
    pub min_req_ns: f32,
    pub min_slack_ns: f32,
    pub min_slew_ns: f32,
}

/// What kind of timing endpoint an [`SkippedEndpoint`] was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// Register data or asynchronous set/reset check.
    Check,
    /// Clock-gate enable check.
    ClockGate,
    /// Top-level output port.
    OutputPort,
    /// `min_pulse_width` check on a clock pin.
    MinPulseWidth,
}

/// Which half of the clock period a `min_pulse_width` check bounds. The names
/// mirror OpenSTA's `(high)`/`(low)` row suffix in `report_pulse_width_checks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseEdge {
    /// High pulse: the width between the rising and the next falling arrival.
    High,
    /// Low pulse: the width between the falling and the next rising arrival.
    Low,
}

/// One evaluated `min_pulse_width` check — a separate report family from
/// `endpoints`, matching OpenSTA where pulse checks never enter
/// `find_timing_paths`, `worst_slack`, or `total_negative_slack` (measured:
/// `viol.sdc` reports 9 violated rows while `tns_min` stays 0.0).
#[derive(Clone, Debug, Serialize)]
pub struct PulseWidthCheckReport {
    pub pin: String,
    pub edge: PulseEdge,
    /// Liberty constraint-table bound for this pulse direction.
    pub required_ns: f32,
    /// Measured pulse width: the smaller of the early and late arrival-pair
    /// widths (OpenSTA picks the narrower pulse).
    pub actual_ns: f32,
    /// `actual_ns - required_ns` (negative = violated), the report column.
    pub slack_ns: f32,
}

/// Why an endpoint candidate produced no check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The check lacks a clock pin or a late/early arc pair.
    IncompleteCheck,
    /// The check's or clock gate's clock pin never received a clock edge
    /// (propagated clock only). `related` names the clock pin.
    NoClock,
    /// The endpoint pin itself never received a data arrival. `related` names the
    /// net driver to inspect next, or is `None` when the net has no driver.
    NoPath,
    /// The endpoint's net driver is a cell that produces no timed arrival: either it
    /// has no Liberty mapping at all, or its Liberty cell declares no timing arcs
    /// (constants such as `conb`, taps, fillers). `related` names the driver. This is
    /// the benign `NoPath` subcase behind most real-design unreached endpoints.
    UntimedDriver,
    /// The endpoint pin was reached but none of its arrived transitions had a
    /// usable NLDM constraint table, so no setup/hold bound could be evaluated.
    NoConstraintTable,
    /// An arrival extreme tying this endpoint was launched on a different
    /// clock domain than the capture. Cross-domain launch/capture pairing is
    /// not implemented yet (mc-xdomain owns the edge-pair search), so the
    /// endpoint is deferred with the launch and capture clocks named in
    /// `related` instead of being folded onto the capture domain's lattice.
    CrossDomain,
}

/// One endpoint candidate that produced no check, with the pin and the reason.
/// `skipped_endpoints` holds one entry per `incomplete_checks` +
/// `unreached_endpoints` + `cross_domain_endpoints` count.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedEndpoint {
    /// Endpoint pin name (`inst/pin` for cell pins, port name for top-level pins).
    pub pin: String,
    pub kind: EndpointKind,
    pub reason: SkipReason,
    /// Context pin or description: the unreached clock pin for `NoClock`, the net
    /// driver for `NoPath`/`UntimedDriver`, the missing piece for `IncompleteCheck`.
    pub related: Option<String>,
}

/// Inputs and checks the analysis left out, counted per category, with the totals they are a
/// share of. A non-zero count means the report is missing timing that the design has.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SkipReport {
    /// Instances that have no Liberty cell mapping (`design.cell_lib_ids[c]` is `None`).
    pub unmapped_cells: usize,
    /// Latch instances whose enable pin does not resolve (`latch_enable` finds no
    /// `setup_*` check): every one of their arcs is dropped, counted per `TimingType` in
    /// `latch_arcs_dropped`.
    pub unmodelled_latch_cells: usize,
    /// Arcs that have no model on non-latch cells, by `TimingType` name.
    pub unsupported_arcs: BTreeMap<String, usize>,
    /// Recognised Liberty check-arc types the endpoint model defers: `non_seq_*`,
    /// `three_state_*`, `preserve` and async `clear`/`preset`. They constrain a
    /// pin rather than propagate data, so they add endpoints outside the modelled set
    /// instead of altering arrivals. Counted here and shown in the summary; like the
    /// design rules they are exempt from `--strict` (see `is_clean`), because dropping a
    /// check arc cannot change the arrival or slack of an endpoint the model does report.
    /// `min_pulse_width` used to count here; it is now an evaluated check family
    /// reported under `StaReport::pulse_width_checks`.
    pub recognized_check_arcs: BTreeMap<String, usize>,
    /// Arcs that have no model on latch cells, by `TimingType` name: anything that is
    /// neither a modelled check, the opening edge arc, a `D -> Q` transparent arc nor a
    /// deferred check arc (those count in `recognized_check_arcs` on every cell kind).
    pub latch_arcs_dropped: BTreeMap<String, usize>,
    /// Arcs whose related or target pin does not resolve on the instance.
    pub unresolved_arcs: usize,
    /// Register checks without a clock or without a late/early arc pair.
    pub incomplete_checks: usize,
    /// Checks and outputs that no polarity of a path reached.
    pub unreached_endpoints: usize,
    /// Endpoints whose launch domain differs from their capture domain:
    /// cross-domain edge pairing is mc-xdomain's job, so these are deferred
    /// (named in `skipped_endpoints` with reason `CrossDomain`) rather than
    /// timed on a wrong lattice.
    pub cross_domain_endpoints: usize,
    /// Per-endpoint detail for every `incomplete_checks` + `unreached_endpoints`
    /// skip: which pin produced no check and why.
    pub skipped_endpoints: Vec<SkippedEndpoint>,
    /// Clock nets driven by a cell output (buffered or gated clock). Counted only for nets
    /// the active clock model does not propagate: all of them under the ideal clock, and the
    /// unreached ones under `set_propagated_clock`.
    pub cell_driven_clock_nets: usize,
    /// `set_driving_cell` rules whose cell or pin does not resolve in the Liberty library; the
    /// port falls back to `set_input_transition`.
    pub unresolved_driving_cells: usize,
    /// Arcs of all Liberty-mapped cell instances, the denominator for the arc share. Unmapped
    /// cells have no Liberty arcs to count, so their arcs are outside this total.
    pub total_arcs: usize,
    /// Endpoint candidates (constrained pins, clock-gate enables, output ports and
    /// `min_pulse_width` check rows), the denominator for the endpoint share.
    pub total_endpoints: usize,
    /// SDC options accepted but not applied (`-lib_cell`, `-pin`), by flag.
    pub sdc_ignored_options: BTreeMap<String, usize>,
    /// SDC commands accepted but not applied to the timing model, by command name.
    pub sdc_ignored_commands: BTreeMap<String, usize>,
    /// Top-level SDC commands evaluated while parsing, the denominator for `sdc_ignored_options`.
    pub sdc_commands: usize,
    /// Endpoint candidates removed entirely by `set_false_path` (every
    /// signature class covering them was dropped). Exempt from `is_clean`
    /// like design rules: the exception is applied, not skipped.
    pub false_path_endpoints: usize,
    /// Endpoint candidates an applicable `set_multicycle_path` could not
    /// shift because their kind has no rigid required-time model (latch-D
    /// checks and clock-gate enables). They keep their unshifted slack.
    pub unsupported_multicycle_endpoints: usize,
    /// Endpoint candidates a covering `set_max_delay`/`set_min_delay` could
    /// not replace because the endpoint kind is outside the supported set
    /// (output ports, clock-gate enables, latch-D and async recovery/removal
    /// checks). They keep their ordinary clock-check values — loudly partial.
    pub unsupported_path_delay_endpoints: usize,
    /// `-from`/`-to` selector sides that resolved to no launch source or no
    /// endpoint, plus bare selector names matching nothing.
    pub unresolved_exception_selectors: usize,
    /// `set_load` `get_nets` names that do not resolve in the design (OpenSTA
    /// warns per missing name and applies the rest — `ld_badnet`), plus net
    /// annotations the parasitic path cannot honour: the measured net
    /// annotation replaces the net's wire capacitance, which the distributed
    /// RC model does not represent, so under `NetParasitics` every selected
    /// net counts here instead of being approximated.
    pub unresolved_load_selectors: usize,
}

impl SkipReport {
    /// Each non-zero category as `name=count`; arc maps are listed by timing type.
    pub fn nonzero(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (name, map) in [
            ("unsupported_arcs", &self.unsupported_arcs),
            ("recognized_check_arcs", &self.recognized_check_arcs),
            ("latch_arcs_dropped", &self.latch_arcs_dropped),
            ("sdc_ignored_options", &self.sdc_ignored_options),
            ("sdc_ignored_commands", &self.sdc_ignored_commands),
        ] {
            if !map.is_empty() {
                let by_kind: Vec<String> = map.iter().map(|(t, n)| format!("{t}={n}")).collect();
                out.push(format!("{name}[{}]", by_kind.join(", ")));
            }
        }
        for (name, count) in [
            ("unmapped_cells", self.unmapped_cells),
            ("unmodelled_latch_cells", self.unmodelled_latch_cells),
            ("unresolved_arcs", self.unresolved_arcs),
            ("incomplete_checks", self.incomplete_checks),
            ("unreached_endpoints", self.unreached_endpoints),
            ("cross_domain_endpoints", self.cross_domain_endpoints),
            ("cell_driven_clock_nets", self.cell_driven_clock_nets),
            ("unresolved_driving_cells", self.unresolved_driving_cells),
            (
                "unsupported_multicycle_endpoints",
                self.unsupported_multicycle_endpoints,
            ),
            (
                "unsupported_path_delay_endpoints",
                self.unsupported_path_delay_endpoints,
            ),
            (
                "unresolved_exception_selectors",
                self.unresolved_exception_selectors,
            ),
            (
                "unresolved_load_selectors",
                self.unresolved_load_selectors,
            ),
        ] {
            if count > 0 {
                out.push(format!("{name}={count}"));
            }
        }
        out
    }

    /// Arcs that never entered the timing graph: unresolved pins, unmodelled or
    /// deferred-from-endpoint timing, and latch arcs. Deferred check arcs count here
    /// even though `is_clean` exempts them, so the skipped share stays honest.
    pub fn arcs_skipped(&self) -> usize {
        self.unresolved_arcs
            + self.unsupported_arcs.values().sum::<usize>()
            + self.recognized_check_arcs.values().sum::<usize>()
            + self.latch_arcs_dropped.values().sum::<usize>()
    }

    /// Endpoint candidates that produced no check: incomplete pairs, unreached
    /// pins, or cross-domain launch/capture pairs that defer to mc-xdomain.
    pub fn endpoints_skipped(&self) -> usize {
        self.incomplete_checks + self.unreached_endpoints + self.cross_domain_endpoints
    }

    /// True when the run left nothing out. Two kinds of skip are exempt because they cannot
    /// change a compared number: SDC design-rule commands (`set_max_*`) and
    /// `set_fanout_load` — OpenSTA accepts it but warns it is unsupported (STA-0461)
    /// and applies nothing, so ignoring it reproduces the reference — and
    /// `recognized_check_arcs`, recognised Liberty check arcs deferred from
    /// the endpoint model — they add checks outside the reported endpoint set rather than
    /// altering an arrival or slack the report does produce. All stay visible in the
    /// summary; `--strict` only fails on skips that can make a reported number wrong.
    pub fn is_clean(&self) -> bool {
        const DESIGN_RULES: [&str; 4] = [
            "set_max_transition",
            "set_max_capacitance",
            "set_max_fanout",
            "set_fanout_load",
        ];
        self.unmapped_cells == 0
            && self.unmodelled_latch_cells == 0
            && self.unresolved_arcs == 0
            && self.incomplete_checks == 0
            && self.unreached_endpoints == 0
            && self.cross_domain_endpoints == 0
            && self.cell_driven_clock_nets == 0
            && self.unresolved_driving_cells == 0
            && self.unsupported_multicycle_endpoints == 0
            && self.unsupported_path_delay_endpoints == 0
            && self.unresolved_exception_selectors == 0
            && self.unresolved_load_selectors == 0
            && self.unsupported_arcs.is_empty()
            && self.latch_arcs_dropped.is_empty()
            && self.sdc_ignored_options.is_empty()
            && self
                .sdc_ignored_commands
                .keys()
                .all(|k| DESIGN_RULES.contains(&k.as_str()))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StaReport {
    pub wns_max_ns: f32,
    pub tns_max_ns: f32,
    pub wns_min_ns: f32,
    pub tns_min_ns: f32,
    /// Endpoints whose worst setup (max) slack is negative.
    pub setup_violations: u32,
    /// Endpoints whose worst hold (min) slack is negative.
    pub hold_violations: u32,
    pub endpoints: Vec<EndpointReport>,
    /// Evaluated `min_pulse_width` checks, one row per (check pin, pulse
    /// direction with a constraint table). Sorted by pin name, `High` first.
    pub pulse_width_checks: Vec<PulseWidthCheckReport>,
    pub skipped: SkipReport,
}

struct Context<'a> {
    design: &'a DesignDb,
    lib: &'a LibDb,
    sdc: &'a Sdc,
    parasitics: Option<&'a NetParasitics>,
    /// Driving-point pi model of every net with parasitics, per output transition.
    pi_models: Vec<Option<[PiModel; 2]>>,
    /// `set_load` wire-capacitance override per net (measured OpenSTA
    /// semantics: the annotation *replaces* the net's wire capacitance,
    /// last-wins per net, additive to pin capacitance). Empty under
    /// `NetParasitics` — the override cannot be injected into the distributed
    /// RC model, so those nets count as `unresolved_load_selectors` instead.
    net_wire_loads: Vec<Option<f32>>,
}

impl Context<'_> {
    fn pin_name(&self, pin: PinId) -> String {
        let p = pin.0 as usize;
        let name = self.design.strings.resolve(self.design.pin_names[p]);
        match self.design.pin_cells[p] {
            Some(cell) => {
                let cell_name = self
                    .design
                    .strings
                    .resolve(self.design.cell_names[cell.0 as usize]);
                format!("{cell_name}/{name}")
            }
            None => name.to_string(),
        }
    }

    fn port_name(&self, pin: PinId) -> &str {
        self.design
            .strings
            .resolve(self.design.pin_names[pin.0 as usize])
    }

    /// Capacitance of one pin of a net for one transition. Zero-wire mode has no wire capacitance,
    /// only sink pins and port loads.
    fn sink_cap(&self, pin: PinId, pol: usize) -> f32 {
        let p = pin.0 as usize;
        if self.design.pin_is_driver[p] {
            return 0.0;
        }
        match (self.design.pin_cells[p], self.design.pin_lib_pins[p]) {
            (None, _) => self.sdc.output_load_pf(self.port_name(pin)),
            (Some(_), Some(lp)) => {
                let lib_pin = &self.lib.pins[lp.0 as usize];
                if pol == RISE {
                    lib_pin.cap_rise
                } else {
                    lib_pin.cap_fall
                }
            }
            (Some(_), None) => 0.0,
        }
    }

    /// Total capacitance on the net driven by an output pin for one output transition.
    fn net_load(&self, net: NetId, pol: usize) -> f32 {
        // A `set_load` net annotation replaces the net's wire capacitance
        // (measured: an SPEF wire cap is overridden, not summed).
        let wire = self.net_wire_loads[net.0 as usize].unwrap_or_else(|| {
            self.parasitics
                .and_then(|par| par.net(net))
                .map_or(0.0, |t| t.total_wire_cap_pf)
        });
        wire + self
            .design
            .net_pins(net)
            .iter()
            .map(|&pin| self.sink_cap(pin, pol))
            .sum::<f32>()
    }

    /// `(slew, delay)` at an input port, per output transition (RISE, FALL). A
    /// `set_driving_cell` rule makes the port behave like the driver cell's output pin: the
    /// worst slew and delay over the cell's combinational arcs into the named pin. Slew is
    /// evaluated at the rule's input transition and the net load; delay uses the source
    /// transition's table at zero load — for an inverting driver the output rise is the
    /// cell's fall delay, and the ideal driver does not see the net's load. This measured
    /// closest to OpenSTA (worst residual ~0.7 ps on `inv_2`, ~10 ps on `inv_16`). Without a
    /// rule, `set_input_transition` applies and the delay is zero.
    /// `(slew_max, slew_min, early_delay, late_delay)` per output transition. The delay is
    /// the worst arc delay derated by its **source** transition — `-rise`/`-fall` derates
    /// select the transition entering the arc, which differs from the output transition on
    /// inverting drivers (measured on OpenSTA). Slew extremes are taken across source
    /// transitions independently of the delay pairing.
    fn port_drive(&self, pin: PinId, skipped: &mut SkipReport) -> [(f32, f32, f32, f32); 2] {
        let port = self.port_name(pin);
        let fallback = || {
            let t = self.sdc.input_transition_ns(port);
            [(t, t, 0.0, 0.0), (t, t, 0.0, 0.0)]
        };
        let Some(rule) = self.sdc.driving_cell(port, true) else {
            return fallback();
        };
        let resolved = (|| {
            let cell_sym = self.lib.strings.get(&rule.cell)?;
            let cell_id = *self.lib.cell_by_name.get(&cell_sym)?;
            let cell = &self.lib.cells[cell_id.0 as usize];
            let pin_sym = self.lib.strings.get(&rule.pin)?;
            let pin_idx = self
                .lib
                .cell_pins(cell)
                .iter()
                .position(|p| p.name == pin_sym)?;
            let target = LibPinId((cell.pin_offset + pin_idx as u32) as u16);
            let net = self.design.pin_nets[pin.0 as usize];
            let mut slew_max = [f32::NEG_INFINITY; 2];
            let mut slew_min = [f32::INFINITY; 2];
            // Worst and best raw delay per (output, source) transition pair; derating is
            // applied per source transition below, before the cross-source extreme. The
            // early delay takes the minimum: `arrival_min` must not be inflated by a
            // slower sibling arc (non-unate or multi-input driving cells).
            let mut delay_max = [[f32::NEG_INFINITY; 2]; 2];
            let mut delay_min = [[f32::INFINITY; 2]; 2];
            for i in cell.arc_offset..cell.arc_offset + cell.arc_count as u32 {
                let arc = &self.lib.arcs[i as usize];
                if arc.target_pin != target {
                    continue;
                }
                let related = &self.lib.pins[arc.related_pin.0 as usize];
                if related.dir != PinDir::Input {
                    continue;
                }
                let combinational = matches!(
                    arc.timing_type,
                    TimingType::Combinational
                        | TimingType::CombinationalRise
                        | TimingType::CombinationalFall
                );
                if !combinational {
                    continue;
                }
                for &(src_pol, dst_pol) in polarities(arc.timing_sense) {
                    if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                        || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                    {
                        continue;
                    }
                    let in_slew = if src_pol == RISE {
                        rule.in_trans_rise
                    } else {
                        rule.in_trans_fall
                    };
                    let load = net.map_or(0.0, |n| self.net_load(n, dst_pol));
                    if let Some(table) = delay_tables(arc, dst_pol).1 {
                        let s = eval_nldm_delay_or_slew(table, in_slew, load);
                        slew_max[dst_pol] = slew_max[dst_pol].max(s);
                        slew_min[dst_pol] = slew_min[dst_pol].min(s);
                    }
                    // The driver is ideal: its delay does not see the net's load. The table is
                    // indexed by the source transition — for an inverting driver the output
                    // rise is the cell's fall delay — which measured closest to OpenSTA.
                    if let Some(table) = delay_tables(arc, src_pol).0 {
                        let d = eval_nldm_delay_or_slew(table, in_slew, 0.0);
                        delay_max[dst_pol][src_pol] = delay_max[dst_pol][src_pol].max(d);
                        delay_min[dst_pol][src_pol] = delay_min[dst_pol][src_pol].min(d);
                    }
                }
            }
            let mut out = [(
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ); 2];
            for dst_pol in [RISE, FALL] {
                let mut early = f32::INFINITY;
                let mut late = f32::NEG_INFINITY;
                for src_pol in [RISE, FALL] {
                    if delay_max[dst_pol][src_pol].is_finite() {
                        late = late.max(
                            delay_max[dst_pol][src_pol]
                                * self.sdc.derate(true, DERATE_CELL, false, src_pol),
                        );
                    }
                    if delay_min[dst_pol][src_pol].is_finite() {
                        early = early.min(
                            delay_min[dst_pol][src_pol]
                                * self.sdc.derate(false, DERATE_CELL, false, src_pol),
                        );
                    }
                }
                out[dst_pol] = (slew_max[dst_pol], slew_min[dst_pol], early, late);
            }
            (out[RISE].0.is_finite()
                && out[RISE].1.is_finite()
                && out[RISE].2.is_finite()
                && out[RISE].3.is_finite()
                && out[FALL].0.is_finite()
                && out[FALL].1.is_finite()
                && out[FALL].2.is_finite()
                && out[FALL].3.is_finite())
            .then_some(out)
        })();
        match resolved {
            Some(out) => out,
            None => {
                skipped.unresolved_driving_cells += 1;
                fallback()
            }
        }
    }

    /// Driver delay and output slew for a NLDM arc. With an RC load the table is read at the
    /// effective capacitance of the net's pi model; without one, at the lumped pin and port load.
    fn driver_response(
        &self,
        net: NetId,
        pol: usize,
        delay_table: &NldmTable,
        slew_table: &NldmTable,
        in_slew: f32,
    ) -> (f32, f32) {
        let lumped = |load: f32| {
            (
                eval_nldm_delay_or_slew(delay_table, in_slew, load),
                eval_nldm_delay_or_slew(slew_table, in_slew, load),
            )
        };
        match &self.pi_models[net.0 as usize] {
            Some(pi) => {
                let table = |c: f64| {
                    let (d, s) = lumped(c as f32);
                    (d as f64, s as f64)
                };
                let r = solve_dmp(&pi[pol], RD_SCALE, &table);
                let wave_slew = driver_waveform_slew(&pi[pol], &r, &table);
                (r.delay as f32, wave_slew as f32)
            }
            None => lumped(self.net_load(net, pol)),
        }
    }

    /// Elmore wire delay from the driver to every pin of the net, per transition, in `net_pins` order.
    fn wire_delays(&self, net: NetId) -> Option<[Vec<f32>; 2]> {
        let tree = self.parasitics?.net(net)?;
        let pins = self.design.net_pins(net);
        let delays = |pol| {
            tree.elmore_ns(
                &pins
                    .iter()
                    .map(|&p| self.sink_cap(p, pol))
                    .collect::<Vec<_>>(),
            )
        };
        Some([delays(RISE), delays(FALL)])
    }

    fn cell_pin_for_lib_pin(
        &self,
        cell: CellId,
        lib_pin: crate::interner::LibPinId,
    ) -> Option<PinId> {
        self.design
            .cell_pins(cell)
            .iter()
            .copied()
            .find(|&p| self.design.pin_lib_pins[p.0 as usize] == Some(lib_pin))
    }
}

struct TimingGraph {
    edges: Vec<ArcEdge>,
    /// CSR adjacency: outgoing edge indices of pin `p` are
    /// `edges_from[edges_from_offsets[p]..edges_from_offsets[p + 1]]`.
    edges_from: Vec<u32>,
    edges_from_offsets: Vec<u32>,
    launches: Vec<LaunchArc>,
    latch_opens: Vec<LatchOpen>,
    latch_info: Vec<LatchInfo>,
    /// `(src, dst)` pin ids of latch `D -> Q` combinational arcs, to `latch_info` indices.
    latch_borrow: BTreeMap<(u32, u32), u32>,
    /// `(enable, clock input, kind)` of clock gates on clock nets (see `gating_enables`).
    /// The clock input is kept so a propagated clock can replace the ideal latency in the
    /// enable's required times.
    gating_enables: Vec<GatingEnable>,
    /// Per net: reached by a register clock pin or by a gated clock output. Such nets keep the ideal
    /// clock latency, so data arriving at a clock gate does not propagate onto them.
    clock_net: Vec<bool>,
    /// Register checks sorted by data pin id.
    checks: Vec<(u32, RegisterCheck)>,
    /// `min_pulse_width` arcs on clock pins, evaluated in
    /// [`eval_pulse_width_checks`].
    pulse_width_checks: Vec<PulseWidthCheck>,
}

/// `min_pulse_width` arc on a clock pin: one Liberty arc whose
/// `rise_constraint` bounds the high pulse and `fall_constraint` the low
/// pulse — up to two check rows at `pin` (a missing table emits no row,
/// measured on `dlxtn_1`).
#[derive(Clone, Copy, Debug)]
struct PulseWidthCheck {
    pin: PinId,
    lib_arc: u32,
}

impl TimingGraph {
    /// Indices into `edges` of the combinational arcs leaving `pin`, in scan order.
    fn outgoing(&self, pin: PinId) -> &[u32] {
        let p = pin.0 as usize;
        &self.edges_from
            [self.edges_from_offsets[p] as usize..self.edges_from_offsets[p + 1] as usize]
    }
}

/// Enable pin, opening polarity, and data-setup arc of a latch: the enable is the
/// clock pin of the latch's setup check on D (`dlxtn`: GATE_N, `dlxtp`: GATE). A
/// `setup_rising` check closes the latch at the rising enable edge, so the latch
/// opens on FALL; `setup_falling` latches open on RISE. Latches without a setup
/// check on a pin resolvable on the instance are not modelled.
fn latch_enable(cx: &Context, cell: CellId, lib_cell: &LibCell) -> Option<(PinId, usize, u32)> {
    for i in lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32 {
        let arc = &cx.lib.arcs[i as usize];
        let open_pol = match arc.timing_type {
            TimingType::SetupRising => FALL,
            TimingType::SetupFalling => RISE,
            _ => continue,
        };
        if let Some(pin) = cx.cell_pin_for_lib_pin(cell, arc.related_pin) {
            return Some((pin, open_pol, i));
        }
    }
    None
}

/// Which clock edge a clock-gate enable check is referenced to. OpenSTA 3.0.16 reports
/// AND/NAND gates as "rising clock gating-check end-points" (setup vs the clock rise at
/// the gate input, hold vs the preceding fall) and OR/NOR as "falling" ones with both
/// edges swapped; the distinction is only visible in the Liberty `function` expression,
/// not in the timing arcs (and2/or2 arcs are identical).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum GateKind {
    Rising,
    Falling,
}

/// One clock-gate enable check: `enable` must be stable while `clk_pin` carries the clock.
#[derive(Clone, Copy, Debug)]
struct GatingEnable {
    enable: PinId,
    clk_pin: PinId,
    kind: GateKind,
}

/// Boolean expression of a Liberty `function` attribute, kept only as deep as clock-gate
/// classification needs.
enum FuncExpr<'a> {
    Lit(&'a str, bool),
    And(Vec<FuncExpr<'a>>),
    Or(Vec<FuncExpr<'a>>),
    Not(Box<FuncExpr<'a>>),
}

struct FuncParser<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> FuncParser<'a> {
    fn peek(&mut self) -> Option<u8> {
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        self.b.get(self.pos).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Option<&'a str> {
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let start = self.pos;
        while self.pos < self.b.len() {
            let c = self.b[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'[' || c == b']' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos > start {
            std::str::from_utf8(&self.b[start..self.pos]).ok()
        } else {
            None
        }
    }

    fn parse_expr(&mut self) -> Option<FuncExpr<'a>> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<FuncExpr<'a>> {
        let mut v = vec![self.parse_and()?];
        while self.eat(b'|') {
            v.push(self.parse_and()?);
        }
        Some(if v.len() == 1 {
            v.pop().unwrap()
        } else {
            FuncExpr::Or(v)
        })
    }

    fn parse_and(&mut self) -> Option<FuncExpr<'a>> {
        let mut v = vec![self.parse_unary()?];
        while self.eat(b'&') {
            v.push(self.parse_unary()?);
        }
        Some(if v.len() == 1 {
            v.pop().unwrap()
        } else {
            FuncExpr::And(v)
        })
    }

    fn parse_unary(&mut self) -> Option<FuncExpr<'a>> {
        if self.eat(b'!') {
            Some(FuncExpr::Not(Box::new(self.parse_unary()?)))
        } else if self.eat(b'(') {
            let e = self.parse_or()?;
            if self.eat(b')') { Some(e) } else { None }
        } else {
            self.ident().map(|n| FuncExpr::Lit(n, false))
        }
    }
}

/// Classify a clock-gate output from its Liberty `function`: a single AND or OR of
/// literals covering every input pin, all literals of the same polarity. AND-of-positive
/// and OR-of-negative (NAND) are Rising endpoints; OR-of-positive and AND-of-negative
/// (NOR) are Falling. Whole-expression negation preserves the class, so leading `!`s are
/// peeled. Anything else (muxes, partial functions, mixed polarity) returns `None`.
fn gate_kind(lib: &LibDb, cell: &LibCell, out: &LibPin) -> Option<GateKind> {
    let src = lib.strings.resolve(out.function?);
    let mut p = FuncParser {
        b: src.as_bytes(),
        pos: 0,
    };
    let ast = p.parse_expr()?;
    if p.peek().is_some() {
        return None;
    }
    let mut node = &ast;
    while let FuncExpr::Not(inner) = node {
        node = inner;
    }
    let (is_and, ops) = match node {
        FuncExpr::And(v) => (true, v),
        FuncExpr::Or(v) => (false, v),
        _ => return None,
    };
    let mut neg: Option<bool> = None;
    let mut names = HashSet::new();
    for op in ops {
        // Peel redundant negations (`!!A`) so only the net polarity matters.
        let mut node = op;
        let mut n = false;
        while let FuncExpr::Not(inner) = node {
            n = !n;
            node = inner;
        }
        let &FuncExpr::Lit(name, p) = node else {
            return None;
        };
        let n = n != p;
        match neg {
            None => neg = Some(n),
            Some(prev) if prev == n => {}
            _ => return None,
        }
        if !names.insert(name) {
            return None;
        }
    }
    let inputs: HashSet<&str> = lib
        .cell_pins(cell)
        .iter()
        .filter(|p| p.dir == PinDir::Input)
        .map(|p| lib.strings.resolve(p.name))
        .collect();
    if names.len() != inputs.len() || !names.iter().all(|n| inputs.contains(n)) {
        return None;
    }
    let all_negated = neg.unwrap_or(false);
    Some(if is_and != all_negated {
        GateKind::Rising
    } else {
        GateKind::Falling
    })
}

/// `(enable, clock input, kind)` of clock gates: a combinational output on a clock
/// net whose Liberty `function` is a plain AND/OR/NAND/NOR of its inputs. The input on
/// the clock net is the clock input; every other input with an arc to that output is an
/// enable. Outputs without a `function` attribute fall back to the legacy positive-unate
/// AND assumption (Rising), which cannot distinguish OR/NAND-type cells.
fn gating_enables(cx: &Context, graph: &TimingGraph) -> Vec<GatingEnable> {
    let design = cx.design;
    let has_arc = |lib_cell: &LibCell, from: PinId, to: PinId| {
        let (Some(f), Some(t)) = (
            design.pin_lib_pins[from.0 as usize],
            design.pin_lib_pins[to.0 as usize],
        ) else {
            return false;
        };
        (lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32).any(|i| {
            let arc = &cx.lib.arcs[i as usize];
            arc.related_pin == f && arc.target_pin == t
        })
    };
    let positive_arc = |lib_cell: &LibCell, from: PinId, to: PinId| {
        let (Some(f), Some(t)) = (
            design.pin_lib_pins[from.0 as usize],
            design.pin_lib_pins[to.0 as usize],
        ) else {
            return false;
        };
        (lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32).any(|i| {
            let arc = &cx.lib.arcs[i as usize];
            arc.related_pin == f
                && arc.target_pin == t
                && arc.timing_sense == TimingSense::PositiveUnate
        })
    };
    let on_clock =
        |pin: PinId| design.pin_nets[pin.0 as usize].is_some_and(|n| graph.clock_net[n.0 as usize]);
    let mut enables = Vec::new();
    for c in 0..design.num_cells() {
        let Some(lib_cell_id) = design.cell_lib_ids[c] else {
            continue;
        };
        let lib_cell = &cx.lib.cells[lib_cell_id.0 as usize];
        if lib_cell.is_sequential || lib_cell.is_latch {
            continue;
        }
        let pins = design.cell_pins(CellId(c as u32));
        for &out in pins.iter().filter(|&&p| design.pin_is_driver[p.0 as usize]) {
            if !on_clock(out) {
                continue;
            }
            let inputs = || {
                pins.iter()
                    .copied()
                    .filter(|&p| !design.pin_is_driver[p.0 as usize])
            };
            // `None` = no `function` attribute (legacy AND guess); `Some(None)` = a
            // declared function that does not classify (mux, partial function).
            let kind = design.pin_lib_pins[out.0 as usize].and_then(|i| {
                let lp = &cx.lib.pins[i.0 as usize];
                lp.function.map(|_| gate_kind(cx.lib, lib_cell, lp))
            });
            // `use_positive` keeps the legacy positive-unate requirement for cells
            // without a `function` attribute.
            let (clk_in, kind, use_positive) = match kind {
                // A declared function that does not classify (mux, partial function)
                // means the cell is not a plain clock gate: no enable checks at all.
                Some(None) => continue,
                Some(Some(k)) => {
                    let Some(clk) = inputs().find(|&p| on_clock(p) && has_arc(lib_cell, p, out))
                    else {
                        continue;
                    };
                    (clk, k, false)
                }
                // No `function` attribute: keep the legacy positive-unate AND guess.
                None => {
                    let Some(clk) =
                        inputs().find(|&p| on_clock(p) && positive_arc(lib_cell, p, out))
                    else {
                        continue;
                    };
                    (clk, GateKind::Rising, true)
                }
            };
            enables.extend(
                inputs()
                    .filter(|&p| {
                        !on_clock(p)
                            && if use_positive {
                                positive_arc(lib_cell, p, out)
                            } else {
                                has_arc(lib_cell, p, out)
                            }
                    })
                    .map(|enable| GatingEnable {
                        enable,
                        clk_pin: clk_in,
                        kind,
                    }),
            );
        }
    }
    enables
}

fn build_graph(cx: &Context, skipped: &mut SkipReport) -> TimingGraph {
    let design = cx.design;
    let mut checks: BTreeMap<u32, RegisterCheck> = BTreeMap::new();
    let mut graph = TimingGraph {
        edges: Vec::new(),
        edges_from: Vec::new(),
        edges_from_offsets: Vec::new(),
        launches: Vec::new(),
        latch_opens: Vec::new(),
        latch_info: Vec::new(),
        latch_borrow: BTreeMap::new(),
        gating_enables: Vec::new(),
        clock_net: Vec::new(),
        checks: Vec::new(),
        pulse_width_checks: Vec::new(),
    };
    for c in 0..design.num_cells() {
        let cell = CellId(c as u32);
        let Some(lib_cell_id) = design.cell_lib_ids[c] else {
            skipped.unmapped_cells += 1;
            continue;
        };
        let lib_cell = &cx.lib.cells[lib_cell_id.0 as usize];
        skipped.total_arcs += lib_cell.arc_count as usize;
        let latch = if lib_cell.is_latch {
            match latch_enable(cx, cell, lib_cell) {
                Some((enable, open_pol, setup_arc)) => {
                    let idx = graph.latch_info.len() as u32;
                    graph.latch_info.push(LatchInfo {
                        enable,
                        open_pol,
                        setup_arc,
                    });
                    Some((enable, open_pol, idx))
                }
                None => {
                    skipped.unmodelled_latch_cells += 1;
                    for i in lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32 {
                        let timing = format!("{:?}", cx.lib.arcs[i as usize].timing_type);
                        *skipped.latch_arcs_dropped.entry(timing).or_default() += 1;
                    }
                    continue;
                }
            }
        } else {
            None
        };
        for i in 0..lib_cell.arc_count as u32 {
            let lib_arc = lib_cell.arc_offset + i;
            let arc = &cx.lib.arcs[lib_arc as usize];
            let (Some(src), Some(dst)) = (
                cx.cell_pin_for_lib_pin(cell, arc.related_pin),
                cx.cell_pin_for_lib_pin(cell, arc.target_pin),
            ) else {
                skipped.unresolved_arcs += 1;
                continue;
            };
            match arc.timing_type {
                TimingType::SetupRising | TimingType::SetupFalling => {
                    let check = checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.latch = lib_cell.is_latch;
                    check.edge = check_transition(arc);
                    check.setup_arc = Some(lib_arc);
                }
                TimingType::HoldRising | TimingType::HoldFalling => {
                    let check = checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.latch = lib_cell.is_latch;
                    check.edge = check_transition(arc);
                    check.hold_arc = Some(lib_arc);
                }
                TimingType::RecoveryRising | TimingType::RecoveryFalling => {
                    let check = checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.edge = check_transition(arc);
                    check.recovery_arc = Some(lib_arc);
                }
                TimingType::RemovalRising | TimingType::RemovalFalling => {
                    let check = checks.entry(dst.0).or_default();
                    check.clk = Some(src);
                    check.edge = check_transition(arc);
                    check.removal_arc = Some(lib_arc);
                }
                // `min_pulse_width` constrains the clock pin it sits on and is
                // evaluated, so it is collected as a check, not deferred. The arm
                // sits before the latch arm so a latch's pulse-width check
                // evaluates here too instead of landing in `latch_arcs_dropped`.
                TimingType::MinPulseWidth => {
                    graph.pulse_width_checks.push(PulseWidthCheck { pin: dst, lib_arc });
                }
                // Recognised check arcs (`non_seq_*`, `three_state_*`, `preserve`,
                // `clear`, `preset`) constrain a pin rather than propagate data.
                // They are deferred to `recognized_check_arcs`, distinct from the unknown
                // `unsupported_arcs` catch-all, so the summary separates "known check we do
                // not endpoint-model" from "arc type we do not recognise". The arm sits
                // before the latch arm so a latch's deferred check counts here too
                // instead of inflating `latch_arcs_dropped`, which `--strict` rejects.
                t if t.is_deferred_check() => {
                    *skipped
                        .recognized_check_arcs
                        .entry(format!("{t:?}"))
                        .or_default() += 1
                }
                _ if let Some((enable, open_pol, latch_idx)) = latch => {
                    // The enable pin's clock-to-Q arc of the opening polarity models
                    // data waiting for the latch to open. Other data pins' combinational
                    // arcs are the transparent D -> Q borrowing path. Anything else on a
                    // latch cell has no model and is dropped like before.
                    if src == enable
                        && design.pin_is_driver[dst.0 as usize]
                        && matches!(
                            arc.timing_type,
                            TimingType::RisingEdge | TimingType::FallingEdge
                        )
                        && check_transition(arc) == open_pol
                    {
                        graph.latch_opens.push(LatchOpen {
                            enable,
                            q: dst,
                            lib_arc,
                            open_pol,
                        });
                    } else if design.pin_is_driver[dst.0 as usize]
                        && matches!(
                            arc.timing_type,
                            TimingType::Combinational
                                | TimingType::CombinationalRise
                                | TimingType::CombinationalFall
                        )
                    {
                        graph.latch_borrow.insert((src.0, dst.0), latch_idx);
                        graph.edges.push(ArcEdge { src, dst, lib_arc });
                    } else {
                        *skipped
                            .latch_arcs_dropped
                            .entry(format!("{:?}", arc.timing_type))
                            .or_default() += 1;
                    }
                }
                TimingType::Combinational
                | TimingType::CombinationalRise
                | TimingType::CombinationalFall => {
                    graph.edges.push(ArcEdge { src, dst, lib_arc });
                }
                TimingType::RisingEdge | TimingType::FallingEdge => {
                    graph.launches.push(LaunchArc {
                        clk: src,
                        q: dst,
                        lib_arc,
                        edge: check_transition(arc),
                    })
                }
                _ => {
                    *skipped
                        .unsupported_arcs
                        .entry(format!("{:?}", arc.timing_type))
                        .or_default() += 1
                }
            }
        }
    }
    // CSR adjacency: count outgoing edges per pin, prefix-sum, then scatter edge
    // indices in `edges` order, so each pin's list keeps the cell/arc scan order.
    let n = design.num_pins();
    let mut offsets = vec![0u32; n + 1];
    for edge in &graph.edges {
        offsets[edge.src.0 as usize + 1] += 1;
    }
    for i in 1..=n {
        offsets[i] += offsets[i - 1];
    }
    graph.edges_from_offsets = offsets;
    graph.edges_from = vec![0; graph.edges.len()];
    let mut cursor = graph.edges_from_offsets[..n].to_vec();
    for (i, edge) in graph.edges.iter().enumerate() {
        let s = edge.src.0 as usize;
        graph.edges_from[cursor[s] as usize] = i as u32;
        cursor[s] += 1;
    }
    graph.checks = checks.into_iter().collect();

    // Clock nets: seeds are the clock source port's net and every net a register or latch
    // clock pin sits on (a latch or unmapped cell can feed a clock net without a
    // combinational arc to cross). From there, combinational arcs are followed forward so
    // the intermediate nets of a buffered/inverting/gated tree are marked too —
    // `gating_enables` needs them to find AND gates whose output reaches no clock pin
    // directly, and the data pass must not let data propagate onto them.
    graph.clock_net = vec![false; design.num_nets()];
    let mut clock_stack: Vec<NetId> = Vec::new();
    let mark_clock_net = |net: NetId, graph: &mut TimingGraph, stack: &mut Vec<NetId>| {
        if !graph.clock_net[net.0 as usize] {
            graph.clock_net[net.0 as usize] = true;
            stack.push(net);
        }
    };
    // Every design clock port seeds the walk; each domain's tree is marked on
    // its own, so a second clock network is never mistaken for data logic.
    for clock in &cx.sdc.clocks {
        if let Some(port) = clock.port.as_deref().and_then(|name| {
            design
                .io_pins
                .iter()
                .copied()
                .find(|&p| cx.port_name(p) == name)
        }) && let Some(net) = design.pin_nets[port.0 as usize]
        {
            mark_clock_net(net, &mut graph, &mut clock_stack);
        }
    }
    // Pulse-check pins are deliberately absent: a `min_pulse_width` arc can
    // sit on a non-clock pin (`dfrtn`'s RESET_B arc) and marking that net a
    // clock net would block the data arrivals feeding its real checks.
    let register_clock_nets: Vec<NetId> = graph
        .launches
        .iter()
        .map(|l| l.clk)
        .chain(graph.checks.iter().filter_map(|(_, c)| c.clk))
        .filter_map(|pin| design.pin_nets[pin.0 as usize])
        .collect();
    for net in register_clock_nets {
        mark_clock_net(net, &mut graph, &mut clock_stack);
    }
    while let Some(net) = clock_stack.pop() {
        let mut next = Vec::new();
        for &pin in design.net_pins(net) {
            for &edge in graph.outgoing(pin) {
                if let Some(dst_net) = design.pin_nets[graph.edges[edge as usize].dst.0 as usize] {
                    next.push(dst_net);
                }
            }
        }
        for dst_net in next {
            mark_clock_net(dst_net, &mut graph, &mut clock_stack);
        }
    }
    // The forward walk also marks clock-fed branches that never reach a clock pin (clock
    // dividers, clock-as-data logic). Prune them: a marked net stays a clock net only when
    // a combinational path from it reaches a pin that consumes a clock edge.
    let mut useful = vec![false; design.num_nets()];
    let consumer_nets = graph
        .launches
        .iter()
        .map(|l| l.clk)
        .chain(graph.checks.iter().filter_map(|(_, c)| c.clk))
        .chain(graph.latch_opens.iter().map(|o| o.enable))
        .chain(graph.pulse_width_checks.iter().map(|c| c.pin))
        .filter_map(|pin| design.pin_nets[pin.0 as usize]);
    for net in consumer_nets {
        useful[net.0 as usize] = true;
    }
    loop {
        let mut changed = false;
        for edge in &graph.edges {
            let (Some(src_net), Some(dst_net)) = (
                design.pin_nets[edge.src.0 as usize],
                design.pin_nets[edge.dst.0 as usize],
            ) else {
                continue;
            };
            if useful[dst_net.0 as usize]
                && graph.clock_net[src_net.0 as usize]
                && graph.clock_net[dst_net.0 as usize]
                && !useful[src_net.0 as usize]
            {
                useful[src_net.0 as usize] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (net, is_useful) in useful.iter().enumerate() {
        if !is_useful {
            graph.clock_net[net] = false;
        }
    }
    // OpenSTA only carries the check on a pin that sees the clock network —
    // `dfrtn`'s RESET_B `min_pulse_width` arc produces no
    // `report_pulse_width_checks` row (measured: the fixture's golden has 9
    // rows, not 10). Pulse pins count as consumers above so a clock branch
    // ending at a bare pulse check is not pruned away first. Dropped arcs
    // are deferred, not modelled: they count in `recognized_check_arcs` so
    // `arcs_skipped()` still covers every arc (ibex drops ~1.7k of these on
    // async set/reset pins).
    graph.pulse_width_checks.retain(|c| {
        let keep = design.pin_nets[c.pin.0 as usize]
            .is_some_and(|n| graph.clock_net[n.0 as usize]);
        if !keep {
            *skipped
                .recognized_check_arcs
                .entry("MinPulseWidth".to_string())
                .or_default() += 1;
        }
        keep
    });
    // Under `set_propagated_clock` these nets are modelled by `propagate_clock` instead;
    // `prepare_sta` counts per domain the ones it cannot reach. The gate is
    // "any propagated row", not "the design row is propagated": a mixed-mode
    // design must not count its propagated domain's nets here.
    if !cx.sdc.clocks.iter().any(|c| c.propagated) {
        for (net, _) in graph
            .clock_net
            .iter()
            .enumerate()
            .filter(|(_, is_clock)| **is_clock)
        {
            if design
                .net_driver(NetId(net as u32))
                .is_some_and(|d| design.pin_cells[d.0 as usize].is_some())
            {
                skipped.cell_driven_clock_nets += 1;
            }
        }
    }

    let enables = gating_enables(cx, &graph);
    graph.gating_enables = enables;
    graph
}

struct LevelizedGraph {
    /// CSR levels: pins of level `l` are
    /// `level_pins[level_offsets[l]..level_offsets[l + 1]]`.
    level_offsets: Vec<u32>,
    level_pins: Vec<PinId>,
    /// CSR reverse adjacency of combinational arcs: incoming `(source pin, lib arc)`
    /// pairs of pin `p` are `edges_to[edges_to_offsets[p]..edges_to_offsets[p + 1]]`.
    edges_to: Vec<(PinId, u32)>,
    edges_to_offsets: Vec<u32>,
    pin_driver: Vec<Option<(u32, u32, PinId)>>,
}

impl LevelizedGraph {
    fn level(&self, l: usize) -> &[PinId] {
        &self.level_pins[self.level_offsets[l] as usize..self.level_offsets[l + 1] as usize]
    }

    fn incoming_to(&self, pin: PinId) -> &[(PinId, u32)] {
        let p = pin.0 as usize;
        &self.edges_to[self.edges_to_offsets[p] as usize..self.edges_to_offsets[p + 1] as usize]
    }
}

/// Kahn topological levels over net edges (driver -> sink) and combinational arcs,
/// plus reverse indices so every level is data-parallel with zero write contention.
/// Register `CLK -> Q` arcs are not edges: they start new paths.
fn topological_levels(
    cx: &Context,
    graph: &TimingGraph,
    include_seq: bool,
) -> Result<LevelizedGraph> {
    let design = cx.design;
    let n = design.num_pins();
    let mut indegree = vec![0u32; n];
    // Count incoming combinational arcs per pin for the `edges_to` CSR.
    let mut edges_to_offsets = vec![0u32; n + 1];
    for src_p in 0..n {
        for &e in graph.outgoing(PinId(src_p as u32)) {
            let edge = graph.edges[e as usize];
            indegree[edge.dst.0 as usize] += 1;
            edges_to_offsets[edge.dst.0 as usize + 1] += 1;
        }
    }
    // `include_seq` adds register clock-to-Q and latch enable-to-Q arcs as ordering
    // edges. Slew needs them: a clock gate's enable can sit on a clock net deeper
    // than the register output it drives, so without the edge the output pin would
    // be levelized before its own clock/enable source. The data pass keeps them out
    // (`false`) because it evaluates those arcs as separate pre-passes instead.
    let mut seq_adj: Vec<Vec<PinId>> = if include_seq {
        vec![Vec::new(); n]
    } else {
        Vec::new()
    };
    if include_seq {
        // Only arcs whose output pin actually drives a net become ordering edges:
        // `propagate_slew` evaluates a sequential arc under the same `pin_nets`
        // guard, so an unconnected Q gains no slew dependency and must not add a
        // cycle constraint either.
        for l in &graph.launches {
            if design.pin_nets[l.q.0 as usize].is_some() {
                indegree[l.q.0 as usize] += 1;
                seq_adj[l.clk.0 as usize].push(l.q);
            }
        }
        for o in &graph.latch_opens {
            if design.pin_nets[o.q.0 as usize].is_some() {
                indegree[o.q.0 as usize] += 1;
                seq_adj[o.enable.0 as usize].push(o.q);
            }
        }
    }
    for i in 1..=n {
        edges_to_offsets[i] += edges_to_offsets[i - 1];
    }
    let mut edges_to = vec![(PinId(0), 0); edges_to_offsets[n] as usize];
    let mut cursor = edges_to_offsets[..n].to_vec();
    for src_p in 0..n {
        let src_pin = PinId(src_p as u32);
        for &e in graph.outgoing(src_pin) {
            let edge = graph.edges[e as usize];
            let d = edge.dst.0 as usize;
            edges_to[cursor[d] as usize] = (src_pin, edge.lib_arc);
            cursor[d] += 1;
        }
    }
    let mut pin_driver: Vec<Option<(u32, u32, PinId)>> = vec![None; n];
    for net in 0..design.num_nets() {
        if let Some(driver) = design.net_driver(NetId(net as u32)) {
            for (i, &sink) in design
                .net_pins(NetId(net as u32))
                .iter()
                .enumerate()
                .skip(1)
            {
                if sink != driver {
                    indegree[sink.0 as usize] += 1;
                    pin_driver[sink.0 as usize] = Some((net as u32, i as u32, driver));
                }
            }
        }
    }
    let mut depth = vec![0u32; n];
    let mut order: Vec<PinId> = (0..n as u32)
        .filter(|&p| indegree[p as usize] == 0)
        .map(PinId)
        .collect();
    let mut head = 0;
    while head < order.len() {
        let pin = order[head];
        head += 1;
        let p = pin.0 as usize;
        let next_depth = depth[p] + 1;
        let mut release = |target: PinId, order: &mut Vec<PinId>| {
            let t = target.0 as usize;
            if next_depth > depth[t] {
                depth[t] = next_depth;
            }
            let d = &mut indegree[t];
            *d -= 1;
            if *d == 0 {
                order.push(target);
            }
        };
        // Release a net's sinks only when this pin is the net's canonical driver. A pin
        // that merely has `pin_is_driver` set (e.g. an INOUT or a second driver on a
        // multi-driver net) must not release them: the indegree was charged against
        // `net_driver`, and releasing early would underflow it.
        if let Some(net) = design.pin_nets[p]
            && design.net_driver(net) == Some(pin)
        {
            for &sink in design.net_pins(net).iter().skip(1) {
                if sink != pin {
                    release(sink, &mut order);
                }
            }
        }
        for &e in graph.outgoing(PinId(p as u32)) {
            release(graph.edges[e as usize].dst, &mut order);
        }
        if include_seq {
            for &dst in &seq_adj[p] {
                release(dst, &mut order);
            }
        }
    }
    if order.len() != n {
        bail!(
            "combinational loop: {} of {} pins could not be levelized",
            n - order.len(),
            n
        );
    }
    let max_depth = depth.iter().copied().max().unwrap_or(0) as usize;
    let num_levels = max_depth + 1;
    // CSR levels: count pins per depth, prefix-sum, then scatter in `order` sequence.
    let mut level_offsets = vec![0u32; num_levels + 1];
    for pin in &order {
        level_offsets[depth[pin.0 as usize] as usize + 1] += 1;
    }
    for i in 1..=num_levels {
        level_offsets[i] += level_offsets[i - 1];
    }
    let mut level_pins = vec![PinId(0); order.len()];
    let mut cursor = level_offsets[..num_levels].to_vec();
    for pin in order {
        let d = depth[pin.0 as usize] as usize;
        level_pins[cursor[d] as usize] = pin;
        cursor[d] += 1;
    }
    Ok(LevelizedGraph {
        level_offsets,
        level_pins,
        edges_to,
        edges_to_offsets,
        pin_driver,
    })
}

/// `driver_state` plus the net's wire delay and slew degradation at sink `pin_idx`.
/// Port-driven nets use `ln 2 * Elmore` delay and additive slew; cell-driven nets use the
/// Elmore delay and slew hypot. Shared by the data pass and `propagate_clock`.
fn sink_state_with_wire(
    driver_state: PinTimingState,
    wire: Option<&[Vec<f32>; 2]>,
    pin_idx: u32,
    is_port: bool,
    clock_path: bool,
    sdc: &Sdc,
) -> PinTimingState {
    let mut sink_state = driver_state;
    let Some(wire) = wire else { return sink_state };
    let i = pin_idx as usize;
    for pol in [RISE, FALL] {
        let elmore = wire[pol][i];
        let degrade = WIRE_SLEW_FACTOR * elmore;
        if is_port {
            let delay = std::f32::consts::LN_2 * elmore;
            let late = delay * sdc.derate(true, DERATE_NET, clock_path, pol);
            let early = delay * sdc.derate(false, DERATE_NET, clock_path, pol);
            sink_state.arrival_max[pol] += late;
            sink_state.arrival_min[pol] += early;
            sink_state.dly_max[pol] += late;
            sink_state.dly_min[pol] += early;
            sink_state.slew_max[pol] = driver_state.slew_max[pol] + degrade;
            sink_state.slew_min[pol] = driver_state.slew_min[pol] + degrade;
        } else {
            let late = elmore * sdc.derate(true, DERATE_NET, clock_path, pol);
            let early = elmore * sdc.derate(false, DERATE_NET, clock_path, pol);
            sink_state.arrival_max[pol] += late;
            sink_state.arrival_min[pol] += early;
            sink_state.dly_max[pol] += late;
            sink_state.dly_min[pol] += early;
            sink_state.slew_max[pol] = driver_state.slew_max[pol].hypot(degrade);
            sink_state.slew_min[pol] = driver_state.slew_min[pol].hypot(degrade);
        }
    }
    sink_state
}

/// Slew-only counterpart of [`sink_state_with_wire`]: the wire degradation applied to a
/// driver's slew at one sink, without touching arrival fields.
fn slew_sink(
    driver: PinTimingState,
    wire: Option<&[Vec<f32>; 2]>,
    pin_idx: u32,
    is_port: bool,
) -> PinTimingState {
    let mut sink = driver;
    let Some(wire) = wire else { return sink };
    let i = pin_idx as usize;
    for pol in [RISE, FALL] {
        let degrade = WIRE_SLEW_FACTOR * wire[pol][i];
        if is_port {
            sink.slew_max[pol] = driver.slew_max[pol] + degrade;
            sink.slew_min[pol] = driver.slew_min[pol] + degrade;
        } else {
            sink.slew_max[pol] = driver.slew_max[pol].hypot(degrade);
            sink.slew_min[pol] = driver.slew_min[pol].hypot(degrade);
        }
    }
    sink
}

/// Accumulate one arc's output slew into a pin's slew bounds: the larger-input-slew
/// result feeds `slew_max`, the smaller feeds `slew_min` (same pairing as `merge`).
fn merge_slew(state: &mut PinTimingState, pol: usize, out_max: f32, out_min: f32) {
    state.slew_max[pol] = state.slew_max[pol].max(out_max);
    state.slew_min[pol] = state.slew_min[pol].min(out_min);
}

fn slew_reached(state: &PinTimingState, pol: usize) -> bool {
    state.slew_max[pol] > f32::NEG_INFINITY
}

/// Unified `slew_max`/`slew_min` for every pin. OpenSTA merges the slew of *every*
/// incoming arc at a vertex — clock and data arcs alike — so a clock gate's enable
/// arc can dominate the slew its clock output pin (and downstream clock pins) see.
/// Propagating only clock-reached arcs, as `propagate_clock` does for arrival, loses
/// that merge. Slew is arrival-independent (each arc's output slew is a function of
/// the source slew and the net load only), so one levelized pass over all arcs —
/// combinational, register clock-to-Q and latch enable-to-Q — reproduces it exactly.
/// Only the slew fields of the returned states are meaningful.
fn propagate_slew(
    cx: &Context,
    graph: &TimingGraph,
    wire_delays_per_net: &[Option<[Vec<f32>; 2]>],
    state: &[PinTimingState],
) -> Result<Vec<PinTimingState>> {
    let design = cx.design;
    let sdc = cx.sdc;
    let n = design.num_pins();
    // Sequential arcs are ordering edges here (unlike the data pass, which handles
    // them as pre-passes): a register output's slew depends on its clock pin's slew,
    // which can sit deeper in the clock tree than the output's own level.
    // On real cores they close a giant cycle instead — a clock-gate latch's Q
    // fans into the gated clock net, and every downstream launch's clk -> Q
    // edge closes the loop (ibex: 42k pins). The seq edges only order a slew
    // dependency, so on the cycle fall back to the data-edge order: a Q may
    // miss its launch-arc slew contribution, but every clock pin's slew —
    // all `propagate_clock` and the pulse-width checks consume — is exact.
    let levelized = match topological_levels(cx, graph, true) {
        Ok(l) => l,
        Err(_) => topological_levels(cx, graph, false)?,
    };
    let mut slew = vec![PinTimingState::UNREACHED; n];

    // Register clock-to-Q and latch enable-to-Q arcs are not in the combinational
    // `edges_to` CSR; index them by output pin so the levelized pass merges their
    // slew like any other incoming arc. The usize is the fixed source polarity
    // (the launching clock edge / the latch's opening edge).
    let mut seq_in: Vec<Vec<(PinId, u32, usize)>> = vec![Vec::new(); n];
    for l in &graph.launches {
        seq_in[l.q.0 as usize].push((l.clk, l.lib_arc, l.edge));
    }
    for o in &graph.latch_opens {
        seq_in[o.q.0 as usize].push((o.enable, o.lib_arc, o.open_pol));
    }

    // Input ports seed slew. Data inputs take the slew the data pass already computed
    // (`port_drive`, which honours `set_driving_cell`); the clock port takes
    // `set_input_transition`, matching `propagate_clock`'s source seed.
    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        if design.pin_dirs[p] != PinDir::Input {
            continue;
        }
        if sdc.is_clock_port(cx.port_name(pin)) {
            let tr = sdc.input_transition_ns(cx.port_name(pin));
            for pol in [RISE, FALL] {
                slew[p].slew_max[pol] = tr;
                slew[p].slew_min[pol] = tr;
            }
        } else {
            slew[p].slew_max = state[p].slew_max;
            slew[p].slew_min = state[p].slew_min;
        }
    }

    // Output slew of one arc for `dst_pol` at both input-slew bounds.
    let arc_slew = |out_net: NetId,
                    arc: &crate::lib_db::LibTimingArc,
                    dst_pol: usize,
                    s_max: f32,
                    s_min: f32|
     -> Option<(f32, f32)> {
        let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol) else {
            return None;
        };
        let (_, out_max) = cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_max);
        let out_min = if s_min == s_max {
            out_max
        } else {
            cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_min)
                .1
        };
        Some((out_max, out_min))
    };

    for lvl in 1..levelized.level_offsets.len() - 1 {
        for &pin in levelized.level(lvl) {
            let p = pin.0 as usize;
            let mut next = slew[p];
            let mut changed = false;

            // Net loads take their driver's slew through the wire model.
            if !design.pin_is_driver[p]
                && let Some((net_idx, pin_idx, drv)) = levelized.pin_driver[p]
            {
                let ds = slew[drv.0 as usize];
                if slew_reached(&ds, RISE) || slew_reached(&ds, FALL) {
                    let wire = wire_delays_per_net
                        .get(net_idx as usize)
                        .and_then(|w| w.as_ref());
                    let is_port = design.pin_cells[drv.0 as usize].is_none();
                    next = slew_sink(ds, wire, pin_idx, is_port);
                    changed = true;
                }
            }

            // Driver pins merge the slew of every incoming arc. Combinational arcs
            // come through `incoming_to`; sequential arcs through `seq_in`.
            if let Some(out_net) = design.pin_nets[p] {
                for &(src_pin, lib_arc) in levelized.incoming_to(pin) {
                    let arc = &cx.lib.arcs[lib_arc as usize];
                    let src = slew[src_pin.0 as usize];
                    // Latch `D -> Q` borrow edges propagate the same transition at
                    // both ends, mirroring the data pass.
                    let borrow = graph.latch_borrow.contains_key(&(src_pin.0, p as u32));
                    let legs: &[(usize, usize)] = if borrow {
                        &[(RISE, RISE), (FALL, FALL)]
                    } else {
                        polarities(arc.timing_sense)
                    };
                    for &(src_pol, dst_pol) in legs {
                        let s_max = src.slew_max[src_pol];
                        let s_min = src.slew_min[src_pol];
                        if s_max == f32::NEG_INFINITY {
                            continue;
                        }
                        if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                            || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                        {
                            continue;
                        }
                        if let Some((out_max, out_min)) =
                            arc_slew(out_net, arc, dst_pol, s_max, s_min)
                        {
                            merge_slew(&mut next, dst_pol, out_max, out_min);
                            changed = true;
                        }
                    }
                }
                for &(src_pin, lib_arc, src_pol) in &seq_in[p] {
                    let arc = &cx.lib.arcs[lib_arc as usize];
                    let src = slew[src_pin.0 as usize];
                    let s_max = src.slew_max[src_pol];
                    let s_min = src.slew_min[src_pol];
                    if s_max == f32::NEG_INFINITY {
                        continue;
                    }
                    for dst_pol in [RISE, FALL] {
                        if let Some((out_max, out_min)) =
                            arc_slew(out_net, arc, dst_pol, s_max, s_min)
                        {
                            merge_slew(&mut next, dst_pol, out_max, out_min);
                            changed = true;
                        }
                    }
                }
            }

            if changed {
                slew[p] = next;
            }
        }
    }
    Ok(slew)
}

/// Arrival of the propagated clock at every pin the clock source reaches through
/// combinational arcs and net wiring, per transition. Kept in a separate state so clock
/// timing can never leak into data endpoints: a pin is filled only when the clock itself
/// arrives there, and arcs contribute only from pins the clock already reached. The caller
/// copies the result onto the pins that consume clock edges (register clock pins, latch
/// enables, clock-gate clock inputs); everything else stays unreached.
///
/// The returned parity marks, per pin and transition, the clock-edge index (mod 2) of the
/// earliest edge that produced the arrival: 0 for the source rise, 1 for its fall. Inverting
/// cells flip it, so a pin behind an odd number of inversions sees its first rise on clock
/// edge 1. `u8::MAX` means unreached.
fn propagate_clock(
    cx: &Context,
    levelized: &LevelizedGraph,
    wire_delays_per_net: &[Option<[Vec<f32>; 2]>],
    unified_slew: &[PinTimingState],
) -> (Vec<PinTimingState>, Vec<[u8; 2]>) {
    let design = cx.design;
    let sdc = cx.sdc;
    let mut clk = vec![PinTimingState::UNREACHED; design.num_pins()];
    let mut parity = vec![[u8::MAX; 2]; design.num_pins()];
    // Every design row seeds its own port with that row's source latency,
    // half period and `set_input_transition` slew — the walk is
    // mode-independent: consumer seeding picks propagated vs. ideal per
    // domain, and the pulse-width oracle needs propagated arrivals on ideal
    // domains too. The dom bits ride along (harmless downstream: capture
    // identity comes from `clock_domain`, not from `clk_state`).
    for (row, clock) in sdc.clocks.iter().enumerate() {
        let Some(port_name) = clock.port.as_deref() else {
            continue;
        };
        let Some(port) = design
            .io_pins
            .iter()
            .copied()
            .find(|&p| cx.port_name(p) == port_name)
        else {
            continue;
        };
        // Each edge leaves the source at its waveform instant
        // (`edge_offset` — P/2 apart for the default 50% duty cycle).
        // The propagated slew starts at the clock port's `set_input_transition` (the port is
        // an input): `set_input_transition 0.15 [all_inputs]` gives 0.15 at directly connected
        // clock pins, while `set_clock_transition` does not reach the propagated network at
        // all (measured on sta_edge/hold_violation and sta_edge/propagated_clock).
        let lat = clock.source_latency_ns;
        let tr = sdc.input_transition_ns(port_name);
        let p = port.0 as usize;
        let dom = dom_bit(row);
        let rise = lat + clock.edge_offset(0);
        let fall = lat + clock.edge_offset(1);
        clk[p].merge(RISE, rise, rise, rise, rise, tr, tr, 1, 1, 0, 0, dom, dom);
        clk[p].merge(FALL, fall, fall, fall, fall, tr, tr, 2, 2, 0, 0, dom, dom);
        parity[p] = [0, 1];
    }

    for lvl in 1..levelized.level_offsets.len() - 1 {
        for &pin in levelized.level(lvl) {
            let p = pin.0 as usize;
            let mut next = clk[p];
            let mut next_par = parity[p];
            let mut changed = false;

            if !design.pin_is_driver[p]
                && let Some((net_idx, pin_idx, drv)) = levelized.pin_driver[p]
            {
                let driver_state = clk[drv.0 as usize];
                if driver_state.reached(RISE) || driver_state.reached(FALL) {
                    let wire = wire_delays_per_net
                        .get(net_idx as usize)
                        .and_then(|w| w.as_ref());
                    let is_port = design.pin_cells[drv.0 as usize].is_none();
                    next = sink_state_with_wire(driver_state, wire, pin_idx, is_port, true, sdc);
                    next_par = parity[drv.0 as usize];
                    changed = true;
                }
            }

            let incoming = levelized.incoming_to(pin);
            if !incoming.is_empty()
                && let Some(out_net) = design.pin_nets[p]
            {
                for &(src_pin, lib_arc) in incoming {
                    let arc = &cx.lib.arcs[lib_arc as usize];
                    let src = clk[src_pin.0 as usize];
                    let src_par = parity[src_pin.0 as usize];
                    for &(src_pol, dst_pol) in polarities(arc.timing_sense) {
                        if !src.reached(src_pol) {
                            continue;
                        }
                        if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                            || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                        {
                            continue;
                        }
                        let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol)
                        else {
                            continue;
                        };
                        // Delay lookups use the unified slew: a clock gate's enable
                        // arc can dominate the slew at its output pin, and that slew
                        // is what downstream clock arcs must see as their input slew.
                        let src_slew = unified_slew[src_pin.0 as usize];
                        let s_max = src_slew.slew_max[src_pol];
                        let s_min = src_slew.slew_min[src_pol];
                        if s_max == f32::NEG_INFINITY {
                            continue;
                        }
                        let (late_delay, late_slew) =
                            cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_max);
                        let (early_delay, early_slew) = if s_min == s_max {
                            (late_delay, late_slew)
                        } else {
                            cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_min)
                        };
                        let cand_min = src.arrival_min[src_pol]
                            + early_delay * sdc.derate(false, DERATE_CELL, true, src_pol);
                        // The parity of a transition is the clock-edge index that produced it;
                        // keep the one belonging to the earliest arrival at this pin.
                        if cand_min < next.arrival_min[dst_pol] {
                            next_par[dst_pol] = src_par[src_pol];
                        }
                        let cand_max = src.arrival_max[src_pol]
                            + late_delay * sdc.derate(true, DERATE_CELL, true, src_pol);
                        next.merge(
                            dst_pol,
                            cand_max,
                            cand_min,
                            cand_max,
                            cand_min,
                            late_slew,
                            early_slew,
                            1 << src_par[src_pol],
                            1 << src_par[src_pol],
                            0,
                            0,
                            src.dom_max[src_pol],
                            src.dom_min[src_pol],
                        );
                        changed = true;
                    }
                }
            }

            if changed {
                clk[p] = next;
                parity[p] = next_par;
            }
        }
    }
    (clk, parity)
}

/// Edge parity (source clock-edge index mod 2) and owning clock domain of every
/// pin transition under the ideal clock. The clock network is traversed exactly
/// like `propagate_clock` but with no timing: every reached transition keeps
/// the parity of the source edge that produces it, so a pin behind an odd
/// number of inverting cells sees its rise on clock edge 1. `u8::MAX` =
/// unreached; consumers fall back to the source-port convention (rise 0, fall 1).
/// Ties (a transition reachable from both parities, e.g. non-unate logic)
/// resolve to the first candidate in levelized order — the same
/// earliest-wins policy `propagate_clock` applies to equal arrivals.
///
/// The second return value tags each pin with the `Sdc::clocks` row whose port
/// seed reached it (`u8::MAX` when no seed did). Seeding every design row keeps
/// a second domain's clock network on its own lattice instead of folding it
/// onto the first row's, and gives every clock consumer its capture domain.
fn ideal_clock_parity(
    cx: &Context,
    levelized: &LevelizedGraph,
) -> (Vec<[u8; 2]>, Vec<u8>) {
    let design = cx.design;
    let mut par = vec![[u8::MAX; 2]; design.num_pins()];
    let mut dom = vec![u8::MAX; design.num_pins()];
    for (row, clock) in cx.sdc.clocks.iter().enumerate() {
        let Some(port_name) = clock.port.as_deref() else {
            continue;
        };
        let Some(port) = design
            .io_pins
            .iter()
            .copied()
            .find(|&p| cx.port_name(p) == port_name)
        else {
            continue;
        };
        par[port.0 as usize] = [0, 1];
        dom[port.0 as usize] = row as u8;
    }
    for lvl in 1..levelized.level_offsets.len() - 1 {
        for &pin in levelized.level(lvl) {
            let p = pin.0 as usize;
            let mut next = par[p];
            let mut next_dom = dom[p];
            if !design.pin_is_driver[p]
                && let Some((_, _, drv)) = levelized.pin_driver[p]
            {
                let d = par[drv.0 as usize];
                if d[RISE] != u8::MAX || d[FALL] != u8::MAX {
                    next = d;
                    next_dom = dom[drv.0 as usize];
                }
            }
            for &(src_pin, lib_arc) in levelized.incoming_to(pin) {
                let arc = &cx.lib.arcs[lib_arc as usize];
                let src_par = par[src_pin.0 as usize];
                for &(src_pol, dst_pol) in polarities(arc.timing_sense) {
                    if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                        || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                    {
                        continue;
                    }
                    if src_par[src_pol] != u8::MAX && next[dst_pol] == u8::MAX {
                        next[dst_pol] = src_par[src_pol];
                        next_dom = dom[src_pin.0 as usize];
                    }
                }
            }
            par[p] = next;
            dom[p] = next_dom;
        }
    }
    (par, dom)
}

fn polarities(sense: TimingSense) -> &'static [(usize, usize)] {
    match sense {
        TimingSense::PositiveUnate => &[(RISE, RISE), (FALL, FALL)],
        TimingSense::NegativeUnate => &[(RISE, FALL), (FALL, RISE)],
        TimingSense::NonUnate => &[(RISE, RISE), (FALL, RISE), (RISE, FALL), (FALL, FALL)],
    }
}

fn delay_tables(
    arc: &crate::lib_db::LibTimingArc,
    dst_pol: usize,
) -> (Option<&NldmTable>, Option<&NldmTable>) {
    if dst_pol == RISE {
        (arc.cell_rise.as_ref(), arc.rise_transition.as_ref())
    } else {
        (arc.cell_fall.as_ref(), arc.fall_transition.as_ref())
    }
}

/// Clock transition a timing arc fires on or is derated by: every `*Falling` check/launch
/// type pairs with the falling clock edge (the transition entering the arc at the clock pin).
fn check_transition(arc: &crate::lib_db::LibTimingArc) -> usize {
    match arc.timing_type {
        TimingType::FallingEdge
        | TimingType::SetupFalling
        | TimingType::HoldFalling
        | TimingType::RecoveryFalling
        | TimingType::RemovalFalling => FALL,
        _ => RISE,
    }
}

fn constraint_table(arc: &crate::lib_db::LibTimingArc, pol: usize) -> Option<&NldmTable> {
    if pol == RISE {
        arc.rise_constraint.as_ref()
    } else {
        arc.fall_constraint.as_ref()
    }
}

/// Worst (`slew_max`) transition slew in ns at each pin, per transition. Unreached
/// pins keep `f32::NEG_INFINITY`; power table lookups treat them as missing.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct PinSlew {
    pub rise_ns: f32,
    pub fall_ns: f32,
}

impl PinSlew {
    pub fn reached(&self, pol: usize) -> bool {
        let s = if pol == RISE {
            self.rise_ns
        } else {
            self.fall_ns
        };
        s.is_finite() && s > 0.0
    }
}

/// Runs the full STA flow and returns the per-pin worst slews, for consumers that
/// need operating-point data (e.g. `internal_power` table lookups) rather than the
/// endpoint report. Costs the same as [`run_sta`].
pub fn sta_pin_slews(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<Vec<PinSlew>> {
    let (_report, state, _parities) = run_sta_detail(design, lib, sdc, parasitics, false)?;
    Ok(state
        .iter()
        .map(|s| PinSlew {
            rise_ns: s.slew_max[RISE],
            fall_ns: s.slew_max[FALL],
        })
        .collect())
}

/// Zero-wire STA when `parasitics` is `None`; otherwise wire capacitance and Elmore wire delay are added.
pub fn run_sta(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<StaReport> {
    run_sta_inner(design, lib, sdc, parasitics, true).map(|(report, _)| report)
}

/// [`run_sta`] without the per-category skip warnings on stderr; the caller
/// merges or reports the [`SkipReport`] itself.
pub fn run_sta_quiet(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<StaReport> {
    run_sta_inner(design, lib, sdc, parasitics, false).map(|(report, _)| report)
}

fn run_sta_inner(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
    emit_warnings: bool,
) -> Result<(StaReport, Vec<PinTimingState>)> {
    let (report, state, _) = run_sta_detail(design, lib, sdc, parasitics, emit_warnings)?;
    Ok((report, state))
}

/// [`run_sta`] that also returns the per-pin timing state (the tests compare it pin by pin).
#[cfg(test)]
fn run_sta_with_state(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
) -> Result<(StaReport, Vec<PinTimingState>)> {
    let (report, state, _) = run_sta_detail(design, lib, sdc, parasitics, true)?;
    Ok((report, state))
}

fn run_sta_detail(
    design: &DesignDb,
    lib: &LibDb,
    sdc: &Sdc,
    parasitics: Option<&NetParasitics>,
    emit_warnings: bool,
) -> Result<(StaReport, Vec<PinTimingState>, Vec<EndpointPolarities>)> {
    let prepared = prepare_sta(design, lib, sdc, parasitics)?;
    if !sdc.exceptions.is_empty() || !sdc.path_delays.is_empty() || !sdc.clock_groups.is_empty() {
        return run_sta_partitioned(&prepared, emit_warnings);
    }
    let outcome = eval_sta(&prepared, None, &HashSet::new(), prepared.skipped.clone())?;
    let (report, polarities) =
        assemble_sta_report(outcome.entries, outcome.pulse_rows, outcome.skipped);
    if emit_warnings {
        emit_sta_warnings(&report);
    }
    Ok((report, outcome.state, polarities))
}

/// One seeded input port's unfiltered merge arguments, per transition:
/// `(arr_max, arr_min, slew_max, slew_min)`. `eval_sta` merges the arrival
/// fields only for ports in the run's allowed set but always merges slew —
/// downstream delay lookups depend on the physical slew at the pin.
/// `dom` is the `Sdc::clocks` row the port's `set_input_delay -clock`
/// resolved to — the launch-domain tag every arrival this port produces
/// carries.
struct PortSeed {
    pin: PinId,
    dom: u8,
    per_pol: [(f32, f32, f32, f32); 2],
}

/// One output transition of a launch's precomputed merge arguments:
/// `(arr_max, arr_min, dly_max, dly_min, slew_max, slew_min)`.
#[derive(Clone, Copy)]
struct LaunchPair {
    q: PinId,
    dst_pol: usize,
    arr_max: f32,
    arr_min: f32,
    dly_max: f32,
    dly_min: f32,
    slew_max: f32,
    slew_min: f32,
}

impl LaunchPair {
    const UNREACHED: Self = Self {
        q: PinId(0),
        dst_pol: RISE,
        arr_max: f32::NEG_INFINITY,
        arr_min: f32::NEG_INFINITY,
        dly_max: f32::NEG_INFINITY,
        dly_min: f32::NEG_INFINITY,
        slew_max: f32::NEG_INFINITY,
        slew_min: f32::NEG_INFINITY,
    };
}

/// One launch's precomputed output pairs plus the clock-edge parity that
/// launched them. `pairs` is indexed by output transition. `tag` is the CRPR
/// launch-tag bitmask for every arrival this seed produces; `dom` is the
/// `Sdc::clocks` row of the clock pin's domain — `u8::MAX` when no design
/// clock reached the pin, in which case `pairs` is all-`UNREACHED` anyway.
struct LaunchSeed {
    q: PinId,
    par: u8,
    tag: u8,
    dom: u8,
    pairs: [LaunchPair; 2],
}

/// Everything `eval_sta` needs that does not depend on which launch sources a
/// run allows: the design context, timing graph, levelization, wire delays,
/// clock seeds, and the precomputed port/launch/latch contributions.
struct StaPrepared<'a> {
    cx: Context<'a>,
    graph: TimingGraph,
    levelized: LevelizedGraph,
    wire_delays_per_net: Vec<Option<[Vec<f32>; 2]>>,
    skipped: SkipReport,
    /// Clock-consumer seeds only (propagated-clock consumers or ideal clock
    /// pins); data arrivals are merged per eval run on top of this.
    base_state: Vec<PinTimingState>,
    port_seeds: Vec<PortSeed>,
    launch_seeds: Vec<LaunchSeed>,
    latch_seeds: Vec<LaunchSeed>,
    clock_parity: Option<Vec<[u8; 2]>>,
    /// Capture-domain identity per pin: the `Sdc::clocks` row whose port seed
    /// reached the pin through the clock network, `u8::MAX` on pins no design
    /// clock reaches. Register/gate checks and latch enables read their row's
    /// period, latency and uncertainty through it.
    clock_domain: Vec<u8>,
    clock_state: Option<Vec<PinTimingState>>,
    /// CRPR launch-tag table: index 0 is the no-credit sentinel (bit 0 of every
    /// `crpr_*` mask — ports, latch clock-to-Q launches, unresolvable or
    /// contested sources); entries 1.. are a launch's `(clock net, clock edge)`
    /// pair. `u8` masks cap the table at 8 entries — past that, launches are
    /// seeded with the sentinel bit (conservative: no credit).
    crpr_tags: Vec<(NetId, usize)>,
}

struct EvalOutcome {
    /// `(pin, report, polarities)` triples, unsorted; `assemble_sta_report`
    /// sorts and reduces them.
    entries: EndpointEntries,
    /// Evaluated `min_pulse_width` rows; `assemble_sta_report` sorts them.
    pulse_rows: Vec<PulseWidthCheckReport>,
    state: Vec<PinTimingState>,
    skipped: SkipReport,
}

/// Run-invariant preparation: context, pi models, wire delays, timing graph,
/// levelization, clock propagation/seeding, and the per-source launch and
/// port contributions. Clock-pin state is identical for every exception
/// signature class, so it is seeded once into `base_state`.
fn prepare_sta<'a>(
    design: &'a DesignDb,
    lib: &'a LibDb,
    sdc: &'a Sdc,
    parasitics: Option<&'a NetParasitics>,
) -> Result<StaPrepared<'a>> {
    // Domain tags ride in `u8` masks on every arrival extreme; more than 8
    // clock rows (design and virtual) cannot be tagged.
    if sdc.clocks.len() > u8::BITS as usize {
        anyhow::bail!(
            "sta: {} clocks declared; at most {} domains can be tagged",
            sdc.clocks.len(),
            u8::BITS
        );
    }
    let mut cx = Context {
        design,
        lib,
        sdc,
        parasitics,
        pi_models: Vec::new(),
        net_wire_loads: vec![None; design.num_nets()],
    };
    // Resolve `set_load` `get_nets` rules once, before any net capacitance
    // consumer: exact names only, file order (last-wins per net), missing
    // names counted like OpenSTA's per-name warning. Under parasitics the
    // annotation's replace-wire-cap semantics do not map onto the distributed
    // RC model, so resolved nets count as unresolved instead of being
    // approximated (measured restriction, `docs/results.md` 2026-11-19).
    let mut unresolved_load_selectors = 0;
    if sdc.net_loads().next().is_some() {
        let net_by_name: HashMap<_, u32> = design
            .net_names
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i as u32))
            .collect();
        for (names, value_pf) in sdc.net_loads() {
            for name in names {
                match design.strings.get(name).and_then(|s| net_by_name.get(&s)) {
                    Some(&net) => cx.net_wire_loads[net as usize] = Some(value_pf),
                    None => unresolved_load_selectors += 1,
                }
            }
        }
        if parasitics.is_some() {
            unresolved_load_selectors += cx.net_wire_loads.iter().flatten().count();
            cx.net_wire_loads.fill(None);
        }
    }
    let build_pi = |n: usize| {
        let net = NetId(n as u32);
        let tree = parasitics?.net(net)?;
        let pins = design.net_pins(net);
        let pi = |pol| {
            tree.pi_model(
                &pins
                    .iter()
                    .map(|&p| cx.sink_cap(p, pol))
                    .collect::<Vec<_>>(),
            )
        };
        Some([pi(RISE), pi(FALL)])
    };
    let n_nets = design.num_nets();
    cx.pi_models = if parasitics.is_none() {
        vec![None; n_nets]
    } else if n_nets < 512 {
        (0..n_nets).map(build_pi).collect()
    } else {
        (0..n_nets).into_par_iter().map(build_pi).collect()
    };

    let wire_delays_per_net: Vec<Option<[Vec<f32>; 2]>> = if parasitics.is_some() {
        if n_nets < 512 {
            (0..n_nets)
                .map(|n| cx.wire_delays(NetId(n as u32)))
                .collect()
        } else {
            (0..n_nets)
                .into_par_iter()
                .map(|n| cx.wire_delays(NetId(n as u32)))
                .collect()
        }
    } else {
        Vec::new()
    };

    let mut skipped = SkipReport {
        unresolved_load_selectors,
        ..Default::default()
    };
    let graph = build_graph(&cx, &mut skipped);
    let levelized = topological_levels(&cx, &graph, false)?;
    // Primary inputs: ideal drivers. No clock port is a data startpoint —
    // `is_clock_port` covers every design row, so a second domain's source
    // port stays out of the data seeds too. Seeds are stored unfiltered;
    // `eval_sta` re-merges them per allowed set. `state_ports` additionally
    // feeds `propagate_slew`, which must see every source's slew regardless
    // of exception filtering.
    let mut port_seeds = Vec::new();
    let mut state_ports = vec![PinTimingState::UNREACHED; design.num_pins()];
    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        let name = cx.port_name(pin);
        if design.pin_dirs[p] != PinDir::Input || sdc.is_clock_port(name) {
            continue;
        }
        // The port launches on the domain its `set_input_delay -clock` row
        // resolves to — `launch_domain` maps a virtual row onto the design
        // grid it coincides with, so an I/O clock sharing the design period
        // pairs same-domain instead of deferring every endpoint.
        let clock_row = sdc.input_clock_row(name);
        let dom = launch_domain(sdc, clock_row) as u8;
        let (in_min, in_max) = sdc.input_delay_ns(name);
        let latency = sdc.input_clock_latency_ns(name);
        // The delay is referenced to the `-clock` row's own rising edge —
        // a `-waveform` instant, 0 for the default duty cycle (measured
        // `t2_wave`: `d_in`'s arrival shifts by cka's 0.3 rise instant).
        let launch_edge = sdc.clocks.get(clock_row).map_or(0.0, |c| c.edge_offset(0));
        let drive = cx.port_drive(pin, &mut skipped);
        let mut per_pol = [(0.0f32, 0.0, 0.0, 0.0); 2];
        for pol in [RISE, FALL] {
            let (slew_max, slew_min, drv_early, drv_late) = drive[pol];
            // An input delay is referenced to the clock's rising edge: launch parity 0.
            per_pol[pol] = (
                latency + launch_edge + in_max + drv_late,
                latency + launch_edge + in_min + drv_early,
                slew_max,
                slew_min,
            );
            state_ports[p].merge(
                pol,
                per_pol[pol].0,
                per_pol[pol].1,
                per_pol[pol].0,
                per_pol[pol].1,
                per_pol[pol].2,
                per_pol[pol].3,
                1,
                0,
                1,
                1,
                dom_bit(dom as usize),
                dom_bit(dom as usize),
            );
        }
        port_seeds.push(PortSeed { pin, dom, per_pol });
    }

    let mut base_state = vec![PinTimingState::UNREACHED; design.num_pins()];
    // The ideal walk runs unconditionally: `clock_domain` is every consumer's
    // capture-domain identity in both clock modes, and `ideal_par` supplies
    // parity wherever the propagated walk does not reach (ideal domains, and
    // pins behind cells `propagate_clock` cannot cross).
    let (ideal_par, clock_domain) = ideal_clock_parity(&cx, &levelized);
    let mut clock_parity = ideal_par.clone();
    let mut clock_state: Option<Vec<PinTimingState>> = None;
    let any_propagated = sdc.clocks.iter().any(|c| c.propagated);
    // OpenSTA's clock-pin vertices carry propagated source-latency +
    // cell-delay arrivals even under the ideal clock — `report_arrival`
    // shows them and `report_pulse_width_checks` measures pulse widths
    // from them. The walk therefore runs whenever any row is propagated or
    // any pulse-width check exists; it seeds every design row's port, so a
    // mixed-mode design still gets real arrivals on each clock network.
    if any_propagated || !graph.pulse_width_checks.is_empty() {
        // `set_clock_latency` without `-source` is ideal network latency and does
        // not apply; `-source` latency and `set_clock_transition` seed the source
        // port. Slew comes from the unified pass: OpenSTA merges clock and data
        // arcs at every vertex, so a clock gate's enable arc can dominate the
        // slew its output pin (and downstream clock pins) see. `propagate_clock`
        // uses those slews for delay lookups; the returned states carry arrival,
        // and their slew fields are overwritten with the unified values below.
        let unified_slew = propagate_slew(&cx, &graph, &wire_delays_per_net, &state_ports)?;
        let (mut clk_state, clk_parity) =
            propagate_clock(&cx, &levelized, &wire_delays_per_net, &unified_slew);
        for (c, u) in clk_state.iter_mut().zip(unified_slew.iter()) {
            c.slew_max = u.slew_max;
            c.slew_min = u.slew_min;
        }
        if any_propagated {
            // The propagated parity is authoritative only on propagated rows;
            // ideal domains keep the ideal walk's answer so a clock pin on an
            // unpropagated domain reads parity exactly as the all-ideal mode
            // reports it.
            for (i, p) in clk_parity.iter().enumerate() {
                let propagated = clock_domain[i] != u8::MAX
                    && sdc.clocks[clock_domain[i] as usize].propagated;
                if propagated && (p[RISE] != u8::MAX || p[FALL] != u8::MAX) {
                    clock_parity[i] = *p;
                }
            }
            let consumers = graph
                .launches
                .iter()
                .map(|l| l.clk)
                .chain(graph.checks.iter().filter_map(|(_, c)| c.clk))
                .chain(graph.latch_opens.iter().map(|o| o.enable))
                .chain(graph.gating_enables.iter().map(|g| g.clk_pin))
                .chain(graph.pulse_width_checks.iter().map(|c| c.pin));
            for pin in consumers {
                let idx = pin.0 as usize;
                let dom = clock_domain[idx];
                if dom == u8::MAX || !sdc.clocks[dom as usize].propagated {
                    continue;
                }
                let s = clk_state[idx];
                if s.reached(RISE) || s.reached(FALL) {
                    base_state[idx] = s;
                }
            }
            // Clock nets on propagated domains the propagation did not reach
            // (e.g. behind a latch or an unmapped cell) keep no timing; their
            // endpoints surface as unreached. A cell-driven net on an ideal
            // domain is the same approximation the all-ideal mode reports in
            // `build_graph`, so it counts here too.
            for (net, _) in graph.clock_net.iter().enumerate().filter(|(_, c)| **c) {
                if let Some(d) = design.net_driver(NetId(net as u32))
                    && design.pin_cells[d.0 as usize].is_some()
                {
                    let idx = d.0 as usize;
                    let dom = clock_domain[idx];
                    let modelled = dom != u8::MAX
                        && sdc.clocks[dom as usize].propagated
                        && (clk_state[idx].reached(RISE) || clk_state[idx].reached(FALL));
                    if !modelled {
                        skipped.cell_driven_clock_nets += 1;
                    }
                }
            }
        }
        clock_state = Some(clk_state);
    }
    // Ideal-clock seeding: every consumer whose domain row is not propagated
    // sees an edge at that row's clock latency (source + network), but the
    // edge's phase still follows the clock network: `ideal_clock_parity`
    // tracks inverting cells so a transition produced by clock-edge parity
    // `p` is seeded half a period later per unit of `p` (rise on edge 0 at
    // the latency, fall on edge 1 half a period on). `set_clock_transition`
    // is the clock pin's slew; without it the slew is zero. A propagated row
    // whose network did not reach the pin leaves it UNREACHED — the NoClock
    // skip — rather than silently taking an ideal edge.
    let clock_pins = graph
        .launches
        .iter()
        .map(|l| l.clk)
        .chain(graph.checks.iter().filter_map(|(_, c)| c.clk))
        .chain(graph.latch_opens.iter().map(|o| o.enable))
        .chain(graph.gating_enables.iter().map(|g| g.clk_pin))
        .chain(graph.pulse_width_checks.iter().map(|c| c.pin));
    for clk in clock_pins {
        let idx = clk.0 as usize;
        let dom = clock_domain[idx];
        if dom == u8::MAX || sdc.clocks[dom as usize].propagated {
            continue;
        }
        let row = &sdc.clocks[dom as usize];
        let ideal_latency = row.latency_ns + row.source_latency_ns;
        let transition = row.transition_ns;
        for pol in [RISE, FALL] {
            // Unreached pins keep the source-port convention: transition
            // `pol` sits on clock-edge parity `pol`.
            let p = ideal_par[idx][pol];
            let edge_par = if p <= 1 { p as usize } else { pol };
            base_state[idx].merge(
                pol,
                ideal_latency + row.edge_offset(edge_par),
                ideal_latency + row.edge_offset(edge_par),
                ideal_latency + row.edge_offset(edge_par),
                ideal_latency + row.edge_offset(edge_par),
                transition,
                transition,
                1 << edge_par,
                1 << edge_par,
                0,
                0,
                dom_bit(dom as usize),
                dom_bit(dom as usize),
            );
        }
    }
    let clock_parity = Some(clock_parity);

    // A launch produces separate late/early arrivals: the clock edge time is not
    // derated, but the cell arc delay is, per `set_timing_derate`. `edge_pol` selects which
    // clock transition launches: RISE for flip-flops, FALL for the opening of a `dlxtn` latch.
    // The clock edge's phase is already inside `clk_state`'s arrivals (real under a
    // propagated clock, parity-seeded under the ideal clock).
    // `delay_edge` is true only for latch open-edge launches: the delay-check
    // arrival (`dly`) re-bases them to the delay edge by stripping the
    // opening clock edge (measured on `latch_borrow` `delay_allregs`:
    // `u_f0/D` reports `u_ln`'s clock-to-Q delay alone). Flop and port
    // launches keep their clock edge, so their `dly` equals `arr`.
    let eval_launch = |q: PinId,
                       clk_state: PinTimingState,
                       lib_arc: u32,
                       edge_pol: usize,
                       delay_edge: bool|
     -> [LaunchPair; 2] {
        let q_net = design.pin_nets[q.0 as usize];
        let arc = &lib.arcs[lib_arc as usize];
        let mut res = [LaunchPair::UNREACHED; 2];
        // An unreached clock edge has infinite slew; do not feed it to the delay calculator.
        if !clk_state.reached(edge_pol) {
            return res;
        }
        if let Some(q_net) = q_net {
            for dst_pol in [RISE, FALL] {
                if let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol) {
                    let (late_delay, late_slew) = cx.driver_response(
                        q_net,
                        dst_pol,
                        delay_table,
                        slew_table,
                        clk_state.slew_max[edge_pol],
                    );
                    let (early_delay, early_slew) =
                        if clk_state.slew_min[edge_pol] == clk_state.slew_max[edge_pol] {
                            (late_delay, late_slew)
                        } else {
                            cx.driver_response(
                                q_net,
                                dst_pol,
                                delay_table,
                                slew_table,
                                clk_state.slew_min[edge_pol],
                            )
                        };
                    let arr_max = clk_state.arrival_max[edge_pol]
                        + late_delay * sdc.derate(true, DERATE_CELL, false, edge_pol);
                    let arr_min = clk_state.arrival_min[edge_pol]
                        + early_delay * sdc.derate(false, DERATE_CELL, false, edge_pol);
                    let (dly_max, dly_min) = if delay_edge {
                        (
                            arr_max - clk_state.arrival_max[edge_pol],
                            arr_min - clk_state.arrival_min[edge_pol],
                        )
                    } else {
                        (arr_max, arr_min)
                    };
                    res[dst_pol] = LaunchPair {
                        q,
                        dst_pol,
                        arr_max,
                        arr_min,
                        dly_max,
                        dly_min,
                        slew_max: late_slew,
                        slew_min: early_slew,
                    };
                }
            }
        }
        res
    };
    // Clock-edge parity (mod 2) of the edge that launches `edge` at clock pin `clk`:
    // the propagated clock's tracked parity, or the ideal clock's parity walk
    // (which sees inverting paths). Recorded on the launched arrivals so endpoint
    // checks can tell whether the capture edge is the first edge after the launch.
    let launch_parity = |clk: PinId, edge: usize| -> u8 {
        clock_parity
            .as_ref()
            .and_then(|p| p.get(clk.0 as usize).map(|e| e[edge]))
            .filter(|&v| v <= 1)
            .unwrap_or(edge as u8)
    };
    // CRPR launch tags: each distinct `(launch clock net, launch edge)` gets a
    // bit so a data-pin extreme records which clock network launched it — the
    // credit is per path, not per cone. Launches whose clock pin sits on no
    // net, and every launch once the table overflows the `u8` mask, degrade to
    // the sentinel bit (no credit).
    let mut crpr_tags: Vec<(NetId, usize)> = vec![(NetId(u32::MAX), usize::MAX)];
    for l in &graph.launches {
        let Some(net) = design.pin_nets[l.clk.0 as usize] else {
            continue;
        };
        let tag = (net, l.edge);
        if !crpr_tags.contains(&tag) {
            crpr_tags.push(tag);
        }
    }
    let launch_tag = |l: &LaunchArc| -> u8 {
        if crpr_tags.len() > u8::BITS as usize {
            return 1;
        }
        let Some(net) = design.pin_nets[l.clk.0 as usize] else {
            return 1;
        };
        let idx = crpr_tags
            .iter()
            .position(|&t| t == (net, l.edge))
            .expect("tag interned above");
        1 << idx
    };
    // Launch and latch-open contributions are run-invariant — they read only
    // clock-pin state, which `base_state` already carries — so the pairs are
    // computed once here; `eval_sta` merges them filtered by allowed source.
    let launch_seeds: Vec<LaunchSeed> = if graph.launches.len() < 64 {
        graph
            .launches
            .iter()
            .map(|l| LaunchSeed {
                q: l.q,
                par: launch_parity(l.clk, l.edge),
                tag: launch_tag(l),
                dom: clock_domain[l.clk.0 as usize],
                pairs: eval_launch(l.q, base_state[l.clk.0 as usize], l.lib_arc, l.edge, false),
            })
            .collect()
    } else {
        graph
            .launches
            .par_iter()
            .map(|l| LaunchSeed {
                q: l.q,
                par: launch_parity(l.clk, l.edge),
                tag: launch_tag(l),
                dom: clock_domain[l.clk.0 as usize],
                pairs: eval_launch(l.q, base_state[l.clk.0 as usize], l.lib_arc, l.edge, false),
            })
            .collect()
    };
    // A latch opens on the clock transition named by `open_pol` (FALL for `dlxtn`,
    // RISE for `dlxtp`). The enable pin's seeded state already carries the edge's
    // phase (real under a propagated clock, parity-seeded under the ideal clock).
    // `open_pol` is passed in both modes so `-fall` derates reach the arc.
    let latch_seeds: Vec<LaunchSeed> = graph
        .latch_opens
        .iter()
        .map(|open| LaunchSeed {
            q: open.q,
            par: launch_parity(open.enable, open.open_pol),
            // Latch clock-to-Q launches take the sentinel: latch CRPR is not
            // modelled, so arrivals through the open edge claim no credit.
            tag: 1,
            dom: clock_domain[open.enable.0 as usize],
            pairs: eval_launch(
                open.q,
                base_state[open.enable.0 as usize],
                open.lib_arc,
                open.open_pol,
                true,
            ),
        })
        .collect();

    Ok(StaPrepared {
        cx,
        graph,
        levelized,
        wire_delays_per_net,
        skipped,
        base_state,
        port_seeds,
        launch_seeds,
        latch_seeds,
        clock_parity,
        clock_domain,
        clock_state,
        crpr_tags,
    })
}

/// One exception-signature eval run: seed `base_state` with the allowed
/// sources' arrivals (every source still merges slew), propagate data through
/// the levelized graph, and collect endpoint candidates.
/// `disabled_borrow` holds `(D, Q)` latch borrow arcs silenced for this class:
/// a `set_max_delay` covering a latch's D pin stops the transparent `D -> Q`
/// forward for every downstream path of the class — covered endpoints or not
/// (measured on `latch_borrow` `delay_from`: `u_f2/D` loses the `p_lp -> u_lp
/// D -> Q` contribution even though its own launch is uncovered). Slew still
/// propagates — slew is physical, not path-scoped.
fn eval_sta(
    prepared: &StaPrepared,
    allowed: Option<&HashSet<u32>>,
    disabled_borrow: &HashSet<(u32, u32)>,
    skipped_seed: SkipReport,
) -> Result<EvalOutcome> {
    let cx = &prepared.cx;
    let (design, lib, sdc) = (cx.design, cx.lib, cx.sdc);
    let graph = &prepared.graph;
    let levelized = &prepared.levelized;
    let wire_delays_per_net = &prepared.wire_delays_per_net;
    let mut state = prepared.base_state.clone();
    for seed in &prepared.port_seeds {
        let p = seed.pin.0 as usize;
        let ok = allowed.is_none_or(|a| a.contains(&seed.pin.0));
        for pol in [RISE, FALL] {
            let (arr_max, arr_min, slew_max, slew_min) = seed.per_pol[pol];
            if ok {
                // A port is no flip-flop launch: the CRPR sentinel bit.
                state[p].merge(
                    pol,
                    arr_max,
                    arr_min,
                    arr_max,
                    arr_min,
                    slew_max,
                    slew_min,
                    1,
                    0,
                    1,
                    1,
                    dom_bit(seed.dom as usize),
                    dom_bit(seed.dom as usize),
                );
            } else {
                state[p].merge(
                    pol,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                    slew_max,
                    slew_min,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
            }
        }
    }
    for seed in prepared.launch_seeds.iter().chain(&prepared.latch_seeds) {
        let ok = allowed.is_none_or(|a| a.contains(&seed.q.0));
        for pair in seed.pairs.iter() {
            if pair.arr_max > f32::NEG_INFINITY {
                if ok {
                    state[pair.q.0 as usize].merge(
                        pair.dst_pol,
                        pair.arr_max,
                        pair.arr_min,
                        pair.dly_max,
                        pair.dly_min,
                        pair.slew_max,
                        pair.slew_min,
                        1 << seed.par,
                        1 << seed.par,
                        seed.tag,
                        seed.tag,
                        dom_bit(seed.dom as usize),
                        dom_bit(seed.dom as usize),
                    );
                } else {
                    state[pair.q.0 as usize].merge(
                        pair.dst_pol,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::INFINITY,
                        pair.slew_max,
                        pair.slew_min,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                    );
                }
            }
        }
    }

    for lvl in 1..levelized.level_offsets.len() - 1 {
        let level = levelized.level(lvl);
        let eval_pin = |state: &[PinTimingState], pin: PinId| -> Option<(usize, PinTimingState)> {
            let p = pin.0 as usize;
            let mut next = state[p];
            let mut changed = false;

            if !design.pin_is_driver[p]
                && let Some((net_idx, pin_idx, drv)) = levelized.pin_driver[p]
                // A pin on a clock net keeps the clock state the consumer seeding wrote;
                // a reached data driver must not overwrite it with a data arrival.
                && !graph.clock_net[net_idx as usize]
            {
                let driver_state = state[drv.0 as usize];
                if driver_state.reached(RISE) || driver_state.reached(FALL) {
                    let wire = wire_delays_per_net
                        .get(net_idx as usize)
                        .and_then(|w| w.as_ref());
                    let is_port = design.pin_cells[drv.0 as usize].is_none();
                    next = sink_state_with_wire(driver_state, wire, pin_idx, is_port, false, sdc);
                    changed = true;
                }
            }

            let incoming = levelized.incoming_to(PinId(p as u32));
            if !incoming.is_empty()
                && let Some(out_net) = design.pin_nets[p]
                && !graph.clock_net[out_net.0 as usize]
            {
                for &(src_pin, lib_arc) in incoming {
                    let arc = &lib.arcs[lib_arc as usize];
                    let src = state[src_pin.0 as usize];
                    let borrow = graph.latch_borrow.get(&(src_pin.0, p as u32));
                    // A covered `set_max_delay` silences the latch's transparent
                    // forward class-wide; the arc still merges slew.
                    let borrow_disabled =
                        borrow.is_some() && disabled_borrow.contains(&(src_pin.0, p as u32));
                    // OpenSTA propagates the transparent `D -> Q` path with the same
                    // transition at both ends (a `v` arrival at D launches `v` at Q),
                    // even where the Q_N arc is negative-unate, so latch borrow edges
                    // launch each transition from the same-polarity arrival.
                    let legs: &[(usize, usize)] = if borrow.is_some() {
                        &[(RISE, RISE), (FALL, FALL)]
                    } else {
                        polarities(arc.timing_sense)
                    };
                    for &(src_pol, dst_pol) in legs {
                        if !src.reached(src_pol) {
                            continue;
                        }
                        if (arc.timing_type == TimingType::CombinationalRise && dst_pol != RISE)
                            || (arc.timing_type == TimingType::CombinationalFall && dst_pol != FALL)
                        {
                            continue;
                        }
                        let (Some(delay_table), Some(slew_table)) = delay_tables(arc, dst_pol)
                        else {
                            continue;
                        };
                        let s_max = src.slew_max[src_pol];
                        let s_min = src.slew_min[src_pol];
                        let (late_delay, late_slew) =
                            cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_max);
                        let (early_delay, early_slew) = if s_min == s_max {
                            (late_delay, late_slew)
                        } else {
                            cx.driver_response(out_net, dst_pol, delay_table, slew_table, s_min)
                        };
                        if borrow_disabled {
                            merge_slew(&mut next, dst_pol, late_slew, early_slew);
                            changed = true;
                            continue;
                        }
                        // Latch `D -> Q` transparent path: only the late side borrows; the early
                        // arrival at Q comes from the open-edge clock-to-Q arc instead. The late
                        // launch is the arrival clamped to the borrow cap (the closing edge of
                        // the window holding the arrival, minus uncertainty and the per-transition
                        // setup). Data arriving before the latch opens contributes nothing — the
                        // clock-to-Q launch covers it. `dly` goes through the same cap; the early
                        // side contributes nothing (clock-to-Q-only launch rule, as for arrivals).
                        let (arr_in_max, dly_in_max) = if let Some(&li) = borrow {
                            let info = &graph.latch_info[li as usize];
                            let en = state[info.enable.0 as usize];
                            let close_pol = 1 - info.open_pol;
                            let en_dom = prepared.clock_domain[info.enable.0 as usize];
                            if !en.reached(info.open_pol)
                                || !en.reached(close_pol)
                                || en_dom == u8::MAX
                            {
                                continue;
                            }
                            // The borrow window and cap read the enable's own
                            // domain row — a latch on a second clock borrows
                            // against its period and uncertainty, not the
                            // design clock's.
                            let en_clk = &sdc.clocks[en_dom as usize];
                            let open = en.arrival_min[info.open_pol];
                            let mut close = en.arrival_min[close_pol];
                            if close <= open {
                                close += en_clk.period_ns;
                            }
                            let pulse = close - open;
                            let Some(setup_table) =
                                constraint_table(&lib.arcs[info.setup_arc as usize], src_pol)
                            else {
                                continue;
                            };
                            let setup =
                                eval_nldm_constraint(setup_table, s_max, en.slew_max[close_pol])
                                    * sdc.derate(
                                        true,
                                        DERATE_CHECK,
                                        false,
                                        check_transition(&lib.arcs[info.setup_arc as usize]),
                                    );
                            // Same accessor as every other eval-side
                            // uncertainty read: the arriving data's launch
                            // mask against the latch enable's domain row —
                            // identical to the row field on rule-free SDCs.
                            let unc =
                                unc_worst(sdc, src.dom_max[src_pol], en_dom as usize, false);
                            if src.arrival_max[src_pol] <= open - unc {
                                continue;
                            }
                            let cap = |arr: f32| {
                                let k =
                                    ((arr - open) / en_clk.period_ns).floor().max(0.0);
                                arr.min(
                                    open + k * en_clk.period_ns + pulse - unc - setup,
                                )
                            };
                            (cap(src.arrival_max[src_pol]), cap(src.dly_max[src_pol]))
                        } else {
                            (src.arrival_max[src_pol], src.dly_max[src_pol])
                        };
                        next.merge(
                            dst_pol,
                            arr_in_max + late_delay * sdc.derate(true, DERATE_CELL, false, src_pol),
                            if borrow.is_none() {
                                src.arrival_min[src_pol]
                                    + early_delay * sdc.derate(false, DERATE_CELL, false, src_pol)
                            } else {
                                f32::INFINITY
                            },
                            dly_in_max + late_delay * sdc.derate(true, DERATE_CELL, false, src_pol),
                            if borrow.is_none() {
                                src.dly_min[src_pol]
                                    + early_delay * sdc.derate(false, DERATE_CELL, false, src_pol)
                            } else {
                                f32::INFINITY
                            },
                            late_slew,
                            early_slew,
                            src.par_max[src_pol],
                            src.par_min[src_pol],
                            src.crpr_max[src_pol],
                            src.crpr_min[src_pol],
                            src.dom_max[src_pol],
                            src.dom_min[src_pol],
                        );
                        changed = true;
                    }
                }
            }

            changed.then_some((p, next))
        };

        if level.len() < 32 {
            for &pin in level {
                if let Some((p, next)) = eval_pin(&state, pin) {
                    state[p] = next;
                }
            }
        } else {
            let updates: Vec<(usize, PinTimingState)> = level
                .par_iter()
                .filter_map(|&pin| eval_pin(&state, pin))
                .collect();
            for (p, next) in updates {
                state[p] = next;
            }
        }
    }

    let (entries, pulse_rows, skipped) = collect_endpoints(
        cx,
        graph,
        levelized,
        prepared.clock_state.as_deref(),
        &state,
        prepared.clock_parity.as_deref(),
        &prepared.clock_domain,
        &prepared.crpr_tags,
        skipped_seed,
    )?;
    Ok(EvalOutcome {
        entries,
        pulse_rows,
        state,
        skipped,
    })
}

/// Sort endpoint entries by pin name and reduce them to the report plus the
/// per-endpoint polarities parallel to `report.endpoints`.
fn assemble_sta_report(
    mut entries: EndpointEntries,
    mut pulse_rows: Vec<PulseWidthCheckReport>,
    skipped: SkipReport,
) -> (StaReport, Vec<EndpointPolarities>) {
    entries.sort_by(|a, b| a.1.pin.cmp(&b.1.pin));
    pulse_rows.sort_by(|a, b| a.pin.cmp(&b.pin).then(a.edge.cmp(&b.edge)));
    let (endpoints, polarities): (Vec<_>, Vec<_>) =
        entries.into_iter().map(|(_, r, p)| (r, p)).unzip();
    let wns = |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).fold(f32::INFINITY, f32::min);
    let tns =
        |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).map(|s| s.min(0.0)).sum::<f32>();
    (
        StaReport {
            wns_max_ns: wns(|e| e.max_slack_ns),
            tns_max_ns: tns(|e| e.max_slack_ns),
            wns_min_ns: wns(|e| e.min_slack_ns),
            tns_min_ns: tns(|e| e.min_slack_ns),
            setup_violations: endpoints.iter().filter(|e| e.max_slack_ns < 0.0).count() as u32,
            hold_violations: endpoints.iter().filter(|e| e.min_slack_ns < 0.0).count() as u32,
            endpoints,
            pulse_width_checks: pulse_rows,
            skipped,
        },
        polarities,
    )
}

/// Endpoint classification for exception application. `Check { latch }`
/// distinguishes latch-D checks — their required time is piecewise under
/// time borrowing, so a multicycle path cannot shift it rigidly.
#[derive(Clone, Copy, PartialEq)]
enum ExceptionEndpointKind {
    Check { latch: bool },
    ClockGate,
    OutputPort,
}

/// Name/index context for resolving symbolic exception selectors against the
/// prepared design: launch sources, endpoint candidates, and the sequential
/// cells `all_registers` means.
struct ExceptionCtx<'p, 'd> {
    prepared: &'p StaPrepared<'d>,
    cell_by_name: HashMap<String, CellId>,
    seq_cells: HashSet<CellId>,
    launch_keys: HashSet<u32>,
    keys_by_cell: HashMap<CellId, Vec<u32>>,
    endpoint_pins: HashSet<u32>,
    eps_by_cell: HashMap<CellId, Vec<u32>>,
    output_ports: Vec<u32>,
    ep_kinds: HashMap<u32, ExceptionEndpointKind>,
    /// `instance/pin` -> PinId for every cell pin (`get_pins` and bare pin
    /// specs resolve through this; port pins are not in it, matching
    /// OpenSTA where `get_pins <port>` returns nothing usable).
    pin_by_path: HashMap<String, u32>,
    /// Sequential clock/enable pin -> the launch keys it launches. Measured
    /// OpenSTA semantics: `-from [get_pins inst/CLK]` selects the launches,
    /// while `inst/Q` and `inst/D` cover nothing.
    clk_launch: HashMap<u32, Vec<u32>>,
    /// Launch key -> `Sdc::clocks` row of the clock launching it: the raw
    /// `-clock` row for an input port (a virtual row keeps its identity),
    /// the clock pin's domain row for a register output. `ObjSel::Clocks`
    /// and bare clock `-name`s select through it (measured `t2_selfa`,
    /// `t2_selbare`); an unclocked port has no entry and is never covered.
    key_dom: HashMap<u32, usize>,
    /// Endpoint pin -> `Sdc::clocks` row it is captured/anchored on: check
    /// and gate rows come from `clock_domain`, an output port's from its
    /// declared `-clock` row. `-to [get_clocks]` selects through it
    /// (measured `t2_selfb`).
    ep_cap: HashMap<u32, usize>,
    unresolved: usize,
}

impl ExceptionCtx<'_, '_> {
    fn design(&self) -> &DesignDb {
        self.prepared.cx.design
    }

    fn sdc(&self) -> &Sdc {
        self.prepared.cx.sdc
    }

    /// `CellSel` -> the instance cells it names. Unresolvable instance names
    /// are counted, not fatal: the selector still applies to what it found.
    fn cell_sel_ids(&mut self, sel: &CellSel) -> HashSet<CellId> {
        match sel {
            CellSel::Cells(names) => {
                let mut ids = HashSet::new();
                for n in names {
                    match self.cell_by_name.get(n) {
                        Some(&c) => {
                            ids.insert(c);
                        }
                        None => self.unresolved += 1,
                    }
                }
                ids
            }
            CellSel::AllRegisters => self.seq_cells.clone(),
            CellSel::Except { base, excluded } => {
                let mut ids = self.cell_sel_ids(base);
                for n in excluded {
                    if let Some(&c) = self.cell_by_name.get(n) {
                        ids.remove(&c);
                    }
                }
                ids
            }
        }
    }

    /// Launch keys covered by one `instance/pin` spec. Measured OpenSTA
    /// semantics: only a sequential cell's clock/enable pin is a launch
    /// object — `inst/Q` and `inst/D` resolve but cover nothing. Unknown
    /// specs count as unresolved.
    fn pin_launches(&mut self, spec: &str, out: &mut HashSet<u32>) {
        match self.pin_by_path.get(spec) {
            Some(&pid) => {
                if let Some(qs) = self.clk_launch.get(&pid) {
                    out.extend(qs.iter().copied());
                }
            }
            None => self.unresolved += 1,
        }
    }

    /// Endpoint pins covered by one `instance/pin` spec: the pin must exist
    /// and be an endpoint (check data pin, clock-gate enable). Unknown specs
    /// count as unresolved.
    fn pin_endpoints(&mut self, spec: &str, out: &mut HashSet<u32>) {
        match self.pin_by_path.get(spec) {
            Some(&pid) if self.endpoint_pins.contains(&pid) => {
                out.insert(pid);
            }
            Some(_) => {}
            None => self.unresolved += 1,
        }
    }

    /// A clock `-name`'s `Sdc::clocks` row — the same name-only lookup
    /// `Parser::resolve_clock`/`clock_rows` apply (measured: a clock's
    /// source-port name is not a clock handle; `-from clk1` resolves to
    /// the port object, `t2_selport`).
    fn clock_row(&self, name: &str) -> Option<usize> {
        self.sdc().clocks.iter().position(|c| c.name == name)
    }

    /// Launch keys anchored on the given clock rows (`ObjSel::Clocks` and
    /// bare clock `-name`s resolve here).
    fn keys_on_clocks(&self, rows: &[usize]) -> HashSet<u32> {
        self.launch_keys
            .iter()
            .copied()
            .filter(|k| self.key_dom.get(k).is_some_and(|r| rows.contains(r)))
            .collect()
    }

    /// Endpoint pins captured on the given clock rows.
    fn eps_on_clocks(&self, rows: &[usize]) -> HashSet<u32> {
        self.endpoint_pins
            .iter()
            .copied()
            .filter(|p| self.ep_cap.get(p).is_some_and(|r| rows.contains(r)))
            .collect()
    }

    /// `ObjSel` -> `(launch-source pin coverage, resolved_any)`. Ports match
    /// only seeded data inputs (the clock port is never a launch source);
    /// cells match launch pins owned by the resolved instances.
    /// `resolved_any` is whether the selector resolved at least one object —
    /// OpenSTA's `no valid objects specified` fires only on zero resolved
    /// objects; a resolved object that covers nothing is a silent dead rule
    /// (measured `t2_selport`: `-from clk1` resolves to the port object,
    /// covers nothing, warns nothing).
    fn resolve_from(&mut self, sel: &ObjSel) -> (HashSet<u32>, bool) {
        let prepared = self.prepared;
        let design = prepared.cx.design;
        match sel {
            ObjSel::Ports(ps) => {
                let resolved_any = design.io_pins.iter().any(|&p| {
                    ps.matches(
                        prepared.cx.port_name(p),
                        design.pin_dirs[p.0 as usize] == PinDir::Input,
                        self.sdc(),
                    )
                });
                (
                    prepared
                        .port_seeds
                        .iter()
                        .filter(|s| ps.matches(prepared.cx.port_name(s.pin), true, self.sdc()))
                        .map(|s| s.pin.0)
                        .collect(),
                    resolved_any,
                )
            }
            ObjSel::Cells(cs) => {
                let ids = self.cell_sel_ids(cs);
                let resolved_any = !ids.is_empty();
                (
                    self.launch_keys
                        .iter()
                        .copied()
                        .filter(|&k| {
                            self.design().pin_cells[k as usize].is_some_and(|c| ids.contains(&c))
                        })
                        .collect(),
                    resolved_any,
                )
            }
            ObjSel::Pins(specs) => {
                let mut out = HashSet::new();
                let mut resolved_any = false;
                for spec in specs {
                    resolved_any |= self.pin_by_path.contains_key(spec);
                    self.pin_launches(spec, &mut out);
                }
                (out, resolved_any)
            }
            ObjSel::Clocks(rows) => (self.keys_on_clocks(rows), !rows.is_empty()),
            ObjSel::Names(names) => {
                let mut out = HashSet::new();
                let mut resolved_any = false;
                for n in names {
                    // Bare `instance/pin` names are pin specs, the same
                    // objects `get_pins` returns.
                    if n.contains('/') {
                        resolved_any |= self.pin_by_path.contains_key(n);
                        self.pin_launches(n, &mut out);
                        continue;
                    }
                    let mut hit = false;
                    for s in &prepared.port_seeds {
                        if prepared.cx.port_name(s.pin) == n {
                            out.insert(s.pin.0);
                            hit = true;
                        }
                    }
                    // A bare port name resolves to the port object whether
                    // or not it launches anything — resolved but dead when
                    // it is not a launch source (measured `t2_selport`).
                    if !hit && design.io_pins.iter().any(|&p| prepared.cx.port_name(p) == n) {
                        hit = true;
                    }
                    if let Some(&c) = self.cell_by_name.get(n)
                        && let Some(ks) = self.keys_by_cell.get(&c)
                    {
                        out.extend(ks.iter().copied());
                        hit = true;
                    }
                    // A bare clock `-name` resolves as the clock object
                    // (measured `t2_selbare`: `-from cka` cuts the same
                    // launches as `-from [get_clocks cka]`). A clock's
                    // source-port name is a port object, not a clock
                    // handle (`t2_selport`), so `clock_row` matches `-name`
                    // only.
                    if let Some(row) = self.clock_row(n) {
                        out.extend(self.keys_on_clocks(&[row]));
                        hit = true;
                    }
                    resolved_any |= hit;
                    if !hit {
                        self.unresolved += 1;
                    }
                }
                (out, resolved_any)
            }
        }
    }

    /// One `instance/pin` spec inside a path-delay `-from`: a clock pin keeps
    /// the launch-key expansion (`pin_launches`); a pin that is itself a
    /// launch element (a Q pin) is a startpoint pin — the delay check then
    /// times the segment from it (measured `t2_mdx`). Any other pin covers
    /// nothing. `out` gets the signature-coverage keys, `pins` the
    /// startpoints.
    fn delay_pin_spec(&mut self, spec: &str, out: &mut HashSet<u32>, pins: &mut HashSet<u32>) {
        match self.pin_by_path.get(spec) {
            Some(&pid) => {
                if let Some(qs) = self.clk_launch.get(&pid) {
                    out.extend(qs.iter().copied());
                } else if self.launch_keys.contains(&pid) {
                    out.insert(pid);
                    pins.insert(pid);
                }
            }
            None => self.unresolved += 1,
        }
    }

    /// `ObjSel` -> `(launch keys, startpoint pins, resolved_any)` for a
    /// path-delay `-from`. Ports and cells resolve like the exception
    /// commands; pin specs go through [`Self::delay_pin_spec`].
    /// `resolved_any` has the same zero-resolved-objects meaning as
    /// [`Self::resolve_from`].
    fn resolve_delay_from(&mut self, sel: &ObjSel) -> (HashSet<u32>, HashSet<u32>, bool) {
        let prepared = self.prepared;
        let design = prepared.cx.design;
        match sel {
            ObjSel::Pins(specs) => {
                let mut out = HashSet::new();
                let mut pins = HashSet::new();
                let mut resolved_any = false;
                for spec in specs {
                    resolved_any |= self.pin_by_path.contains_key(spec);
                    self.delay_pin_spec(spec, &mut out, &mut pins);
                }
                (out, pins, resolved_any)
            }
            ObjSel::Names(names) => {
                let mut out = HashSet::new();
                let mut pins = HashSet::new();
                let mut resolved_any = false;
                for n in names {
                    if n.contains('/') {
                        resolved_any |= self.pin_by_path.contains_key(n);
                        self.delay_pin_spec(n, &mut out, &mut pins);
                        continue;
                    }
                    let mut hit = false;
                    for s in &prepared.port_seeds {
                        if prepared.cx.port_name(s.pin) == n {
                            out.insert(s.pin.0);
                            hit = true;
                        }
                    }
                    if !hit && design.io_pins.iter().any(|&p| prepared.cx.port_name(p) == n) {
                        hit = true;
                    }
                    if let Some(&c) = self.cell_by_name.get(n)
                        && let Some(ks) = self.keys_by_cell.get(&c)
                    {
                        out.extend(ks.iter().copied());
                        hit = true;
                    }
                    if let Some(row) = self.clock_row(n) {
                        out.extend(self.keys_on_clocks(&[row]));
                        hit = true;
                    }
                    resolved_any |= hit;
                    if !hit {
                        self.unresolved += 1;
                    }
                }
                (out, pins, resolved_any)
            }
            _ => {
                let (out, resolved_any) = self.resolve_from(sel);
                (out, HashSet::new(), resolved_any)
            }
        }
    }

    /// `ObjSel` -> `(endpoint pin coverage, resolved_any)`. Ports match
    /// output ports only; cells match endpoint pins owned by the resolved
    /// instances (register checks and clock-gate enables alike). A bare
    /// port name resolves to the port object regardless of direction —
    /// `-to clk2` resolves dead (measured `t2_selpto`: OpenSTA warns
    /// `not a valid endpoint` and covers nothing).
    fn resolve_to(&mut self, sel: &ObjSel) -> (HashSet<u32>, bool) {
        let prepared = self.prepared;
        let design = prepared.cx.design;
        match sel {
            ObjSel::Ports(ps) => {
                let resolved_any = design.io_pins.iter().any(|&p| {
                    ps.matches(
                        prepared.cx.port_name(p),
                        design.pin_dirs[p.0 as usize] == PinDir::Input,
                        self.sdc(),
                    )
                });
                (
                    self.output_ports
                        .iter()
                        .copied()
                        .filter(|&p| {
                            ps.matches(prepared.cx.port_name(PinId(p)), false, self.sdc())
                        })
                        .collect(),
                    resolved_any,
                )
            }
            ObjSel::Cells(cs) => {
                let ids = self.cell_sel_ids(cs);
                let resolved_any = !ids.is_empty();
                (
                    self.endpoint_pins
                        .iter()
                        .copied()
                        .filter(|&p| {
                            self.design().pin_cells[p as usize].is_some_and(|c| ids.contains(&c))
                        })
                        .collect(),
                    resolved_any,
                )
            }
            ObjSel::Pins(specs) => {
                let mut out = HashSet::new();
                let mut resolved_any = false;
                for spec in specs {
                    resolved_any |= self.pin_by_path.contains_key(spec);
                    self.pin_endpoints(spec, &mut out);
                }
                (out, resolved_any)
            }
            ObjSel::Clocks(rows) => (self.eps_on_clocks(rows), !rows.is_empty()),
            ObjSel::Names(names) => {
                let mut out = HashSet::new();
                let mut resolved_any = false;
                for n in names {
                    if n.contains('/') {
                        resolved_any |= self.pin_by_path.contains_key(n);
                        self.pin_endpoints(n, &mut out);
                        continue;
                    }
                    let mut hit = false;
                    for &p in &self.output_ports {
                        if prepared.cx.port_name(PinId(p)) == n {
                            out.insert(p);
                            hit = true;
                        }
                    }
                    if !hit && design.io_pins.iter().any(|&p| prepared.cx.port_name(p) == n) {
                        hit = true;
                    }
                    if let Some(&c) = self.cell_by_name.get(n)
                        && let Some(eps) = self.eps_by_cell.get(&c)
                    {
                        out.extend(eps.iter().copied());
                        hit = true;
                    }
                    if let Some(row) = self.clock_row(n) {
                        out.extend(self.eps_on_clocks(&[row]));
                        hit = true;
                    }
                    resolved_any |= hit;
                    if !hit {
                        self.unresolved += 1;
                    }
                }
                (out, resolved_any)
            }
        }
    }
}

/// A timing exception with its selectors resolved to concrete pin sets.
/// `from`/`to` are `None` when the flag was absent (every launch source /
/// every endpoint); `Some(empty)` means the selector resolved to nothing and
/// was diagnosed — it covers no source or endpoint.
struct ResolvedException {
    kind: ExceptionKind,
    from: Option<HashSet<u32>>,
    to: Option<HashSet<u32>>,
    setup_n: Option<u32>,
    hold_n: Option<i32>,
}

/// A `set_max_delay`/`set_min_delay` with its selectors resolved to concrete
/// pin sets — the same `from`/`to` convention as [`ResolvedException`], plus
/// `from_pins`: `get_pins inst/<non-clock-pin>` `-from` specs are startpoint
/// pins, not launch selectors — measured OpenSTA semantics (`t2_mdx`): the
/// pin itself becomes the path startpoint, so the delay check times the
/// segment from it (arrival measured from the pin, no clock anchor at all).
/// `None` when the rule has no pin-startpoint `-from`.
struct ResolvedPathDelay {
    kind: PathDelayKind,
    from: Option<HashSet<u32>>,
    to: Option<HashSet<u32>>,
    from_pins: Option<HashSet<u32>>,
    value_ns: f32,
}

/// One timing constraint after selector resolution: a launch source's
/// signature is the file-ordered index list of the rules in this vector whose
/// `-from` covers it, so exceptions and path delays share the signature.
enum ResolvedRule {
    Exception(ResolvedException),
    Delay(ResolvedPathDelay),
}

impl ResolvedRule {
    fn from(&self) -> &Option<HashSet<u32>> {
        match self {
            ResolvedRule::Exception(e) => &e.from,
            ResolvedRule::Delay(d) => &d.from,
        }
    }

    fn to(&self) -> &Option<HashSet<u32>> {
        match self {
            ResolvedRule::Exception(e) => &e.to,
            ResolvedRule::Delay(d) => &d.to,
        }
    }
}

/// The reason a reached endpoint still produced no report, shared by
/// `record_endpoint` and the partitioned skip pass.
fn endpoint_skip_reason(
    cx: &Context,
    levelized: &LevelizedGraph,
    state: &[PinTimingState],
    kind: EndpointKind,
    pin: PinId,
) -> (SkipReason, Option<String>) {
    let s = state[pin.0 as usize];
    // A reached check pin with no candidate means every arrived transition
    // lacked a usable NLDM constraint table; gates and ports have no such
    // table lookup, so their only failure is an unreached pin.
    if kind == EndpointKind::Check && (s.reached(RISE) || s.reached(FALL)) {
        return (SkipReason::NoConstraintTable, None);
    }
    match levelized.pin_driver[pin.0 as usize] {
        Some((_, _, drv)) => {
            // A driver cell with no Liberty mapping or no timing arcs at all
            // (constant `conb`, tap, filler) can never deliver a timed arrival;
            // name it so the skip reads as the expected untimed case.
            let untimed = cx.design.pin_cells[drv.0 as usize].is_some_and(|c| {
                match cx.design.cell_lib_ids[c.0 as usize] {
                    Some(lcid) => cx.lib.cells[lcid.0 as usize].arc_count == 0,
                    None => true,
                }
            });
            (
                if untimed {
                    SkipReason::UntimedDriver
                } else {
                    SkipReason::NoPath
                },
                Some(cx.pin_name(drv)),
            )
        }
        None => (SkipReason::NoPath, None),
    }
}

/// Fanout-cone leaves of one launch key: pins with neither net sinks nor
/// outgoing graph edges. A `set_max_delay`/`set_min_delay` whose `-from`
/// covers the key produces an "internal path endpoint" row on each leaf
/// (measured `t2_selmd`: `u_fy/Q`, a fanout-free net driver).
fn dead_end_leaves(design: &DesignDb, graph: &TimingGraph, start: PinId) -> Vec<u32> {
    let mut leaves = Vec::new();
    let mut stack = vec![start];
    let mut seen = HashSet::new();
    while let Some(pin) = stack.pop() {
        if !seen.insert(pin) {
            continue;
        }
        let mut fwd = false;
        if let Some(net) = design.pin_nets[pin.0 as usize]
            && design.net_driver(net) == Some(pin)
        {
            for &sink in design.net_pins(net).iter().skip(1) {
                if sink != pin {
                    stack.push(sink);
                    fwd = true;
                }
            }
        }
        for &e in graph.outgoing(pin) {
            stack.push(graph.edges[e as usize].dst);
            fwd = true;
        }
        if !fwd {
            leaves.push(pin.0);
        }
    }
    leaves
}

fn emit_sta_warnings(report: &StaReport) {
    for category in report.skipped.nonzero() {
        // Deferred check arcs are a documented, counted deferral shown in the report —
        // not an unrecognised gap, and `non_seq_*`/`clear` alone would warn on every
        // flop design. They stay in `nonzero()`/the summary and JSON.
        if category.starts_with("recognized_check_arcs") {
            continue;
        }
        eprintln!("warning: STA skipped {category}; the report does not model these");
    }
}

/// STA with timing exceptions applied: resolve selectors, partition launch
/// sources by exception signature, evaluate each class once, then compose
/// per-endpoint candidates — false paths drop covered classes, multicycle
/// paths shift required times by the measured OpenSTA formulas.
fn run_sta_partitioned(
    prepared: &StaPrepared,
    emit_warnings: bool,
) -> Result<(StaReport, Vec<PinTimingState>, Vec<EndpointPolarities>)> {
    let cx = &prepared.cx;
    let (design, lib, sdc) = (cx.design, cx.lib, cx.sdc);
    let graph = &prepared.graph;

    let mut cell_by_name = HashMap::new();
    for c in 0..design.num_cells() {
        cell_by_name.insert(
            design.strings.resolve(design.cell_names[c]).to_string(),
            CellId(c as u32),
        );
    }
    // Sequential cells own a register check or a launch arc; `all_registers`
    // covers exactly these — never input-port launch sources.
    let mut seq_cells = HashSet::new();
    for &(d, _) in &graph.checks {
        if let Some(c) = design.pin_cells[d as usize] {
            seq_cells.insert(c);
        }
    }
    for s in prepared.launch_seeds.iter().chain(&prepared.latch_seeds) {
        if let Some(c) = design.pin_cells[s.q.0 as usize] {
            seq_cells.insert(c);
        }
    }

    let mut launch_keys = HashSet::new();
    let mut keys_by_cell: HashMap<CellId, Vec<u32>> = HashMap::new();
    // Clock rows each launch key is anchored on: a port reads its declared
    // `-clock` (a virtual row keeps its identity for `get_clocks` matching),
    // a register output the domain row of its clock pin.
    let mut key_dom: HashMap<u32, usize> = HashMap::new();
    for s in &prepared.port_seeds {
        launch_keys.insert(s.pin.0);
        if let Some(row) = sdc.declared_input_clock_row(cx.port_name(s.pin)) {
            key_dom.insert(s.pin.0, row);
        }
    }
    for s in prepared.launch_seeds.iter().chain(&prepared.latch_seeds) {
        launch_keys.insert(s.q.0);
        if s.dom != u8::MAX {
            key_dom.insert(s.q.0, s.dom as usize);
        }
        if let Some(c) = design.pin_cells[s.q.0 as usize] {
            keys_by_cell.entry(c).or_default().push(s.q.0);
        }
    }

    let mut endpoint_pins = HashSet::new();
    let mut eps_by_cell: HashMap<CellId, Vec<u32>> = HashMap::new();
    let mut ep_kinds = HashMap::new();
    // Clock rows each endpoint is captured/anchored on — the `-to`
    // `[get_clocks]`/group side of the launch/capture pair.
    let mut ep_cap: HashMap<u32, usize> = HashMap::new();
    for &(d, check) in &graph.checks {
        endpoint_pins.insert(d);
        ep_kinds.insert(d, ExceptionEndpointKind::Check { latch: check.latch });
        if let Some(clk) = check.clk {
            let row = prepared.clock_domain[clk.0 as usize];
            if row != u8::MAX {
                ep_cap.insert(d, row as usize);
            }
        }
        if let Some(c) = design.pin_cells[d as usize] {
            eps_by_cell.entry(c).or_default().push(d);
        }
    }
    for g in &graph.gating_enables {
        let e = g.enable.0;
        endpoint_pins.insert(e);
        ep_kinds
            .entry(e)
            .or_insert(ExceptionEndpointKind::ClockGate);
        let row = prepared.clock_domain[g.clk_pin.0 as usize];
        if row != u8::MAX {
            ep_cap.insert(e, row as usize);
        }
        if let Some(c) = design.pin_cells[e as usize] {
            eps_by_cell.entry(c).or_default().push(e);
        }
    }
    let mut output_ports = Vec::new();
    for &p in &design.io_pins {
        if design.pin_dirs[p.0 as usize] == PinDir::Output {
            endpoint_pins.insert(p.0);
            output_ports.push(p.0);
            ep_kinds
                .entry(p.0)
                .or_insert(ExceptionEndpointKind::OutputPort);
            if let Some(row) = sdc.declared_output_clock_row(cx.port_name(p)) {
                ep_cap.insert(p.0, row);
            }
        }
    }

    let mut pin_by_path = HashMap::new();
    for c in 0..design.num_cells() {
        let cell = CellId(c as u32);
        let inst = design.strings.resolve(design.cell_names[c]).to_string();
        for &p in design.cell_pins(cell) {
            let pname = design.strings.resolve(design.pin_names[p.0 as usize]);
            pin_by_path.insert(format!("{inst}/{pname}"), p.0);
        }
    }
    let mut clk_launch: HashMap<u32, Vec<u32>> = HashMap::new();
    for l in &graph.launches {
        clk_launch.entry(l.clk.0).or_default().push(l.q.0);
    }
    for o in &graph.latch_opens {
        clk_launch.entry(o.enable.0).or_default().push(o.q.0);
    }

    let mut ctx = ExceptionCtx {
        prepared,
        cell_by_name,
        seq_cells,
        launch_keys,
        keys_by_cell,
        endpoint_pins,
        eps_by_cell,
        output_ports,
        ep_kinds,
        pin_by_path,
        clk_launch,
        key_dom,
        ep_cap,
        unresolved: 0,
    };

    let mut rules = Vec::with_capacity(sdc.exceptions.len() + sdc.path_delays.len());
    // A selector side is unresolved only when it resolves zero objects —
    // OpenSTA's `no valid objects specified` form. A resolved object that
    // covers nothing is a silent dead rule, not a diagnostic.
    let mut resolve_sides = |from_sel: Option<&ObjSel>, to_sel: Option<&ObjSel>| {
        let from = from_sel.map(|sel| {
            let (keys, resolved_any) = ctx.resolve_from(sel);
            ctx.unresolved += usize::from(!resolved_any);
            keys
        });
        let to = to_sel.map(|sel| {
            let (eps, resolved_any) = ctx.resolve_to(sel);
            ctx.unresolved += usize::from(!resolved_any);
            eps
        });
        (from, to)
    };
    for ex in &sdc.exceptions {
        let (from, to) = resolve_sides(ex.from.as_ref(), ex.to.as_ref());
        rules.push(ResolvedRule::Exception(ResolvedException {
            kind: ex.kind,
            from,
            to,
            setup_n: ex.setup_n,
            hold_n: ex.hold_n,
        }));
    }
    for pd in &sdc.path_delays {
        let (from, from_pins, resolved_any) = pd
            .from
            .as_ref()
            .map(|sel| ctx.resolve_delay_from(sel))
            .unwrap_or_default();
        if pd.from.is_some() && !resolved_any {
            ctx.unresolved += 1;
        }
        let to = pd.to.as_ref().map(|sel| {
            let (eps, resolved_any) = ctx.resolve_to(sel);
            ctx.unresolved += usize::from(!resolved_any);
            eps
        });
        rules.push(ResolvedRule::Delay(ResolvedPathDelay {
            kind: pd.kind,
            from: pd.from.as_ref().map(|_| from),
            to,
            from_pins: (!from_pins.is_empty()).then_some(from_pins),
            value_ns: pd.value_ns,
        }));
    }

    // `set_clock_groups` desugars to false-path pairs on clock rows
    // (measured `t2_grp`, `t2_grp1`): each `-group` makes its members
    // exclusive against every clock not in that group — same-group pairs
    // stay synchronous, and a lone group cuts its members against all other
    // clocks. The three mode flags share this exclusion path
    // (`t2_grpl`/`t2_grpp` measured identical); `-allow_paths` stores no
    // group at all. Group coverage anchors on the same declared rows the
    // selectors use, so a group cut also voids a delay rule crossing the
    // pair (`t2_grpd`/`t2_grpd2`).
    for cg in &sdc.clock_groups {
        for grp in &cg.groups {
            let in_group = |row: &usize| grp.contains(row);
            let keys_where = |member: bool| {
                ctx.launch_keys
                    .iter()
                    .copied()
                    .filter(|k| ctx.key_dom.get(k).is_some_and(|r| in_group(r) == member))
                    .collect::<HashSet<u32>>()
            };
            let eps_where = |member: bool| {
                ctx.endpoint_pins
                    .iter()
                    .copied()
                    .filter(|p| ctx.ep_cap.get(p).is_some_and(|r| in_group(r) == member))
                    .collect::<HashSet<u32>>()
            };
            for (from, to) in [
                (keys_where(true), eps_where(false)),
                (keys_where(false), eps_where(true)),
            ] {
                if from.is_empty() || to.is_empty() {
                    continue;
                }
                rules.push(ResolvedRule::Exception(ResolvedException {
                    kind: ExceptionKind::FalsePath,
                    from: Some(from),
                    to: Some(to),
                    setup_n: None,
                    hold_n: None,
                }));
            }
        }
    }

    // A launch source's signature is the file-ordered rule indices whose
    // `from` covers it (absent `from` = universal). Sources sharing a
    // signature see identical applicable rules at every endpoint, so one
    // eval run per signature class is sufficient. Path-delay coverage is part
    // of the signature: a covered source contributes a delay-check candidate
    // instead of a clock-check candidate, which an aggregate would lose.
    let mut class_map: HashMap<Vec<u32>, HashSet<u32>> = HashMap::new();
    for &k in &ctx.launch_keys {
        let mut sig = Vec::new();
        for (i, rule) in rules.iter().enumerate() {
            if rule.from().as_ref().is_none_or(|s| s.contains(&k)) {
                sig.push(i as u32);
            }
        }
        class_map.entry(sig).or_default().insert(k);
    }
    let mut classes: Vec<(Vec<u32>, HashSet<u32>)> = class_map.into_iter().collect();
    classes.sort_by(|a, b| a.0.cmp(&b.0));

    let mut class_maps = Vec::with_capacity(classes.len());
    let mut class_states = Vec::with_capacity(classes.len());
    for (sigma, keys) in &classes {
        // A `set_max_delay` covering a latch's D pin silences its transparent
        // `D -> Q` borrow arc for the whole signature class — downstream paths
        // of every source in the class lose the borrowed contribution,
        // whether or not their own endpoint is covered (measured:
        // `delay_from`'s `u_f2/D`). `set_min_delay` coverage does not disable.
        let mut disabled_borrow = HashSet::new();
        for &ri in sigma {
            if let ResolvedRule::Delay(d) = &rules[ri as usize]
                && d.kind == PathDelayKind::Max
            {
                for &(d_pin, q_pin) in graph.latch_borrow.keys() {
                    if d.to.as_ref().is_none_or(|t| t.contains(&d_pin)) {
                        disabled_borrow.insert((d_pin, q_pin));
                    }
                }
            }
        }
        let outcome = eval_sta(
            prepared,
            Some(keys),
            &disabled_borrow,
            SkipReport::default(),
        )?;
        class_maps.push(
            outcome
                .entries
                .iter()
                .map(|(pin, _, pols)| (pin.0, *pols))
                .collect::<HashMap<u32, EndpointPolarities>>(),
        );
        class_states.push(outcome.state);
    }

    // Compose per endpoint: a class's candidates survive unless a false path
    // covers them; a surviving rule then transforms the candidate — a delay
    // constraint replaces the check on its side, a multicycle shifts required
    // times by the measured OpenSTA formulas. The worst slack across
    // surviving (class, polarity) candidates wins.
    let check_by_pin: HashMap<u32, RegisterCheck> = graph.checks.iter().copied().collect();
    // The polarity of the first clock edge reaching a pin (`edge0`): OpenSTA
    // anchors latch and asynchronous-pin delay checks there, not at the check
    // edge (measured: `delay_allregs`'s `u_rbp/RESET_B` anchors at RISE while
    // its recovery check edge is FALL). Decoded from `clock_parity` — a RISE
    // state on edge parity 0 means the edge-0 transition is RISE; parity 1
    // (inverted clock net) makes it FALL. Absent parity falls back to RISE;
    // contested parity was already resolved by the parity walk — earliest
    // arrival under the propagated clock, first-written source under the
    // ideal one — and can be FALL.
    let edge0_pol = |clk: PinId| -> usize {
        match prepared
            .clock_parity
            .as_deref()
            .and_then(|p| p.get(clk.0 as usize))
            .map(|e| e[RISE])
        {
            Some(1) => FALL,
            _ => RISE,
        }
    };
    let mut composed: HashMap<u32, EndpointPolarities> = HashMap::new();
    let mut fp_endpoints: HashSet<u32> = HashSet::new();
    let mut unsupported_mc: HashSet<u32> = HashSet::new();
    let mut unsupported_pd: HashSet<u32> = HashSet::new();
    for (&ep, &kind) in &ctx.ep_kinds {
        let mut best_max: [Option<Worst>; 2] = [None; 2];
        let mut best_min: [Option<Worst>; 2] = [None; 2];
        let mut had = false;
        let mut survived = false;
        for (ci, (sigma, _)) in classes.iter().enumerate() {
            let Some(pols) = class_maps[ci].get(&ep) else {
                continue;
            };
            had = true;
            let mut drop = false;
            // Applicable multicycles merge per field in file order: a later
            // command overrides only the counts it specifies (measured:
            // `-setup 2` then `-hold 1` keeps setup 2 and takes hold 1).
            // Same-kind delay rules fold last-wins in file order (`md_overlap*`).
            let mut mc_setup: Option<u32> = None;
            let mut mc_hold: Option<i32> = None;
            let mut delay_max_n: Option<f32> = None;
            let mut delay_min_n: Option<f32> = None;
            let mut delay_max_pins: Option<HashSet<u32>> = None;
            let mut delay_min_pins: Option<HashSet<u32>> = None;
            // A pin-startpoint `-from` (`get_pins inst/Q`) replaces the
            // clocked launch of every source in the class with paths starting
            // at the pin (measured `t2_mdx`): an endpoint the rule's `-to`
            // does not cover loses the clocked check on that side entirely.
            let mut pin_from_sig_max = false;
            let mut pin_from_sig_min = false;
            for &ri in sigma {
                let rule = &rules[ri as usize];
                if let ResolvedRule::Delay(d) = rule
                    && d.from_pins.is_some()
                {
                    match d.kind {
                        PathDelayKind::Max => pin_from_sig_max = true,
                        PathDelayKind::Min => pin_from_sig_min = true,
                    }
                }
                if !rule.to().as_ref().is_none_or(|s| s.contains(&ep)) {
                    continue;
                }
                match rule {
                    ResolvedRule::Exception(ex) => match ex.kind {
                        ExceptionKind::FalsePath => {
                            drop = true;
                            break;
                        }
                        ExceptionKind::Multicycle => {
                            if ex.setup_n.is_some() {
                                mc_setup = ex.setup_n;
                            }
                            if ex.hold_n.is_some() {
                                mc_hold = ex.hold_n;
                            }
                        }
                    },
                    ResolvedRule::Delay(d) => match d.kind {
                        PathDelayKind::Max => {
                            delay_max_n = Some(d.value_ns);
                            delay_max_pins = d.from_pins.clone();
                        }
                        PathDelayKind::Min => {
                            delay_min_n = Some(d.value_ns);
                            delay_min_pins = d.from_pins.clone();
                        }
                    },
                }
            }
            if drop {
                continue;
            }
            survived = true;
            let mut pols = *pols;
            if pin_from_sig_max && delay_max_n.is_none() {
                pols.max = [None; 2];
            }
            if pin_from_sig_min && delay_min_n.is_none() {
                pols.min = [None; 2];
            }
            // Delay checks are modelled for every check pin carrying a check
            // arc — FF setup/hold, latch-D, and async recovery/removal — plus
            // output ports (measured `delay_*` goldens). Clock-gate enables
            // keep their clock-check values and stay counted.
            let delay_supported = match kind {
                ExceptionEndpointKind::OutputPort => true,
                ExceptionEndpointKind::Check { .. } => check_by_pin
                    .get(&ep)
                    .is_some_and(|c| c.late_arc().is_some() || c.early_arc().is_some()),
                _ => false,
            };
            if (delay_max_n.is_some() || delay_min_n.is_some()) && !delay_supported {
                unsupported_pd.insert(ep);
                delay_max_n = None;
                delay_min_n = None;
            }
            // Measured precedence per side (`md_mc*`, `md_min_mcp_*`): a delay
            // constraint replaces the clock check on its side, absorbing the
            // multicycle shift that side would otherwise take.
            if let Some(n) = delay_max_n
                && kind == ExceptionEndpointKind::OutputPort
            {
                // Measured (`mdo_max*`, `pmo_max*`): the port delay check is
                // referenced to the delay edge itself —
                // req = N - output_delay_max - setup_uncertainty in both
                // clock modes (no latency or period term; `pmo_*` shows the
                // propagated clock does not move the anchor). The reported
                // arrival is the delay arrival (identical to `arr` unless the
                // path launched through a latch open edge).
                let (_, delay_max) = sdc.output_delay_ns(cx.port_name(PinId(ep)));
                let out_row = sdc.output_clock_row(cx.port_name(PinId(ep)));
                let s = class_states[ci][ep as usize];
                // Cross-domain launch bits anchor at the capture domain's
                // edge 0 (delay checks skip pairing); each polarity's tying
                // launch mask picks the worst matching inter-clock rule.
                for pol in [RISE, FALL] {
                    let Some(mut w) = pols.max[pol] else { continue };
                    let unc = unc_worst(sdc, s.dom_max[pol], out_row, false);
                    if let Some(pins) = &delay_max_pins {
                        let Some(arr) =
                            pin_segment_arrival(pins, &class_states[ci], w.dly, pol, false)
                        else {
                            pols.max[pol] = None;
                            continue;
                        };
                        w.arr = arr;
                    } else {
                        w.arr = w.dly;
                    }
                    w.req = n - delay_max - unc;
                    w.slack = w.req - w.arr;
                    pols.max[pol] = Some(w);
                }
            }
            if let Some(n) = delay_max_n
                && kind != ExceptionEndpointKind::OutputPort
                && let Some(&check) = check_by_pin.get(&ep)
            {
                let s = class_states[ci][ep as usize];
                for pol in [RISE, FALL] {
                    let (Some(w), Some(late_arc), Some(clk)) =
                        (pols.max[pol], check.late_arc(), check.clk)
                    else {
                        continue;
                    };
                    let late = &lib.arcs[late_arc as usize];
                    let Some(table) = constraint_table(late, pol) else {
                        continue;
                    };
                    let clk_s = class_states[ci][clk.0 as usize];
                    // The delay check rewrites the required time on the
                    // endpoint register's own capture row.
                    let cap_row = prepared.clock_domain[clk.0 as usize] as usize;
                    let cap_clk = &sdc.clocks[cap_row];
                    let t_setup =
                        eval_nldm_constraint(table, s.slew_max[pol], clk_s.slew_max[check.edge])
                            * sdc.derate(true, DERATE_CHECK, false, check_transition(late));
                    if let Some(pins) = &delay_max_pins {
                        // Pin `-from` startpoint (measured `t2_mdx`): the
                        // check times the segment from the pin — arrival is
                        // the endpoint's delay minus the pin's own, required
                        // time `N - t_setup` with no clock anchor, latency,
                        // or uncertainty. Each reached pin yields its own
                        // constrained path; the largest segment arrival is
                        // worst, and with none reached the polarity drops.
                        let Some(arr) =
                            pin_segment_arrival(pins, &class_states[ci], w.dly, pol, false)
                        else {
                            pols.max[pol] = None;
                            continue;
                        };
                        let req = n - t_setup;
                        pols.max[pol] = Some(Worst {
                            arr,
                            req,
                            slack: req - arr,
                            ..w
                        });
                        continue;
                    }
                    if check.latch && check.setup_arc.is_some() {
                        // Latch-D delay check (measured `latch_borrow`
                        // `delay_*`): the delay edge opens a borrow window of
                        // the latch's own pulse width — OpenSTA's
                        // `report_checks` decomposes it as
                        // `req = N + lat0 - unc + actual_borrow` with
                        // `actual_borrow = clamp(dly - base, 0, max_borrow)`
                        // and `max_borrow = pulse - t_setup` (the borrow cap
                        // minus the data setup, like the transparent check).
                        // `arr` reports the re-based delay arrival;
                        // `slack = req - arr_dly` rigidly.
                        let open = clk_s.arrival_min[1 - check.edge];
                        let mut close = clk_s.arrival_min[check.edge];
                        if close <= open {
                            close += cap_clk.period_ns;
                        }
                        let base = n + clk_s.arrival_min[edge0_pol(clk)]
                            - unc_worst(sdc, s.dom_max[pol], cap_row, false);
                        let req = w.dly.clamp(base, base + (close - open - t_setup).max(0.0));
                        pols.max[pol] = Some(Worst {
                            arr: w.dly,
                            req,
                            slack: req - w.dly,
                            ..w
                        });
                        continue;
                    }
                    // req = capture - uncertainty - t_setup + N,
                    // slack = req - arr_dly. The reported arrival is the
                    // covered sources' delay arrival (`dly`): identical to the
                    // ordinary arrival for flop/port launches, re-based to the
                    // delay edge through a latch's open edge (measured
                    // `delay_allregs`: `u_f0/D` reports the clock-to-Q delay).
                    // The required time is anchored to the endpoint
                    // register's own clock pin (the early capture arrival,
                    // like the setup check), not the launch source's clock:
                    // under `set_propagated_clock` with skewed clock trees
                    // the two differ. Async recovery checks anchor at the
                    // delay edge (`edge0_pol`), not the check edge — measured
                    // on the latch-cell `u_rbp/RESET_B` (check.edge FALL,
                    // anchor RISE); identical on `dfrtp` where both are RISE.
                    // The delay check takes the same CRPR credit the clock
                    // check adds (measured `pcr_max`/`pcr_min`): the tag comes
                    // from this class's data-pin extreme.
                    let anchor = if check.recovery_arc.is_some() {
                        edge0_pol(clk)
                    } else {
                        check.edge
                    };
                    let capture = clk_s.arrival_min[anchor];
                    // `clock_state` is also filled under the ideal clock for
                    // the pulse-width checks, so gate on the clock mode like
                    // `credit_for` does — the credit is zero under ideal
                    // (measured: `pcr_*` scenarios all use
                    // `set_propagated_clock`).
                    let credit = if cap_clk.propagated {
                        design.pin_nets[clk.0 as usize].map_or(0.0, |capture_net| {
                            crpr_credit(
                                design,
                                prepared.clock_state.as_deref(),
                                &prepared.crpr_tags,
                                s.crpr_max[pol],
                                capture_net,
                                anchor,
                            )
                        })
                    } else {
                        0.0
                    };
                    let req = capture - unc_worst(sdc, s.dom_max[pol], cap_row, false) - t_setup
                        + n + credit;
                    pols.max[pol] = Some(Worst {
                        arr: w.dly,
                        req,
                        slack: req - w.dly,
                        ..w
                    });
                }
            }
            if let Some(n) = delay_min_n
                && kind == ExceptionEndpointKind::OutputPort
            {
                // Measured (`mdo_min*`, `pmo_min*`): req =
                // N - output_delay_min + hold_uncertainty — the same edge-0
                // reference as the max side. `mdo_mindl`/`mdo_minhu` vary one
                // term at a time to separate delay_min from hold_uncertainty
                // (they cancel at the preamble's 0.1/0.1).
                let (delay_min, _) = sdc.output_delay_ns(cx.port_name(PinId(ep)));
                let out_row = sdc.output_clock_row(cx.port_name(PinId(ep)));
                let s = class_states[ci][ep as usize];
                for pol in [RISE, FALL] {
                    let Some(mut w) = pols.min[pol] else { continue };
                    let unc = unc_worst(sdc, s.dom_min[pol], out_row, true);
                    if let Some(pins) = &delay_min_pins {
                        let Some(arr) =
                            pin_segment_arrival(pins, &class_states[ci], w.dly, pol, true)
                        else {
                            pols.min[pol] = None;
                            continue;
                        };
                        w.arr = arr;
                    } else {
                        w.arr = w.dly;
                    }
                    w.req = n - delay_min + unc;
                    w.slack = w.arr - w.req;
                    pols.min[pol] = Some(w);
                }
            }
            if let Some(n) = delay_min_n
                && kind != ExceptionEndpointKind::OutputPort
            {
                let check = check_by_pin.get(&ep).copied();
                let s = class_states[ci][ep as usize];
                for pol in [RISE, FALL] {
                    let Some(w) = pols.min[pol] else {
                        continue;
                    };
                    if let Some(pins) = &delay_min_pins
                        && let Some(c) = check
                        && let (Some(hold_arc), Some(clk)) = (c.hold_arc, c.clk)
                        && let Some(table) = constraint_table(&lib.arcs[hold_arc as usize], pol)
                    {
                        // Pin `-from` startpoint, hold side: smallest segment
                        // arrival is worst; `req = N + t_hold` (no anchor).
                        let clk_s = class_states[ci][clk.0 as usize];
                        let t_hold =
                            eval_nldm_constraint(table, s.slew_min[pol], clk_s.slew_min[c.edge])
                                * sdc.derate(false, DERATE_CHECK, false, check_transition(
                                    &lib.arcs[hold_arc as usize],
                                ));
                        let Some(arr) =
                            pin_segment_arrival(pins, &class_states[ci], w.dly, pol, true)
                        else {
                            pols.min[pol] = None;
                            continue;
                        };
                        let req = n + t_hold;
                        pols.min[pol] = Some(Worst {
                            arr,
                            req,
                            slack: arr - req,
                            ..w
                        });
                        continue;
                    }
                    if let Some(c) = check
                        && c.latch
                        && let (Some(hold_arc), Some(clk)) = (c.hold_arc, c.clk)
                        && let Some(table) = constraint_table(&lib.arcs[hold_arc as usize], pol)
                    {
                        // Latch-D min delay check (measured `delay_min` on
                        // `u_lp/D`): delay-edge anchored —
                        // `req = N + lat0 + t_hold + hold_uncertainty`,
                        // `arr = dly`, `slack = dly - req`.
                        let hold = &lib.arcs[hold_arc as usize];
                        let clk_s = class_states[ci][clk.0 as usize];
                        let t_hold =
                            eval_nldm_constraint(table, s.slew_min[pol], clk_s.slew_min[c.edge])
                                * sdc.derate(false, DERATE_CHECK, false, check_transition(hold));
                        let cap_row = prepared.clock_domain[clk.0 as usize] as usize;
                        let req = n
                            + clk_s.arrival_max[edge0_pol(clk)]
                            + t_hold
                            + unc_worst(sdc, s.dom_min[pol], cap_row, true);
                        pols.min[pol] = Some(Worst {
                            arr: w.dly,
                            req,
                            slack: w.dly - req,
                            ..w
                        });
                        continue;
                    }
                    // FF-D and async removal: the min-side delay check takes
                    // the pre-MCP hold/removal required time plus N (measured
                    // `md_min_mcp_*`, `delay_min` on `u_ff0/RESET_B`): the
                    // multicycle shift never feeds it. `arr` is the delay
                    // arrival.
                    let req = w.req + n;
                    pols.min[pol] = Some(Worst {
                        arr: w.dly,
                        req,
                        slack: w.dly - req,
                        ..w
                    });
                }
            }
            if mc_setup.is_some() || mc_hold.is_some() {
                // Measured OpenSTA semantics: setup shifts max required by
                // (N-1) periods and min required by ((N-1) - H) periods.
                // Latch-D and clock-gate endpoints have no rigid required
                // time, so they are diagnosed instead of shifted.
                if matches!(
                    kind,
                    ExceptionEndpointKind::Check { latch: false }
                        | ExceptionEndpointKind::OutputPort
                ) {
                    let ns = mc_setup.unwrap_or(1) as f32;
                    let nh = mc_hold.unwrap_or(0) as f32;
                    // The multiplier scales the endpoint's own capture period:
                    // a register reads the row its clock pin's domain reached,
                    // an output port the row its `-clock` resolved to.
                    let period = match kind {
                        ExceptionEndpointKind::OutputPort => sdc
                            .clocks
                            .get(sdc.output_clock_row(cx.port_name(PinId(ep))))
                            .map_or(0.0, |c| c.period_ns),
                        _ => check_by_pin
                            .get(&ep)
                            .and_then(|c| c.clk)
                            .and_then(|clk| {
                                sdc.clocks
                                    .get(prepared.clock_domain[clk.0 as usize] as usize)
                                    .map(|c| c.period_ns)
                            })
                            .unwrap_or_else(|| sdc.period_ns()),
                    };
                    if delay_max_n.is_none() {
                        let d_max = (ns - 1.0) * period;
                        for w in pols.max.iter_mut().flatten() {
                            w.req += d_max;
                            w.slack = w.req - w.arr;
                        }
                    }
                    if delay_min_n.is_none() {
                        // A cross-domain hold pair lives on the min-gap
                        // grid — the setup multiplier does not carry into it
                        // (measured `t2_mcp`: `d_min = -nh·Pc`). Mixed masks
                        // take the cross-domain form, conservative.
                        let s = class_states[ci][ep as usize];
                        let cap_tag = match kind {
                            ExceptionEndpointKind::OutputPort => launch_domain(
                                sdc,
                                sdc.output_clock_row(cx.port_name(PinId(ep))),
                            ),
                            _ => check_by_pin
                                .get(&ep)
                                .and_then(|c| c.clk)
                                .map_or(usize::MAX, |clk| {
                                    prepared.clock_domain[clk.0 as usize] as usize
                                }),
                        };
                        let cap_bit = dom_bit(cap_tag);
                        for (pol, w) in pols.min.iter_mut().enumerate() {
                            let Some(w) = w else { continue };
                            let d_min = if s.dom_min[pol] & !cap_bit != 0 {
                                // A cross-domain hold pair on the min-gap
                                // grid: an explicit `-hold` replaces the
                                // hold edge outright (measured `t2_mcp`:
                                // `-nh·Pc`); an absent one leaves the
                                // setup multiplier's one-cycle carry
                                // (`t2_selmc`: `(ns-1)·Pc`).
                                match mc_hold {
                                    Some(_) => -nh * period,
                                    None => (ns - 1.0) * period,
                                }
                            } else {
                                ((ns - 1.0) - nh) * period
                            };
                            w.req += d_min;
                            w.slack = w.arr - w.req;
                        }
                    }
                } else {
                    unsupported_mc.insert(ep);
                }
            }
            for pol in [RISE, FALL] {
                for (cand, best) in [
                    (pols.max[pol], &mut best_max[pol]),
                    (pols.min[pol], &mut best_min[pol]),
                ] {
                    if let Some(w) = cand
                        && best.is_none_or(|b| w.slack < b.slack)
                    {
                        *best = Some(w);
                    }
                }
            }
        }
        if !had {
            continue;
        }
        if !survived {
            fp_endpoints.insert(ep);
            continue;
        }
        composed.insert(
            ep,
            EndpointPolarities {
                max: best_max,
                min: best_min,
            },
        );
    }

    // Internal path endpoints (measured `t2_selmd`): a delay rule whose
    // `-from` covers a launch key also covers the path that ends at a
    // dead-end pin in its fanout cone — OpenSTA reports it as an
    // "internal path endpoint" row (`u_fy/Q`, a fanout-free net driver).
    // A leaf is a pin with neither net sinks nor outgoing arcs. `-to`
    // covers a leaf the same way it covers an endpoint; a resolved `-to`
    // set only ever holds declared endpoints, so a leaf is reachable only
    // through an absent `-to` (measured `t2_mdo`: no internal row).
    for (ci, (sigma, keys)) in classes.iter().enumerate() {
        // The leaf pass only ever produces a row under a delay rule — a
        // signature class without one never traverses the fanout cone.
        if !sigma
            .iter()
            .any(|&ri| matches!(rules[ri as usize], ResolvedRule::Delay(_)))
        {
            continue;
        }
        for &k in keys {
            for leaf in dead_end_leaves(design, graph, PinId(k)) {
                if ctx.endpoint_pins.contains(&leaf) {
                    continue;
                }
                // Same per-class fold as the endpoint compose: last-wins
                // per kind, a covering false path drops the leaf outright.
                let covers = |t: &Option<HashSet<u32>>| {
                    t.as_ref().is_none_or(|s| s.contains(&leaf))
                };
                let mut drop = false;
                let mut delay_max_n = None;
                let mut delay_min_n = None;
                for &ri in sigma {
                    match &rules[ri as usize] {
                        ResolvedRule::Exception(ex)
                            if ex.kind == ExceptionKind::FalsePath && covers(&ex.to) =>
                        {
                            drop = true;
                        }
                        ResolvedRule::Delay(d) if covers(&d.to) => match d.kind {
                            PathDelayKind::Max => delay_max_n = Some(d.value_ns),
                            PathDelayKind::Min => delay_min_n = Some(d.value_ns),
                        },
                        _ => {}
                    }
                }
                if drop || (delay_max_n.is_none() && delay_min_n.is_none()) {
                    continue;
                }
                let s = class_states[ci][leaf as usize];
                let pols = composed.entry(leaf).or_insert_with(|| EndpointPolarities {
                    max: [None; 2],
                    min: [None; 2],
                });
                // The check anchors at the delay edge itself: `req = N`,
                // no capture clock, uncertainty, or constraint term —
                // the arrival is the pin's own (clk->Q for a register
                // startpoint).
                for pol in [RISE, FALL] {
                    if !s.reached(pol) {
                        continue;
                    }
                    if let Some(n) = delay_max_n {
                        let cand = Worst {
                            arr: s.arrival_max[pol],
                            dly: s.dly_max[pol],
                            req: n,
                            slack: n - s.arrival_max[pol],
                            slew: s.slew_max[pol],
                        };
                        if pols.max[pol].is_none_or(|w| cand.slack < w.slack) {
                            pols.max[pol] = Some(cand);
                        }
                    }
                    if let Some(n) = delay_min_n {
                        let cand = Worst {
                            arr: s.arrival_min[pol],
                            dly: s.dly_min[pol],
                            req: n,
                            slack: s.arrival_min[pol] - n,
                            slew: s.slew_min[pol],
                        };
                        if pols.min[pol].is_none_or(|w| cand.slack < w.slack) {
                            pols.min[pol] = Some(cand);
                        }
                    }
                }
            }
        }
    }

    let mut entries = EndpointEntries::new();
    let mut reported = HashSet::new();
    for (&ep, &pols) in &composed {
        if let (Some(max), Some(min)) = (
            pick_worst_polarity(&pols.max),
            pick_worst_polarity(&pols.min),
        ) {
            entries.push((
                PinId(ep),
                endpoint_report(cx.pin_name(PinId(ep)), &max, &min),
                pols,
            ));
            reported.insert(ep);
        }
    }

    // Skip pass on the composed endpoint set: per-run skips were discarded;
    // reached-ness comes from the union of all class states.
    let mut merged = vec![PinTimingState::UNREACHED; design.num_pins()];
    for st in &class_states {
        for (i, s) in st.iter().enumerate() {
            merged[i].merge_all(s);
        }
    }
    let mut skipped = prepared.skipped.clone();
    skipped.total_endpoints = ctx.endpoint_pins.len() + pulse_row_count(graph, lib);
    skipped.sdc_ignored_options = sdc.ignored_options.clone();
    skipped.sdc_ignored_commands = sdc.ignored_commands.clone();
    skipped.sdc_commands = sdc.commands;
    skipped.false_path_endpoints = fp_endpoints.len();
    skipped.unsupported_multicycle_endpoints = unsupported_mc.len();
    skipped.unsupported_path_delay_endpoints = unsupported_pd.len();
    skipped.unresolved_exception_selectors = ctx.unresolved;

    // Re-diagnosis of an unreported endpoint against the union of every
    // class's arrival state — the same pairability test `collect_endpoints`
    // applies per run: only a foreign launch domain that cannot pair
    // (undeclared, latch borrow, non-coincident hold parity, non-converging
    // LCM) lands here; pairable foreign domains reported already.
    // `cap_row` names the capture in the skip line while `cap_tag` is the
    // pairing domain (`launch_domain`-resolved, equal to `cap_row` on
    // checks/gates; distinct for a virtual output-port anchor sharing a
    // design grid — equal periods by construction, so `cap_tag`'s row also
    // supplies the pairing period).
    let parity_at = |clk: PinId, pol: usize| -> usize {
        prepared
            .clock_parity
            .as_deref()
            .and_then(|p| p.get(clk.0 as usize).map(|e| e[pol]))
            .filter(|&v| v <= 1)
            .unwrap_or(pol as u8) as usize
    };
    #[allow(clippy::too_many_arguments)]
    let xdomain_skip = |skipped: &mut SkipReport,
                        kind: EndpointKind,
                        pin: PinId,
                        s: PinTimingState,
                        cap_row: usize,
                        cap_tag: usize,
                        setup_c0: usize,
                        hold_c0: usize,
                        hold_next: bool,
                        latch: bool|
     -> bool {
        let empty = Clock::default();
        let cap_clk = sdc.clocks.get(cap_tag).unwrap_or(&empty);
        let Some(launch) = xdomain_defer(
            s,
            sdc,
            dom_bit(cap_tag),
            cap_clk,
            setup_c0,
            hold_c0,
            hold_next,
            latch,
        ) else {
            return false;
        };
        skipped.cross_domain_endpoints += 1;
        skipped.skipped_endpoints.push(SkippedEndpoint {
            pin: cx.pin_name(pin),
            kind,
            reason: SkipReason::CrossDomain,
            related: Some(format!(
                "launch {launch} -> capture {}",
                sdc.clocks[cap_row].name
            )),
        });
        true
    };
    let skip_endpoint = |skipped: &mut SkipReport,
                         kind: EndpointKind,
                         pin: PinId,
                         reason: SkipReason,
                         related: Option<String>| {
        skipped.unreached_endpoints += 1;
        skipped.skipped_endpoints.push(SkippedEndpoint {
            pin: cx.pin_name(pin),
            kind,
            reason,
            related,
        });
    };
    for &(d, check) in &graph.checks {
        if reported.contains(&d) || fp_endpoints.contains(&d) {
            continue;
        }
        let (Some(clk), Some(_), Some(_)) = (check.clk, check.late_arc(), check.early_arc()) else {
            skipped.incomplete_checks += 1;
            let mut missing = Vec::new();
            if check.clk.is_none() {
                missing.push("clock pin");
            }
            if check.late_arc().is_none() {
                missing.push("late arc");
            }
            if check.early_arc().is_none() {
                missing.push("early arc");
            }
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(PinId(d)),
                kind: EndpointKind::Check,
                reason: SkipReason::IncompleteCheck,
                related: Some(missing.join(", ")),
            });
            continue;
        };
        let clk_s = merged[clk.0 as usize];
        let cap = prepared.clock_domain[clk.0 as usize];
        if cap == u8::MAX
            || !clk_s.reached(check.edge)
            || (check.latch
                && sdc.clocks[cap as usize].propagated
                && !clk_s.reached(1 - check.edge))
        {
            skip_endpoint(
                &mut skipped,
                EndpointKind::Check,
                PinId(d),
                SkipReason::NoClock,
                Some(cx.pin_name(clk)),
            );
            continue;
        }
        let c0 = parity_at(clk, check.edge);
        if xdomain_skip(
            &mut skipped,
            EndpointKind::Check,
            PinId(d),
            merged[d as usize],
            cap as usize,
            cap as usize,
            c0,
            c0,
            false,
            check.latch,
        ) {
            continue;
        }
        let (reason, related) = endpoint_skip_reason(
            cx,
            &prepared.levelized,
            &merged,
            EndpointKind::Check,
            PinId(d),
        );
        skip_endpoint(&mut skipped, EndpointKind::Check, PinId(d), reason, related);
    }
    for g in &graph.gating_enables {
        let e = g.enable.0;
        if reported.contains(&e) || fp_endpoints.contains(&e) {
            continue;
        }
        let c = merged[g.clk_pin.0 as usize];
        let cap = prepared.clock_domain[g.clk_pin.0 as usize];
        if cap == u8::MAX
            || (sdc.clocks[cap as usize].propagated
                && (!c.reached(RISE) || !c.reached(FALL)))
        {
            skip_endpoint(
                &mut skipped,
                EndpointKind::ClockGate,
                g.enable,
                SkipReason::NoClock,
                Some(cx.pin_name(g.clk_pin)),
            );
            continue;
        }
        let (setup_edge, hold_edge) = match g.kind {
            GateKind::Rising => (RISE, FALL),
            GateKind::Falling => (FALL, RISE),
        };
        if xdomain_skip(
            &mut skipped,
            EndpointKind::ClockGate,
            g.enable,
            merged[e as usize],
            cap as usize,
            cap as usize,
            parity_at(g.clk_pin, setup_edge),
            parity_at(g.clk_pin, hold_edge),
            true,
            false,
        ) {
            continue;
        }
        let (reason, related) = endpoint_skip_reason(
            cx,
            &prepared.levelized,
            &merged,
            EndpointKind::ClockGate,
            g.enable,
        );
        skip_endpoint(
            &mut skipped,
            EndpointKind::ClockGate,
            g.enable,
            reason,
            related,
        );
    }
    for &p in &ctx.output_ports {
        if reported.contains(&p) || fp_endpoints.contains(&p) {
            continue;
        }
        let out_row = sdc.output_clock_row(cx.port_name(PinId(p)));
        if xdomain_skip(
            &mut skipped,
            EndpointKind::OutputPort,
            PinId(p),
            merged[p as usize],
            out_row,
            launch_domain(sdc, out_row),
            0,
            0,
            false,
            false,
        ) {
            continue;
        }
        let (reason, related) = endpoint_skip_reason(
            cx,
            &prepared.levelized,
            &merged,
            EndpointKind::OutputPort,
            PinId(p),
        );
        skip_endpoint(
            &mut skipped,
            EndpointKind::OutputPort,
            PinId(p),
            reason,
            related,
        );
    }

    // Pulse checks have no launch-source side, so exceptions cannot
    // partition them: evaluate once on the merged state.
    let mut pulse_rows = Vec::new();
    eval_pulse_width_checks(
        cx,
        graph,
        prepared.clock_state.as_deref(),
        &prepared.clock_domain,
        &merged,
        &mut skipped,
        &mut pulse_rows,
    );

    let (report, polarities) = assemble_sta_report(entries, pulse_rows, skipped);
    if emit_warnings {
        emit_sta_warnings(&report);
    }
    Ok((report, merged, polarities))
}

#[derive(Clone, Copy)]
struct Worst {
    arr: f32,
    /// Delay-check arrival (`PinTimingState::dly_*`): equals `arr` unless the
    /// path launched through a latch open edge, in which case it is re-based
    /// to the delay edge.
    dly: f32,
    req: f32,
    slack: f32,
    slew: f32,
}

#[derive(Clone, Copy)]
struct EndpointPolarities {
    max: [Option<Worst>; 2],
    min: [Option<Worst>; 2],
}

fn pick_worst_polarity(pair: &[Option<Worst>; 2]) -> Option<Worst> {
    match (pair[RISE], pair[FALL]) {
        (Some(r), Some(f)) => Some(if f.slack < r.slack { f } else { r }),
        (Some(w), None) | (None, Some(w)) => Some(w),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn record_endpoint(
    cx: &Context,
    levelized: &LevelizedGraph,
    state: &[PinTimingState],
    entries: &mut EndpointEntries,
    skipped: &mut SkipReport,
    kind: EndpointKind,
    pin: PinId,
    cands: EndpointPolarities,
) {
    match (
        pick_worst_polarity(&cands.max),
        pick_worst_polarity(&cands.min),
    ) {
        (Some(max), Some(min)) => {
            entries.push((pin, endpoint_report(cx.pin_name(pin), &max, &min), cands))
        }
        _ => {
            skipped.unreached_endpoints += 1;
            let (reason, related) = endpoint_skip_reason(cx, levelized, state, kind, pin);
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(pin),
                kind,
                reason,
                related,
            });
        }
    }
}

/// CRPR credit for one required-time term: the early/late spread at the shared
/// clock-net driver, granted only when `tag_mask` is a single non-sentinel
/// launch tag whose `(clock net, clock edge)` equals `(capture_net, trans)`.
/// A contested or sentinel-tagged extreme is credited nothing — the same
/// conservative answer the old all-source cone consensus gave.
fn crpr_credit(
    design: &DesignDb,
    clock_state: Option<&[PinTimingState]>,
    tags: &[(NetId, usize)],
    tag_mask: u8,
    capture_net: NetId,
    trans: usize,
) -> f32 {
    if tag_mask.count_ones() != 1 || tag_mask & 1 != 0 {
        return 0.0;
    }
    let Some(&(net, edge)) = tags.get(tag_mask.trailing_zeros() as usize) else {
        return 0.0;
    };
    if net != capture_net || edge != trans {
        return 0.0;
    }
    let (Some(clock_state), Some(driver)) = (clock_state, design.net_driver(net)) else {
        return 0.0;
    };
    let ds = clock_state[driver.0 as usize];
    if ds.reached(trans) {
        (ds.arrival_max[trans] - ds.arrival_min[trans]).max(0.0)
    } else {
        0.0
    }
}

/// `(pin, report, polarities)` per reached endpoint; `assemble_sta_report`
/// sorts by pin name and reduces to the report.
type EndpointEntries = Vec<(PinId, EndpointReport, EndpointPolarities)>;

/// Endpoint candidates the `min_pulse_width` arcs produce: one row per
/// constraint table present on each arc (OpenSTA emits no row for a missing
/// table — `dlxtn_1` carries only `fall_constraint` and reports only `(low)`).
fn pulse_row_count(graph: &TimingGraph, lib: &LibDb) -> usize {
    graph
        .pulse_width_checks
        .iter()
        .map(|c| {
            let arc = &lib.arcs[c.lib_arc as usize];
            arc.rise_constraint.is_some() as usize
                + arc.fall_constraint.is_some() as usize
        })
        .sum()
}

/// Evaluate `min_pulse_width` arcs the way `report_pulse_width_checks`
/// reports them (measured on `sta_edge/min_pulse_width`):
///
/// - `rise_constraint` bounds the high pulse (rise -> next fall),
///   `fall_constraint` the low pulse (fall -> next rise).
/// - `actual = min(early_pair, late_pair)` where each pair is
///   `arrival_<corner>[close] - arrival_<corner>[open]` of the *propagated*
///   clock state (carried under the ideal clock too), wrapped one period
///   when `<= 0` — the first-occurrence arrivals reorder behind inverting
///   clock cells.
/// - `required` is the raw constraint-table value at the check pin's own
///   `slew_max` of the opening transition — no `-cell_check` derate and no
///   clock uncertainty (both measured to not apply).
/// - `slack = actual - required` (negative = violated).
///
/// `state` supplies the slews (the clock pin's seeded or propagated check
/// slew, `set_clock_transition` under the ideal clock); `clock_state`
/// supplies the arrival pairs. Pulse checks have no launch-source side, so
/// exception partitioning calls this once on the merged state.
fn eval_pulse_width_checks(
    cx: &Context,
    graph: &TimingGraph,
    clock_state: Option<&[PinTimingState]>,
    clock_domain: &[u8],
    state: &[PinTimingState],
    skipped: &mut SkipReport,
    out: &mut Vec<PulseWidthCheckReport>,
) {
    let sdc = cx.sdc;
    for check in &graph.pulse_width_checks {
        let pin = check.pin;
        let arc = &cx.lib.arcs[check.lib_arc as usize];
        let c = clock_state.map(|s| s[pin.0 as usize]);
        // The wrap period is the check pin's own domain's.
        let period = clock_domain
            .get(pin.0 as usize)
            .filter(|&&d| d != u8::MAX)
            .map_or_else(|| sdc.period_ns(), |&d| sdc.clocks[d as usize].period_ns);
        for (edge, open, close, table) in [
            (PulseEdge::High, RISE, FALL, arc.rise_constraint.as_ref()),
            (PulseEdge::Low, FALL, RISE, arc.fall_constraint.as_ref()),
        ] {
            let Some(table) = table else {
                continue;
            };
            let reached = c.is_some_and(|s| s.reached(open) && s.reached(close));
            if !reached {
                skipped.unreached_endpoints += 1;
                // `pin` stays a bare pin name like every other skipped
                // endpoint; the pulse edge goes in `related`.
                skipped.skipped_endpoints.push(SkippedEndpoint {
                    pin: cx.pin_name(pin),
                    kind: EndpointKind::MinPulseWidth,
                    reason: SkipReason::NoClock,
                    related: Some(edge_label(edge).to_string()),
                });
                continue;
            }
            let c = c.unwrap();
            let width = |arr: &[f32; 2]| {
                let mut w = arr[close] - arr[open];
                if w <= 0.0 {
                    w += period;
                }
                w
            };
            let actual = width(&c.arrival_min).min(width(&c.arrival_max));
            let slew = state[pin.0 as usize].slew_max[open];
            let required = eval_nldm_constraint(table, slew, slew);
            out.push(PulseWidthCheckReport {
                pin: cx.pin_name(pin),
                edge,
                required_ns: required,
                actual_ns: actual,
                slack_ns: actual - required,
            });
        }
    }
}

fn edge_label(edge: PulseEdge) -> &'static str {
    match edge {
        PulseEdge::High => "high",
        PulseEdge::Low => "low",
    }
}

/// Comma-joined names of the clock rows set in a domain mask — the launch
/// side of a `CrossDomain` skip's `related` text.
fn clock_names(sdc: &Sdc, mask: u8) -> String {
    sdc.clocks
        .iter()
        .enumerate()
        .filter(|(d, _)| *d < 8 && mask & (1 << d) != 0)
        .map(|(_, c)| c.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Domain tag → mask bit. Tags are `Sdc::clocks` rows (so at most 8 rows
/// carry a bit — a row ≥ 8 and the `u8::MAX` unreached sentinel both map to
/// 0, the "no domain" mask, which defers at every capture instead of folding).
fn dom_bit(tag: usize) -> u8 {
    if tag < 8 { 1 << tag } else { 0 }
}

/// Launch-domain row for pairing: a port-bearing row pairs as itself; a
/// virtual row maps to the first design row whose period equals the virtual
/// clock's — same-period grids share edge times, so the pair is the
/// coincident-edge pair the same-domain evaluator already computes. A
/// virtual row matching no design grid keeps its own row: foreign to every
/// capture, deferred to mc-xdomain's LCM search.
fn launch_domain(sdc: &Sdc, row: usize) -> usize {
    let Some(clk) = sdc.clocks.get(row) else {
        return row;
    };
    if clk.port.is_some() {
        return row;
    }
    sdc.clocks
        .iter()
        .enumerate()
        .find(|(_, c)| c.port.is_some() && c.period_ns == clk.period_ns)
        .map(|(r, _)| r)
        .unwrap_or(row)
}

/// `lcm(a, b)` of two clock periods via f64 Euclid. The remainder test is
/// relative to the *input* periods (~1e-6 of the smaller one), not to the
/// running residue: `1.4f32` is `1.399999976…` in f64, so an exact-Euclid
/// gcd would sink to the f32 rounding residue (~1.19e-7) and report an
/// absurd LCM (~2.3e7) — the window must accept the f32-scale gcd `0.2`
/// instead. `None` when the residue never drops to f32 resolution within
/// the bound — periods incommensurate at our precision, a defensive bail,
/// not a model case.
fn grid_lcm(a: f32, b: f32) -> Option<f64> {
    let (mut x, mut y) = (a as f64, b as f64);
    let tol = 1e-6 * (a as f64).min(b as f64);
    for _ in 0..32 {
        if y <= 0.0 {
            return None;
        }
        let r = x % y;
        if r <= tol {
            // `y` divides both periods at f32 resolution: lcm = a / gcd * b.
            return Some(a as f64 / y * b as f64);
        }
        if y <= tol {
            return None;
        }
        x = y;
        y = r;
    }
    None
}

/// Launch-edge window bound shared by the pair searches: edges repeat at the
/// period-LCM boundary, so launch edges `launch0 + m·PL < lcm` enumerate the
/// full residue pattern (`launch0` is the launch edge's waveform instant).
/// The bound carries f32-scale noise (`grid_lcm` tolerance), so the margin
/// is 1e-6-relative, not edge-exact.
fn launch_window(launch0: f64, pl: f64, lcm: f64) -> impl Iterator<Item = (usize, f64)> {
    (0usize..)
        .map(move |m| (m, launch0 + m as f64 * pl))
        .take_while(move |&(_, tl)| tl < lcm * (1.0 - 1e-6))
}

/// Setup pair (measured `docs/multi-clock.md` §1.2): every launch edge
/// `edge_offset(g) + m·PL` inside the period-LCM window pairs with the
/// smallest capture edge `edge_offset(c0) + j·PC` strictly after it; the
/// winner minimises `capture − launch`. Returns `(m·PL, j·PC)` — additive
/// shifts on the recorded arrivals, which are referenced to each parity's
/// first edge. `None` only when `grid_lcm` fails. Equal gaps keep the
/// first found (smallest launch edge); OpenSTA's tie-break is unmeasured.
fn setup_edge_pair(lclk: &Clock, cclk: &Clock, g: usize, c0: usize) -> Option<(f32, f32)> {
    let lcm = grid_lcm(lclk.period_ns, cclk.period_ns)?;
    let (pl, pc) = (lclk.period_ns as f64, cclk.period_ns as f64);
    let launch0 = lclk.edge_offset(g) as f64;
    let phase_c = cclk.edge_offset(c0) as f64;
    // A capture edge coincident with the launch edge is not strictly after;
    // the tolerance keeps f64 noise at grid points from picking it, and a
    // degenerate sub-eps gap is treated as coincidence either way.
    let eps = 1e-9 * pc.max(pl);
    let mut best: Option<(f64, f64, f64)> = None;
    for (m, tl) in launch_window(launch0, pl, lcm) {
        let mut j = ((tl - phase_c) / pc).floor().max(-1.0) as i64 + 1;
        j = j.max(0);
        while j > 0 && phase_c + (j - 1) as f64 * pc > tl + eps {
            j -= 1;
        }
        while phase_c + j as f64 * pc <= tl + eps {
            j += 1;
        }
        let gap = phase_c + j as f64 * pc - tl;
        if best.is_none_or(|(g0, _, _)| gap < g0 - 1e-9) {
            best = Some((gap, m as f64 * pl, j as f64 * pc));
        }
    }
    best.map(|(_, lsh, csh)| (lsh as f32, csh as f32))
}

/// Hold pair (§1.2): each launch edge `edge_offset(g) + m·PL` pairs with the
/// largest capture edge `edge_offset(c0) + j·PC` at-or-before it; the winner
/// minimises `launch − capture`. Measured on `t2_wave` (ckb→cka launch 1.0 /
/// capture 0.3): a non-coincident minimum-gap pair is real, so the rule is
/// gap-minimising, not coincidence — the coincident grids of every
/// pre-waveform scenario still return the same pair (gap 0, earliest
/// launch). `None` when `grid_lcm` fails or no non-negative capture edge
/// sits at-or-before any launch edge.
fn hold_edge_pair(lclk: &Clock, cclk: &Clock, g: usize, c0: usize) -> Option<(f32, f32)> {
    let lcm = grid_lcm(lclk.period_ns, cclk.period_ns)?;
    let (pl, pc) = (lclk.period_ns as f64, cclk.period_ns as f64);
    let launch0 = lclk.edge_offset(g) as f64;
    let phase_c = cclk.edge_offset(c0) as f64;
    let eps = 1e-9 * pc.max(pl);
    let mut best: Option<(f64, f64, f64)> = None;
    for (m, tl) in launch_window(launch0, pl, lcm) {
        // Largest j >= 0 with `phase_c + j·PC <= tl`.
        let mut j = ((tl - phase_c) / pc).floor() as i64;
        while j > 0 && phase_c + j as f64 * pc > tl + eps {
            j -= 1;
        }
        if j < 0 || phase_c + j as f64 * pc > tl + eps {
            continue;
        }
        let gap = tl - (phase_c + j as f64 * pc);
        if best.is_none_or(|(g0, _, _)| gap < g0 - 1e-9) {
            best = Some((gap, m as f64 * pl, j as f64 * pc));
        }
    }
    best.map(|(_, lsh, csh)| (lsh as f32, csh as f32))
}

/// Whole-endpoint `CrossDomain` deferral predicate, shared between
/// `collect_endpoints` and the partitioned skip pass. `Some(launch names)`
/// when any reached arrival extreme cannot be evaluated against `cap_bit` —
/// the whole endpoint defers (both max and min sides are needed to report):
///   * reached extreme, empty dom mask      -> `<undeclared>` (the fold
///     `mc-eval` exists to catch)
///   * latch-D check, any foreign bit       -> parked borrow pairing
///   * min-side (d, g) with no capture edge
///     at-or-before a launch edge           -> parked hold selection
///   * non-converging `grid_lcm`            -> defensive
///
/// `None` = every tying (domain, parity) pairs — evaluate.
///
/// The min-side pairing rule is `hold_next`-selected: register checks and
/// output ports pair min-gap edges on the check/anchor grid; a gating
/// enable's hold edge *follows* the launch on the opposite-polarity grid, so
/// it uses the strictly-after rule (always pairs).
#[allow(clippy::too_many_arguments)]
fn xdomain_defer(
    s: PinTimingState,
    sdc: &Sdc,
    cap_bit: u8,
    cap_clk: &Clock,
    setup_c0: usize,
    hold_c0: usize,
    hold_next: bool,
    latch: bool,
) -> Option<String> {
    let mut foreign = 0u8;
    let mut undeclared = false;
    for pol in [RISE, FALL] {
        if s.arrival_max[pol] > f32::NEG_INFINITY {
            if s.dom_max[pol] == 0 {
                undeclared = true;
            } else {
                foreign |= s.dom_max[pol] & !cap_bit;
            }
        }
        if s.arrival_min[pol] < f32::INFINITY {
            if s.dom_min[pol] == 0 {
                undeclared = true;
            } else {
                foreign |= s.dom_min[pol] & !cap_bit;
            }
        }
    }
    if foreign == 0 && !undeclared {
        return None;
    }
    let mut names = clock_names(sdc, foreign);
    if undeclared {
        if !names.is_empty() {
            names.push_str(", ");
        }
        names.push_str("<undeclared>");
    }
    if undeclared || (latch && foreign != 0) {
        return Some(names);
    }
    // Every foreign (domain, parity) on a reached extreme must pair.
    let pair_ok = |dom: u8, par: u8, c0: usize, min_side: bool| -> bool {
        let par = if par == 0 { 1 } else { par };
        for d in 0..8usize {
            if dom & !cap_bit & (1 << d) == 0 {
                continue;
            }
            let Some(lclk) = sdc.clocks.get(d) else {
                return false;
            };
            for g in 0..2usize {
                if par & (1 << g) == 0 {
                    continue;
                }
                let ok = if min_side && !hold_next {
                    hold_edge_pair(lclk, cap_clk, g, c0).is_some()
                } else {
                    setup_edge_pair(lclk, cap_clk, g, c0).is_some()
                };
                if !ok {
                    return false;
                }
            }
        }
        true
    };
    for pol in [RISE, FALL] {
        if s.arrival_max[pol] > f32::NEG_INFINITY
            && !pair_ok(s.dom_max[pol], s.par_max[pol], setup_c0, false)
        {
            return Some(names.clone());
        }
        if s.arrival_min[pol] < f32::INFINITY
            && !pair_ok(s.dom_min[pol], s.par_min[pol], hold_c0, true)
        {
            return Some(names.clone());
        }
    }
    None
}

/// Worst inter-clock uncertainty for a `launch-mask -> capture` read: setup
/// subtracts and hold adds the value, so the largest match across the tying
/// launch bits is the worst either way. An empty mask falls back to the
/// capture row's own value.
fn unc_worst(sdc: &Sdc, mask: u8, cap: usize, hold: bool) -> f32 {
    if mask == 0 {
        return sdc.uncertainty_ns(cap, cap, hold);
    }
    let mut worst = 0.0f32;
    for d in 0..8usize {
        if mask & (1 << d) != 0 {
            worst = worst.max(sdc.uncertainty_ns(d, cap, hold));
        }
    }
    worst
}

/// `ep_dly − pin_dly` over the `-from` pins reached in this class on
/// (pol, side): a pin whose own delay arrival is still the sentinel
/// contributes no constrained path and is skipped. `None` when no pin in
/// the set is reached — the caller drops the polarity's candidate rather
/// than report an infinite segment or fall back to the clocked `w.dly`
/// (the pin `-from` already replaced that launch).
fn pin_segment_arrival(
    pins: &HashSet<u32>,
    states: &[PinTimingState],
    ep_dly: f32,
    pol: usize,
    min_side: bool,
) -> Option<f32> {
    let mut seg: Option<f32> = None;
    for &fp in pins {
        let pin_dly = if min_side {
            states[fp as usize].dly_min[pol]
        } else {
            states[fp as usize].dly_max[pol]
        };
        if !pin_dly.is_finite() {
            continue;
        }
        let v = ep_dly - pin_dly;
        seg = Some(seg.map_or(v, |a| if min_side { a.min(v) } else { a.max(v) }));
    }
    seg
}

#[allow(clippy::too_many_arguments)]
fn collect_endpoints(
    cx: &Context,
    graph: &TimingGraph,
    levelized: &LevelizedGraph,
    clock_state: Option<&[PinTimingState]>,
    state: &[PinTimingState],
    clock_parity: Option<&[[u8; 2]]>,
    clock_domain: &[u8],
    crpr_tags: &[(NetId, usize)],
    mut skipped: SkipReport,
) -> Result<(EndpointEntries, Vec<PulseWidthCheckReport>, SkipReport)> {
    let (design, lib, sdc) = (cx.design, cx.lib, cx.sdc);
    skipped.total_endpoints = graph.checks.len()
        + graph.gating_enables.len()
        + design
            .io_pins
            .iter()
            .filter(|&&p| design.pin_dirs[p.0 as usize] == PinDir::Output)
            .count()
        + pulse_row_count(graph, lib);
    skipped.sdc_ignored_options = sdc.ignored_options.clone();
    skipped.sdc_ignored_commands = sdc.ignored_commands.clone();
    skipped.sdc_commands = sdc.commands;
    let mut entries = Vec::new();
    // Clock-edge index parity of a pin's first arrival of `pol` (0 = even edge). Both
    // clock modes carry it: propagated clocks track it through the network, the ideal
    // clock derives it from the parity-only walk. The launch side carries its own
    // parity on each arrival extreme, so inverting launch paths are handled per endpoint.
    // Common-path pessimism removal (CRPR): when launch and capture clocks share a net,
    // the shared prefix is counted late on the launch side and early on the capture side.
    // OpenSTA credits each *path* the early/late spread at the last shared point — the
    // net driver — back to both required times. Only a propagated clock carries that
    // spread; the ideal clock seeds identical arrivals. Which launch won each arrival
    // extreme is recorded in the pin's `crpr_*` tag mask; the credit applies only when
    // the winning launch's clock net and edge match the capture's.
    let edge_parity = |pin: PinId, pol: usize| -> usize {
        clock_parity
            .and_then(|p| p.get(pin.0 as usize).map(|e| e[pol]))
            .filter(|&v| v <= 1)
            // No propagated parity (ideal clock or unreached pin): a fall edge sits on
            // clock edge 1, a rise on edge 0 — the source-port convention.
            .unwrap_or(pol as u8) as usize
    };

    for &(data_id, check) in &graph.checks {
        let data = PinId(data_id);
        let (Some(clk), Some(late_arc), Some(early_arc)) =
            (check.clk, check.late_arc(), check.early_arc())
        else {
            skipped.incomplete_checks += 1;
            let mut missing = Vec::new();
            if check.clk.is_none() {
                missing.push("clock pin");
            }
            if check.late_arc().is_none() {
                missing.push("late arc");
            }
            if check.early_arc().is_none() {
                missing.push("early arc");
            }
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(data),
                kind: EndpointKind::Check,
                reason: SkipReason::IncompleteCheck,
                related: Some(missing.join(", ")),
            });
            continue;
        };
        let (s, clk_state) = (state[data.0 as usize], state[clk.0 as usize]);
        // The check fires on `check.edge`: RISE for `*Rising` cells, FALL for inverting-clock
        // cells (`dfrtn`). The clock pin's seeded arrival already carries the edge's phase.
        let edge = check.edge;
        // The capture domain is the clock row whose port seed reached the
        // clock pin; `u8::MAX` means none did (unreached clock — NoClock).
        let cap = clock_domain[clk.0 as usize];
        // A propagated clock can leave a clock pin unreached; the ideal seed always reaches.
        // A latch check also needs the opening edge (the opposite polarity of the
        // setup check edge): the borrow branch reads its arrival.
        if cap == u8::MAX
            || !clk_state.reached(edge)
            || (check.latch
                && sdc.clocks[cap as usize].propagated
                && !clk_state.reached(1 - edge))
        {
            skipped.unreached_endpoints += 1;
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(data),
                kind: EndpointKind::Check,
                reason: SkipReason::NoClock,
                related: Some(cx.pin_name(clk)),
            });
            continue;
        }
        let cap_clk = &sdc.clocks[cap as usize];
        // The setup capture edge is the first edge of `edge` polarity strictly after the
        // launch edge. `c0` is the clock-edge index parity of the capture pin's first edge
        // of that polarity; each arrival extreme carries the parity of the launch edge
        // that produced it. The first capture edge is used only when it already sits after
        // the launch edge, i.e. when `c0` exceeds the launch parity; otherwise the capture
        // edge is one period later. An extreme with no recorded parity falls back to edge
        // 0, the pre-tracking assumption.
        let c0 = edge_parity(clk, edge);
        // Cross-domain launch/capture pairing (mc-xdomain): every reached
        // extreme's (domain, parity) must pair on the two period grids —
        // min-gap on setup and hold alike. Anything unpairable defers
        // with the domains named — never folds onto the capture lattice.
        if let Some(launch) = xdomain_defer(
            s,
            sdc,
            dom_bit(cap as usize),
            cap_clk,
            c0,
            c0,
            false,
            check.latch,
        ) {
            skipped.cross_domain_endpoints += 1;
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(data),
                kind: EndpointKind::Check,
                reason: SkipReason::CrossDomain,
                related: Some(format!("launch {launch} -> capture {}", cap_clk.name)),
            });
            continue;
        }
        // `trans` is the clock transition the credit is evaluated for: the check edge for
        // the closing/setup required times, the opening polarity for a latch's opening
        // edge. `tag_mask` is the launch-tag mask of the data extreme being composed
        // (`crpr_max[pol]` for closing req, `crpr_min[pol]` for hold req): it must
        // resolve to one tag matching both the capture net and the transition.
        let credit_for = |trans: usize, tag_mask: u8| -> f32 {
            if !cap_clk.propagated {
                return 0.0;
            }
            let Some(capture_net) = design.pin_nets[clk.0 as usize] else {
                return 0.0;
            };
            crpr_credit(design, clock_state, crpr_tags, tag_mask, capture_net, trans)
        };
        let (late, early) = (&lib.arcs[late_arc as usize], &lib.arcs[early_arc as usize]);
        let mut cands = EndpointPolarities {
            max: [None; 2],
            min: [None; 2],
        };
        for pol in [RISE, FALL] {
            if !s.reached(pol) {
                continue;
            }
            let (Some(late_table), Some(early_table)) =
                (constraint_table(late, pol), constraint_table(early, pol))
            else {
                continue;
            };
            let late_time =
                eval_nldm_constraint(late_table, s.slew_max[pol], clk_state.slew_max[edge])
                    * sdc.derate(true, DERATE_CHECK, false, check_transition(late));
            // Equal arrivals from different launch-edge parities/domains tie
            // `arrival_max`; each tying (domain, parity) can shift both edges,
            // so evaluate the required time per bit pair and keep the worst
            // slack. The masks are independent unions over the tying launches,
            // so the cross product can name a combination no single launch
            // produced — conservative, and exact on single-bit masks (every
            // committed fixture). Empty masks cannot occur on a reached pin
            // post-`xdomain_defer`; parity 0 and the capture bit are the
            // fallbacks anyway.
            let max_mask = if s.par_max[pol] == 0 {
                1
            } else {
                s.par_max[pol]
            };
            let max_dom = if s.dom_max[pol] == 0 {
                dom_bit(cap as usize)
            } else {
                s.dom_max[pol]
            };
            let mut max_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if max_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if max_mask & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req, slack) = if d == cap as usize {
                        let capture_extra = if cap_clk.edge_offset(c0) <= cap_clk.edge_offset(g) {
                            cap_clk.period_ns
                        } else {
                            0.0
                        };
                        let closing_max = capture_extra + clk_state.arrival_min[edge]
                            - late_time
                            - sdc.uncertainty_ns(d, cap as usize, false)
                            + credit_for(edge, s.crpr_max[pol]);
                        // OpenSTA latch model: data arriving in the transparent phase borrows time. The required time
                        // is reported at the opening edge and the slack is what remains before the closing edge.
                        // The opening edge is the latest opening-polarity arrival before the
                        // closing edge; edges recur every period, so align by whole periods —
                        // the first arrival alone sits one period early when the closing edge
                        // was shifted (inverting clock, parity-1 launch). Both clock modes read
                        // the same seeded/propagated arrivals here.
                        // The latch's opening edge is the opposite polarity of the setup check
                        // edge (`dlxtn`: setup at rise, opens on fall; `dlxtp`: setup at fall,
                        // opens on rise).
                        let opening_max = if !check.latch {
                            0.0 // only read on the latch branch below
                        } else {
                            let closing = clk_state.arrival_min[edge] + capture_extra;
                            let open_pol = 1 - edge;
                            let periods = ((closing - clk_state.arrival_min[open_pol])
                                / cap_clk.period_ns)
                                .floor()
                                .max(0.0);
                            clk_state.arrival_min[open_pol] + periods * cap_clk.period_ns
                                - sdc.uncertainty_ns(d, cap as usize, false)
                                + credit_for(open_pol, s.crpr_max[pol])
                        };
                        let arr_max = s.arrival_max[pol];
                        let (req, slack) = if !check.latch {
                            (closing_max, closing_max - arr_max)
                        } else if arr_max <= opening_max {
                            // The data waits for the latch to open; the margin is the time until
                            // the opening edge.
                            (opening_max, opening_max - arr_max)
                        } else {
                            // OpenSTA latch model: arrival inside the transparent window borrows
                            // time, so the required time equals the arrival (zero slack) until the
                            // borrow cap — the closing required time of this transition (closing
                            // edge minus uncertainty and the transition's setup constraint).
                            let req = arr_max.min(closing_max);
                            (req, req - arr_max)
                        };
                        (arr_max, req, slack)
                    } else {
                        // Cross-domain candidate: the min-gap edge pair on the
                        // launch/capture grids shifts both sides of the check
                        // (`docs/multi-clock.md` §1.2). A foreign launch never
                        // shares the capture clock net — no CRPR credit.
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = setup_edge_pair(lclk, cap_clk, g, c0)
                        else {
                            continue;
                        };
                        let arr = s.arrival_max[pol] + lsh;
                        let req = clk_state.arrival_min[edge] + csh - late_time
                            - sdc.uncertainty_ns(d, cap as usize, false);
                        (arr, req, req - arr)
                    };
                    if max_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        max_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_max, req_max, slack_max) = max_pick.unwrap();
            // The hold (early) constraint pairs with the capture clock's early slew; the
            // setup (late) constraint uses the late slew. Under the ideal clock both are
            // the seeded transition and identical.
            let early_time =
                eval_nldm_constraint(early_table, s.slew_min[pol], clk_state.slew_min[edge])
                    * sdc.derate(false, DERATE_CHECK, false, check_transition(early));
            // Same (domain, parity) candidate shape on the early side. The
            // same-domain branch holds the measured formula verbatim — the
            // hold realign on waveform instants, `+P` when the launch
            // parity's edge precedes the capture pin's first check edge
            // (`cap_clk.edge_offset(c0) > cap_clk.edge_offset(g)`); a
            // foreign bit pairs min-gap (`hold_edge_pair`) instead.
            let min_mask = if s.par_min[pol] == 0 {
                1
            } else {
                s.par_min[pol]
            };
            let min_dom = if s.dom_min[pol] == 0 {
                dom_bit(cap as usize)
            } else {
                s.dom_min[pol]
            };
            let mut min_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if min_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if min_mask & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req) = if d == cap as usize {
                        let req = clk_state.arrival_max[edge] + early_time
                            + sdc.uncertainty_ns(d, cap as usize, true)
                            - credit_for(edge, s.crpr_min[pol]);
                        (
                            s.arrival_min[pol]
                                + if cap_clk.edge_offset(c0) > cap_clk.edge_offset(g) {
                                    cap_clk.period_ns
                                } else {
                                    0.0
                                },
                            req,
                        )
                    } else {
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = hold_edge_pair(lclk, cap_clk, g, c0)
                        else {
                            continue;
                        };
                        (
                            s.arrival_min[pol] + lsh,
                            clk_state.arrival_max[edge] + csh + early_time
                                + sdc.uncertainty_ns(d, cap as usize, true),
                        )
                    };
                    let slack = arr - req;
                    if min_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        min_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_min, req_min, slack_min) = min_pick.unwrap();
            cands.max[pol] = Some(Worst {
                arr: arr_max,
                dly: s.dly_max[pol],
                req: req_max,
                slack: slack_max,
                slew: s.slew_max[pol],
            });
            cands.min[pol] = Some(Worst {
                arr: arr_min,
                // Delay checks report the raw delay arrival, without the
                // launch-parity realignment the hold check applies to `arr`
                // (measured `delay_min` on `u_lp/D`: `arr = 3.156276`, not
                // the hold-aligned `8.156276`).
                dly: s.dly_min[pol],
                req: req_min,
                slack: slack_min,
                slew: s.slew_min[pol],
            });
        }
        record_endpoint(
            cx,
            levelized,
            state,
            &mut entries,
            &mut skipped,
            EndpointKind::Check,
            data,
            cands,
        );
    }

    // Clock gate: the enable must settle before the propagating clock edge and hold past
    // the opposite edge of the same cycle. AND/NAND gates ("rising" check end-point) pair
    // setup against the clock rise and hold against the preceding fall; OR/NOR ("falling")
    // swap both. `pe` is the clock-edge parity of the setup edge, playing the role `c0`
    // played for the rising case: the setup capture edge is the first setup-polarity edge
    // strictly after the launch edge (a period later unless `pe` exceeds the launch
    // parity), and the min arrival shifts one period under the mirror condition.
    for g in &graph.gating_enables {
        let (enable, clk_pin) = (g.enable, g.clk_pin);
        let (setup_edge, hold_edge) = match g.kind {
            GateKind::Rising => (RISE, FALL),
            GateKind::Falling => (FALL, RISE),
        };
        let pe_s = edge_parity(clk_pin, setup_edge);
        let pe_h = edge_parity(clk_pin, hold_edge);
        let s = state[enable.0 as usize];
        let c = state[clk_pin.0 as usize];
        let cap = clock_domain[clk_pin.0 as usize];
        if cap == u8::MAX
            || (sdc.clocks[cap as usize].propagated
                && (!c.reached(RISE) || !c.reached(FALL)))
        {
            skipped.unreached_endpoints += 1;
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(enable),
                kind: EndpointKind::ClockGate,
                reason: SkipReason::NoClock,
                related: Some(cx.pin_name(clk_pin)),
            });
            continue;
        }
        let cap_clk = &sdc.clocks[cap as usize];
        // Same rule as the register checks: an enable launched on a domain
        // other than the gate clock's must pair on both grids. The gate's
        // hold edge follows the launch edge (opposite-polarity grid,
        // strictly-after), so the min side always pairs — only an
        // undeclared launch or a non-converging LCM defers.
        if let Some(launch) = xdomain_defer(
            s,
            sdc,
            dom_bit(cap as usize),
            cap_clk,
            pe_s,
            pe_h,
            true,
            false,
        ) {
            skipped.cross_domain_endpoints += 1;
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(enable),
                kind: EndpointKind::ClockGate,
                reason: SkipReason::CrossDomain,
                related: Some(format!(
                    "launch {launch} -> capture {}",
                    sdc.clocks[cap as usize].name
                )),
            });
            continue;
        }
        let mut cands = EndpointPolarities {
            max: [None; 2],
            min: [None; 2],
        };
        for pol in [RISE, FALL] {
            if !s.reached(pol) {
                continue;
            }
            // Tying launch (domain, parity) bits can shift the setup capture edge
            // and the hold arrival; evaluate each pair and keep the worst.
            // Both clock modes read the clock pin's seeded/propagated arrivals:
            // under the ideal clock the parity walk has already placed each
            // transition on its clock-edge parity, so `pe_s`/`pe_h` are the
            // real parities.
            let par_bits = |mask: u8| if mask == 0 { 1 } else { mask & 0b11 };
            let max_dom = if s.dom_max[pol] == 0 {
                dom_bit(cap as usize)
            } else {
                s.dom_max[pol]
            };
            let mut max_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if max_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if par_bits(s.par_max[pol]) & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req) = if d == cap as usize {
                        (
                            s.arrival_max[pol],
                            c.arrival_min[setup_edge]
                                + if cap_clk.edge_offset(pe_s) <= cap_clk.edge_offset(g) {
                                    cap_clk.period_ns
                                } else {
                                    0.0
                                }
                                - sdc.uncertainty_ns(d, cap as usize, false),
                        )
                    } else {
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = setup_edge_pair(lclk, cap_clk, g, pe_s)
                        else {
                            continue;
                        };
                        (
                            s.arrival_max[pol] + lsh,
                            c.arrival_min[setup_edge] + csh
                                - sdc.uncertainty_ns(d, cap as usize, false),
                        )
                    };
                    let slack = req - arr;
                    if max_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        max_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_max, req_max, slack_max) = max_pick.unwrap();
            let min_dom = if s.dom_min[pol] == 0 {
                dom_bit(cap as usize)
            } else {
                s.dom_min[pol]
            };
            let mut min_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if min_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if par_bits(s.par_min[pol]) & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req) = if d == cap as usize {
                        (
                            s.arrival_min[pol]
                                + if cap_clk.edge_offset(g) < cap_clk.edge_offset(pe_s) {
                                    cap_clk.period_ns
                                } else {
                                    0.0
                                },
                            c.arrival_max[hold_edge]
                                + sdc.uncertainty_ns(d, cap as usize, true),
                        )
                    } else {
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = setup_edge_pair(lclk, cap_clk, g, pe_h)
                        else {
                            continue;
                        };
                        (
                            s.arrival_min[pol] + lsh,
                            c.arrival_max[hold_edge] + csh
                                + sdc.uncertainty_ns(d, cap as usize, true),
                        )
                    };
                    let slack = arr - req;
                    if min_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        min_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_min, req_min, slack_min) = min_pick.unwrap();
            cands.max[pol] = Some(Worst {
                arr: arr_max,
                dly: s.dly_max[pol],
                req: req_max,
                slack: slack_max,
                slew: s.slew_max[pol],
            });
            cands.min[pol] = Some(Worst {
                arr: arr_min,
                dly: s.dly_min[pol],
                req: req_min,
                slack: slack_min,
                slew: s.slew_min[pol],
            });
        }
        record_endpoint(
            cx,
            levelized,
            state,
            &mut entries,
            &mut skipped,
            EndpointKind::ClockGate,
            enable,
            cands,
        );
    }

    for &pin in &design.io_pins {
        let p = pin.0 as usize;
        if design.pin_dirs[p] != PinDir::Output {
            continue;
        }
        let s = state[p];
        let port = cx.port_name(pin);
        // The required-time anchor is the port's own `-clock` row — possibly a
        // virtual clock, whose period/latency still anchor the check. The
        // pairing domain is the row's `launch_domain`: a virtual row sharing
        // a design grid pairs same-domain, a unique one defers every launch.
        let out_row = sdc.output_clock_row(port);
        let empty_clk = Clock::default();
        let out_clk = sdc.clocks.get(out_row).unwrap_or(&empty_clk);
        let cap_tag = launch_domain(sdc, out_row);
        // No clock pin anchors a port check; the anchor grid's edge-0 rise
        // lattice (`c0 = 0`) is the capture phase for pairing.
        if let Some(launch) = xdomain_defer(
            s,
            sdc,
            dom_bit(cap_tag),
            out_clk,
            0,
            0,
            false,
            false,
        ) {
            skipped.cross_domain_endpoints += 1;
            skipped.skipped_endpoints.push(SkippedEndpoint {
                pin: cx.pin_name(pin),
                kind: EndpointKind::OutputPort,
                reason: SkipReason::CrossDomain,
                related: Some(format!("launch {launch} -> capture {}", out_clk.name)),
            });
            continue;
        }
        let (delay_min, delay_max) = sdc.output_delay_ns(port);
        let latency = sdc.output_clock_latency_ns(port);
        let mut cands = EndpointPolarities {
            max: [None; 2],
            min: [None; 2],
        };
        for pol in [RISE, FALL] {
            if !s.reached(pol) {
                continue;
            }
            // OpenSTA applies clock uncertainty to output-port required times too (setup subtracts,
            // hold adds), the same terms the register checks carry. Port delays are not derated.
            // Foreign launch bits pair on the anchor's rise-edge grid.
            let max_dom = if s.dom_max[pol] == 0 {
                dom_bit(cap_tag)
            } else {
                s.dom_max[pol]
            };
            let max_par = if s.par_max[pol] == 0 {
                1
            } else {
                s.par_max[pol]
            };
            let mut max_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if max_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if max_par & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req) = if d == cap_tag {
                        (
                            s.arrival_max[pol],
                            latency + out_clk.edge_offset(0) + out_clk.period_ns - delay_max
                                - sdc.uncertainty_ns(d, out_row, false),
                        )
                    } else {
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = setup_edge_pair(lclk, out_clk, g, 0)
                        else {
                            continue;
                        };
                        (
                            s.arrival_max[pol] + lsh,
                            latency + out_clk.edge_offset(0) + csh - delay_max
                                - sdc.uncertainty_ns(d, out_row, false),
                        )
                    };
                    let slack = req - arr;
                    if max_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        max_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_max, req_max, slack_max) = max_pick.unwrap();
            let min_dom = if s.dom_min[pol] == 0 {
                dom_bit(cap_tag)
            } else {
                s.dom_min[pol]
            };
            let min_par = if s.par_min[pol] == 0 {
                1
            } else {
                s.par_min[pol]
            };
            let mut min_pick: Option<(f32, f32, f32)> = None;
            for d in 0..8usize {
                if min_dom & (1 << d) == 0 {
                    continue;
                }
                for g in 0..2usize {
                    if min_par & (1 << g) == 0 {
                        continue;
                    }
                    let (arr, req) = if d == cap_tag {
                        (
                            s.arrival_min[pol],
                            latency + out_clk.edge_offset(0) - delay_min
                                + sdc.uncertainty_ns(d, out_row, true),
                        )
                    } else {
                        let Some(lclk) = sdc.clocks.get(d) else {
                            continue;
                        };
                        let Some((lsh, csh)) = hold_edge_pair(lclk, out_clk, g, 0)
                        else {
                            continue;
                        };
                        (
                            s.arrival_min[pol] + lsh,
                            latency + out_clk.edge_offset(0) + csh - delay_min
                                + sdc.uncertainty_ns(d, out_row, true),
                        )
                    };
                    let slack = arr - req;
                    if min_pick.is_none_or(|(_, _, sl)| slack < sl) {
                        min_pick = Some((arr, req, slack));
                    }
                }
            }
            let (arr_min, req_min, slack_min) = min_pick.unwrap();
            cands.max[pol] = Some(Worst {
                arr: arr_max,
                dly: s.dly_max[pol],
                req: req_max,
                slack: slack_max,
                slew: s.slew_max[pol],
            });
            cands.min[pol] = Some(Worst {
                arr: arr_min,
                dly: s.dly_min[pol],
                req: req_min,
                slack: slack_min,
                slew: s.slew_min[pol],
            });
        }
        record_endpoint(
            cx,
            levelized,
            state,
            &mut entries,
            &mut skipped,
            EndpointKind::OutputPort,
            pin,
            cands,
        );
    }

    let mut pulse_rows = Vec::new();
    eval_pulse_width_checks(
        cx,
        graph,
        clock_state,
        clock_domain,
        state,
        &mut skipped,
        &mut pulse_rows,
    );

    Ok((entries, pulse_rows, skipped))
}

fn endpoint_report(pin: String, max: &Worst, min: &Worst) -> EndpointReport {
    EndpointReport {
        pin,
        max_arr_ns: max.arr,
        max_req_ns: max.req,
        max_slack_ns: max.slack,
        max_slew_ns: max.slew,
        min_arr_ns: min.arr,
        min_req_ns: min.req,
        min_slack_ns: min.slack,
        min_slew_ns: min.slew,
    }
}

/// Merges a setup-corner and a hold-corner [`StaReport`] into one worst-case
/// report: `max_*` fields and setup violations come from `setup`, `min_*`
/// fields and hold violations from `hold`. Endpoint sets must match —
/// different Liberty coverage would silently drop checks.
pub fn merge_corner_reports(setup: StaReport, hold: StaReport) -> Result<StaReport> {
    let setup_pins: HashSet<&str> = setup.endpoints.iter().map(|e| e.pin.as_str()).collect();
    let hold_pins: HashSet<&str> = hold.endpoints.iter().map(|e| e.pin.as_str()).collect();
    if setup_pins != hold_pins {
        let only_setup: Vec<_> = setup_pins.difference(&hold_pins).collect();
        let only_hold: Vec<_> = hold_pins.difference(&setup_pins).collect();
        bail!(
            "corner libraries produced different endpoint sets: {} only in setup, {} only in hold",
            only_setup.len(),
            only_hold.len()
        );
    }
    let hold_by_pin: HashMap<&str, &EndpointReport> =
        hold.endpoints.iter().map(|e| (e.pin.as_str(), e)).collect();
    let endpoints: Vec<EndpointReport> = setup
        .endpoints
        .iter()
        .map(|e| {
            let h = hold_by_pin[e.pin.as_str()];
            EndpointReport {
                pin: e.pin.clone(),
                max_arr_ns: e.max_arr_ns,
                max_req_ns: e.max_req_ns,
                max_slack_ns: e.max_slack_ns,
                max_slew_ns: e.max_slew_ns,
                min_arr_ns: h.min_arr_ns,
                min_req_ns: h.min_req_ns,
                min_slack_ns: h.min_slack_ns,
                min_slew_ns: h.min_slew_ns,
            }
        })
        .collect();
    let wns = |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).fold(f32::INFINITY, f32::min);
    let tns =
        |f: fn(&EndpointReport) -> f32| endpoints.iter().map(f).map(|s| s.min(0.0)).sum::<f32>();
    Ok(StaReport {
        wns_max_ns: wns(|e| e.max_slack_ns),
        tns_max_ns: tns(|e| e.max_slack_ns),
        wns_min_ns: wns(|e| e.min_slack_ns),
        tns_min_ns: tns(|e| e.min_slack_ns),
        setup_violations: endpoints.iter().filter(|e| e.max_slack_ns < 0.0).count() as u32,
        hold_violations: endpoints.iter().filter(|e| e.min_slack_ns < 0.0).count() as u32,
        endpoints,
        pulse_width_checks: merge_pulse_rows(
            setup.pulse_width_checks,
            hold.pulse_width_checks,
        )?,
        skipped: merge_skip_reports(setup.skipped, hold.skipped),
    })
}

/// Merge the two corners' `min_pulse_width` rows on `(pin, edge)`: the
/// merged row keeps the corner with the smaller slack, its required/actual
/// pair and edge. Row sets must match like the endpoint sets do.
fn merge_pulse_rows(
    a: Vec<PulseWidthCheckReport>,
    b: Vec<PulseWidthCheckReport>,
) -> Result<Vec<PulseWidthCheckReport>> {
    let key = |r: &PulseWidthCheckReport| (r.pin.clone(), r.edge);
    let a_keys: HashSet<_> = a.iter().map(key).collect();
    let b_keys: HashSet<_> = b.iter().map(key).collect();
    if a_keys != b_keys {
        bail!(
            "corner libraries produced different pulse-width check sets: {} vs {} rows",
            a.len(),
            b.len()
        );
    }
    let b_by_key: HashMap<_, _> = b.iter().map(|r| (key(r), r)).collect();
    Ok(a
        .into_iter()
        .map(|r| {
            let h = b_by_key[&key(&r)];
            if h.slack_ns < r.slack_ns { h.clone() } else { r }
        })
        .collect())
}

/// Element-wise max of two [`SkipReport`]s: either corner may skip an input
/// the other models, so the merged report keeps the larger count per category.
fn merge_skip_reports(a: SkipReport, b: SkipReport) -> SkipReport {
    fn merge_map(
        a: BTreeMap<String, usize>,
        b: BTreeMap<String, usize>,
    ) -> BTreeMap<String, usize> {
        let mut out = a;
        for (k, v) in b {
            out.entry(k).and_modify(|e| *e = (*e).max(v)).or_insert(v);
        }
        out
    }
    let mut skipped_endpoints = a.skipped_endpoints;
    for e in b.skipped_endpoints {
        if !skipped_endpoints.iter().any(|x| {
            x.pin == e.pin && x.kind == e.kind && x.reason == e.reason && x.related == e.related
        }) {
            skipped_endpoints.push(e);
        }
    }
    SkipReport {
        unmapped_cells: a.unmapped_cells.max(b.unmapped_cells),
        unmodelled_latch_cells: a.unmodelled_latch_cells.max(b.unmodelled_latch_cells),
        unsupported_arcs: merge_map(a.unsupported_arcs, b.unsupported_arcs),
        recognized_check_arcs: merge_map(a.recognized_check_arcs, b.recognized_check_arcs),
        latch_arcs_dropped: merge_map(a.latch_arcs_dropped, b.latch_arcs_dropped),
        unresolved_arcs: a.unresolved_arcs.max(b.unresolved_arcs),
        incomplete_checks: a.incomplete_checks.max(b.incomplete_checks),
        unreached_endpoints: a.unreached_endpoints.max(b.unreached_endpoints),
        cross_domain_endpoints: a.cross_domain_endpoints.max(b.cross_domain_endpoints),
        skipped_endpoints,
        cell_driven_clock_nets: a.cell_driven_clock_nets.max(b.cell_driven_clock_nets),
        unresolved_driving_cells: a.unresolved_driving_cells.max(b.unresolved_driving_cells),
        total_arcs: a.total_arcs.max(b.total_arcs),
        total_endpoints: a.total_endpoints.max(b.total_endpoints),
        sdc_ignored_options: merge_map(a.sdc_ignored_options, b.sdc_ignored_options),
        sdc_ignored_commands: merge_map(a.sdc_ignored_commands, b.sdc_ignored_commands),
        sdc_commands: a.sdc_commands.max(b.sdc_commands),
        false_path_endpoints: a.false_path_endpoints.max(b.false_path_endpoints),
        unsupported_multicycle_endpoints: a
            .unsupported_multicycle_endpoints
            .max(b.unsupported_multicycle_endpoints),
        unsupported_path_delay_endpoints: a
            .unsupported_path_delay_endpoints
            .max(b.unsupported_path_delay_endpoints),
        unresolved_exception_selectors: a
            .unresolved_exception_selectors
            .max(b.unresolved_exception_selectors),
        unresolved_load_selectors: a
            .unresolved_load_selectors
            .max(b.unresolved_load_selectors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::{load_liberty, load_tech_db, parse_def};
    use crate::sta::sdc::{load_sdc, parse_sdc};
    use crate::test_support::{design_dir, sky130_hd};

    #[test]
    fn skip_report_is_clean_exempts_only_design_rules_and_deferred_checks() {
        let mut skipped = SkipReport::default();
        assert!(skipped.is_clean());
        for cmd in [
            "set_max_transition",
            "set_max_capacitance",
            "set_max_fanout",
        ] {
            skipped.sdc_ignored_commands.insert(cmd.to_string(), 1);
        }
        assert!(
            skipped.is_clean(),
            "design rules cannot change a compared number"
        );
        skipped
            .sdc_ignored_commands
            .insert("set_propagated_clock".to_string(), 1);
        assert!(
            !skipped.is_clean(),
            "an unmodelled clock must fail --strict"
        );
        skipped.sdc_ignored_commands.clear();
        skipped
            .recognized_check_arcs
            .insert("MinPulseWidth".to_string(), 2);
        assert!(
            skipped.is_clean(),
            "a deferred check arc adds an endpoint outside the report, it does not alter one"
        );
        skipped.latch_arcs_dropped.insert("Other".to_string(), 1);
        assert!(
            !skipped.is_clean(),
            "an unmodelled latch arc can hide a data path and must fail --strict"
        );
        skipped.latch_arcs_dropped.clear();
        skipped.unsupported_arcs.insert("Other".to_string(), 1);
        assert!(!skipped.is_clean());
        skipped.unsupported_arcs.clear();
        skipped.false_path_endpoints = 3;
        assert!(
            skipped.is_clean(),
            "an applied false path is a declared exception, not a modelling gap"
        );
        skipped.unresolved_driving_cells = 1;
        assert!(!skipped.is_clean());
    }

    #[test]
    fn test_edge_pairing_unit_cases() {
        // `grid_lcm` resolves the f32 representation noise: 1.4f32 is
        // 1.39999997… in f64, so the LCM itself carries the residue — the
        // window bound absorbs it, but compare it loosely, not exactly.
        let lcm = |a: f32, b: f32, want: f64| {
            grid_lcm(a, b).is_some_and(|l| (l - want).abs() < 1e-4)
        };
        assert!(lcm(2.0, 1.4, 14.0));
        assert!(lcm(2.0, 1.0, 2.0));
        let clk = |period: f32, waveform_ns: Option<[f32; 2]>| Clock {
            period_ns: period,
            waveform_ns,
            ..Clock::default()
        };
        // `edge_offset` is the declared waveform instant, the `p·P/2`
        // uniform grid when no `-waveform` was declared.
        assert_eq!(clk(2.0, None).edge_offset(0), 0.0);
        assert_eq!(clk(2.0, None).edge_offset(1), 1.0);
        assert_eq!(clk(2.0, Some([0.0, 0.5])).edge_offset(1), 0.5);
        assert_eq!(clk(2.0, Some([0.3, 1.7])).edge_offset(0), 0.3);
        assert_eq!(clk(2.0, Some([0.3, 1.7])).edge_offset(1), 1.7);
        let pair = |a: f32, b: f32, c: f32, d: f32| {
            setup_edge_pair(&clk(a, None), &clk(b, None), c as usize, d as usize)
                .map(|(l, c)| ((l * 1e4).round() / 1e4, (c * 1e4).round() / 1e4))
        };
        // A->B on the 2.0/1.4 grids: launch 4.0, capture 4.2 (`t2_frac`).
        assert_eq!(pair(2.0, 1.4, 0.0, 0.0), Some((4.0, 4.2)));
        // B->A: launch 9.8, capture 10.0 (`t2_frac`).
        assert_eq!(pair(1.4, 2.0, 0.0, 0.0), Some((9.8, 10.0)));
        // Equal-grid integer case keeps the min-gap pair (launch 1.0,
        // capture 2.0), not the edge-0 fold (`t2_int`).
        assert_eq!(pair(1.0, 2.0, 0.0, 0.0), Some((1.0, 2.0)));
        assert_eq!(pair(2.0, 1.0, 0.0, 0.0), Some((0.0, 1.0)));
        // Same-period grids degenerate to the same-domain pair; the `g=1`
        // launch parity's first edge is already inside the recorded arrival,
        // so its additive shift is 0.
        assert_eq!(pair(2.0, 2.0, 0.0, 0.0), Some((0.0, 2.0)));
        assert_eq!(pair(2.0, 2.0, 1.0, 0.0), Some((0.0, 2.0)));
        let hold = |a: f32, b: f32, c: f32, d: f32| {
            hold_edge_pair(&clk(a, None), &clk(b, None), c as usize, d as usize)
                .map(|(l, c)| ((l * 1e4).round() / 1e4, (c * 1e4).round() / 1e4))
        };
        assert_eq!(hold(2.0, 1.4, 0.0, 0.0), Some((0.0, 0.0)));
        // Coincidence is the zero gap; the first one on the odd launch
        // parity is T=7.0.
        assert_eq!(hold(2.0, 1.4, 1.0, 0.0), Some((6.0, 7.0)));
        // B->A odd grid never coincides but still pairs: the minimum
        // non-negative gap is launch 2.1 / capture 2.0 (`t2_wave` measured
        // a non-coincident hold pair on ckb->cka).
        assert_eq!(hold(1.4, 2.0, 1.0, 0.0), Some((1.4, 2.0)));

        // Non-uniform grids (measured `t2_wave`): the pair searches run on
        // the declared waveform instants, not `p·P/2`.
        let pairw = |a: &Clock, b: &Clock, c: usize, d: usize| {
            setup_edge_pair(a, b, c, d)
                .map(|(l, c)| ((l * 1e4).round() / 1e4, (c * 1e4).round() / 1e4))
        };
        let holdw = |a: &Clock, b: &Clock, c: usize, d: usize| {
            hold_edge_pair(a, b, c, d)
                .map(|(l, c)| ((l * 1e4).round() / 1e4, (c * 1e4).round() / 1e4))
        };
        let cka_w = clk(2.0, Some([0.3, 1.7]));
        let ckb = clk(1.0, None);
        // Setup cka{0.3,1.7}->ckb: the 0.3 rise launch pairs with the first
        // ckb edge strictly after (1.0); the 1.7 fall launch with 2.0.
        assert_eq!(pairw(&cka_w, &ckb, 0, 0), Some((0.0, 1.0)));
        assert_eq!(pairw(&cka_w, &ckb, 1, 0), Some((0.0, 2.0)));
        // Hold ckb->cka{0.3,1.7} — the `t2_wave` measurement: ckb's 1.0
        // launch pairs with cka's 0.3 capture edge, a non-coincident
        // minimum gap.
        assert_eq!(holdw(&ckb, &cka_w, 0, 0), Some((1.0, 0.0)));
        // Hold cka{0.3,1.7}->ckb: the 0.3 launch pairs with the ckb edge 0
        // at-or-before it — zero shifts even though the instants differ.
        assert_eq!(holdw(&cka_w, &ckb, 0, 0), Some((0.0, 0.0)));
        // Identical waveforms degenerate to the same-domain pair.
        assert_eq!(pairw(&cka_w, &cka_w, 0, 0), Some((0.0, 2.0)));
        // A waveform restating the uniform grid ([0, P/2]) is the `None`
        // lattice.
        let cka_flat = clk(2.0, Some([0.0, 1.0]));
        assert_eq!(pairw(&cka_flat, &ckb, 0, 0), Some((0.0, 1.0)));
        assert_eq!(
            pairw(&cka_flat, &ckb, 1, 0),
            pairw(&clk(2.0, None), &ckb, 1, 0)
        );
    }

    #[test]
    fn test_pin_segment_arrival() {
        // A `-from` pin still UNREACHED in the class contributes no segment
        // (the sentinel delay would fold to ±∞); with no reached pin the
        // fold is `None` and the caller drops the polarity's candidate.
        let mut reached = PinTimingState::UNREACHED;
        reached.dly_max = [0.5, 0.5];
        reached.dly_min = [0.25, 0.25];
        let states = [PinTimingState::UNREACHED, reached];
        let pins: HashSet<u32> = [0, 1].into_iter().collect();
        assert_eq!(
            pin_segment_arrival(&pins, &states, 1.0, 0, false),
            Some(0.5)
        );
        assert_eq!(
            pin_segment_arrival(&pins, &states, 1.0, 0, true),
            Some(0.75)
        );
        let unreached: HashSet<u32> = [0].into_iter().collect();
        assert_eq!(pin_segment_arrival(&unreached, &states, 1.0, 0, false), None);
        assert_eq!(pin_segment_arrival(&unreached, &states, 1.0, 0, true), None);
    }

    /// Tolerances against OpenSTA. A value passes if |ours - ref| <= max(abs, rel * |ref|).
    /// Arrival, required time and WNS use `arr_rel` or 5 ps; the E3 done criterion is 1%.
    #[derive(Clone, Copy)]
    struct Tol {
        arr_rel: f64,
        arr_abs_ns: f64,
        slack_abs_ns: f64,
        slew_rel: f64,
        tns_rel: f64,
    }

    impl Tol {
        const STRICT: Tol = Tol {
            arr_rel: 0.01,
            arr_abs_ns: 0.005,
            slack_abs_ns: 0.005,
            slew_rel: 0.01,
            tns_rel: 0.01,
        };
        /// gcd with OpenSTA's own RC: slews 3% (table slew vs OpenSTA's driver waveform slew).
        const GCD_SPEF: Tol = Tol {
            slew_rel: 0.03,
            ..Tol::STRICT
        };
        /// Placement trees are OpenROAD's Prim-Dijkstra output (edge-identical to its SPEF on all
        /// five designs) and the built parasitics agree with the SPEF readback within the strict
        /// floor (<= 3e-6 ns bare wire, <= 3.8e-3 ns under uniform load), so the residual
        /// tolerance covers the shared cell-stage engine gap (driver waveform slew,
        /// extreme-load Ceff — E3.5), not tree topology or the wire model.
        const GCD_PLACEMENT: Tol = Tol {
            arr_rel: 0.02,
            slew_rel: 0.05,
            ..Tol::STRICT
        };
        /// picorv32 with OpenSTA's own RC: at single-constant RD_SCALE = 3.0, 7x-extrapolated
        /// dfxtp_2 rising launch arcs (_09979_, _11506_) leave a -0.31% (~-28 ps) arrival residual
        /// that amplifies into -2.45% TNS (-89.38 vs -91.62 ns) while all 14,384 endpoint fields pass;
        /// see docs/results.md.
        const PICORV32_SPEF: Tol = Tol {
            arr_abs_ns: 0.006,
            slack_abs_ns: 0.04,
            slew_rel: 0.10,
            tns_rel: 0.025,
            ..Tol::STRICT
        };
        /// picorv32 propagated clock shares `PICORV32_SPEF` and widens only arrival and
        /// slew. The three outliers — `_11496_`/`_10754_` arrival+required (~0.10-0.12 ns)
        /// and `_11498_` slew (12.6%) — sit on `resetn`-launched data paths (OpenSTA
        /// `report_checks`: same startpoint in ideal and propagated modes) through
        /// 1.8-3.5 ns-slew cells, i.e. the clock-independent E3.5 cell-stage gap
        /// (OpenSTA #582 regime) compounded by OpenSTA's non-uniform I/O-launch shift
        /// under propagated clocks — not a clock-arrival defect. Propagated-clock
        /// arrival itself is covered strictly by the `sta_edge` fixtures and the clean
        /// `gcd` pass; this test is end-to-end skew/sanity coverage. See docs/results.md.
        const PICORV32_PROPAGATED: Tol = Tol {
            arr_rel: 0.03,
            slew_rel: 0.15,
            ..Tol::PICORV32_SPEF
        };
        const PICORV32_PLACEMENT: Tol = Tol {
            arr_rel: 0.03,
            slack_abs_ns: 0.2,
            slew_rel: 0.20,
            tns_rel: 0.10,
            ..Tol::STRICT
        };
        /// aes placement RC shares the SPEF tolerance: the PD trees are
        /// edge-identical to OpenROAD's SPEF and our STA on them agrees with
        /// the SPEF readback to <= 0.003 ps per endpoint field
        /// (`test_placement_vs_spef_sta_agree`), so the residual vs OpenSTA is
        /// the same cell-stage gap `AES_SPEF` covers.
        const AES_PLACEMENT: Tol = Tol::AES_SPEF;
        const AES_SPEF: Tol = Tol {
            slack_abs_ns: 0.025,
            slew_rel: 0.055,
            ..Tol::STRICT
        };
        /// jpeg placement uses GCD baseline; SPEF stays #[ignore]d for extreme out-of-range _43767_/Y (OpenSTA #582).
        const JPEG_PLACEMENT: Tol = Tol::GCD_PLACEMENT;
        const JPEG_SPEF: Tol = Tol::GCD_SPEF;
        /// ibex with OpenSTA's own RC: all 3,702 endpoints pass arrival, required time, WNS, and TNS
        /// within strict max(5 ps, 1%); setup/hold slacks agree within 36.6 ps (_21466_/D behind
        /// _10092_/Y fall at 4.3-5.2 ns input slew) and slews within 5.09%.
        /// ibex placement RC shares the SPEF tolerance for the same reason as
        /// `AES_PLACEMENT`: STA on the PD parasitics agrees with the SPEF
        /// readback to <= 0.013 ps per endpoint field, shared engine residual.
        const IBEX_PLACEMENT: Tol = Tol::IBEX_SPEF;
        const IBEX_SPEF: Tol = Tol {
            slack_abs_ns: 0.04,
            slew_rel: 0.055,
            ..Tol::STRICT
        };
    }

    fn within_tolerance(ours: f32, reference: f64, rel_tol: f64, abs_tol: f64) -> (bool, f64) {
        let diff = (ours as f64 - reference).abs();
        (
            diff <= abs_tol.max(rel_tol * reference.abs()),
            diff / reference.abs().max(1e-9),
        )
    }

    #[test]
    fn test_picorv32_zero_wire_matches_opensta() {
        compare_design_with_opensta("picorv32", "zero_wire", Rc::None, Tol::STRICT);
    }

    #[test]
    fn test_picorv32_placement_rc_matches_opensta() {
        compare_design_with_opensta(
            "picorv32",
            "placement_rc",
            Rc::Placement,
            Tol::PICORV32_PLACEMENT,
        );
    }

    #[test]
    fn test_picorv32_placed_spef_matches_opensta() {
        compare_design_with_opensta("picorv32", "placed_spef", Rc::Spef, Tol::PICORV32_SPEF);
    }

    #[test]
    fn test_picorv32_routed_spef_matches_opensta() {
        compare_design_with_opensta(
            "picorv32",
            "routed_spef",
            Rc::RoutedSpef,
            Tol::PICORV32_SPEF,
        );
    }

    /// `picorv32.propagated.sdc` marks `core_clk` propagated on the unbuffered clock net:
    /// routed RC now sets a per-pin launch/capture edge, so clock skew appears (hold WNS
    /// drops to -2.48 ns, TNS -1053.67 ns vs the ideal-clock golden's clean min side).
    /// Golden from `tests/designs/generate_prop_ref.sh picorv32` (OpenSTA, same inputs);
    /// its `clock_pins` array asserts per-pin arrival/slew independently of the endpoint
    /// tolerance (max abs diff 8e-6 ns).
    #[test]
    fn test_picorv32_propagated_clock_matches_opensta() {
        compare_design_files(
            "picorv32",
            "picorv32.propagated.sdc",
            "picorv32.propagated.reference.json",
            "propagated_routed_spef",
            Rc::RoutedSpef,
            Tol::PICORV32_PROPAGATED,
        );
    }

    #[test]
    fn test_gcd_zero_wire_matches_opensta() {
        compare_design_with_opensta("gcd", "zero_wire", Rc::None, Tol::STRICT);
    }

    #[test]
    fn test_gcd_placement_rc_matches_opensta() {
        // Arrival, required time and slack must agree within 1% or 5 ps. Slews get 3%: OpenSTA's
        // DMP calculator derives the driver slew from a waveform model that differs from the plain
        // table lookup by about 1% (`report_dcalc`: 0.278732 vs 0.282240); see docs/results.md.
        compare_design_with_opensta("gcd", "placement_rc", Rc::Placement, Tol::GCD_PLACEMENT);
    }

    #[test]
    fn test_gcd_placed_spef_matches_opensta() {
        compare_design_with_opensta("gcd", "placed_spef", Rc::Spef, Tol::GCD_SPEF);
    }

    #[test]
    fn test_gcd_routed_spef_matches_opensta() {
        compare_design_with_opensta("gcd", "routed_spef", Rc::RoutedSpef, Tol::GCD_SPEF);
    }

    /// `gcd.propagated.sdc` marks `core_clk` propagated: the clock net's routed RC now sets a
    /// per-pin launch/capture edge (WNS drops 1.573 -> 1.412 ns vs the ideal-clock golden).
    /// Golden from `generate_prop_ref.sh` (OpenSTA on the same placed DEF + rcx SPEF); its
    /// `clock_pins` array asserts per-pin arrival/slew (max abs diff 1e-6 ns).
    #[test]
    fn test_gcd_propagated_clock_matches_opensta() {
        compare_design_files(
            "gcd",
            "gcd.propagated.sdc",
            "gcd.propagated.reference.json",
            "propagated_routed_spef",
            Rc::RoutedSpef,
            Tol::GCD_SPEF,
        );
    }

    #[test]
    fn test_aes_zero_wire_matches_opensta() {
        compare_design_with_opensta("aes", "zero_wire", Rc::None, Tol::STRICT);
    }

    /// 17 endpoint fields sit within the SPEF-mode residual band (slack <= 16 ps,
    /// slew <= 5.24%) and our STA on the PD parasitics matches the SPEF
    /// readback to <= 0.003 ps per endpoint field; `AES_PLACEMENT` therefore
    /// uses `AES_SPEF`'s tolerance. See docs/results.md.
    #[test]
    fn test_aes_placement_rc_matches_opensta() {
        compare_design_with_opensta("aes", "placement_rc", Rc::Placement, Tol::AES_PLACEMENT);
    }

    #[test]
    fn test_aes_placed_spef_matches_opensta() {
        compare_design_with_opensta("aes", "placed_spef", Rc::Spef, Tol::AES_SPEF);
    }

    #[test]
    fn test_jpeg_zero_wire_matches_opensta() {
        compare_design_with_opensta("jpeg", "zero_wire", Rc::None, Tol::STRICT);
    }

    /// Ignored: the PD trees are edge-identical to OpenROAD's placement SPEF (verified on all
    /// 46,567 nets), so the residual is identical to `placed_spef`'s — the OpenSTA #582
    /// out-of-table NLDM extrapolation on extreme slew/load (WNS ours -78.084 vs ref -61.199 ns,
    /// TNS -134,605 vs -123,078 ns). See docs/results.md.
    #[test]
    #[ignore]
    fn test_jpeg_placement_rc_matches_opensta() {
        compare_design_with_opensta("jpeg", "placement_rc", Rc::Placement, Tol::JPEG_PLACEMENT);
    }

    /// Ignored (out of scope per OpenSTA #582): `_43767_/Y` drives 2.23 pF (7x Liberty table max
    /// 0.303 pF) at 9.14 ns input slew (6x table max 1.50 ns); the OpenSTA maintainer confirmed
    /// "You can't expect the delay calculator to work very well when the slew/load are outside the
    /// table values that characterize the gate." With `RD_SCALE = 3.0` + `driver_waveform_slew` +
    /// `is_lumped_bailout`, mismatches drop 7,045 -> 1,419 (`_42236_/Y` error is 0.0000 ns; WNS
    /// -78.084 vs ref -61.199 ns, TNS -134,605 vs -123,078 ns). See docs/results.md.
    #[test]
    #[ignore]
    fn test_jpeg_placed_spef_matches_opensta() {
        compare_design_with_opensta("jpeg", "placed_spef", Rc::Spef, Tol::JPEG_SPEF);
    }

    /// ibex's SDC (ORFS `constraint.sdc`, verbatim) sets `set_clock_latency 1.095` and I/O delays
    /// against the virtual clock `vclk_core_clock`, which no earlier design exercised. Any failure
    /// here must be checked against those constructs before the tolerance is touched.
    /// The two clock-gate endpoints (latch D and the gate enable) come from ticket 10.
    #[test]
    fn test_ibex_zero_wire_matches_opensta() {
        compare_design_with_opensta("ibex", "zero_wire", Rc::None, Tol::STRICT);
    }

    /// 3 `max_slack_ns` fields (<= 36 ps) sit within the SPEF-mode residual band
    /// and our STA on the PD parasitics matches the SPEF readback to
    /// <= 0.013 ps per endpoint field; `IBEX_PLACEMENT` therefore uses
    /// `IBEX_SPEF`'s tolerance. See docs/results.md.
    #[test]
    fn test_ibex_placement_rc_matches_opensta() {
        compare_design_with_opensta("ibex", "placement_rc", Rc::Placement, Tol::IBEX_PLACEMENT);
    }

    #[test]
    fn test_ibex_placed_spef_matches_opensta() {
        compare_design_with_opensta("ibex", "placed_spef", Rc::Spef, Tol::IBEX_SPEF);
    }

    /// Placement RC vs the placement-estimate SPEF read back through our own
    /// STA engine on the same DEF/SDC: every endpoint field, max/min WNS/TNS
    /// and the violation counts must agree. This is the direct evidence that
    /// `placement_rc` and `placed_spef` measure the same residual against
    /// OpenSTA, which licenses `AES_PLACEMENT`/`IBEX_PLACEMENT` sharing the
    /// SPEF tolerances (per-net Elmore alone cannot show path accumulation).
    #[test]
    fn test_placement_vs_spef_sta_agree() {
        for design_name in ["aes", "ibex", "jpeg"] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(
                &dir.join(format!("{design_name}.placed.def")),
                &tech,
                Some(&lib),
            )
            .expect("DEF");
            let sdc = load_sdc(&dir.join(format!("{design_name}.sdc"))).expect("SDC");
            let rc = crate::sta::WireRc::from_layer(&tech, "met2").expect("met2 RC");
            let parasitics_p = crate::sta::estimate_placement_parasitics(&design, &tech, rc);
            // `*.routed.spef` is OpenROAD's `estimate_parasitics -placement`
            // output for these designs — a placement estimate, not a routed
            // extraction (tests/designs/README.md).
            let spef = crate::readers::load_spef(&dir.join(format!("{design_name}.routed.spef")))
                .expect("SPEF");
            let parasitics_s =
                crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
            let (rp, _, _) = run_sta_detail(&design, &lib, &sdc, Some(&parasitics_p), true)
                .expect("placement STA");
            let (rs, _, _) =
                run_sta_detail(&design, &lib, &sdc, Some(&parasitics_s), true).expect("SPEF STA");
            assert_eq!(
                rp.endpoints.len(),
                rs.endpoints.len(),
                "{design_name}: endpoint count"
            );
            let fields = |e: &EndpointReport| {
                [
                    e.max_arr_ns,
                    e.max_req_ns,
                    e.max_slack_ns,
                    e.max_slew_ns,
                    e.min_arr_ns,
                    e.min_req_ns,
                    e.min_slack_ns,
                    e.min_slew_ns,
                ]
            };
            let mut worst = [0.0f64; 8];
            for (ep, es) in rp.endpoints.iter().zip(rs.endpoints.iter()) {
                assert_eq!(ep.pin, es.pin, "{design_name}: endpoint order");
                for (w, (a, b)) in worst.iter_mut().zip(fields(ep).into_iter().zip(fields(es))) {
                    *w = (*w).max((a as f64 - b as f64).abs());
                }
            }
            let wns_d = [
                (rp.wns_max_ns as f64 - rs.wns_max_ns as f64).abs(),
                (rp.wns_min_ns as f64 - rs.wns_min_ns as f64).abs(),
            ];
            let tns_d = [
                (rp.tns_max_ns as f64 - rs.tns_max_ns as f64).abs(),
                (rp.tns_min_ns as f64 - rs.tns_min_ns as f64).abs(),
            ];
            let [wns_max_d, wns_min_d] = wns_d;
            let [tns_max_d, tns_min_d] = tns_d;
            println!(
                "{design_name}: placement-vs-SPEF worst endpoint |diff| {:?} ps, \
                 wns_max {wns_max_d:.6} ns, wns_min {wns_min_d:.6} ns, \
                 tns_max {tns_max_d:.6} ns, tns_min {tns_min_d:.6} ns",
                worst.map(|w| (w * 1e6).round() / 1e3)
            );
            assert_eq!(
                rp.setup_violations, rs.setup_violations,
                "{design_name}: setup violations"
            );
            assert_eq!(
                rp.hold_violations, rs.hold_violations,
                "{design_name}: hold violations"
            );
            // Measured 2026-11-18; see docs/results.md. Endpoint fields stay
            // under 10 ps (the SPEF slack floors are 25/40 ps); TNS is a sum
            // over violating endpoints so per-pin diffs accumulate — bound it
            // at max(10 ps, 1% of |reference TNS|), the golden tns_rel
            // convention with a floor for near-zero TNS.
            assert!(
                worst.iter().all(|&w| w <= 0.010),
                "{design_name}: placement vs SPEF endpoint fields diverge: {worst:?} ns"
            );
            for (side, ((wd, td), tns_ref)) in ["max", "min"].into_iter().zip(
                wns_d
                    .into_iter()
                    .zip(tns_d)
                    .zip([rs.tns_max_ns, rs.tns_min_ns]),
            ) {
                assert!(wd <= 0.010, "{design_name}: {side} WNS diverges by {wd} ns");
                assert!(
                    td <= 0.01 * (tns_ref as f64).abs().max(1.0),
                    "{design_name}: {side} TNS diverges by {td} ns"
                );
            }
        }
    }

    #[test]
    fn test_skip_report_counts_unmapped_cells() {
        let lib_dir = sky130_hd();
        let dir = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let mut design = parse_def(&dir.join("gcd.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("gcd.sdc")).expect("SDC");

        let baseline = run_sta(&design, &lib, &sdc, None).expect("baseline STA");
        // gcd.placed.def includes 93 sky130_fd_sc_hd__tapvpwrvgnd_1 welltap instances that have no Liberty cell.
        assert_eq!(baseline.skipped.unmapped_cells, 93);

        let target = design
            .cell_lib_ids
            .iter()
            .position(|id| id.is_some())
            .expect("mapped cell");
        design.cell_lib_ids[target] = None;
        let report = run_sta(&design, &lib, &sdc, None).expect("STA with unmapped cell");
        assert_eq!(report.skipped.unmapped_cells, 94);
        assert!(
            report
                .skipped
                .nonzero()
                .contains(&"unmapped_cells=94".to_string())
        );
    }

    /// N1: every endpoint candidate that produces no check is named with its pin,
    /// kind and reason. ibex's unreached endpoints are all driven by constant
    /// `conb_1` cells (no Liberty timing arcs), so they report `untimed_driver` and
    /// name the driving pin — covering both the `check` and `output_port` kinds.
    #[test]
    fn test_unreached_endpoint_diagnostics() {
        let lib_dir = sky130_hd();
        let dir = design_dir("ibex");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("ibex.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("ibex.sdc")).expect("SDC");
        let report = run_sta(&design, &lib, &sdc, None).expect("STA");

        let skipped = &report.skipped;
        assert_eq!(
            skipped.skipped_endpoints.len(),
            skipped.unreached_endpoints + skipped.incomplete_checks
        );
        assert!(!skipped.skipped_endpoints.is_empty());
        for e in &skipped.skipped_endpoints {
            assert_eq!(e.reason, SkipReason::UntimedDriver, "{}", e.pin);
            assert!(e.related.is_some(), "{}", e.pin);
        }
        let kind_of = |pin: &str| {
            skipped
                .skipped_endpoints
                .iter()
                .find(|e| e.pin == pin)
                .unwrap_or_else(|| panic!("{pin} not reported"))
                .kind
        };
        assert_eq!(kind_of("_21812_/D"), EndpointKind::Check);
        assert_eq!(kind_of("alert_major_o"), EndpointKind::OutputPort);
    }

    /// Recount of the latch arcs `build_graph` drops, tallied straight from the Liberty
    /// arcs and the netlist pins, mirroring `build_graph`'s classification: a latch is
    /// modelled when `latch_enable` resolves its enable; a modelled latch keeps
    /// setup/hold/recovery/removal checks, the opening edge arc and `D -> Q` transparent
    /// arcs, and drops everything else (`min_pulse_width` and unknown types).
    fn expected_latch_skips(
        cx: &Context,
        design: &DesignDb,
        lib: &LibDb,
    ) -> (usize, BTreeMap<String, usize>) {
        let mut cells = 0;
        let mut arcs: BTreeMap<String, usize> = BTreeMap::new();
        for c in 0..design.num_cells() {
            let Some(lib_cell_id) = design.cell_lib_ids[c] else {
                continue;
            };
            let lib_cell = &lib.cells[lib_cell_id.0 as usize];
            if !lib_cell.is_latch {
                continue;
            }
            let cell = CellId(c as u32);
            let Some((enable, open_pol, _)) = latch_enable(cx, cell, lib_cell) else {
                cells += 1;
                for i in lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32 {
                    *arcs
                        .entry(format!("{:?}", lib.arcs[i as usize].timing_type))
                        .or_default() += 1;
                }
                continue;
            };
            for i in lib_cell.arc_offset..lib_cell.arc_offset + lib_cell.arc_count as u32 {
                let arc = &lib.arcs[i as usize];
                let (Some(src), Some(dst)) = (
                    cx.cell_pin_for_lib_pin(cell, arc.related_pin),
                    cx.cell_pin_for_lib_pin(cell, arc.target_pin),
                ) else {
                    continue; // unresolved arcs are counted in `unresolved_arcs` instead
                };
                let is_check = matches!(
                    arc.timing_type,
                    TimingType::SetupRising
                        | TimingType::SetupFalling
                        | TimingType::HoldRising
                        | TimingType::HoldFalling
                        | TimingType::RecoveryRising
                        | TimingType::RecoveryFalling
                        | TimingType::RemovalRising
                        | TimingType::RemovalFalling
                );
                let is_open = src == enable
                    && matches!(
                        arc.timing_type,
                        TimingType::RisingEdge | TimingType::FallingEdge
                    )
                    && check_transition(arc) == open_pol
                    && design.pin_is_driver[dst.0 as usize];
                let is_borrow = matches!(
                    arc.timing_type,
                    TimingType::Combinational
                        | TimingType::CombinationalRise
                        | TimingType::CombinationalFall
                ) && design.pin_is_driver[dst.0 as usize];
                // `min_pulse_width` resolves to a `PulseWidthCheck` before the
                // latch arm; deferred checks land in `recognized_check_arcs`.
                if !is_check
                    && !is_open
                    && !is_borrow
                    && !arc.timing_type.is_deferred_check()
                    && arc.timing_type != TimingType::MinPulseWidth
                {
                    *arcs.entry(format!("{:?}", arc.timing_type)).or_default() += 1;
                }
            }
        }
        (cells, arcs)
    }

    /// R0: the skip summary counts latch cells and arcs per kind, with totals that an
    /// independent recount from the Liberty and the netlist reproduces. The literal maps are
    /// the ground truth read off `sky130_fd_sc_hd__tt_025C_1v80.lib`: `dlxtp_1` has five arcs
    /// (`setup_falling`, `hold_falling`, `min_pulse_width`, `combinational`,
    /// `rising_edge`) and drops none — its `min_pulse_width` resolves to an evaluated
    /// pulse-width check before the latch arm. `dlxtn_1` drops nothing for the same
    /// reason.
    #[test]
    fn test_skip_report_counts_latch_arcs() {
        for (design_name, def_file, sdc_file, want) in [
            (
                "sta_edge",
                "non_dlxtn_latch/edge_latch.def",
                "non_dlxtn_latch/edge_latch.sdc",
                (0, BTreeMap::new(), BTreeMap::new()),
            ),
            (
                "ibex",
                "ibex.placed.def",
                "ibex.sdc",
                (
                    0,
                    BTreeMap::new(),
                    BTreeMap::from([
                        ("Clear".to_string(), 1655),
                        ("Preset".to_string(), 9),
                    ]),
                ),
            ),
        ] {
            let lib_dir = sky130_hd();
            let dir = design_dir(design_name);
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(&dir.join(def_file), &tech, Some(&lib)).expect("DEF");
            let sdc = load_sdc(&dir.join(sdc_file)).expect("SDC");
            let report = run_sta(&design, &lib, &sdc, None).expect("STA");

            let (want_cells, want_arcs, want_checks) = want;
            assert_eq!(
                report.skipped.unmodelled_latch_cells, want_cells,
                "{design_name}"
            );
            assert_eq!(
                report.skipped.latch_arcs_dropped, want_arcs,
                "{design_name}"
            );
            let cx = Context {
                design: &design,
                lib: &lib,
                sdc: &sdc,
                parasitics: None,
                pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
            };
            let (cells, arcs) = expected_latch_skips(&cx, &design, &lib);
            assert_eq!((cells, arcs), (want_cells, want_arcs), "{design_name}");

            let total_arcs: usize = (0..design.num_cells())
                .filter_map(|c| design.cell_lib_ids[c])
                .map(|id| lib.cells[id.0 as usize].arc_count as usize)
                .sum();
            assert_eq!(report.skipped.total_arcs, total_arcs, "{design_name}");
            assert!(
                report.skipped.arcs_skipped() <= report.skipped.total_arcs,
                "{design_name}"
            );
            // `min_pulse_width` arcs evaluate to check rows only on
            // clock-network pins — `build_graph` defers the rest into
            // `recognized_check_arcs` (OpenSTA emits no row for a
            // `dfrtn` RESET_B arc). Recount constraint tables on the
            // retained graph entries for the expected row count and the
            // dropped-arc count for the expected deferred map.
            let all_mpw_arcs: usize = (0..design.num_cells())
                .filter_map(|c| design.cell_lib_ids[c])
                .flat_map(|id| {
                    let lc = &lib.cells[id.0 as usize];
                    &lib.arcs[lc.arc_offset as usize
                        ..(lc.arc_offset + lc.arc_count as u32) as usize]
                })
                .filter(|a| a.timing_type == TimingType::MinPulseWidth)
                .count();
            let mut throwaway = SkipReport::default();
            let graph = build_graph(&cx, &mut throwaway);
            let want_pulse_rows: usize = graph
                .pulse_width_checks
                .iter()
                .map(|c| {
                    let a = &lib.arcs[c.lib_arc as usize];
                    a.rise_constraint.is_some() as usize
                        + a.fall_constraint.is_some() as usize
                })
                .sum();
            let mut want_checks = want_checks.clone();
            let dropped_mpw = all_mpw_arcs - graph.pulse_width_checks.len();
            if dropped_mpw > 0 {
                want_checks.insert("MinPulseWidth".to_string(), dropped_mpw);
            }
            assert_eq!(
                report.skipped.recognized_check_arcs, want_checks,
                "{design_name}"
            );
            let pulse_skipped = report
                .skipped
                .skipped_endpoints
                .iter()
                .filter(|e| e.kind == EndpointKind::MinPulseWidth)
                .count();
            assert_eq!(
                report.pulse_width_checks.len() + pulse_skipped,
                want_pulse_rows,
                "{design_name}"
            );
            assert_eq!(
                report.endpoints.len()
                    + report.pulse_width_checks.len()
                    + report.skipped.endpoints_skipped(),
                report.skipped.total_endpoints,
                "{design_name}"
            );

            let json = serde_json::to_value(&report).expect("report JSON");
            assert_eq!(
                json["skipped"]["unmodelled_latch_cells"].as_u64().unwrap() as usize,
                want_cells
            );
            assert_eq!(
                json["skipped"]["total_arcs"].as_u64().unwrap() as usize,
                total_arcs
            );
            assert!(json["skipped"]["sdc_commands"].as_u64().unwrap() > 0);
        }
    }

    /// Pi models and effective capacitances that OpenSTA's `report_dcalc` printed for six arcs of
    /// picorv32 (`resetn` to `_10018_/D` path). OpenSTA names the driver-side capacitor C2 and the
    /// far one C1; ours are the other way round.
    #[test]
    fn test_dmp_ceff_matches_opensta_report_dcalc() {
        use crate::sta::dmp::solve_dmp;
        let lib_dir = sky130_hd();
        let dir = design_dir("picorv32");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("picorv32.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("picorv32.sdc")).expect("SDC");
        let spef = crate::readers::load_spef(&dir.join("picorv32.routed.spef")).expect("SPEF");
        let par = crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
        let cx = Context {
            design: &design,
            lib: &lib,
            sdc: &sdc,
            parasitics: Some(&par),
            pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
        };
        let graph = build_graph(&cx, &mut SkipReport::default());
        let find_pin = |name: &str| {
            (0..design.num_pins())
                .map(|i| PinId(i as u32))
                .find(|&p| cx.pin_name(p) == name)
                .expect("pin")
        };
        // (input pin, output pin, output transition, input slew, OpenSTA (far C1, rpi, near C2, ceff))
        let cases = [
            (
                "_04923_/A",
                "_04923_/Y",
                FALL,
                0.167736,
                (0.218153, 0.140104, 0.072105, 0.277980),
            ),
            (
                "_04923_/A",
                "_04923_/Y",
                RISE,
                0.166990,
                (0.239916, 0.140532, 0.079293, 0.313281),
            ),
            (
                "_06198_/C",
                "_06198_/X",
                RISE,
                0.203853,
                (0.177339, 0.395220, 0.039440, 0.203829),
            ),
            (
                "_06198_/C",
                "_06198_/X",
                FALL,
                0.207974,
                (0.170106, 0.392899, 0.037627, 0.179647),
            ),
            (
                "_06200_/A",
                "_06200_/Y",
                FALL,
                1.057125,
                (0.123982, 0.116795, 0.038922, 0.158764),
            ),
            (
                "_06200_/A",
                "_06200_/Y",
                RISE,
                0.483218,
                (0.130804, 0.116436, 0.040705, 0.170241),
            ),
        ];
        for (src, dst, pol, slew_in, (far, rpi, near, ceff)) in cases {
            let (src, dst) = (find_pin(src), find_pin(dst));
            let edge = graph
                .outgoing(src)
                .iter()
                .map(|&i| graph.edges[i as usize])
                .find(|e| e.dst == dst)
                .expect("arc");
            let arc = &lib.arcs[edge.lib_arc as usize];
            let net = design.pin_nets[dst.0 as usize].unwrap();
            let caps: Vec<f32> = design
                .net_pins(net)
                .iter()
                .map(|&p| cx.sink_cap(p, pol))
                .collect();
            let pi = par.net(net).unwrap().pi_model(&caps);
            let name = cx.pin_name(dst);
            let close = |ours: f64, reference: f64, rel: f64| {
                (ours - reference).abs() <= rel * reference.abs()
            };
            // The reference prints 6 digits; the pi moments must match to about that precision.
            assert!(
                close(pi.c2, far, 1e-4) && close(pi.c1, near, 1e-4) && close(pi.rpi, rpi, 1e-4),
                "{name} pol {pol}: pi {pi:?}"
            );
            let (Some(delay_table), Some(slew_table)) = delay_tables(arc, pol) else {
                panic!("no tables")
            };
            let table = |c: f64| {
                (
                    eval_nldm_delay_or_slew(delay_table, slew_in, c as f32) as f64,
                    eval_nldm_delay_or_slew(slew_table, slew_in, c as f32) as f64,
                )
            };
            let result = solve_dmp(&pi, RD_SCALE, &table);
            // A single fitted driver-resistance scale cannot fit every arc (asserted within 6% at `RD_SCALE = 3.0`).
            assert!(
                close(result.ceff, ceff, 0.06),
                "{name} pol {pol}: ceff ours {} vs OpenSTA {ceff}",
                result.ceff
            );
        }
    }

    /// One data row of `dcalc_routed_ref.txt`: OpenSTA `report_dcalc` on the routed SPEF.
    struct DcalcRow {
        line: usize,
        from: String,
        to: String,
        sense: String,
        sense_kind: TimingSense,
        out: String,
        out_pol: usize,
        input_slew: f64,
        /// OpenSTA's far capacitor C1 (ours: `PiModel::c2`).
        far_c1: f64,
        rpi: f64,
        /// OpenSTA's near capacitor C2 (ours: `PiModel::c1`).
        near_c2: f64,
        ceff: f64,
        delay: f64,
        slew: f64,
        driver_slew: f64,
    }

    fn parse_dcalc_rows(text: &str) -> Vec<DcalcRow> {
        let mut rows = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = trimmed.split_whitespace().collect();
            assert_eq!(
                f.len(),
                12,
                "dcalc_routed_ref.txt line {line}: expected 12 columns, got {}",
                f.len()
            );
            let num = |s: &str| -> f64 {
                s.parse().unwrap_or_else(|_| {
                    panic!("dcalc_routed_ref.txt line {line}: bad number {s:?}")
                })
            };
            let sense_kind = match f[2] {
                "positive_unate" => TimingSense::PositiveUnate,
                "negative_unate" => TimingSense::NegativeUnate,
                "non_unate" => TimingSense::NonUnate,
                other => panic!("dcalc_routed_ref.txt line {line}: unknown arc sense {other:?}"),
            };
            let out_pol = match f[3] {
                "rise" => RISE,
                "fall" => FALL,
                other => {
                    panic!("dcalc_routed_ref.txt line {line}: unknown output transition {other:?}")
                }
            };
            rows.push(DcalcRow {
                line,
                from: f[0].to_string(),
                to: f[1].to_string(),
                sense: f[2].to_string(),
                sense_kind,
                out: f[3].to_string(),
                out_pol,
                input_slew: num(f[4]),
                far_c1: num(f[5]),
                rpi: num(f[6]),
                near_c2: num(f[7]),
                ceff: num(f[8]),
                delay: num(f[9]),
                slew: num(f[10]),
                driver_slew: num(f[11]),
            });
        }
        rows
    }

    /// Graph arcs from `src` to `dst` that a `report_dcalc` row can refer to: same Liberty sense, and
    /// the same transition filter `build_graph` applies to combinational arcs. Register `CLK -> Q`
    /// arcs are launches, not edges, so both lists are searched.
    fn matching_arcs(
        lib: &LibDb,
        graph: &TimingGraph,
        src: PinId,
        dst: PinId,
        row: &DcalcRow,
    ) -> Vec<u32> {
        let combinational = graph
            .outgoing(src)
            .iter()
            .map(|&i| graph.edges[i as usize])
            .filter(|e| e.dst == dst)
            .map(|e| e.lib_arc);
        let launches = graph
            .launches
            .iter()
            .filter(|l| l.clk == src && l.q == dst)
            .map(|l| l.lib_arc);
        combinational
            .chain(launches)
            .filter(|&a| {
                let arc = &lib.arcs[a as usize];
                let transition_ok = !((arc.timing_type == TimingType::CombinationalRise
                    && row.out_pol != RISE)
                    || (arc.timing_type == TimingType::CombinationalFall && row.out_pol != FALL));
                arc.timing_sense == row.sense_kind && transition_ok
            })
            .collect()
    }

    fn median(mut v: Vec<f64>) -> f64 {
        assert!(!v.is_empty(), "median of no values");
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        }
    }

    /// Ticket 17 (E3.5, step 7): our pi model and DMP solution against every row of OpenSTA's
    /// `report_dcalc` on the routed SPEF. Measurement only: prints one row per arc and a summary; no
    /// error magnitude is asserted. Errors are ours minus OpenSTA, in percent of OpenSTA's value.
    #[test]
    fn test_dmp_matches_opensta_report_dcalc_routed() {
        use crate::sta::dmp::solve_dmp;
        const QUANTITIES: [&str; 7] = [
            "C1far", "Rpi", "C2near", "Ceff", "delay", "slew", "drv_slew",
        ];
        let lib_dir = sky130_hd();
        let dir = design_dir("picorv32");
        let text = std::fs::read_to_string(dir.join("dcalc_routed_ref.txt"))
            .expect("dcalc_routed_ref.txt");
        let rows = parse_dcalc_rows(&text);
        let data_lines = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .count();
        assert_eq!(rows.len(), data_lines, "parsed rows vs data lines");
        assert!(
            rows.len() >= 60,
            "dcalc_routed_ref.txt has {} rows; expected at least 60",
            rows.len()
        );

        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("picorv32.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("picorv32.sdc")).expect("SDC");
        let spef = crate::readers::load_spef(&dir.join("picorv32.rcx.spef")).expect("SPEF");
        let par = crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
        let cx = Context {
            design: &design,
            lib: &lib,
            sdc: &sdc,
            parasitics: Some(&par),
            pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
        };
        let graph = build_graph(&cx, &mut SkipReport::default());
        let find_pin = |name: &str| {
            (0..design.num_pins())
                .map(|i| PinId(i as u32))
                .find(|&p| cx.pin_name(p) == name)
        };
        let pct = |ours: f64, reference: f64| (ours - reference) / reference.abs() * 100.0;

        println!(
            "{:<52} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9}",
            "key (from -> to sense out)",
            "C1far%",
            "Rpi%",
            "C2near%",
            "Ceff%",
            "delay%",
            "slew%",
            "drvslew%"
        );
        let mut matched_errors: Vec<[f64; 7]> = Vec::new();
        let mut unmodelled = 0usize;
        for row in &rows {
            let key = format!("{} -> {} {} {}", row.from, row.to, row.sense, row.out);
            let (Some(src), Some(dst)) = (find_pin(&row.from), find_pin(&row.to)) else {
                println!("UNMODELLED {key} (line {}): pin not in design", row.line);
                unmodelled += 1;
                continue;
            };
            let arc_index = match matching_arcs(&lib, &graph, src, dst, row).as_slice() {
                [] => {
                    println!(
                        "UNMODELLED {key} (line {}): no graph arc with this sense and transition",
                        row.line
                    );
                    unmodelled += 1;
                    continue;
                }
                [one] => *one,
                many => {
                    println!(
                        "UNMODELLED {key} (line {}): {} matching arcs, ambiguous",
                        row.line,
                        many.len()
                    );
                    unmodelled += 1;
                    continue;
                }
            };
            let arc = &lib.arcs[arc_index as usize];
            let net = design.pin_nets[dst.0 as usize].expect("output pin has a net");
            let caps: Vec<f32> = design
                .net_pins(net)
                .iter()
                .map(|&p| cx.sink_cap(p, row.out_pol))
                .collect();
            let pi = par
                .net(net)
                .unwrap_or_else(|| panic!("{key}: no routed parasitics for its net"))
                .pi_model(&caps);
            let (Some(delay_table), Some(slew_table)) = delay_tables(arc, row.out_pol) else {
                panic!("{key}: no NLDM tables for this transition")
            };
            let slew_in = row.input_slew as f32;
            let table = |c: f64| {
                (
                    eval_nldm_delay_or_slew(delay_table, slew_in, c as f32) as f64,
                    eval_nldm_delay_or_slew(slew_table, slew_in, c as f32) as f64,
                )
            };
            let result = solve_dmp(&pi, RD_SCALE, &table);
            let wave_slew = driver_waveform_slew(&pi, &result, &table);
            let errors = [
                pct(pi.c2, row.far_c1),
                pct(pi.rpi, row.rpi),
                pct(pi.c1, row.near_c2),
                pct(result.ceff, row.ceff),
                pct(result.delay, row.delay),
                pct(result.slew, row.slew),
                pct(wave_slew, row.driver_slew),
            ];
            println!(
                "{key:<52} {:>+8.2} {:>+8.2} {:>+8.2} {:>+8.2} {:>+8.2} {:>+8.2} {:>+9.2}",
                errors[0], errors[1], errors[2], errors[3], errors[4], errors[5], errors[6]
            );
            matched_errors.push(errors);
        }

        let matched = matched_errors.len();
        assert_eq!(
            matched + unmodelled,
            rows.len(),
            "every row is matched or unmodelled"
        );
        println!(
            "summary: rows {} matched {matched} unmodelled {unmodelled}",
            rows.len()
        );
        assert!(matched > 0, "no row matched a graph arc");
        for (q, name) in QUANTITIES.iter().enumerate() {
            let abs: Vec<f64> = matched_errors.iter().map(|e| e[q].abs()).collect();
            let max = abs.iter().copied().fold(0.0, f64::max);
            println!(
                "summary {name}: median |err| {:.2}%  max |err| {:.2}%",
                median(abs.clone()),
                max
            );
        }
    }

    /// `visit(row, pi, table, table_at)`: `table(c)` is the arc at the row's input slew, `table_at(slew, c)` at any input slew.
    type RowVisitor<'a> = dyn FnMut(
            &DcalcRow,
            &crate::sta::dmp::PiModel,
            crate::sta::dmp::DriverTable,
            &dyn Fn(f64, f64) -> (f64, f64),
        ) + 'a;

    /// Calls `visit(row, pi, table)` for every row of `file` (a `collect_dcalc.sh` table of `design`, read on
    /// the DEF and SPEF named) that matches exactly one graph arc; returns `(matched, unmodelled)`.
    fn for_each_dcalc_row(
        design_name: &str,
        def_file: &str,
        spef_file: &str,
        file: &str,
        visit: &mut RowVisitor,
    ) -> (usize, usize) {
        let lib_dir = sky130_hd();
        let dir = design_dir(design_name);
        let text =
            std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let rows = parse_dcalc_rows(&text);
        assert!(!rows.is_empty(), "{file} has no rows");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join(def_file), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join(format!("{design_name}.sdc"))).expect("SDC");
        let spef = crate::readers::load_spef(&dir.join(spef_file)).expect("SPEF");
        let par = crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
        let cx = Context {
            design: &design,
            lib: &lib,
            sdc: &sdc,
            parasitics: Some(&par),
            pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
        };
        let graph = build_graph(&cx, &mut SkipReport::default());
        let by_name: std::collections::HashMap<String, PinId> = (0..design.num_pins())
            .map(|i| PinId(i as u32))
            .map(|p| (cx.pin_name(p), p))
            .collect();
        let (mut matched, mut unmodelled) = (0usize, 0usize);
        for row in &rows {
            let (Some(&src), Some(&dst)) = (by_name.get(&row.from), by_name.get(&row.to)) else {
                unmodelled += 1;
                continue;
            };
            let candidates = matching_arcs(&lib, &graph, src, dst, row);
            let [arc_index] = candidates.as_slice() else {
                unmodelled += 1;
                continue;
            };
            let arc = &lib.arcs[*arc_index as usize];
            let net = design.pin_nets[dst.0 as usize].expect("output pin has a net");
            let caps: Vec<f32> = design
                .net_pins(net)
                .iter()
                .map(|&p| cx.sink_cap(p, row.out_pol))
                .collect();
            let pi = par
                .net(net)
                .unwrap_or_else(|| panic!("no parasitics for the net of {}", row.to))
                .pi_model(&caps);
            let (Some(delay_table), Some(slew_table)) = delay_tables(arc, row.out_pol) else {
                continue;
            };
            let slew_in = row.input_slew as f32;
            let table = |c: f64| {
                (
                    eval_nldm_delay_or_slew(delay_table, slew_in, c as f32) as f64,
                    eval_nldm_delay_or_slew(slew_table, slew_in, c as f32) as f64,
                )
            };
            let table_at = |slew: f64, c: f64| {
                (
                    eval_nldm_delay_or_slew(delay_table, slew as f32, c as f32) as f64,
                    eval_nldm_delay_or_slew(slew_table, slew as f32, c as f32) as f64,
                )
            };
            visit(row, &pi, &table, &table_at);
            matched += 1;
        }
        (matched, unmodelled)
    }

    /// Measurement (E3.5, `docs/history/e35`): driver-resistance rules against the `report_dcalc` tables of
    /// `collect_dcalc.sh`. Constants are fitted on picorv32 and scored on aes (gcd has no shielded rows). Prints; asserts only that the
    /// rows matched. Shielded = OpenSTA's Ceff is below 97% of the total load.
    #[test]
    fn test_dmp_rd_rule_sweep() {
        use crate::sta::dmp::solve_dmp_with;
        type Rule = Box<dyn Fn(&dyn Fn(f64) -> (f64, f64), f64) -> f64>;
        let slope = |table: &dyn Fn(f64) -> (f64, f64), c: f64, which: usize| {
            let h = 0.05 * c;
            let (hi, lo) = (table(c + h), table(c - h));
            if which == 0 {
                (hi.0 - lo.0) / (2.0 * h)
            } else {
                (hi.1 - lo.1) / (2.0 * h)
            }
        };
        let mut rules: Vec<(String, Rule)> = Vec::new();
        for scale in [std::f64::consts::LOG2_E, 2.0, 3.0, 4.5] {
            rules.push((
                format!("delay_slope x {scale}"),
                Box::new(move |t, c| (scale * slope(t, c, 0)).max(1e-6)),
            ));
        }
        let ln4 = 4f64.ln();
        for scale in [3.0, 4.5, 6.0] {
            rules.push((
                format!("min(delay_slope x {scale}, slew/(ln4 C))"),
                Box::new(move |t, c| (scale * slope(t, c, 0)).min(t(c).1 / (ln4 * c)).max(1e-6)),
            ));
        }
        for k in [1.8, 1.9, 2.0, 2.1, 2.2] {
            rules.push((
                format!("slew_slope x {k}"),
                Box::new(move |t, c| (k * slope(t, c, 1)).max(1e-6)),
            ));
        }
        let sets = [
            (
                "picorv32",
                "picorv32.placed.def",
                "picorv32.rcx.spef",
                "dcalc_routed_all.txt",
            ),
            (
                "gcd",
                "gcd.placed.def",
                "gcd.rcx.spef",
                "dcalc_routed_all.txt",
            ),
            (
                "aes",
                "aes.placed.def",
                "aes.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "ibex",
                "ibex.placed.def",
                "ibex.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "jpeg",
                "jpeg.placed.def",
                "jpeg.routed.spef",
                "dcalc_placed_all.txt",
            ),
        ];
        for (design, def, spef, file) in sets {
            for (name, rule) in &rules {
                // [Ceff, delay, slew] absolute percent errors, all rows and shielded rows.
                let mut all: [Vec<f64>; 3] = Default::default();
                let mut shielded: [Vec<f64>; 3] = Default::default();
                let mut worst: Vec<(f64, String)> = Vec::new();
                let mut degenerate = 0usize;
                let (matched, unmodelled) = for_each_dcalc_row(
                    design,
                    def,
                    spef,
                    file,
                    &mut |row, pi, table, _table_at| {
                        let r = solve_dmp_with(pi, &|t, c| rule(t, c), table);
                        if crate::sta::dmp::ramp_fit_degenerate(rule(table, r.ceff), r.ceff, r.slew)
                        {
                            degenerate += 1;
                        }
                        let errs = [
                            (r.ceff - row.ceff) / row.ceff * 100.0,
                            (r.delay - row.delay) / row.delay * 100.0,
                            (r.slew - row.slew) / row.slew * 100.0,
                        ];
                        let is_shielded = row.ceff < 0.97 * (pi.c1 + pi.c2);
                        worst.push((
                        errs[0],
                        format!(
                            "{} -> {} {} {}: Ceff {:+.2}% delay {:+.2}% slew {:+.2}% (ref Ceff/total {:.3}, total {:.4} pF, rpi {:.3}, in slew {:.3})",
                            row.from, row.to, row.sense, row.out, errs[0], errs[1], errs[2], row.ceff / (pi.c1 + pi.c2), pi.c1 + pi.c2, pi.rpi, row.input_slew
                        ),
                    ));
                        for (q, e) in errs.iter().enumerate() {
                            all[q].push(e.abs());
                            if is_shielded {
                                shielded[q].push(e.abs());
                            }
                        }
                    },
                );
                assert!(matched > 0);
                let stat = |v: &Vec<f64>| {
                    if v.is_empty() {
                        "n/a".to_string()
                    } else {
                        format!(
                            "{:.2}/{:.2}",
                            median(v.clone()),
                            v.iter().copied().fold(0.0, f64::max)
                        )
                    }
                };
                worst.sort_by(|x, y| y.0.abs().total_cmp(&x.0.abs()));
                for (_, line) in worst.iter().take(3) {
                    println!("WORST {design} {name}: {line}");
                }
                println!(
                    "SWEEP {design} {name:<22} rows {matched} (unmodelled {unmodelled}) dt0 {degenerate} shielded {} | all Ceff {} delay {} slew {} | shielded Ceff {} delay {} slew {}",
                    shielded[0].len(),
                    stat(&all[0]),
                    stat(&all[1]),
                    stat(&all[2]),
                    stat(&shielded[0]),
                    stat(&shielded[1]),
                    stat(&shielded[2])
                );
            }
        }
    }

    /// One stage line of an OpenSTA `report_checks` path: a cell output (`Q`, `X`, ...) or a cell input pin.
    struct PathStage {
        pin: String,
        pol: usize,
        slew: f64,
        delay: f64,
        arrival: f64,
        is_output: bool,
        fanout_cap: Option<f64>,
    }

    /// Parses every `Path Type: max` block of `collect_paths.sh` output into `(endpoint, stages)`.
    fn parse_paths(text: &str) -> Vec<(String, Vec<PathStage>)> {
        let mut paths = Vec::new();
        let mut cur: Option<(String, Vec<PathStage>)> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("Endpoint: ") {
                cur = Some((
                    rest.split_whitespace().next().unwrap().to_string(),
                    Vec::new(),
                ));
            } else if line.contains("data arrival time") {
                if let Some(p) = cur.take() {
                    paths.push(p);
                }
            } else if let Some((_, stages)) = cur.as_mut() {
                let w: Vec<&str> = line.split_whitespace().collect();
                // A stage line ends with `^|v <pin> (<cell>)`; an output line has fanout and cap in front of slew/delay/time.
                let n = w.len();
                if n < 5 || !(w[n - 3] == "^" || w[n - 3] == "v") || !w[n - 2].contains('/') {
                    continue;
                }
                let num = |s: &str| s.parse::<f64>().ok();
                let (fanout_cap, tail) = if n == 8 && num(w[0]).is_some_and(|f| f.fract() == 0.0) {
                    (num(w[1]), &w[2..])
                } else {
                    (None, &w[0..])
                };
                let t = tail.len();
                if t < 6 {
                    continue;
                }
                let (Some(slew), Some(delay), Some(arrival)) =
                    (num(tail[t - 6]), num(tail[t - 5]), num(tail[t - 4]))
                else {
                    continue;
                };
                let pin = w[n - 2].to_string();
                let out = pin
                    .rsplit('/')
                    .next()
                    .map(|p| matches!(p, "Q" | "X" | "Y" | "Q_N" | "Z"))
                    .unwrap_or(false);
                stages.push(PathStage {
                    pin,
                    pol: if w[n - 3] == "^" { RISE } else { FALL },
                    slew,
                    delay,
                    arrival,
                    is_output: out,
                    fanout_cap,
                });
            }
        }
        paths
    }

    /// Measurement (E3.5): our per-pin arrival and slew against OpenSTA's worst paths on the picorv32 routed
    /// SPEF (`collect_paths.sh`), stage by stage. Prints `STAGE` lines: for each pin on a path, OpenSTA's
    /// slew and arrival, ours, and the error added by this stage (change in arrival error since the previous
    /// listed pin). Asserts only that paths were parsed.
    #[test]
    fn test_path_stages_vs_opensta_picorv32_routed() {
        compare_path_stages(
            "picorv32",
            "picorv32.placed.def",
            "picorv32.rcx.spef",
            "paths_routed.txt",
        );
    }

    /// Same comparison on aes with its placement-estimate SPEF (`aes.routed.spef`).
    #[test]
    fn test_path_stages_vs_opensta_aes_placed() {
        compare_path_stages(
            "aes",
            "aes.placed.def",
            "aes.routed.spef",
            "paths_placed.txt",
        );
        compare_path_stages(
            "aes",
            "aes.placed.def",
            "aes.routed.spef",
            "paths_from_inputs_placed.txt",
        );
    }

    /// Same comparison on ibex and jpeg with their placement-estimate SPEFs.
    #[test]
    fn test_path_stages_vs_opensta_ibex_placed() {
        compare_path_stages(
            "ibex",
            "ibex.placed.def",
            "ibex.routed.spef",
            "paths_placed.txt",
        );
    }

    #[test]
    fn test_path_stages_vs_opensta_jpeg_placed() {
        compare_path_stages(
            "jpeg",
            "jpeg.placed.def",
            "jpeg.routed.spef",
            "paths_placed.txt",
        );
        compare_path_stages(
            "jpeg",
            "jpeg.placed.def",
            "jpeg.routed.spef",
            "paths_from_inputs_placed.txt",
        );
    }

    fn compare_path_stages(design_name: &str, def_file: &str, spef_file: &str, paths_file: &str) {
        let lib_dir = sky130_hd();
        let dir = design_dir(design_name);
        let text = std::fs::read_to_string(dir.join(paths_file))
            .unwrap_or_else(|e| panic!("{paths_file}: {e}"));
        let paths = parse_paths(&text);
        assert!(paths.len() >= 3, "parsed {} paths", paths.len());
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join(def_file), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join(format!("{design_name}.sdc"))).expect("SDC");
        let spef = crate::readers::load_spef(&dir.join(spef_file)).expect("SPEF");
        let par = crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
        let (_, state) = run_sta_with_state(&design, &lib, &sdc, Some(&par)).expect("STA");
        let cx = Context {
            design: &design,
            lib: &lib,
            sdc: &sdc,
            parasitics: Some(&par),
            pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
        };
        let by_name: std::collections::HashMap<String, PinId> = (0..design.num_pins())
            .map(|i| PinId(i as u32))
            .map(|p| (cx.pin_name(p), p))
            .collect();
        for (endpoint, stages) in &paths {
            println!("PATH to {endpoint}");
            let mut prev_err = 0.0;
            for st in stages {
                let Some(&pin) = by_name.get(&st.pin) else {
                    continue;
                };
                let ps = &state[pin.0 as usize];
                let (our_arr, our_slew) =
                    (ps.arrival_max[st.pol] as f64, ps.slew_max[st.pol] as f64);
                let err = our_arr - st.arrival;
                println!(
                    "STAGE {:<14} {} {:<6} cap {:>9} | ref slew {:>9.4} arr {:>9.4} delay {:>8.4} | ours slew {:>9.4} arr {:>9.4} | arr err {:>+8.4} stage {:>+8.4} slew err {:>+8.4}",
                    st.pin,
                    if st.pol == RISE { "^" } else { "v" },
                    if st.is_output { "out" } else { "in" },
                    st.fanout_cap.map_or(String::new(), |c| format!("{c:.4}")),
                    st.slew,
                    st.arrival,
                    st.delay,
                    our_slew,
                    our_arr,
                    err,
                    err - prev_err,
                    our_slew - st.slew
                );
                prev_err = err;
            }
        }
    }

    /// Measurement (E3.5): our DMP result against `report_dcalc` on arcs far outside the Liberty table range
    /// (`dcalc_extreme_placed.txt`, collected for the instances that carry the worst jpeg and ibex paths).
    /// Prints one `EXTREME` line per row with the table-range ratios; asserts only that rows matched.
    #[test]
    fn test_dmp_extreme_rows_vs_opensta() {
        use crate::sta::dmp::solve_dmp;
        let mut matched_total = 0;
        for (design, def, spef) in [
            ("jpeg", "jpeg.placed.def", "jpeg.routed.spef"),
            ("ibex", "ibex.placed.def", "ibex.routed.spef"),
        ] {
            let (matched, unmodelled) = for_each_dcalc_row(
                design,
                def,
                spef,
                "dcalc_extreme_placed.txt",
                &mut |row, pi, table, table_at| {
                    let r = solve_dmp(pi, RD_SCALE, table);
                    // rd from the slope at the input slew clamped to the Liberty range (1.5 ns), scale 3.
                    let clamped = row.input_slew.min(1.5);
                    let rc = crate::sta::dmp::solve_dmp_with(
                        pi,
                        &|_, c| {
                            let h = 0.05 * c;
                            (3.0 * (table_at(clamped, c + h).0 - table_at(clamped, c - h).0)
                                / (2.0 * h))
                                .max(1e-6)
                        },
                        table,
                    );
                    let (mut lo, mut hi) = (1e-3f64, 1e4f64);
                    for _ in 0..70 {
                        let mid = (lo * hi).sqrt();
                        let r = crate::sta::dmp::solve_dmp_with(pi, &|_, _| mid, table);
                        if r.ceff > row.ceff {
                            hi = mid
                        } else {
                            lo = mid
                        }
                    }
                    let rd_root = (lo * hi).sqrt();
                    let h = 0.05 * row.ceff;
                    let (dsl, ssl) = (
                        (table(row.ceff + h).0 - table(row.ceff - h).0) / (2.0 * h),
                        (table(row.ceff + h).1 - table(row.ceff - h).1) / (2.0 * h),
                    );
                    println!(
                        "EXTREME-RD {design} {} -> {} {}: tin {:.2} rd_root {:.3} dslope {:.3} sslope {:.3} rd/dslope {:.2} rd/sslope {:.2} delay/Ceff {:.2}",
                        row.from,
                        row.to,
                        row.out,
                        row.input_slew,
                        rd_root,
                        dsl,
                        ssl,
                        rd_root / dsl,
                        rd_root / ssl,
                        row.delay / row.ceff
                    );
                    println!(
                        "EXTREME-CLAMP {design} {} -> {} {}: ref Ceff {:.4} | clamped-slope rd: Ceff {:.4} ({:+.1}%) delay {:+.1}%",
                        row.from,
                        row.to,
                        row.out,
                        row.ceff,
                        rc.ceff,
                        (rc.ceff / row.ceff - 1.0) * 100.0,
                        (rc.delay / row.delay - 1.0) * 100.0
                    );
                    let total = pi.c1 + pi.c2;
                    println!(
                        "EXTREME {design} {} -> {} {} tin {:.2} total {:.3} shield {:.3} | ref Ceff {:.4} delay {:.3} slew {:.3} wave {:.3} | ours Ceff {:.4} delay {:.3} slew {:.3} | Ceff {:+.1}% delay {:+.1}% slew vs wave {:+.1}%",
                        row.from,
                        row.to,
                        row.out,
                        row.input_slew,
                        total,
                        row.ceff / total,
                        row.ceff,
                        row.delay,
                        row.slew,
                        row.driver_slew,
                        r.ceff,
                        r.delay,
                        r.slew,
                        (r.ceff / row.ceff - 1.0) * 100.0,
                        (r.delay / row.delay - 1.0) * 100.0,
                        (r.slew / row.driver_slew - 1.0) * 100.0
                    );
                },
            );
            println!("EXTREME {design}: matched {matched} unmodelled {unmodelled}");
            matched_total += matched;
        }
        assert!(matched_total > 10);
    }

    fn format_spread(v: &[f64], decimals: usize) -> String {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        format!(
            "p10 {:.decimals$} median {:.decimals$} p90 {:.decimals$}",
            s[s.len() / 10],
            median(s.clone()),
            s[9 * s.len() / 10],
        )
    }

    fn for_each_pins_design(
        tag: &str,
        mut visit: impl FnMut(&str, &Context, &std::collections::HashMap<&str, [f64; 4]>),
    ) {
        let sets = [
            (
                "aes",
                "aes.placed.def",
                "aes.routed.spef",
                "pins_placed.txt",
            ),
            (
                "picorv32",
                "picorv32.placed.def",
                "picorv32.rcx.spef",
                "pins_routed.txt",
            ),
            (
                "ibex",
                "ibex.placed.def",
                "ibex.routed.spef",
                "pins_placed.txt",
            ),
            (
                "jpeg",
                "jpeg.placed.def",
                "jpeg.routed.spef",
                "pins_placed.txt",
            ),
        ];
        for (design_name, def_file, spef_file, pins_file) in sets {
            let dir = design_dir(design_name);
            let Ok(text) = std::fs::read_to_string(dir.join(pins_file)) else {
                println!("{tag} {design_name}: no {pins_file}, skipped (collect_pins.sh)");
                continue;
            };
            let lib_dir = sky130_hd();
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"))
                .expect("Liberty");
            let mut interner = StringInterner::new();
            let tech = load_tech_db(
                &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
                &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
                &mut interner,
            )
            .expect("LEF")
            .0;
            let design = parse_def(&dir.join(def_file), &tech, Some(&lib)).expect("DEF");
            let sdc = load_sdc(&dir.join(format!("{design_name}.sdc"))).expect("SDC");
            let spef = crate::readers::load_spef(&dir.join(spef_file)).expect("SPEF");
            let par = crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics");
            let cx = Context {
                design: &design,
                lib: &lib,
                sdc: &sdc,
                parasitics: Some(&par),
                pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
            };
            // pin name -> [slew_max rise, fall, arrival_max rise, fall]
            let mut ref_pins: std::collections::HashMap<&str, [f64; 4]> =
                std::collections::HashMap::new();
            for line in text.lines() {
                let w: Vec<&str> = line.split_whitespace().collect();
                if w.len() < 5 {
                    continue;
                }
                let n = |i: usize| w[i].parse::<f64>().unwrap_or(f64::NAN);
                ref_pins.insert(w[0], [n(1), n(2), n(3), n(4)]);
            }
            visit(design_name, &cx, &ref_pins);
        }
    }

    /// Measurement (E3.5 wire delay): for every cell-driven net, OpenSTA's per-sink wire delay (sink arrival minus
    /// driver arrival, from `collect_pins.sh`) against our Elmore delay, binned by Elmore delay and by the driver's slew.
    /// Prints `WIREDELAY` lines (median and p10/p90 of ref delay / Elmore and ref slew degrade / Elmore) per bin.
    #[test]
    fn test_wire_delay_vs_elmore() {
        let e_bins = [
            (0.0, 0.001),
            (0.001, 0.01),
            (0.01, 0.1),
            (0.1, 1.0),
            (1.0, 100.0),
        ];
        let mut total_rows = 0usize;
        for_each_pins_design("WIREDELAY", |design_name, cx, ref_pins| {
            let design = cx.design;
            // [bin] -> (delay ratios, slew-degrade ratios)
            let mut by_e: Vec<(Vec<f64>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); e_bins.len()];
            let mut by_slew: Vec<(Vec<f64>, Vec<f64>)> = vec![(Vec::new(), Vec::new()); 4];
            let slew_bins = [(0.0, 0.1), (0.1, 0.5), (0.5, 2.0), (2.0, 100.0)];
            let mut triples: Vec<(f64, f64, f64)> = Vec::new();
            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let pins = design.net_pins(net);
                let Some(&drv) = pins.first() else { continue };
                if design.pin_cells[drv.0 as usize].is_none()
                    || !design.pin_is_driver[drv.0 as usize]
                {
                    continue;
                }
                let Some(wire) = cx.wire_delays(net) else {
                    continue;
                };
                let Some(drv_ref) = ref_pins.get(cx.pin_name(drv).as_str()) else {
                    continue;
                };
                for (i, &sink) in pins.iter().enumerate().skip(1) {
                    let Some(sink_ref) = ref_pins.get(cx.pin_name(sink).as_str()) else {
                        continue;
                    };
                    for pol in [RISE, FALL] {
                        let (drv_slew, drv_arr) = (drv_ref[pol], drv_ref[2 + pol]);
                        let (sink_slew, sink_arr) = (sink_ref[pol], sink_ref[2 + pol]);
                        let elmore = wire[pol][i] as f64;
                        if !(drv_arr.is_finite()
                            && sink_arr.is_finite()
                            && drv_slew.is_finite()
                            && sink_slew.is_finite())
                            || elmore <= 1e-7
                        {
                            continue;
                        }
                        let delay = sink_arr - drv_arr;
                        let degrade = (sink_slew * sink_slew - drv_slew * drv_slew)
                            .max(0.0)
                            .sqrt();
                        let b = e_bins
                            .iter()
                            .position(|&(lo, hi)| elmore >= lo && elmore < hi)
                            .unwrap();
                        by_e[b].0.push(delay / elmore);
                        by_e[b].1.push(degrade / elmore);
                        let sb = slew_bins
                            .iter()
                            .position(|&(lo, hi)| drv_slew >= lo && drv_slew < hi)
                            .unwrap();
                        by_slew[sb].0.push(delay / elmore);
                        by_slew[sb].1.push(degrade / elmore);
                        triples.push((drv_slew, elmore, sink_slew));
                        total_rows += 1;
                    }
                }
            }
            for k in [1.386, 1.5, 1.6, 1.7, 1.8, 2.0] {
                // Relative error of the predicted sink slew hypot(driver slew, k * Elmore), all sinks and big-wire sinks.
                let rel = |sel: &dyn Fn(&&(f64, f64, f64)) -> bool| {
                    let v: Vec<f64> = triples
                        .iter()
                        .filter(sel)
                        .map(|&(d, e, sk)| ((d * d + (k * e) * (k * e)).sqrt() - sk) / sk)
                        .collect();
                    if v.is_empty() {
                        "n/a".to_string()
                    } else {
                        format!(
                            "n {} median {:+.4} mean abs {:.4}",
                            v.len(),
                            median(v.clone()),
                            v.iter().map(|x| x.abs()).sum::<f64>() / v.len() as f64
                        )
                    }
                };
                println!(
                    "WIREFIT {design_name} k {k}: all [{}] elmore>0.01ns [{}]",
                    rel(&|_| true),
                    rel(&|t| t.1 > 0.01)
                );
            }
            for (b, &(lo, hi)) in e_bins.iter().enumerate() {
                if by_e[b].0.len() >= 5 {
                    println!(
                        "WIREDELAY {design_name} elmore [{lo},{hi}) n {} | ref delay / elmore {} | ref slew degrade / elmore {}",
                        by_e[b].0.len(),
                        format_spread(&by_e[b].0, 3),
                        format_spread(&by_e[b].1, 3)
                    );
                }
            }
            for (b, &(lo, hi)) in slew_bins.iter().enumerate() {
                if by_slew[b].0.len() >= 5 {
                    println!(
                        "WIRESLEW {design_name} driver slew [{lo},{hi}) n {} | ref delay / elmore {} | ref slew degrade / elmore {}",
                        by_slew[b].0.len(),
                        format_spread(&by_slew[b].0, 3),
                        format_spread(&by_slew[b].1, 3)
                    );
                }
            }
        });
        assert!(total_rows > 100, "only {total_rows} sink rows");
    }

    /// Measurement (E3.5): OpenSTA's driver waveform slew divided by the table slew at the total load and at
    /// OpenSTA's Ceff, by shielding class (`ceff / total`), per design. Prints `WAVE` lines; asserts only that rows exist.
    #[test]
    fn test_dmp_wave_slew_scan() {
        let sets = [
            (
                "picorv32",
                "picorv32.placed.def",
                "picorv32.rcx.spef",
                "dcalc_routed_all.txt",
            ),
            (
                "aes",
                "aes.placed.def",
                "aes.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "ibex",
                "ibex.placed.def",
                "ibex.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "jpeg",
                "jpeg.placed.def",
                "jpeg.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "gcd",
                "gcd.placed.def",
                "gcd.rcx.spef",
                "dcalc_routed_all.txt",
            ),
        ];
        let bins = [
            (0.0, 0.8),
            (0.8, 0.9),
            (0.9, 0.97),
            (0.97, 0.99),
            (0.99, 1.01),
        ];
        for (design, def, spef, file) in sets {
            let mut total_ratio: Vec<Vec<f64>> = vec![Vec::new(); bins.len()];
            let mut ceff_ratio: Vec<Vec<f64>> = vec![Vec::new(); bins.len()];
            for_each_dcalc_row(design, def, spef, file, &mut |row, pi, table, _table_at| {
                let total = pi.c1 + pi.c2;
                let shield = row.ceff / total;
                let Some(b) = bins
                    .iter()
                    .position(|&(lo, hi)| shield >= lo && shield < hi)
                else {
                    return;
                };
                total_ratio[b].push(row.driver_slew / table(total).1);
                ceff_ratio[b].push(row.driver_slew / table(row.ceff).1);
            });
            for (b, &(lo, hi)) in bins.iter().enumerate() {
                if total_ratio[b].is_empty() {
                    continue;
                }
                let spread = |v: &Vec<f64>| {
                    let mut s = v.clone();
                    s.sort_by(f64::total_cmp);
                    format!(
                        "p10 {:.4} median {:.4} p90 {:.4}",
                        s[s.len() / 10],
                        median(s.clone()),
                        s[9 * s.len() / 10]
                    )
                };
                println!(
                    "WAVE {design} shield [{lo:.2},{hi:.2}) n {} | wave/table(total) {} | wave/table(ceff) {}",
                    total_ratio[b].len(),
                    spread(&total_ratio[b]),
                    spread(&ceff_ratio[b])
                );
            }
        }
    }

    /// Measurement (E3.5): for every shielded row (OpenSTA Ceff below 97% of the total load) the driver
    /// resistance that makes our Ceff equal OpenSTA's, with the table quantities it might be derived
    /// from. Prints `ROOT` lines (one per row) for analysis; asserts only that rows were found.
    #[test]
    fn test_dmp_rd_root_scan() {
        use crate::sta::dmp::solve_dmp_with;
        let mut found = 0usize;
        for_each_dcalc_row(
            "picorv32",
            "picorv32.placed.def",
            "picorv32.rcx.spef",
            "dcalc_routed_all.txt",
            &mut |row, pi, table, _table_at| {
                let total = pi.c1 + pi.c2;
                if row.ceff >= 0.97 * total {
                    return;
                }
                // Our Ceff grows with rd, so bisect rd (kohm) on a log scale.
                let (mut lo, mut hi) = (1e-3f64, 1e3f64);
                for _ in 0..60 {
                    let mid = (lo * hi).sqrt();
                    let r = solve_dmp_with(pi, &|_, _| mid, table);
                    if r.ceff > row.ceff {
                        hi = mid
                    } else {
                        lo = mid
                    }
                }
                let rd = (lo * hi).sqrt();
                let c = row.ceff;
                let h = 0.05 * c;
                let (up, dn) = (table(c + h), table(c - h));
                let (dsl, ssl) = ((up.0 - dn.0) / (2.0 * h), (up.1 - dn.1) / (2.0 * h));
                let (d0, s0) = table(c);
                let near = pi.c1.max(1e-4 * total);
                let secant = |a: f64, b: f64, q: usize| {
                    let (ta, tb) = (table(a), table(b));
                    if q == 0 {
                        (tb.0 - ta.0) / (b - a)
                    } else {
                        (tb.1 - ta.1) / (b - a)
                    }
                };
                println!(
                    "ROOT {} {} {} tin {:.4} shield {:.4} total {:.4} rpi {:.4} rd {:.4} dslope {:.4} sslope {:.4} delay {:.4} slew {:.4} dsec_ct {:.4} ssec_ct {:.4} dsec_nt {:.4} ssec_nt {:.4} dsec_half {:.4} ssec_half {:.4}",
                    row.from,
                    row.to,
                    row.out,
                    row.input_slew,
                    row.ceff / total,
                    total,
                    pi.rpi,
                    rd,
                    dsl,
                    ssl,
                    d0,
                    s0,
                    secant(c, total, 0),
                    secant(c, total, 1),
                    secant(near, total, 0),
                    secant(near, total, 1),
                    secant(0.5 * total, total, 0),
                    secant(0.5 * total, total, 1)
                );
                found += 1;
            },
        );
        assert!(found > 100, "only {found} shielded rows");
    }

    /// Endpoint timing of one estimator, pin name, max arrival (ns), max slack (ns), plus WNS/TNS (max path).
    struct Timing {
        endpoints: Vec<(String, f64, f64)>,
        wns_ns: f64,
        tns_ns: f64,
    }

    fn timing_from_report(report: &StaReport) -> Timing {
        Timing {
            endpoints: report
                .endpoints
                .iter()
                .map(|e| (e.pin.clone(), e.max_arr_ns as f64, e.max_slack_ns as f64))
                .collect(),
            wns_ns: report.wns_max_ns as f64,
            tns_ns: report.tns_max_ns as f64,
        }
    }

    fn timing_from_golden(block: &serde_json::Value) -> Timing {
        Timing {
            endpoints: block["endpoints"]
                .as_array()
                .expect("endpoints")
                .iter()
                .map(|e| {
                    (
                        e["pin"].as_str().unwrap().to_string(),
                        e["max_arr_ns"].as_f64().unwrap(),
                        e["max_slack_ns"].as_f64().unwrap(),
                    )
                })
                .collect(),
            wns_ns: block["wns_max_ns"].as_f64().unwrap(),
            tns_ns: block["tns_max_ns"].as_f64().unwrap(),
        }
    }

    /// One row of the measurement table. Errors are ours minus the reference, in ps (WNS/TNS signed).
    struct Score {
        label: &'static str,
        arr_err_ps: Vec<f64>,
        arr_fail: usize,
        slack_fail: usize,
        either_fail: usize,
        wns_err_ps: f64,
        tns_err_ps: f64,
    }

    /// The strict criterion `max(5 ps, 1%)` of the reference value, in ns.
    fn strict_ok(err_ns: f64, reference_ns: f64) -> bool {
        err_ns.abs() <= 0.005f64.max(0.01 * reference_ns.abs())
    }

    fn score(label: &'static str, ours: &Timing, reference: &Timing) -> Score {
        assert_eq!(
            ours.endpoints.len(),
            reference.endpoints.len(),
            "{label}: endpoint count"
        );
        let (mut arr_err_ps, mut arr_fail, mut slack_fail, mut either_fail) = (Vec::new(), 0, 0, 0);
        for (pin, ref_arr, ref_slack) in &reference.endpoints {
            let (_, our_arr, our_slack) = ours
                .endpoints
                .iter()
                .find(|(p, _, _)| p == pin)
                .unwrap_or_else(|| panic!("{label}: endpoint {pin} missing"));
            let arr_ok = strict_ok(our_arr - ref_arr, *ref_arr);
            let slack_ok = strict_ok(our_slack - ref_slack, *ref_slack);
            arr_err_ps.push((our_arr - ref_arr).abs() * 1000.0);
            arr_fail += usize::from(!arr_ok);
            slack_fail += usize::from(!slack_ok);
            either_fail += usize::from(!(arr_ok && slack_ok));
        }
        Score {
            label,
            arr_err_ps,
            arr_fail,
            slack_fail,
            either_fail,
            wns_err_ps: (ours.wns_ns - reference.wns_ns) * 1000.0,
            tns_err_ps: (ours.tns_ns - reference.tns_ns) * 1000.0,
        }
    }

    /// Nearest-rank quantile of an ascending list.
    fn nearest_rank(sorted: &[f64], q: f64) -> f64 {
        sorted[((q * sorted.len() as f64).ceil() as usize).max(1) - 1]
    }

    fn print_scores(title: &str, rows: &[Score]) {
        println!("\n{title}");
        println!(
            "{:<72} {:>3} {:>22} {:>22} {:>22} {:>22} {:>22} {:>6} {:>6} {:>6}",
            "configuration",
            "n",
            "arr_err_med_ps",
            "arr_err_p95_ps",
            "arr_err_max_ps",
            "wns_err_ps",
            "tns_err_ps",
            "arr_f",
            "slk_f",
            "any_f"
        );
        for row in rows {
            let mut sorted = row.arr_err_ps.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{:<72} {:>3} {:>22} {:>22} {:>22} {:>22} {:>22} {:>6} {:>6} {:>6}",
                row.label,
                sorted.len(),
                nearest_rank(&sorted, 0.5),
                nearest_rank(&sorted, 0.95),
                nearest_rank(&sorted, 1.0),
                row.wns_err_ps,
                row.tns_err_ps,
                row.arr_fail,
                row.slack_fail,
                row.either_fail
            );
        }
    }

    /// `(routed_spef, placed_spef)` blocks of `gcd.sta.reference.json`: OpenSTA on the placed DEF with
    /// OpenRCX's routed parasitics and with OpenROAD's placement estimate.
    fn gcd_golden_pair() -> (Timing, Timing) {
        let reference: serde_json::Value = serde_json::from_slice(
            &std::fs::read(design_dir("gcd").join("gcd.sta.reference.json")).expect("golden file"),
        )
        .expect("golden JSON");
        (
            timing_from_golden(&reference["routed_spef"]),
            timing_from_golden(&reference["placed_spef"]),
        )
    }

    /// Ticket 15, configuration (d): OpenSTA's placed-SPEF golden scored against its routed-SPEF golden.
    /// Golden-vs-golden, so it needs no PDK and no STA run; it isolates the estimator difference.
    #[test]
    fn measure_gcd_placed_golden_against_routed_golden() {
        let (routed_truth, placed_golden) = gcd_golden_pair();
        print_scores(
            "gcd, OpenSTA golden-vs-golden (no STA run)",
            &[score(
                "(d) OpenSTA placed SPEF golden vs routed_spef golden",
                &placed_golden,
                &routed_truth,
            )],
        );
    }

    /// Ticket 15 (review step 6, part 3): scores both placement tree builders and both SPEF modes
    /// against OpenSTA on the routed parasitics (`routed_spef` golden). Measurement only: prints
    /// tables with `--nocapture`; the numbers are recorded by the maintainer, not asserted here.
    #[test]
    fn measure_gcd_estimators_against_routed_truth() {
        use crate::congestion::rsmt::{build_precise_rsmt_points, build_rsmt_points};
        use crate::sta::parasitics::estimate_placement_parasitics_with_tree;
        use crate::sta::{WireRc, spef_parasitics};

        let lib_dir = sky130_hd();
        let gcd = design_dir("gcd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&gcd.join("gcd.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&gcd.join("gcd.sdc")).expect("SDC");
        let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");

        let (routed_truth, placed_golden) = gcd_golden_pair();

        let sta = |parasitics: &crate::sta::NetParasitics| {
            timing_from_report(&run_sta(&design, &lib, &sdc, Some(parasitics)).expect("STA"))
        };
        let load = |file: &str| {
            let spef = crate::readers::load_spef(&gcd.join(file)).expect("SPEF");
            spef_parasitics(&design, &spef).expect("SPEF parasitics")
        };

        let greedy = estimate_placement_parasitics_with_tree(&design, &tech, rc, build_rsmt_points);
        let exact =
            estimate_placement_parasitics_with_tree(&design, &tech, rc, build_precise_rsmt_points);
        let placed = load("gcd.routed.spef");
        let routed = load("gcd.rcx.spef");
        let ours_placed = sta(&placed);
        let ours_routed = sta(&routed);

        let vs_truth = vec![
            score(
                "(a) ours, placement RC, greedy tree (build_rsmt_points)",
                &sta(&greedy),
                &routed_truth,
            ),
            score(
                "(b) ours, placement RC, exact tree (build_precise_rsmt_points)",
                &sta(&exact),
                &routed_truth,
            ),
            score(
                "(c) ours, placed SPEF (OpenROAD tree and RC)",
                &ours_placed,
                &routed_truth,
            ),
            score(
                "(d) OpenSTA placed SPEF golden",
                &placed_golden,
                &routed_truth,
            ),
        ];
        print_scores(
            "gcd, timing error against OpenSTA on routed_spef (max path, 53 endpoints)",
            &vs_truth,
        );

        let residuals = vec![
            score(
                "(e) ours, routed SPEF, vs OpenSTA routed_spef golden",
                &ours_routed,
                &routed_truth,
            ),
            score(
                "(f) ours, placed SPEF, vs OpenSTA placed_spef golden",
                &ours_placed,
                &placed_golden,
            ),
        ];
        print_scores(
            "gcd, our SPEF-mode residuals against each parasitics' own OpenSTA golden",
            &residuals,
        );
    }

    /// `(routed_spef, placed_spef)` blocks of `picorv32.sta.reference.json`; see `gcd_golden_pair`.
    fn picorv32_golden_pair() -> (Timing, Timing) {
        let reference: serde_json::Value = serde_json::from_slice(
            &std::fs::read(design_dir("picorv32").join("picorv32.sta.reference.json"))
                .expect("golden file"),
        )
        .expect("golden JSON");
        (
            timing_from_golden(&reference["routed_spef"]),
            timing_from_golden(&reference["placed_spef"]),
        )
    }

    /// Ticket 16: scores picorv32's estimators against OpenSTA on the routed parasitics (`routed_spef`
    /// golden), as `measure_gcd_estimators_against_routed_truth` does for gcd. Measurement only: the
    /// tables are printed with `--nocapture`; endpoint presence and count are the only assertions.
    #[test]
    fn measure_picorv32_estimators_against_routed_truth() {
        use crate::congestion::rsmt::build_precise_rsmt_points;
        use crate::sta::parasitics::estimate_placement_parasitics_with_tree;
        use crate::sta::{WireRc, spef_parasitics};

        let lib_dir = sky130_hd();
        let dir = design_dir("picorv32");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("picorv32.placed.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("picorv32.sdc")).expect("SDC");
        let rc = WireRc::from_layer(&tech, "met2").expect("met2 RC");

        let (routed_truth, placed_golden) = picorv32_golden_pair();

        let sta = |parasitics: &crate::sta::NetParasitics| {
            timing_from_report(&run_sta(&design, &lib, &sdc, Some(parasitics)).expect("STA"))
        };
        let load = |file: &str| {
            let spef = crate::readers::load_spef(&dir.join(file)).expect("SPEF");
            spef_parasitics(&design, &spef).expect("SPEF parasitics")
        };

        let exact =
            estimate_placement_parasitics_with_tree(&design, &tech, rc, build_precise_rsmt_points);
        let placed = load("picorv32.routed.spef");
        let routed = load("picorv32.rcx.spef");

        let vs_truth = vec![
            score(
                "(a) ours, placement RC, exact tree (build_precise_rsmt_points)",
                &sta(&exact),
                &routed_truth,
            ),
            score(
                "(b) ours, placed SPEF (OpenROAD tree and RC)",
                &sta(&placed),
                &routed_truth,
            ),
            score(
                "(c) OpenSTA placed SPEF golden",
                &placed_golden,
                &routed_truth,
            ),
        ];
        print_scores(
            "picorv32, timing error against OpenSTA on routed_spef (max path)",
            &vs_truth,
        );

        let residuals = vec![score(
            "(d) ours, routed SPEF, vs OpenSTA routed_spef golden",
            &sta(&routed),
            &routed_truth,
        )];
        print_scores(
            "picorv32, our routed-SPEF residual against its own OpenSTA golden",
            &residuals,
        );
    }

    enum Rc {
        None,
        Placement,
        Spef,
        RoutedSpef,
    }

    /// When `RISE` and `FALL` slacks are tied within `tie_ns`, an `O(0.1 ps)` timing shift can
    /// flip which polarity wins the endpoint (changing arrival, req, and slew); pick the tied
    /// polarity closer to the reference `(arr, req, slew)` tuple so all 4 fields compare the same transition.
    fn resolve_tied_polarity(
        default: Worst,
        cands: &[Option<Worst>; 2],
        ref_arr: f64,
        ref_req: f64,
        ref_slew: f64,
        tie_ns: f64,
    ) -> Worst {
        if let [Some(r), Some(f)] = *cands
            && ((r.slack - f.slack).abs() as f64) <= tie_ns
        {
            let dist = |w: Worst| {
                (w.arr as f64 - ref_arr).abs()
                    + (w.req as f64 - ref_req).abs()
                    + (w.slew as f64 - ref_slew).abs()
            };
            if dist(f) < dist(r) { f } else { r }
        } else {
            default
        }
    }

    fn compare_design_with_opensta(design_name: &str, golden_key: &str, rc_mode: Rc, tol: Tol) {
        compare_design_files(
            design_name,
            &format!("{design_name}.sdc"),
            &format!("{design_name}.sta.reference.json"),
            golden_key,
            rc_mode,
            tol,
        );
    }

    /// `compare_design_with_opensta` with explicit SDC and golden files, for constraint
    /// variants such as `gcd.propagated.sdc` that share the design's DEF and SPEF.
    fn compare_design_files(
        design_name: &str,
        sdc_file: &str,
        golden_file: &str,
        golden_key: &str,
        rc_mode: Rc,
        tol: Tol,
    ) {
        let lib_dir = sky130_hd();
        let gcd = design_dir(design_name);

        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(
            &gcd.join(format!("{design_name}.placed.def")),
            &tech,
            Some(&lib),
        )
        .expect("DEF");
        let sdc = load_sdc(&gcd.join(sdc_file)).expect("SDC");
        let parasitics = match rc_mode {
            Rc::None => None,
            Rc::Placement => {
                let rc = crate::sta::WireRc::from_layer(&tech, "met2").expect("met2 RC");
                Some(crate::sta::estimate_placement_parasitics(
                    &design, &tech, rc,
                ))
            }
            Rc::Spef => {
                let spef =
                    crate::readers::load_spef(&gcd.join(format!("{design_name}.routed.spef")))
                        .expect("SPEF");
                Some(crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics"))
            }
            Rc::RoutedSpef => {
                let spef = crate::readers::load_spef(&gcd.join(format!("{design_name}.rcx.spef")))
                    .expect("SPEF");
                Some(crate::sta::spef_parasitics(&design, &spef).expect("SPEF parasitics"))
            }
        };
        let (report, state, polarities) =
            run_sta_detail(&design, &lib, &sdc, parasitics.as_ref(), true).expect("STA");

        let reference: serde_json::Value =
            serde_json::from_slice(&std::fs::read(gcd.join(golden_file)).expect("golden file"))
                .expect("golden JSON");
        let zero_wire = &reference[golden_key];
        let ref_endpoints = zero_wire["endpoints"].as_array().expect("endpoints");
        assert_eq!(
            report.endpoints.len(),
            ref_endpoints.len(),
            "endpoint count"
        );

        const SLACK_TIE_NS: f64 = 0.002;
        let mut failures = Vec::new();
        let mut max_diff = [0.0f64; 8];
        let fields = [
            "max_arr_ns",
            "max_req_ns",
            "max_slack_ns",
            "max_slew_ns",
            "min_arr_ns",
            "min_req_ns",
            "min_slack_ns",
            "min_slew_ns",
        ];
        for r in ref_endpoints {
            let name = r["pin"].as_str().unwrap();
            let Some(idx) = report.endpoints.iter().position(|e| e.pin == name) else {
                failures.push(format!("{name}: missing"));
                continue;
            };
            let ep = &polarities[idx];
            let max = resolve_tied_polarity(
                pick_worst_polarity(&ep.max).unwrap(),
                &ep.max,
                r["max_arr_ns"].as_f64().unwrap(),
                r["max_req_ns"].as_f64().unwrap(),
                r["max_slew_ns"].as_f64().unwrap(),
                SLACK_TIE_NS,
            );
            let min = resolve_tied_polarity(
                pick_worst_polarity(&ep.min).unwrap(),
                &ep.min,
                r["min_arr_ns"].as_f64().unwrap(),
                r["min_req_ns"].as_f64().unwrap(),
                r["min_slew_ns"].as_f64().unwrap(),
                SLACK_TIE_NS,
            );
            let values = [
                max.arr, max.req, max.slack, max.slew, min.arr, min.req, min.slack, min.slew,
            ];
            for (i, field) in fields.iter().enumerate() {
                let expected = r[*field].as_f64().unwrap();
                let (rel_tol, abs_tol) = if field.ends_with("slew_ns") {
                    (tol.slew_rel, 0.005)
                } else if field.ends_with("slack_ns") {
                    (tol.arr_rel, tol.slack_abs_ns)
                } else {
                    (tol.arr_rel, tol.arr_abs_ns)
                };
                let (ok, diff) = within_tolerance(values[i], expected, rel_tol, abs_tol);
                max_diff[i] = max_diff[i].max(diff);
                if !ok {
                    failures.push(format!("{name}.{field}: ours={} ref={expected}", values[i]));
                }
            }
        }

        // Propagated-clock goldens also carry per-clock-pin arrival/slew — what
        // `set_propagated_clock` actually computes — asserted independently of the
        // endpoint tolerance so a systematic clock-network error cannot hide under
        // the (looser) data-path bounds. `state[clk]` holds the clock arrival because
        // consumer clock pins are seeded from `clk_state` before data propagation.
        // A propagated run requires a non-empty `clock_pins` array: without it this
        // check would be silently disabled, so a missing or empty one is a hard error.
        if sdc.propagated_clock() {
            let clock_pins = zero_wire
                .get("clock_pins")
                .and_then(|c| c.as_array())
                .filter(|c| !c.is_empty())
                .expect("propagated-clock golden has no non-empty `clock_pins` array");
            let cx = Context {
                design: &design,
                lib: &lib,
                sdc: &sdc,
                parasitics: parasitics.as_ref(),
                pi_models: Vec::new(),
            net_wire_loads: vec![None; design.num_nets()],
            };
            let by_name: std::collections::HashMap<String, PinId> = (0..design.num_pins())
                .map(|i| PinId(i as u32))
                .map(|p| (cx.pin_name(p), p))
                .collect();
            // Every clock consumer our graph models must appear in the golden, or the
            // collector missed a pin and this check would not cover it.
            let graph = build_graph(&cx, &mut SkipReport::default());
            // The golden's pin set must equal our modeled clock-consumer set exactly:
            // a modeled consumer absent from the golden means the collector skipped a
            // pin, and a golden pin we do not model means it enumerated something
            // outside the timing graph — neither may pass silently. Per-pin values are
            // compared below regardless of set membership.
            let mut golden_names = std::collections::HashSet::new();
            let mut golden_entries_with_pin = 0usize;
            for e in clock_pins {
                if let Some(p) = e["pin"].as_str() {
                    golden_entries_with_pin += 1;
                    golden_names.insert(p.to_owned());
                }
            }
            let modeled_consumers = graph
                .launches
                .iter()
                .map(|l| l.clk)
                .chain(graph.checks.iter().filter_map(|(_, c)| c.clk))
                .chain(graph.latch_opens.iter().map(|o| o.enable))
                .chain(graph.gating_enables.iter().map(|g| g.clk_pin))
                .map(|p| cx.pin_name(p))
                .collect::<std::collections::HashSet<_>>();
            assert!(
                !modeled_consumers.is_empty(),
                "{design_name} {golden_key}: no modeled clock consumers to check"
            );
            for name in &modeled_consumers {
                if !golden_names.contains(name.as_str()) {
                    failures.push(format!(
                        "{name}: modeled clock consumer missing from golden `clock_pins`"
                    ));
                }
            }
            for name in &golden_names {
                if !modeled_consumers.contains(name) {
                    failures.push(format!(
                        "{name}: golden `clock_pins` entry is not a modeled clock consumer"
                    ));
                }
            }
            if golden_names.len() != golden_entries_with_pin {
                failures.push("golden `clock_pins` contains duplicate `pin` entries".to_string());
            }
            // (field, transition, is_arrival, is_max). Clock arrival/slew match OpenSTA to
            // ~8e-6 ns (f32 noise); 1 ps is ~100x that margin yet far below any real skew
            // defect, so a systematic clock-network error cannot slip under it.
            const CLK_ABS_NS: f64 = 0.001;
            let clk_fields: [(&str, usize, bool, bool); 8] = [
                ("arr_rise_max_ns", RISE, true, true),
                ("arr_rise_min_ns", RISE, true, false),
                ("arr_fall_max_ns", FALL, true, true),
                ("arr_fall_min_ns", FALL, true, false),
                ("slew_rise_max_ns", RISE, false, true),
                ("slew_rise_min_ns", RISE, false, false),
                ("slew_fall_max_ns", FALL, false, true),
                ("slew_fall_min_ns", FALL, false, false),
            ];
            let mut clk_max = 0.0f64;
            let mut clk_checked = 0usize;
            for entry in clock_pins {
                let Some(name) = entry["pin"].as_str() else {
                    failures.push("clock_pins entry missing `pin`".to_string());
                    continue;
                };
                let Some(&pin) = by_name.get(name) else {
                    failures.push(format!("{name}: clock pin missing"));
                    continue;
                };
                let s = state[pin.0 as usize];
                for &(field, pol, is_arr, is_max) in &clk_fields {
                    let Some(expected) = entry[field].as_f64() else {
                        failures.push(format!(
                            "{name}.{field}: missing or non-numeric golden value"
                        ));
                        continue;
                    };
                    let ours = match (is_arr, is_max) {
                        (true, true) => s.arrival_max[pol],
                        (true, false) => s.arrival_min[pol],
                        (false, true) => s.slew_max[pol],
                        (false, false) => s.slew_min[pol],
                    } as f64;
                    let diff = (ours - expected).abs();
                    clk_max = clk_max.max(diff);
                    if diff > CLK_ABS_NS {
                        failures.push(format!("{name}.{field}: ours={ours} ref={expected}"));
                    }
                    clk_checked += 1;
                }
            }
            println!(
                "{design_name} {golden_key} clock_pins checked={clk_checked} modeled_consumers={} max_abs_diff={clk_max:.6} ns",
                modeled_consumers.len()
            );
        }
        println!(
            "{design_name} {golden_key} max relative diff per field: {:?}",
            fields.iter().zip(max_diff).collect::<Vec<_>>()
        );
        for (ours, key) in [
            (report.wns_max_ns, "wns_max_ns"),
            (report.tns_max_ns, "tns_max_ns"),
            (report.wns_min_ns, "wns_min_ns"),
            (report.tns_min_ns, "tns_min_ns"),
        ] {
            let expected = zero_wire[key].as_f64().unwrap();
            let (rel, abs) = if key.starts_with("tns") {
                (tol.tns_rel, 0.005)
            } else {
                // WNS is min_e (e.max_slack_ns) / min_e (e.min_slack_ns), so it inherits slack_abs_ns.
                (tol.arr_rel, tol.slack_abs_ns)
            };
            if !within_tolerance(ours, expected, rel, abs).0 {
                failures.push(format!("{key}: ours={ours} ref={expected}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Phase 3 edge-case fixtures under `tests/designs/sta_edge/`: hand-written DEF/SDC(/SPEF) per
    /// case, golden emitted by OpenSTA via the case's `generate_ref.sh`. The golden JSON reuses the
    /// `sta.reference.json` endpoint schema under a single mode key.
    fn compare_sta_edge(
        case: &str,
        def: &str,
        sdc: &str,
        spef: Option<&str>,
        golden_key: &str,
        tol: Tol,
    ) {
        let lib_dir = sky130_hd();
        let case_dir = design_dir("sta_edge").join(case);

        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&design_dir("sta_edge").join(def), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&case_dir.join(sdc)).expect("SDC");
        let parasitics = spef.map(|s| {
            let spef_doc = crate::readers::load_spef(&case_dir.join(s)).expect("SPEF");
            crate::sta::spef_parasitics(&design, &spef_doc).expect("SPEF parasitics")
        });
        let (report, _, polarities) =
            run_sta_detail(&design, &lib, &sdc, parasitics.as_ref(), true).expect("STA");

        let reference: serde_json::Value = serde_json::from_slice(
            &std::fs::read(case_dir.join(format!("{case}.reference.json"))).expect("golden file"),
        )
        .expect("golden JSON");
        let golden = &reference[golden_key];
        let ref_endpoints = golden["endpoints"].as_array().expect("endpoints");
        assert_eq!(
            report.endpoints.len(),
            ref_endpoints.len(),
            "endpoint count"
        );

        const SLACK_TIE_NS: f64 = 0.002;
        let mut failures = Vec::new();
        let fields = [
            "max_arr_ns",
            "max_req_ns",
            "max_slack_ns",
            "max_slew_ns",
            "min_arr_ns",
            "min_req_ns",
            "min_slack_ns",
            "min_slew_ns",
        ];
        for r in ref_endpoints {
            let name = r["pin"].as_str().unwrap();
            let Some(idx) = report.endpoints.iter().position(|e| e.pin == name) else {
                failures.push(format!("{name}: missing"));
                continue;
            };
            let ep = &polarities[idx];
            let max = resolve_tied_polarity(
                pick_worst_polarity(&ep.max).unwrap(),
                &ep.max,
                r["max_arr_ns"].as_f64().unwrap(),
                r["max_req_ns"].as_f64().unwrap(),
                r["max_slew_ns"].as_f64().unwrap(),
                SLACK_TIE_NS,
            );
            let min = resolve_tied_polarity(
                pick_worst_polarity(&ep.min).unwrap(),
                &ep.min,
                r["min_arr_ns"].as_f64().unwrap(),
                r["min_req_ns"].as_f64().unwrap(),
                r["min_slew_ns"].as_f64().unwrap(),
                SLACK_TIE_NS,
            );
            let values = [
                max.arr, max.req, max.slack, max.slew, min.arr, min.req, min.slack, min.slew,
            ];
            for (i, field) in fields.iter().enumerate() {
                let expected = r[*field].as_f64().unwrap();
                let (rel_tol, abs_tol) = if field.ends_with("slew_ns") {
                    (tol.slew_rel, 0.005)
                } else if field.ends_with("slack_ns") {
                    (tol.arr_rel, tol.slack_abs_ns)
                } else {
                    (tol.arr_rel, tol.arr_abs_ns)
                };
                if !within_tolerance(values[i], expected, rel_tol, abs_tol).0 {
                    failures.push(format!("{name}.{field}: ours={} ref={expected}", values[i]));
                }
            }
        }
        for (ours, key) in [
            (report.wns_max_ns, "wns_max_ns"),
            (report.tns_max_ns, "tns_max_ns"),
            (report.wns_min_ns, "wns_min_ns"),
            (report.tns_min_ns, "tns_min_ns"),
        ] {
            let expected = golden[key].as_f64().unwrap();
            let (rel, abs) = if key.starts_with("tns") {
                (tol.tns_rel, 0.005)
            } else {
                (tol.arr_rel, tol.slack_abs_ns)
            };
            if !within_tolerance(ours, expected, rel, abs).0 {
                failures.push(format!("{key}: ours={ours} ref={expected}"));
            }
        }
        // Goldens that carry OpenSTA's violation counts must match exactly: a count is an
        // integer, not a tolerance question.
        for (ours, key) in [
            (report.setup_violations, "setup_violations"),
            (report.hold_violations, "hold_violations"),
        ] {
            if let Some(expected) = golden.get(key)
                && ours != expected.as_u64().unwrap() as u32
            {
                failures.push(format!(
                    "{key}: ours={ours} ref={}",
                    expected.as_u64().unwrap()
                ));
            }
        }
        // `pulse_width` is a separate report family: pin and edge match exactly,
        // the three value columns sit at the arrival/slack tolerances.
        if let Some(rows) = golden.get("pulse_width") {
            let rows = rows.as_array().expect("pulse_width");
            if report.pulse_width_checks.len() != rows.len() {
                failures.push(format!(
                    "pulse_width count: ours={} ref={}",
                    report.pulse_width_checks.len(),
                    rows.len()
                ));
            }
            for r in rows {
                let name = r["pin"].as_str().unwrap();
                let edge = r["edge"].as_str().unwrap();
                let want_edge = if edge == "high" {
                    PulseEdge::High
                } else {
                    PulseEdge::Low
                };
                let Some(row) = report
                    .pulse_width_checks
                    .iter()
                    .find(|e| e.pin == name && e.edge == want_edge)
                else {
                    failures.push(format!("pulse {name} ({edge}): missing"));
                    continue;
                };
                for (ours, field) in [
                    (row.required_ns, "required_ns"),
                    (row.actual_ns, "actual_ns"),
                    (row.slack_ns, "slack_ns"),
                ] {
                    let expected = r[field].as_f64().unwrap();
                    let abs = if field == "slack_ns" {
                        tol.slack_abs_ns
                    } else {
                        tol.arr_abs_ns
                    };
                    if !within_tolerance(ours, expected, tol.arr_rel, abs).0 {
                        failures.push(format!(
                            "pulse {name} ({edge}).{field}: ours={ours} ref={expected}"
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Hand-written SPEF whose data net has resistor loops (parallel paths and a cycle): the reader
    /// keeps a BFS spanning tree, OpenSTA keeps its own reduction. Both must agree on the endpoints.
    #[test]
    fn test_sta_edge_resistor_loop() {
        compare_sta_edge(
            "resistor_loop",
            "base.def",
            "edge.sdc",
            Some("edge.spef"),
            "spef",
            Tol::STRICT,
        );
    }

    /// `set_clock_uncertainty -setup 0.2 [get_clocks core_clk]`: OpenSTA applies setup uncertainty
    /// to register checks and to the output-port required time; ours now does both.
    #[test]
    fn test_sta_edge_clock_uncertainty() {
        compare_sta_edge(
            "clock_uncertainty",
            "base.def",
            "edge.sdc",
            None,
            "zero_wire",
            Tol::STRICT,
        );
    }

    /// `mc-xdomain` edge pairing: the two-clock fixture reports cross-domain
    /// endpoints on the min-gap (launch, capture) edge pair instead of
    /// deferring (`docs/multi-clock.md` §1) — `u_fx/D` pairs launch edge 0
    /// with capture 1.0, `u_fy/D` launch 1.0 with capture 2.0 — while
    /// same-domain checks keep their capture lattice's period (`u_fa1/D` on
    /// `cka`'s 2.0 ns, `u_fb1/D` on `ckb`'s 1.0 ns). The whole OpenSTA
    /// `t2_int` endpoint set is asserted.
    #[test]
    fn sta_evaluates_two_clock() {
        let lib_dir = sky130_hd();
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let dir = design_dir("sta_clocks");
        let design = parse_def(&dir.join("two_clock.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = load_sdc(&dir.join("t2_int.sdc")).expect("SDC");
        assert_eq!(sdc.design_clock_count(), 2);
        let report = run_sta(&design, &lib, &sdc, None).expect("two-clock sta");
        let mut pins: Vec<_> = report.endpoints.iter().map(|e| e.pin.as_str()).collect();
        pins.sort_unstable();
        assert_eq!(
            pins,
            [
                "u_fa0/D", "u_fa1/D", "u_fb0/D", "u_fb1/D", "u_fx/D", "u_fy/D", "ya", "yb"
            ]
        );
        // Capture-domain periods: cka 2.0 vs ckb 1.0, minus the same setup
        // arc (measured OpenSTA `t2_int`: 1.885446 / 0.885446).
        let req = |pin: &str| {
            report
                .endpoints
                .iter()
                .find(|e| e.pin == pin)
                .unwrap()
                .max_req_ns
        };
        assert!((req("u_fa1/D") - 1.885446).abs() < 1e-4, "{}", req("u_fa1/D"));
        assert!((req("u_fb1/D") - 0.885446).abs() < 1e-4, "{}", req("u_fb1/D"));
        // Cross-domain min-gap pairs (measured `t2_int`): cka→ckb pairs launch
        // edge 0 with capture 1.0 (`u_fx/D` req = 1.0 − setup); ckb→cka pairs
        // launch 1.0 with capture 2.0 (`u_fy/D` req = 2.0 − setup), not the
        // edge-0 fold. `u_fb0/D` launches on `d_in`'s edge-0 arrival.
        assert!((req("u_fx/D") - 0.885446).abs() < 1e-4, "{}", req("u_fx/D"));
        assert!((req("u_fy/D") - 1.885446).abs() < 1e-4, "{}", req("u_fy/D"));
        assert!((req("u_fb0/D") - 0.854762).abs() < 1e-4, "{}", req("u_fb0/D"));
        // Per-domain output anchors: `ya` -clock cka, `yb` -clock ckb.
        assert!((req("ya") - 1.3).abs() < 1e-5, "{}", req("ya"));
        assert!((req("yb") - 0.3).abs() < 1e-5, "{}", req("yb"));
        assert_eq!(report.skipped.cross_domain_endpoints, 0);
    }

    /// `mc-xdomain`/`mc-selectors` scenario matrix: `tests/designs/sta_clocks/*.probe.json`
    /// was emitted by `probe.sh` (OpenSTA in the LibreLane container). In every
    /// scenario each reported endpoint must equal OpenSTA's row at
    /// STRICT tolerance, and every golden endpoint we do not report must be a
    /// named `CrossDomain` skip — nothing folds silently onto the capture
    /// lattice, nothing is lost. Cross-domain rows evaluate on the min-gap
    /// edge pair now, so `cross_domain_endpoints` is 0 on this fixture and the
    /// wns/tns/violations aggregates compare in every scenario. Clock-object
    /// selectors, `set_clock_groups`, and `-waveform` scenarios are covered
    /// here since `mc-selectors`.
    #[test]
    fn test_sta_clocks_match_opensta() {
        let lib_dir = sky130_hd();
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let dir = design_dir("sta_clocks");
        let mut evaluated = 0;
        for (def_name, probe_name) in [
            ("two_clock.def", "two_clock.probe.json"),
            ("two_clock_latch.def", "two_clock_latch.probe.json"),
        ] {
            let design = parse_def(&dir.join(def_name), &tech, Some(&lib)).expect("DEF");
            let reference: serde_json::Value = serde_json::from_slice(
                &std::fs::read(dir.join(probe_name)).expect("probe file"),
            )
            .expect("probe JSON");
            let scenarios = reference["scenarios"].as_object().expect("scenarios");
            for (name, golden) in scenarios {
                let sdc_path = dir.join(format!("{name}.sdc"));
                let sdc = load_sdc(&sdc_path)
                    .unwrap_or_else(|e| panic!("{name}: unexpected parse failure: {e}"));
                let (report, _, polarities) =
                    run_sta_detail(&design, &lib, &sdc, None, true).expect("STA");
                let golden_eps = golden["endpoints"].as_array().expect("endpoints");
                let is_deferred = |pin: &str| {
                    report
                        .skipped
                        .skipped_endpoints
                        .iter()
                        .any(|s| s.pin == pin && s.reason == SkipReason::CrossDomain)
                };
                const SLACK_TIE_NS: f64 = 0.002;
                let fields = [
                    "max_arr_ns",
                    "max_req_ns",
                    "max_slack_ns",
                    "max_slew_ns",
                    "min_arr_ns",
                    "min_req_ns",
                    "min_slack_ns",
                    "min_slew_ns",
                ];
                let tol = Tol::STRICT;
                let mut failures = Vec::new();
                for r in golden_eps {
                    let pin = r["pin"].as_str().unwrap();
                    let Some(idx) = report.endpoints.iter().position(|e| e.pin == pin) else {
                        if !is_deferred(pin) {
                            failures.push(format!("{pin}: missing and not a CrossDomain skip"));
                        }
                        continue;
                    };
                    let ep = &polarities[idx];
                    let max = resolve_tied_polarity(
                        pick_worst_polarity(&ep.max).unwrap(),
                        &ep.max,
                        r["max_arr_ns"].as_f64().unwrap(),
                        r["max_req_ns"].as_f64().unwrap(),
                        r["max_slew_ns"].as_f64().unwrap(),
                        SLACK_TIE_NS,
                    );
                    let min = resolve_tied_polarity(
                        pick_worst_polarity(&ep.min).unwrap(),
                        &ep.min,
                        r["min_arr_ns"].as_f64().unwrap(),
                        r["min_req_ns"].as_f64().unwrap(),
                        r["min_slew_ns"].as_f64().unwrap(),
                        SLACK_TIE_NS,
                    );
                    let values = [
                        max.arr, max.req, max.slack, max.slew, min.arr, min.req, min.slack,
                        min.slew,
                    ];
                    for (i, field) in fields.iter().enumerate() {
                        let expected = r[*field].as_f64().unwrap();
                        let (rel_tol, abs_tol) = if field.ends_with("slew_ns") {
                            (tol.slew_rel, 0.005)
                        } else if field.ends_with("slack_ns") {
                            (tol.arr_rel, tol.slack_abs_ns)
                        } else {
                            (tol.arr_rel, tol.arr_abs_ns)
                        };
                        if !within_tolerance(values[i], expected, rel_tol, abs_tol).0 {
                            failures
                                .push(format!("{pin}.{field}: ours={} ref={expected}", values[i]));
                        }
                    }
                }
                for e in &report.endpoints {
                    if !golden_eps.iter().any(|r| r["pin"] == e.pin) {
                        failures.push(format!("{}: reported but absent from OpenSTA", e.pin));
                    }
                }
                for s in &report.skipped.skipped_endpoints {
                    if s.reason == SkipReason::CrossDomain
                        && !golden_eps.iter().any(|r| r["pin"] == s.pin)
                    {
                        failures.push(format!("{}: deferred but OpenSTA never timed it", s.pin));
                    }
                }
                // Aggregates only when the endpoint set is complete — OpenSTA's
                // wns/tns/violation counts span the cross-domain paths that
                // defer, so a scenario with `CrossDomain` skips cannot compare.
                // An empty golden table (`t2_selac`/`t2_selunion` cut every
                // launch) carries `null` aggregates — the endpoint list is
                // the measurement there.
                if report.skipped.cross_domain_endpoints == 0 && !golden_eps.is_empty() {
                    for (ours, key) in [
                        (report.wns_max_ns, "wns_max_ns"),
                        (report.tns_max_ns, "tns_max_ns"),
                        (report.wns_min_ns, "wns_min_ns"),
                        (report.tns_min_ns, "tns_min_ns"),
                    ] {
                        let expected = golden[key].as_f64().unwrap();
                        let (rel, abs) = if key.starts_with("tns") {
                            (tol.tns_rel, 0.005)
                        } else {
                            (tol.arr_rel, tol.slack_abs_ns)
                        };
                        if !within_tolerance(ours, expected, rel, abs).0 {
                            failures.push(format!("{key}: ours={ours} ref={expected}"));
                        }
                    }
                    for (ours, key) in [
                        (report.setup_violations, "setup_violations"),
                        (report.hold_violations, "hold_violations"),
                    ] {
                        if let Some(expected) = golden.get(key)
                            && ours != expected.as_u64().unwrap() as u32
                        {
                            failures.push(format!(
                                "{key}: ours={ours} ref={}",
                                expected.as_u64().unwrap()
                            ));
                        }
                    }
                }
                assert!(
                    failures.is_empty(),
                    "{name}: {} mismatches:\n{}",
                    failures.len(),
                    failures.join("\n")
                );
                if name == "t2_selport" {
                    // `-from clk1` resolves to the port object — a resolved
                    // but dead selector, so no diagnostic (OpenSTA warns
                    // nothing either).
                    assert_eq!(report.skipped.unresolved_exception_selectors, 0);
                }
                evaluated += 1;
            }
        }
        assert_eq!(evaluated, 40, "every scenario must evaluate");
    }

    /// Undeclared-domain hazard (`docs/multi-clock.md` §2): a single-clock
    /// SDC on the two-clock DEF must not fold `cb`'s flops onto `cka`'s
    /// lattice. The `cka` side evaluates; every `cb` endpoint is named in
    /// `skipped_endpoints` — `NoClock` where the capture clock pin is
    /// unreached, a no-arrival skip where the launch was dead.
    #[test]
    fn one_clock_sdc_on_two_clock_def() {
        let lib_dir = sky130_hd();
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let dir = design_dir("sta_clocks");
        let design = parse_def(&dir.join("two_clock.def"), &tech, Some(&lib)).expect("DEF");
        let sdc = parse_sdc(
            "create_clock -name cka -period 2.0 [get_ports clk1]\n\
             set_input_delay -min 0.1 -clock cka [get_ports d_in]\n\
             set_input_delay -max 0.7 -clock cka [get_ports d_in]\n\
             set_output_delay -min 0.1 -clock cka [get_ports ya]\n\
             set_output_delay -max 0.7 -clock cka [get_ports ya]\n\
             set_output_delay -min 0.1 -clock cka [get_ports yb]\n\
             set_output_delay -max 0.7 -clock cka [get_ports yb]\n\
             set_input_transition 0.1 [get_ports d_in]\n\
             set_load 0.05 [get_ports ya]\n\
             set_load 0.05 [get_ports yb]\n",
        )
        .expect("SDC");
        let report = run_sta(&design, &lib, &sdc, None).expect("sta");
        let mut pins: Vec<_> = report.endpoints.iter().map(|e| e.pin.as_str()).collect();
        pins.sort_unstable();
        assert_eq!(pins, ["u_fa0/D", "u_fa1/D", "ya"]);
        let reason = |pin: &str| {
            report
                .skipped
                .skipped_endpoints
                .iter()
                .find(|s| s.pin == pin)
                .unwrap_or_else(|| panic!("{pin} not in skipped_endpoints"))
                .reason
        };
        for pin in ["u_fb0/D", "u_fb1/D", "u_fx/D"] {
            assert_eq!(reason(pin), SkipReason::NoClock, "{pin}");
        }
        for pin in ["u_fy/D", "yb"] {
            assert!(
                matches!(
                    reason(pin),
                    SkipReason::NoPath | SkipReason::UntimedDriver
                ),
                "{pin}: {:?}",
                reason(pin)
            );
        }
    }

    /// `set_propagated_clock`: clock arrival is propagated through the buffered tree (buf ->
    /// inverting buffer -> AND clock gate -> latch/FF clock pins) for both transitions. The
    /// fixture also carries an inverting branch (`clkinv`) so `u_ff2/CLK` sees its first rise
    /// on clock edge 1; the golden comes from OpenSTA `report_checks`/endpoint collection.
    #[test]
    fn test_sta_edge_propagated_clock() {
        compare_sta_edge(
            "propagated_clock",
            "propagated_clock/propagated.def",
            "edge.sdc",
            None,
            "zero_wire",
            Tol::STRICT,
        );
    }

    /// Real hold violations on the direct `d_in -> D` paths (zero data delay, `-min 0.0`
    /// input delay, 0.1 ns hold uncertainty): 2 of 4 endpoints violate under both the ideal
    /// clock and the propagated clock through `u_clkb`. The golden carries OpenSTA's
    /// violation counts.
    #[test]
    fn test_sta_edge_hold_violation_ideal() {
        compare_sta_edge(
            "hold_violation",
            "hold_violation/hold.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_hold_violation_propagated() {
        compare_sta_edge(
            "hold_violation",
            "hold_violation/hold.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// Combinational clock gates (`and2`/`or2`/`nand2`/`nor2`) plus the `dlclkp` integrated
    /// clock-gating cell. The Liberty `function` expression decides the check endpoint:
    /// AND/NAND are "rising" (setup vs clock rise, hold vs preceding fall), OR/NOR
    /// "falling" (both swapped, min arrival one period later) — measured on OpenSTA
    /// 3.0.16. `dlclkp` has no `function`, so it is not an enable-check gate; its GATE pin
    /// is checked by the cell's own setup/hold arcs and CLK propagates to GCLK through
    /// the ordinary combinational arc.
    /// `gate_kind` reads the output pin's Liberty `function`: AND/NAND are Rising check
    /// end-points, OR/NOR Falling; muxes and partial functions must not classify.
    #[test]
    fn test_gate_kind_classifies_liberty_functions() {
        fn kind_of(expr: &str) -> Option<GateKind> {
            let mut lib = LibDb::new();
            let cell_name = lib.strings.intern("g");
            for (n, dir) in [
                ("A", PinDir::Input),
                ("B", PinDir::Input),
                ("Y", PinDir::Output),
            ] {
                lib.pins.push(LibPin {
                    name: lib.strings.intern(n),
                    dir,
                    cap_rise: 0.0,
                    cap_fall: 0.0,
                    cap_nom: 0.0,
                    is_clock: false,
                    max_cap: None,
                    max_transition: None,
                    function: if dir == PinDir::Output {
                        Some(lib.strings.intern(expr))
                    } else {
                        None
                    },
                    internal_power: Vec::new(),
                });
            }
            lib.cells.push(LibCell {
                name: cell_name,
                area: 0.0,
                is_sequential: false,
                is_latch: false,
                pin_offset: 0,
                pin_count: 3,
                arc_offset: 0,
                arc_count: 0,
                leakage_power_w: None,
                leakage_states: Vec::new(),
            });
            let cell = &lib.cells[0];
            gate_kind(&lib, cell, &lib.pins[2])
        }
        assert_eq!(kind_of("(A&B)"), Some(GateKind::Rising));
        assert_eq!(kind_of("(!A) | (!B)"), Some(GateKind::Rising));
        assert_eq!(kind_of("!(A&B)"), Some(GateKind::Rising));
        assert_eq!(kind_of("(A) | (B)"), Some(GateKind::Falling));
        assert_eq!(kind_of("(!A&!B)"), Some(GateKind::Falling));
        assert_eq!(kind_of("!(A|B)"), Some(GateKind::Falling));
        // Clock muxes and non-unate functions must not be treated as clock gates.
        assert_eq!(kind_of("(A&S) | (B&!S)"), None);
        assert_eq!(kind_of("(A&B&C)"), None);
        assert_eq!(kind_of("(A)"), None);
        assert_eq!(kind_of("(A&!B)"), None);
    }

    /// Equal arrivals from different launch-edge parities must keep both parity bits:
    /// the endpoint required time shifts by a period per parity, so dropping one would
    /// report the wrong slack (external review S1). The CRPR launch-tag and clock-domain
    /// masks follow the same replace/union discipline.
    #[test]
    fn test_parity_mask_keeps_tied_arrivals() {
        let mut s = PinTimingState::UNREACHED;
        s.merge(
            RISE, 1.0, 0.5, 1.0, 0.5, 0.1, 0.1, 0b01, 0b01, 0b010, 0b010, 0b001, 0b001,
        );
        // Same max arrival, opposite parity: both bits survive.
        s.merge(
            RISE, 1.0, 0.4, 1.0, 0.4, 0.1, 0.1, 0b10, 0b10, 0b100, 0b100, 0b010, 0b010,
        );
        assert_eq!(s.par_max[RISE], 0b11);
        assert_eq!(s.crpr_max[RISE], 0b110);
        assert_eq!(s.dom_max[RISE], 0b011);
        // The better min arrival replaces its mask entirely.
        assert_eq!(s.par_min[RISE], 0b10);
        assert_eq!(s.crpr_min[RISE], 0b100);
        assert_eq!(s.dom_min[RISE], 0b010);
        // A strictly better max replaces the mask; an equal max unions again. The
        // equal min arrival also unions its parity bit.
        s.merge(
            RISE, 1.2, 0.4, 1.2, 0.4, 0.1, 0.1, 0b10, 0b01, 0b100, 0b010, 0b100, 0b001,
        );
        assert_eq!(s.par_max[RISE], 0b10);
        assert_eq!(s.par_min[RISE], 0b11);
        assert_eq!(s.crpr_max[RISE], 0b100);
        assert_eq!(s.crpr_min[RISE], 0b110);
        assert_eq!(s.dom_max[RISE], 0b100);
        assert_eq!(s.dom_min[RISE], 0b011);
        s.merge(
            RISE, 1.2, 0.45, 1.2, 0.45, 0.1, 0.1, 0b01, 0b10, 0b010, 0b100, 0b001, 0b010,
        );
        assert_eq!(s.par_max[RISE], 0b11);
        assert_eq!(s.par_min[RISE], 0b11);
        assert_eq!(s.crpr_max[RISE], 0b110);
        assert_eq!(s.crpr_min[RISE], 0b110);
        assert_eq!(s.dom_max[RISE], 0b101);
        assert_eq!(s.dom_min[RISE], 0b011);
    }

    /// OpenSTA's ideal clock still tracks edge inversion through cell-driven clock
    /// nets: `nand2`/`nor2` swap the `dfrtn` capture edges on `c_nand`/`c_nor`, and
    /// `ideal_clock_parity` reproduces that phase without propagating delays.
    #[test]
    fn test_sta_edge_clock_gate_ideal() {
        compare_sta_edge(
            "clock_gate",
            "clock_gate/clock_gate.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_clock_gate_propagated() {
        compare_sta_edge(
            "clock_gate",
            "clock_gate/clock_gate.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// Transparent latches of every `sky130_fd_sc_hd` shape (dlxt[n/p], dlxb[n/p],
    /// dlrt[n/p], dlrb[n/p], lpflow_inputisolatch): the `D -> Q` transparent path
    /// borrows up to the closing edge minus setup, data waiting at the open edge
    /// launches through the clock-to-Q arc, and `RESET_B` pins carry
    /// recovery/removal checks. The borrowed-time cap was reverse-engineered from
    /// OpenSTA's `report_checks` on this fixture.
    #[test]
    fn test_sta_edge_latch_borrow_ideal() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "latch.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_propagated() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "prop.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// `set_max_delay`/`set_min_delay` on latch-D pins: OpenSTA applies the
    /// delay check — `req` clamps the delay arrival into the borrow window
    /// `[N + lat0 - unc, + pulse - t_setup]` on the max side and
    /// `N + lat0 + t_hold + hold_unc` on the min side. A covered `-to` also
    /// silences the latch's `D -> Q` borrow arc for the whole signature
    /// class, and paths launched through a latch open edge re-base their
    /// reported arrival to the delay edge.
    #[test]
    fn test_sta_edge_latch_borrow_delay_max() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_max.sdc",
            None,
            "delay_max",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_delay_min() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_min.sdc",
            None,
            "delay_min",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_delay_ln() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_ln.sdc",
            None,
            "delay_ln",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_delay_from() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_from.sdc",
            None,
            "delay_from",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_delay_allregs() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_allregs.sdc",
            None,
            "delay_allregs",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_latch_borrow_delay_prop() {
        compare_sta_edge(
            "latch_borrow",
            "latch_borrow/latch_borrow.def",
            "delay_prop.sdc",
            None,
            "delay_prop",
            Tol::STRICT,
        );
    }

    /// `set_timing_derate` early/late plus the `-cell_check`, `-clock`/`-data` and
    /// `-rise`/`-fall` selectors on the hold.def topology (one shared clock net, one
    /// register-to-register path — the case same-net CRPR applies to), under both the
    /// ideal and the propagated clock. Selector semantics were measured on OpenSTA: a bare corner covers
    /// cell and net delays on both paths and transitions but not checks; `-rise`/`-fall`
    /// select the transition entering each arc.
    #[test]
    fn test_sta_edge_timing_derate_ideal() {
        compare_sta_edge(
            "timing_derate",
            "timing_derate/timing_derate.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_timing_derate_propagated() {
        compare_sta_edge(
            "timing_derate",
            "timing_derate/timing_derate.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// dfrtp async reset: OpenSTA does not propagate `clear` arcs (RESET_B -> Q) into
    /// ordinary timing paths — `find_timing_paths -from rst_n -through u_ff0/Q` is empty and
    /// `report_arrival u_ff0/Q` shows only clock-derived arrival. OpenSTA only checks
    /// recovery/removal on the RESET_B pin, so `u_ff0/RESET_B` is the sole async endpoint.
    /// The Clear arc stays in the unsupported-arc skip count.
    #[test]
    fn test_sta_edge_async_reset_ideal() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_reset_propagated() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// `set_max_delay`/`set_min_delay` on an async-check pin: OpenSTA applies
    /// the delay check with the recovery/removal arc playing the
    /// setup/hold role — `req = capture - unc - t_recovery + N` anchored at
    /// the delay edge on the max side, `removal_req + N` on the min side.
    /// `-from` restricts coverage per launch source; `-to [get_cells]`
    /// covers the cell's check pins.
    #[test]
    fn test_sta_edge_async_reset_delay_max() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "delay_max.sdc",
            None,
            "delay_max",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_reset_delay_min() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "delay_min.sdc",
            None,
            "delay_min",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_reset_delay_from() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "delay_from.sdc",
            None,
            "delay_from",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_reset_delay_cell() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "delay_cell.sdc",
            None,
            "delay_cell",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_reset_delay_prop() {
        compare_sta_edge(
            "async_reset",
            "async_reset/async_reset.def",
            "delay_prop.sdc",
            None,
            "delay_prop",
            Tol::STRICT,
        );
    }

    /// `dfrtn_1` is an inverting-clock flip-flop: `setup_falling`/`hold_falling` on D,
    /// `falling_edge` launch on Q, `recovery_falling`/`removal_falling` on RESET_B.
    /// The ideal clock seeds both transitions at the latency, so falling edges sit half a
    /// period later and carry parity 1 — the same convention the propagated clock computes.
    #[test]
    fn test_sta_edge_async_fall_ideal() {
        compare_sta_edge(
            "async_fall",
            "async_fall/async_fall.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_fall_propagated() {
        compare_sta_edge(
            "async_fall",
            "async_fall/async_fall.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// `dfrtn_1 -> dfrtn_1` register-to-register: u_ff1/D is launched on the same clock-edge
    /// parity that captures it, so a capture-side-only parity model shifts its required time
    /// by a full period (measured: OpenSTA max_req 6.5178 ns vs 2.5178 ns before the fix).
    /// u_ff2 (`dfxtp_1`) covers the mixed-polarity pair on the same clock net.
    #[test]
    fn test_sta_edge_async_fall_chain_ideal() {
        compare_sta_edge(
            "async_fall_chain",
            "async_fall_chain/async_fall_chain.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    #[test]
    fn test_sta_edge_async_fall_chain_propagated() {
        compare_sta_edge(
            "async_fall_chain",
            "async_fall_chain/async_fall_chain.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// `dlxtp_1` is a positive latch (`setup_falling`/`hold_falling` on GATE): the R5
    /// latch model handles it through `open_pol` like `dlxtn`, so all 3 OpenSTA
    /// endpoints (`u_ff/D`, `u_lt/D`, `y`) are reached.
    #[test]
    fn test_sta_edge_non_dlxtn_latch() {
        compare_sta_edge(
            "non_dlxtn_latch",
            "non_dlxtn_latch/edge_latch.def",
            "edge_latch.sdc",
            None,
            "zero_wire",
            Tol::STRICT,
        );
    }

    /// `min_pulse_width` checks evaluate under the ideal clock: 9 OpenSTA rows
    /// (direct, inverted and double-buffered clock branches; falling-edge FF;
    /// a latch enable with a `fall_constraint`-only arc), all met at period 5.0.
    /// The propagated clock network is still what OpenSTA measures the width on.
    #[test]
    fn test_sta_edge_min_pulse_width_ideal() {
        compare_sta_edge(
            "min_pulse_width",
            "min_pulse_width/mpw.def",
            "ideal.sdc",
            None,
            "ideal",
            Tol::STRICT,
        );
    }

    /// Same design under `set_propagated_clock` with `-clock`/`-cell_check`
    /// derates: early/late derated arrivals make the pulse widths asymmetric
    /// across the three branches, and the inverted branch exercises the
    /// next-edge + period-wrap width rule.
    #[test]
    fn test_sta_edge_min_pulse_width_propagated() {
        compare_sta_edge(
            "min_pulse_width",
            "min_pulse_width/mpw.def",
            "propagated.sdc",
            None,
            "propagated",
            Tol::STRICT,
        );
    }

    /// Period 0.6 violates every `min_pulse_width` row. Pulse slack is a
    /// separate family: OpenSTA's ordinary WNS/TNS and `find_timing_paths`
    /// endpoints are untouched by the nine violations.
    #[test]
    fn test_sta_edge_min_pulse_width_viol() {
        compare_sta_edge(
            "min_pulse_width",
            "min_pulse_width/mpw.def",
            "viol.sdc",
            None,
            "viol",
            Tol::STRICT,
        );
    }

    struct ReproSweepRow {
        input_slew: f64,
        out_pol: usize,
        near_c2: f64,
        rpi: f64,
        far_c1: f64,
        ceff: f64,
        delay: f64,
        slew: f64,
        driver_slew: f64,
    }

    fn parse_repro_sweep(text: &str) -> Vec<ReproSweepRow> {
        let mut rows = Vec::new();
        let mut cur_tin: Option<f64> = None;
        let mut cur_pol: Option<usize> = None;
        let mut cur_pi: Option<(f64, f64, f64, f64)> = None;
        let mut cur_delay: Option<f64> = None;
        let mut cur_slew: Option<f64> = None;

        let parse_kv = |token: &str, prefix: &str| -> f64 {
            let val = token
                .trim_end_matches(',')
                .strip_prefix(prefix)
                .unwrap_or_else(|| panic!("expected prefix {prefix:?} in {token:?}"));
            val.parse()
                .unwrap_or_else(|_| panic!("bad float in {token:?}"))
        };

        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if let Some(rest) = line.strip_prefix("SWEEP input_slew ") {
                cur_tin =
                    Some(rest.trim().parse().unwrap_or_else(|_| {
                        panic!("line {}: bad input_slew {rest:?}", line_no + 1)
                    }));
                cur_pol = None;
            } else if line == "A ^ -> Y v" {
                cur_pol = Some(FALL);
                cur_pi = None;
                cur_delay = None;
                cur_slew = None;
            } else if line == "A v -> Y ^" {
                cur_pol = Some(RISE);
                cur_pi = None;
                cur_delay = None;
                cur_slew = None;
            } else if let Some(rest) = line.strip_prefix("Pi model ") {
                let f: Vec<&str> = rest.split_whitespace().collect();
                assert_eq!(
                    f.len(),
                    4,
                    "line {}: bad Pi model line {line:?}",
                    line_no + 1
                );
                cur_pi = Some((
                    parse_kv(f[0], "C2="),
                    parse_kv(f[1], "Rpi="),
                    parse_kv(f[2], "C1="),
                    parse_kv(f[3], "Ceff="),
                ));
            } else if let Some(rest) = line.strip_prefix("Delay = ") {
                cur_delay = Some(
                    rest.trim()
                        .parse()
                        .unwrap_or_else(|_| panic!("line {}: bad Delay {rest:?}", line_no + 1)),
                );
            } else if let Some(rest) = line.strip_prefix("Slew = ") {
                cur_slew = Some(
                    rest.trim()
                        .parse()
                        .unwrap_or_else(|_| panic!("line {}: bad Slew {rest:?}", line_no + 1)),
                );
            } else if let Some(rest) = line.strip_prefix("Driver waveform slew = ") {
                let driver_slew: f64 = rest.trim().parse().unwrap_or_else(|_| {
                    panic!("line {}: bad Driver waveform slew {rest:?}", line_no + 1)
                });
                let (near_c2, rpi, far_c1, ceff) =
                    cur_pi.take().expect("Pi model before Driver waveform slew");
                rows.push(ReproSweepRow {
                    input_slew: cur_tin.expect("SWEEP input_slew before sub-block"),
                    out_pol: cur_pol
                        .take()
                        .expect("transition before Driver waveform slew"),
                    near_c2,
                    rpi,
                    far_c1,
                    ceff,
                    delay: cur_delay.take().expect("Delay before Driver waveform slew"),
                    slew: cur_slew.take().expect("Slew before Driver waveform slew"),
                    driver_slew,
                });
            }
        }
        rows
    }

    /// Measurement (E3.5, OpenSTA #582): compares our NLDM extrapolation and `solve_dmp` (`RD_SCALE = 4.5`
    /// and `3.0`) against OpenSTA's 7-point input-slew sweep in `tests/designs/repro_extreme_ceff/sweep_reference.txt`.
    #[test]
    fn test_repro_extreme_ceff_sweep_vs_opensta() {
        use crate::sta::dmp::{PiModel, solve_dmp, solve_dmp_with};

        let dir = design_dir("repro_extreme_ceff");
        let text =
            std::fs::read_to_string(dir.join("sweep_reference.txt")).expect("sweep_reference.txt");
        let rows = parse_repro_sweep(&text);
        assert_eq!(
            rows.len(),
            14,
            "expected 7 input slews x 2 transitions = 14 rows, got {}",
            rows.len()
        );

        let spef = crate::readers::load_spef(&dir.join("top.spef")).expect("top.spef");
        let y_net = spef.nets.get("y").expect("SPEF net y");
        let cap_at = |node: &str| {
            y_net
                .caps
                .iter()
                .find(|(n, _)| n == node)
                .map(|&(_, c)| c)
                .expect("SPEF cap")
        };
        let (spef_near, spef_far) = (cap_at("u1:Y"), cap_at("y"));
        let spef_rpi = y_net
            .resistors
            .iter()
            .find(|(a, b, _)| a == "u1:Y" && b == "y")
            .map(|&(_, _, r)| r)
            .expect("SPEF resistor");

        let lib_dir = sky130_hd();
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let cell = lib
            .cells
            .iter()
            .find(|c| lib.strings.resolve(c.name) == "sky130_fd_sc_hd__nor2_2")
            .expect("sky130_fd_sc_hd__nor2_2");
        let arcs: Vec<&crate::lib_db::LibTimingArc> = lib
            .cell_arcs(cell)
            .iter()
            .filter(|a| {
                lib.strings.resolve(lib.pins[a.related_pin.0 as usize].name) == "A"
                    && lib.strings.resolve(lib.pins[a.target_pin.0 as usize].name) == "Y"
                    && a.timing_type == TimingType::Combinational
            })
            .collect();
        assert_eq!(
            arcs.len(),
            1,
            "expected exactly 1 A->Y combinational arc on nor2_2"
        );
        let arc = arcs[0];
        assert_eq!(arc.timing_sense, TimingSense::NegativeUnate);

        println!(
            "REPRO_SWEEP {:>8} {:>4} | {:>12} {:>8} {:>8} {:>8} | {:>22} | {:>22} | {:>10}",
            "tin",
            "out",
            "refCeff(%tot)",
            "refDly",
            "refSlw",
            "refWave",
            "RD4.5 Ceff(%) Dly(%) Slw(%)",
            "RD3.0 Ceff(%) Dly(%) Slw(%)",
            "rdRoot/dSl"
        );
        let pct = |ours: f64, reference: f64| (ours - reference) / reference.abs() * 100.0;
        for row in &rows {
            assert!((spef_near as f64 - row.near_c2).abs() < 1e-6);
            assert!((spef_far as f64 - row.far_c1).abs() < 1e-6);
            assert!((spef_rpi as f64 - row.rpi).abs() < 1e-6);
            let pi = PiModel {
                c1: row.near_c2,
                rpi: row.rpi,
                c2: row.far_c1,
            };
            let total = pi.total();

            let (Some(delay_table), Some(slew_table)) = delay_tables(arc, row.out_pol) else {
                panic!("missing NLDM tables");
            };
            let tin = row.input_slew as f32;
            let table = |c: f64| {
                (
                    eval_nldm_delay_or_slew(delay_table, tin, c as f32) as f64,
                    eval_nldm_delay_or_slew(slew_table, tin, c as f32) as f64,
                )
            };

            let (tbl_dly, tbl_slw) = table(row.ceff);
            let rel_d = (tbl_dly - row.delay).abs() / row.delay.abs();
            let rel_s = (tbl_slw - row.slew).abs() / row.slew.abs();
            assert!(
                rel_d < 1e-4 && rel_s < 1e-4,
                "NLDM mismatch at tin={} out={}: ours=({tbl_dly}, {tbl_slw}) ref=({}, {}), rel=({rel_d:.3e}, {rel_s:.3e})",
                row.input_slew,
                row.out_pol,
                row.delay,
                row.slew
            );

            let r45 = solve_dmp(&pi, 4.5, &table);
            let r30 = solve_dmp(&pi, 3.0, &table);

            let (mut lo, mut hi) = (1e-3f64, 1e4f64);
            for _ in 0..70 {
                let mid = (lo * hi).sqrt();
                let r = solve_dmp_with(&pi, &|_, _| mid, &table);
                if r.ceff > row.ceff {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            let rd_root = (lo * hi).sqrt();
            let h = 0.05 * row.ceff;
            let (dsl, _ssl) = (
                (table(row.ceff + h).0 - table(row.ceff - h).0) / (2.0 * h),
                (table(row.ceff + h).1 - table(row.ceff - h).1) / (2.0 * h),
            );
            let ratio = rd_root / dsl;
            let ratio_str = if rd_root < 1.05e-3 {
                "<lo_bnd".to_string()
            } else if rd_root > 0.95e4 {
                ">hi_bnd".to_string()
            } else {
                format!("{ratio:>10.3}")
            };

            println!(
                "REPRO_SWEEP {:>8.4} {:>4} | {:>6.4}({:>4.0}%) {:>8.4} {:>8.4} {:>8.4} | {:>6.4}({:>+5.1}%) {:>+5.1}% {:>+5.1}% | {:>6.4}({:>+5.1}%) {:>+5.1}% {:>+5.1}% | {:>10}",
                row.input_slew,
                if row.out_pol == RISE { "rise" } else { "fall" },
                row.ceff,
                row.ceff / total * 100.0,
                row.delay,
                row.slew,
                row.driver_slew,
                r45.ceff,
                pct(r45.ceff, row.ceff),
                pct(r45.delay, row.delay),
                pct(r45.slew, row.slew),
                r30.ceff,
                pct(r30.ceff, row.ceff),
                pct(r30.delay, row.delay),
                pct(r30.slew, row.slew),
                ratio_str
            );
        }
    }

    /// Verifies OpenSTA's primary-input port-driven wire delay (`ln(2) * Elmore`) and linear sink slew
    /// (`s_in + ln(4) * Elmore = s_in + 2 * delay`) across all pins in `collect_pins.sh`, both against
    /// the raw formulas and against `run_sta_with_state`'s port sink arrivals and slews.
    #[test]
    fn test_port_wire_delay_vs_opensta() {
        let mut total_rows = 0usize;
        for_each_pins_design("PORTWIRE", |design_name, cx, ref_pins| {
            let (design, sdc) = (cx.design, cx.sdc);
            let (_, state) = run_sta_with_state(design, cx.lib, sdc, cx.parasitics).expect("STA");
            let mut r_elmore = Vec::new();
            let mut r_ln2 = Vec::new();
            let mut r_half_ramp = Vec::new();
            let mut r_degrade = Vec::new();
            let mut sta_dly_err = Vec::new();
            let mut sta_slw_err = Vec::new();
            let mut max_half_slew_diff = 0.0f64;

            for n in 0..design.num_nets() {
                let net = NetId(n as u32);
                let pins = design.net_pins(net);
                let Some(&drv) = pins.first() else { continue };
                if design.pin_cells[drv.0 as usize].is_some()
                    || !design.pin_is_driver[drv.0 as usize]
                {
                    continue;
                }
                let port_name = cx.port_name(drv);
                if sdc.is_clock_port(port_name) {
                    continue;
                }
                let Some(wire) = cx.wire_delays(net) else {
                    continue;
                };
                let drv_arr = (sdc.input_clock_latency_ns(port_name)
                    + sdc.input_delay_ns(port_name).1) as f64;
                let drv_slew = sdc.input_transition_ns(port_name) as f64;

                let mut net_elmore = Vec::new();
                let mut net_half = Vec::new();
                let mut net_max_slew_diff = 0.0f64;
                for (i, &sink) in pins.iter().enumerate().skip(1) {
                    let Some(sink_ref) = ref_pins.get(cx.pin_name(sink).as_str()) else {
                        continue;
                    };
                    for pol in [RISE, FALL] {
                        let (sink_slew, sink_arr) = (sink_ref[pol], sink_ref[2 + pol]);
                        let elmore = wire[pol][i] as f64;
                        if !(sink_arr.is_finite() && sink_slew.is_finite()) || elmore <= 1e-7 {
                            continue;
                        }
                        let ref_delay = sink_arr - drv_arr;
                        let ref_degrade = (sink_slew * sink_slew - drv_slew * drv_slew)
                            .max(0.0)
                            .sqrt();
                        let pred_half =
                            0.5 * (drv_slew.hypot(WIRE_SLEW_FACTOR as f64 * elmore) - drv_slew);
                        let half_slew_diff = (ref_delay - 0.5 * (sink_slew - drv_slew)).abs();
                        let ps = state[sink.0 as usize];
                        let sta_dly = ps.arrival_max[pol] as f64 - drv_arr;
                        let sta_slw = ps.slew_max[pol] as f64;
                        sta_dly_err.push((sta_dly - ref_delay).abs() / ref_delay.abs().max(1e-9));
                        sta_slw_err.push((sta_slw - sink_slew).abs() / sink_slew.abs().max(1e-9));
                        r_elmore.push(ref_delay / elmore);
                        r_ln2.push(ref_delay / (std::f64::consts::LN_2 * elmore));
                        if pred_half > 1e-12 {
                            r_half_ramp.push(ref_delay / pred_half);
                            net_half.push(ref_delay / pred_half);
                        }
                        r_degrade.push(ref_degrade / elmore);
                        max_half_slew_diff = max_half_slew_diff.max(half_slew_diff);
                        net_elmore.push(ref_delay / elmore);
                        net_max_slew_diff = net_max_slew_diff.max(half_slew_diff);
                        total_rows += 1;
                    }
                }
                if !net_elmore.is_empty() {
                    println!(
                        "PORTWIRE_NET {design_name} {port_name:<16} tin {:.3} n {:>4} | dly/elm {} | dly/half_pred {} | max|dly - (s_out-s_in)/2| {:.2e}",
                        drv_slew,
                        net_elmore.len(),
                        format_spread(&net_elmore, 4),
                        if net_half.is_empty() {
                            "n/a".to_string()
                        } else {
                            format_spread(&net_half, 4)
                        },
                        net_max_slew_diff,
                    );
                }
            }
            if !r_elmore.is_empty() {
                let p90 = |v: &[f64]| {
                    let mut s = v.to_vec();
                    s.sort_by(f64::total_cmp);
                    s[9 * s.len() / 10]
                };
                let r_ln2_med = median(r_ln2.clone());
                let sta_dly_med = median(sta_dly_err.clone());
                let sta_dly_p90 = p90(&sta_dly_err);
                let sta_slw_med = median(sta_slw_err.clone());
                let sta_slw_p90 = p90(&sta_slw_err);
                println!(
                    "PORTWIRE {design_name} n {} | dly/elmore {} | dly/(ln2*elmore) {} | dly/half_pred {} | degrade/elmore {} | max|dly-(s_out-s_in)/2| {:.2e} | sta dly err med {:.5} p90 {:.5} | sta slew err med {:.5} p90 {:.5}",
                    r_elmore.len(),
                    format_spread(&r_elmore, 4),
                    format_spread(&r_ln2, 4),
                    format_spread(&r_half_ramp, 4),
                    format_spread(&r_degrade, 4),
                    max_half_slew_diff,
                    sta_dly_med,
                    sta_dly_p90,
                    sta_slw_med,
                    sta_slw_p90,
                );
                assert!(
                    (r_ln2_med - 1.0).abs() < 0.005,
                    "{design_name}: median dly/(ln2*elmore) = {r_ln2_med}"
                );
                assert!(
                    max_half_slew_diff < 1e-5,
                    "{design_name}: max|dly-(s_out-s_in)/2| = {max_half_slew_diff:.3e}"
                );
                assert!(
                    sta_dly_med < 0.002 && sta_dly_p90 < 0.01,
                    "{design_name}: STA port delay rel err med={sta_dly_med:.5} p90={sta_dly_p90:.5}"
                );
                assert!(
                    sta_slw_med < 0.002 && sta_slw_p90 < 0.005,
                    "{design_name}: STA port slew rel err med={sta_slw_med:.5} p90={sta_slw_p90:.5}"
                );
            }
        });
        assert!(total_rows > 100, "only {total_rows} port-driven sink rows");
    }

    /// Measurement (E3.5 Step 1): evaluates two-pole driving-point waveform slew `pi_waveform_slew`
    /// at `rd_w = k_w * d(delay)/dC` and the extreme-load bailout `table(total).1 < (ln2)^2 * dslope(total) * total`
    /// across all five designs (`dcalc_*_all.txt`, `dcalc_extreme_placed.txt`, `repro_extreme_ceff/sweep_reference.txt`).
    #[test]
    fn test_dmp_pi_waveform_slew_vs_opensta() {
        use crate::sta::dmp::{
            PiModel, driver_waveform_slew, fit_ramp_duration, pi_waveform_slew, solve_dmp,
        };

        let dslope = |table: &dyn Fn(f64) -> (f64, f64), c: f64| -> f64 {
            let h = 0.05 * c;
            ((table(c + h).0 - table(c - h).0) / (2.0 * h)).max(1e-6)
        };
        let ln2 = std::f64::consts::LN_2;
        let bail_thresh = 2.0 * 1.25f64.ln();
        let k_candidates = [("ln2", ln2), ("1/ln4", 1.0 / 4f64.ln()), ("0.75", 0.75)];
        let dcalc_sets = [
            (
                "picorv32",
                "picorv32.placed.def",
                "picorv32.rcx.spef",
                "dcalc_routed_all.txt",
            ),
            (
                "aes",
                "aes.placed.def",
                "aes.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "ibex",
                "ibex.placed.def",
                "ibex.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "jpeg",
                "jpeg.placed.def",
                "jpeg.routed.spef",
                "dcalc_placed_all.txt",
            ),
            (
                "gcd",
                "gcd.placed.def",
                "gcd.rcx.spef",
                "dcalc_routed_all.txt",
            ),
            (
                "ibex_ext",
                "ibex.placed.def",
                "ibex.routed.spef",
                "dcalc_extreme_placed.txt",
            ),
            (
                "jpeg_ext",
                "jpeg.placed.def",
                "jpeg.routed.spef",
                "dcalc_extreme_placed.txt",
            ),
        ];

        struct WaveObs {
            design: String,
            label: String,
            tin: f64,
            total: f64,
            ref_ceff: f64,
            ref_wave: f64,
            tbl_ceff_slew: f64,
            tbl_tot_slew: f64,
            dsl_tot: f64,
            our_ceff: f64,
            our_slew: f64,
            our_wave: f64,
            // Per k_w candidate: (deg_at_ref, pi_at_ref, deg_at_ours, pi_at_ours)
            preds: [(bool, f64, bool, f64); 3],
        }

        let mut obs: Vec<WaveObs> = Vec::new();
        for (tag, def, spef, file) in dcalc_sets {
            let design_name = tag.trim_end_matches("_ext");
            let (matched, _unmodelled) = for_each_dcalc_row(
                design_name,
                def,
                spef,
                file,
                &mut |row, pi, table, _table_at| {
                    let total = pi.total();
                    let dsl_tot = dslope(table, total);
                    let (tbl_ceff_d, tbl_ceff_s) = table(row.ceff);
                    let (_, tbl_tot_s) = table(total);
                    let _ = tbl_ceff_d;
                    let r = solve_dmp(pi, RD_SCALE, table);
                    let our_wave = driver_waveform_slew(pi, &r, table);
                    let dsl_ref = dslope(table, row.ceff);
                    let dsl_our = dslope(table, r.ceff);
                    let mut preds = [(false, 0.0, false, 0.0); 3];
                    for (ki, &(_, kw)) in k_candidates.iter().enumerate() {
                        let rd_ref = kw * dsl_ref;
                        let (dt_ref, deg_ref) = fit_ramp_duration(rd_ref, row.ceff, tbl_ceff_s);
                        let pi_ref = pi_waveform_slew(pi, rd_ref, dt_ref);
                        let rd_our = kw * dsl_our;
                        let (dt_our, deg_our) = fit_ramp_duration(rd_our, r.ceff, r.slew);
                        let pi_our = pi_waveform_slew(pi, rd_our, dt_our);
                        preds[ki] = (deg_ref, pi_ref, deg_our, pi_our);
                    }
                    obs.push(WaveObs {
                        design: tag.to_string(),
                        label: format!("{} -> {} {}", row.from, row.to, row.out),
                        tin: row.input_slew,
                        total,
                        ref_ceff: row.ceff,
                        ref_wave: row.driver_slew,
                        tbl_ceff_slew: tbl_ceff_s,
                        tbl_tot_slew: tbl_tot_s,
                        dsl_tot,
                        our_ceff: r.ceff,
                        our_slew: r.slew,
                        our_wave,
                        preds,
                    });
                },
            );
            assert!(matched > 0, "{tag}: no matched rows");
        }

        // Check repro_extreme_ceff sweep rows as well.
        {
            let dir = design_dir("repro_extreme_ceff");
            let rows = parse_repro_sweep(
                &std::fs::read_to_string(dir.join("sweep_reference.txt")).unwrap(),
            );
            let lib_dir = sky130_hd();
            let lib = load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).unwrap();
            let cell = lib
                .cells
                .iter()
                .find(|c| lib.strings.resolve(c.name) == "sky130_fd_sc_hd__nor2_2")
                .unwrap();
            let arc = lib
                .cell_arcs(cell)
                .iter()
                .find(|a| {
                    lib.strings.resolve(lib.pins[a.related_pin.0 as usize].name) == "A"
                        && lib.strings.resolve(lib.pins[a.target_pin.0 as usize].name) == "Y"
                        && a.timing_type == TimingType::Combinational
                })
                .unwrap();
            for row in &rows {
                let pi = PiModel {
                    c1: row.near_c2,
                    rpi: row.rpi,
                    c2: row.far_c1,
                };
                let total = pi.total();
                let (Some(dtbl), Some(stbl)) = delay_tables(arc, row.out_pol) else {
                    continue;
                };
                let tin = row.input_slew as f32;
                let table = |c: f64| {
                    (
                        eval_nldm_delay_or_slew(dtbl, tin, c as f32) as f64,
                        eval_nldm_delay_or_slew(stbl, tin, c as f32) as f64,
                    )
                };
                let dsl_tot = dslope(&table, total);
                let tbl_ceff_s = table(row.ceff).1;
                let tbl_tot_s = table(total).1;
                let r = solve_dmp(&pi, RD_SCALE, &table);
                let our_wave = driver_waveform_slew(&pi, &r, &table);
                let dsl_ref = dslope(&table, row.ceff);
                let dsl_our = dslope(&table, r.ceff);
                let mut preds = [(false, 0.0, false, 0.0); 3];
                for (ki, &(_, kw)) in k_candidates.iter().enumerate() {
                    let rd_ref = kw * dsl_ref;
                    let (dt_ref, deg_ref) = fit_ramp_duration(rd_ref, row.ceff, tbl_ceff_s);
                    let pi_ref = pi_waveform_slew(&pi, rd_ref, dt_ref);
                    let rd_our = kw * dsl_our;
                    let (dt_our, deg_our) = fit_ramp_duration(rd_our, r.ceff, r.slew);
                    let pi_our = pi_waveform_slew(&pi, rd_our, dt_our);
                    preds[ki] = (deg_ref, pi_ref, deg_our, pi_our);
                }
                obs.push(WaveObs {
                    design: "repro_sweep".to_string(),
                    label: format!(
                        "A -> Y {} tin={:.3}",
                        if row.out_pol == RISE { "rise" } else { "fall" },
                        row.input_slew
                    ),
                    tin: row.input_slew,
                    total,
                    ref_ceff: row.ceff,
                    ref_wave: row.driver_slew,
                    tbl_ceff_slew: tbl_ceff_s,
                    tbl_tot_slew: tbl_tot_s,
                    dsl_tot,
                    our_ceff: r.ceff,
                    our_slew: r.slew,
                    our_wave,
                    preds,
                });
            }
        }

        // 1. Audit the extreme-load bailout condition across ALL rows.
        let mut bailout_rows = Vec::new();
        let mut bailout_shielded = 0usize;
        for o in &obs {
            let ratio = o.tbl_tot_slew / (o.dsl_tot * o.total).max(1e-12);
            if ratio < bail_thresh {
                if o.ref_ceff < 0.99 * o.total {
                    bailout_shielded += 1;
                }
                bailout_rows.push(format!(
                    "BAILOUT_CHECK {} {} | tin {:.3} total {:.4} ratio {:.4} (< 2ln(5/4)={:.4}) | refCe/tot {:.4}",
                    o.design, o.label, o.tin, o.total, ratio, bail_thresh, o.ref_ceff / o.total
                ));
            }
        }
        println!(
            "=== Bailout check across {} rows: triggered {} times (shielded <0.99: {}) ===",
            obs.len(),
            bailout_rows.len(),
            bailout_shielded
        );
        for line in bailout_rows.iter().take(40) {
            println!("{line}");
        }
        // Also find minimum non-bailout ratio across the *_all sets to see the margin.
        let mut min_ratio_by_shield: Vec<(f64, String)> = obs
            .iter()
            .filter(|o| o.tbl_tot_slew / (o.dsl_tot * o.total).max(1e-12) >= bail_thresh)
            .map(|o| {
                let ratio = o.tbl_tot_slew / (o.dsl_tot * o.total).max(1e-12);
                (
                    ratio,
                    format!(
                        "{} {} tin={:.3} tot={:.4} refCe/tot={:.4} ratio={:.4}",
                        o.design,
                        o.label,
                        o.tin,
                        o.total,
                        o.ref_ceff / o.total,
                        ratio
                    ),
                )
            })
            .collect();
        min_ratio_by_shield.sort_by(|a, b| a.0.total_cmp(&b.0));
        println!("=== Lowest non-bailout slw/(dsl*tot) ratios ===");
        for (_, line) in min_ratio_by_shield.iter().take(15) {
            println!("  {line}");
        }

        // 2. Stratify waveform slew error on the 5 main designs (`picorv32`, `aes`, `ibex`, `jpeg`, `gcd`).
        let abs_stats = |errs: &[f64]| -> (f64, f64, f64) {
            let mut abs: Vec<f64> = errs.iter().map(|e| e.abs()).collect();
            abs.sort_by(f64::total_cmp);
            (
                median(abs.clone()),
                abs[9 * abs.len() / 10],
                abs[abs.len() - 1],
            )
        };
        let summarize = |errs: &[f64]| -> String {
            if errs.is_empty() {
                return "n=0".to_string();
            }
            let (med, p90, max) = abs_stats(errs);
            let mut sgn = errs.to_vec();
            sgn.sort_by(f64::total_cmp);
            format!(
                "n={} abs(med={:.2}%, p90={:.2}%, max={:.2}%) signed(p10={:+.2}%, med={:+.2}%, p90={:+.2}%)",
                errs.len(),
                med * 100.0,
                p90 * 100.0,
                max * 100.0,
                sgn[sgn.len() / 10] * 100.0,
                median(sgn.clone()) * 100.0,
                sgn[9 * sgn.len() / 10] * 100.0,
            )
        };

        type StatsRow<'a> = (&'a str, Option<f64>, (f64, f64, f64));
        let mut jpeg_rule2_top: Vec<(f64, String)> = Vec::new();
        let mut selected_stats: Vec<StatsRow<'_>> = Vec::new();
        let mut prod_stats: Vec<StatsRow<'_>> = Vec::new();
        let main_designs = ["picorv32", "aes", "ibex", "jpeg", "gcd"];
        for design in main_designs {
            let sub: Vec<&WaveObs> = obs.iter().filter(|o| o.design == design).collect();
            let base_all: Vec<f64> = sub
                .iter()
                .map(|o| (o.tbl_ceff_slew - o.ref_wave) / o.ref_wave)
                .collect();
            let base_sh: Vec<f64> = sub
                .iter()
                .filter(|o| o.ref_ceff < 0.97 * o.total)
                .map(|o| (o.tbl_ceff_slew - o.ref_wave) / o.ref_wave)
                .collect();
            println!(
                "\n=== WAVE_FIT {design} (n={}) | baseline table(refCe): all [{}] | shielded [{}] ===",
                sub.len(),
                summarize(&base_all),
                summarize(&base_sh)
            );
            for (ki, &(kname, _)) in k_candidates.iter().enumerate() {
                for rule_name in ["Rule1(deg)", "Rule1b(deg|sh)", "Rule2(all_pi)"] {
                    let mut all_ref = Vec::new();
                    let mut deg_ref = Vec::new();
                    let mut nondeg_ref = Vec::new();
                    let mut sh_ref = Vec::new();
                    let mut unsh_ref = Vec::new();
                    let mut all_our = Vec::new();
                    for o in &sub {
                        let (d_r, p_r, d_o, p_o) = o.preds[ki];
                        let is_sh_ref = o.ref_ceff < 0.97 * o.total;
                        let is_sh_our = o.our_ceff < 0.97 * o.total;
                        let pick = |d: bool, sh: bool, pi_s: f64, tbl_s: f64| match rule_name {
                            "Rule1(deg)" => {
                                if d {
                                    pi_s
                                } else {
                                    tbl_s
                                }
                            }
                            "Rule1b(deg|sh)" => {
                                if d || sh {
                                    pi_s
                                } else {
                                    tbl_s
                                }
                            }
                            _ => pi_s,
                        };
                        let pred_ref = pick(d_r, is_sh_ref, p_r, o.tbl_ceff_slew);
                        let err_ref = (pred_ref - o.ref_wave) / o.ref_wave;
                        all_ref.push(err_ref);
                        if d_r {
                            deg_ref.push(err_ref);
                        } else {
                            nondeg_ref.push(err_ref);
                        }
                        if is_sh_ref {
                            sh_ref.push(err_ref);
                        } else {
                            unsh_ref.push(err_ref);
                        }

                        let pred_our = if o.tbl_tot_slew < bail_thresh * o.dsl_tot * o.total {
                            o.tbl_tot_slew
                        } else {
                            pick(d_o, is_sh_our, p_o, o.our_slew)
                        };
                        let err_our = (pred_our - o.ref_wave) / o.ref_wave;
                        all_our.push(err_our);
                        if design == "jpeg" && ki == 0 && rule_name == "Rule2(all_pi)" {
                            jpeg_rule2_top.push((
                                err_our.abs(),
                                format!(
                                    "{} tin={:.3} tot={:.4} refCe={:.4} ourCe={:.4} pred={:.4} refWave={:.4} err={:+.2}%",
                                    o.label, o.tin, o.total, o.ref_ceff, o.our_ceff, pred_our, o.ref_wave, err_our * 100.0
                                ),
                            ));
                        }
                    }
                    println!(
                        "  k={kname:<5} {rule_name:<14} | refCe all [{}] | deg [{}] | nondeg [{}] | sh<0.97 [{}] | unsh [{}] | ourCe+bail [{}]",
                        summarize(&all_ref),
                        summarize(&deg_ref),
                        summarize(&nondeg_ref),
                        summarize(&sh_ref),
                        summarize(&unsh_ref),
                        summarize(&all_our)
                    );
                    if ki == 0 && rule_name == "Rule2(all_pi)" {
                        let sh_med = (!sh_ref.is_empty()).then(|| abs_stats(&sh_ref).0);
                        let prod_errs: Vec<f64> = sub
                            .iter()
                            .map(|o| (o.our_wave - o.ref_wave) / o.ref_wave)
                            .collect();
                        let prod_sh_errs: Vec<f64> = sub
                            .iter()
                            .filter(|o| o.ref_ceff < 0.97 * o.total)
                            .map(|o| (o.our_wave - o.ref_wave) / o.ref_wave)
                            .collect();
                        let prod_sh_med =
                            (!prod_sh_errs.is_empty()).then(|| abs_stats(&prod_sh_errs).0);
                        selected_stats.push((design, sh_med, abs_stats(&all_our)));
                        prod_stats.push((design, prod_sh_med, abs_stats(&prod_errs)));
                    }
                }
            }
        }
        jpeg_rule2_top.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("\n=== Top 5 jpeg ourCe+bail errors (k=ln2, Rule2(all_pi)) ===");
        for (_, line) in jpeg_rule2_top.iter().take(5) {
            println!("  {line}");
        }

        // Print extreme & sweep comparison as well.
        for tag in ["ibex_ext", "jpeg_ext", "repro_sweep"] {
            println!("\n=== {tag} ===");
            for o in obs.iter().filter(|o| o.design == tag) {
                let (_, _, d0, p0) = o.preds[0];
                let (_, _, d1, p1) = o.preds[1];
                let (_, _, d2, p2) = o.preds[2];
                let bail = o.tbl_tot_slew < bail_thresh * o.dsl_tot * o.total;
                println!(
                    "  {:<28} refCe={:.4}({:>3.0}%) refWave={:.4} tbl(ref)={:.4} ({:+.1}%) | k=ln2({}):{:.4}({:+.1}%) 1/ln4({}):{:.4}({:+.1}%) 0.75({}):{:.4}({:+.1}%) bail={}",
                    o.label,
                    o.ref_ceff,
                    o.ref_ceff / o.total * 100.0,
                    o.ref_wave,
                    o.tbl_ceff_slew,
                    (o.tbl_ceff_slew - o.ref_wave) / o.ref_wave * 100.0,
                    d0 as u8,
                    p0,
                    (p0 - o.ref_wave) / o.ref_wave * 100.0,
                    d1 as u8,
                    p1,
                    (p1 - o.ref_wave) / o.ref_wave * 100.0,
                    d2 as u8,
                    p2,
                    (p2 - o.ref_wave) / o.ref_wave * 100.0,
                    bail
                );
            }
        }

        assert_eq!(
            bailout_shielded, 0,
            "bailout must not trigger on any row with ref_ceff < 0.99 * total"
        );
        assert_eq!(
            bailout_rows.len(),
            14,
            "expected 14 bailout rows across jpeg, jpeg_ext, and repro_sweep"
        );
        for (design, sh_med, (med, p90, max)) in selected_stats {
            if design != "gcd" {
                let sm = sh_med.expect("shielded rows present");
                assert!(
                    sm < 0.01,
                    "{design}: Rule2 k=ln2 sh_ref median {sm:.4} >= 0.01"
                );
            }
            assert!(
                med < 0.03 && p90 < 0.04 && max < 0.16,
                "{design}: Rule2 k=ln2 ourCe+bail med={med:.4} p90={p90:.4} max={max:.4}"
            );
        }
        for (design, sh_med, (med, p90, max)) in prod_stats {
            if design != "gcd" {
                let sm = sh_med.expect("shielded rows present");
                assert!(
                    sm < 0.015,
                    "{design}: driver_waveform_slew shielded median {sm:.4} >= 0.015"
                );
            }
            assert!(
                med < 0.03 && p90 < 0.04 && max < 0.16,
                "{design}: driver_waveform_slew med={med:.4} p90={p90:.4} max={max:.4}"
            );
        }
    }

    /// Regression for the propagated-clock slew merge. `u_and` is a clock gate whose
    /// clock input is `A` and whose enable input is `B`; its `B -> X` falling arc
    /// produces a larger output slew than `A -> X`. OpenSTA merges every incoming arc
    /// at a vertex, so `u_and/X` — and the `u_lt/GATE_N` pin it drives — must see the
    /// enable-dominated slew, not the clock-arc-only slew `propagate_clock` used to
    /// compute. The ss_n40C_1v35 corner is where the ~6 ps launch-side residual showed.
    #[test]
    fn test_propagated_clock_gate_merges_enable_arc_slew() {
        let home = std::env::var("HOME").unwrap();
        let lib_dir = std::path::Path::new(&home).join(".ciel/sky130A/libs.ref/sky130_fd_sc_hd");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__ss_n40C_1v35.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(
            &design_dir("sta_edge").join("propagated_clock/propagated.def"),
            &tech,
            Some(&lib),
        )
        .expect("DEF");
        let sdc = load_sdc(&design_dir("sta_edge").join("propagated_clock/edge.sdc")).expect("SDC");
        let (_report, state, _pol) = run_sta_detail(&design, &lib, &sdc, None, true).expect("STA");
        let cx = Context {
            design: &design,
            lib: &lib,
            sdc: &sdc,
            parasitics: None,
            pi_models: vec![None; design.num_nets()],
            net_wire_loads: vec![None; design.num_nets()],
        };
        let mut skipped = SkipReport::default();
        let graph = build_graph(&cx, &mut skipped);
        let levelized = topological_levels(&cx, &graph, false).expect("levelized");
        let wire = vec![None; design.num_nets()];
        let unified = propagate_slew(&cx, &graph, &wire, &state).expect("slew");
        let find_pin = |name: &str| {
            (0..design.num_pins())
                .map(|i| PinId(i as u32))
                .find(|&p| cx.pin_name(p) == name)
                .unwrap_or_else(|| panic!("pin {name}"))
        };
        let x = find_pin("u_and/X");
        let gate_n = find_pin("u_lt/GATE_N");
        let x_net = design.pin_nets[x.0 as usize].unwrap();

        // The merged falling slew at u_and/X is the max over its input arcs' output
        // slews; the clock (A) arc alone must be smaller, proving the enable arc won.
        let mut arc_slews = Vec::new();
        let mut clock_only = f32::NEG_INFINITY;
        for &(src_pin, lib_arc) in levelized.incoming_to(x) {
            let arc = &lib.arcs[lib_arc as usize];
            let src = unified[src_pin.0 as usize];
            for &(src_pol, dst_pol) in polarities(arc.timing_sense) {
                if dst_pol != FALL {
                    continue;
                }
                let (Some(dt), Some(st)) = delay_tables(arc, dst_pol) else {
                    continue;
                };
                let out = cx
                    .driver_response(x_net, dst_pol, dt, st, src.slew_max[src_pol])
                    .1;
                arc_slews.push(out);
                if cx.pin_name(src_pin) == "u_and/A" {
                    clock_only = clock_only.max(out);
                }
            }
        }
        let merged = arc_slews.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            merged > clock_only,
            "enable arc should dominate: merged={merged} clock_only={clock_only}"
        );
        assert!(
            (unified[x.0 as usize].slew_max[FALL] - merged).abs() < 1e-6,
            "u_and/X slew {} != merged arc slew {merged}",
            unified[x.0 as usize].slew_max[FALL]
        );
        // Zero wire: the latch enable pin sees its clock-net driver's slew unchanged.
        assert!(
            (state[gate_n.0 as usize].slew_max[FALL] - unified[x.0 as usize].slew_max[FALL]).abs()
                < 1e-6,
            "u_lt/GATE_N slew {} != u_and/X slew {}",
            state[gate_n.0 as usize].slew_max[FALL],
            unified[x.0 as usize].slew_max[FALL]
        );

        // Sequential arcs are ordering edges in the slew pass: a register output
        // must levelize after the clock pin that launches it, or its slew would be
        // read before the clock network produced it.
        let seq_levelized = topological_levels(&cx, &graph, true).expect("seq levelized");
        // u_ff6/Q drives the y output net, so its clk -> Q arc is a slew dependency.
        let ff_clk = find_pin("u_ff6/CLK");
        let ff_q = find_pin("u_ff6/Q");
        assert!(
            pin_level(&seq_levelized, ff_q) > pin_level(&seq_levelized, ff_clk),
            "register Q must levelize after its clock pin"
        );
    }

    fn pin_level(levelized: &LevelizedGraph, pin: PinId) -> usize {
        for l in 0..levelized.level_offsets.len() - 1 {
            if levelized.level(l).contains(&pin) {
                return l;
            }
        }
        panic!("pin {pin:?} not levelized")
    }

    fn endpoint(pin: &str, max_slack: f32, min_slack: f32) -> EndpointReport {
        EndpointReport {
            pin: pin.to_string(),
            max_arr_ns: 0.0,
            max_req_ns: 0.0,
            max_slack_ns: max_slack,
            max_slew_ns: 0.0,
            min_arr_ns: 0.0,
            min_req_ns: 0.0,
            min_slack_ns: min_slack,
            min_slew_ns: 0.0,
        }
    }

    fn report(endpoints: Vec<EndpointReport>) -> StaReport {
        let wns_max = endpoints
            .iter()
            .map(|e| e.max_slack_ns)
            .fold(f32::INFINITY, f32::min);
        let tns_max = endpoints
            .iter()
            .map(|e| e.max_slack_ns)
            .map(|s| s.min(0.0))
            .sum();
        let wns_min = endpoints
            .iter()
            .map(|e| e.min_slack_ns)
            .fold(f32::INFINITY, f32::min);
        let tns_min = endpoints
            .iter()
            .map(|e| e.min_slack_ns)
            .map(|s| s.min(0.0))
            .sum();
        StaReport {
            wns_max_ns: wns_max,
            tns_max_ns: tns_max,
            wns_min_ns: wns_min,
            tns_min_ns: tns_min,
            setup_violations: endpoints.iter().filter(|e| e.max_slack_ns < 0.0).count() as u32,
            hold_violations: endpoints.iter().filter(|e| e.min_slack_ns < 0.0).count() as u32,
            endpoints,
            pulse_width_checks: Vec::new(),
            skipped: SkipReport::default(),
        }
    }

    #[test]
    fn test_merge_corner_reports_max_from_setup_min_from_hold() {
        let setup = report(vec![endpoint("a/Q", 0.5, 0.1), endpoint("b/Q", -0.2, 0.3)]);
        let hold = report(vec![endpoint("a/Q", 0.9, -0.4), endpoint("b/Q", 0.1, 0.2)]);
        let merged = merge_corner_reports(setup, hold).expect("merge");

        assert_eq!(merged.wns_max_ns, -0.2);
        assert_eq!(merged.wns_min_ns, -0.4);
        assert_eq!(merged.setup_violations, 1);
        assert_eq!(merged.hold_violations, 1);
        assert_eq!(merged.endpoints.len(), 2);
        let a = merged
            .endpoints
            .iter()
            .find(|e| e.pin == "a/Q")
            .expect("a/Q");
        assert_eq!(a.max_slack_ns, 0.5);
        assert_eq!(a.min_slack_ns, -0.4);
    }

    #[test]
    fn test_merge_corner_reports_rejects_different_endpoint_sets() {
        let setup = report(vec![endpoint("a/Q", 0.5, 0.1)]);
        let hold = report(vec![endpoint("b/Q", 0.9, -0.4)]);
        assert!(merge_corner_reports(setup, hold).is_err());
    }

    #[test]
    fn test_merge_pulse_rows_keeps_the_smaller_slack() {
        let row = |pin: &str, edge: PulseEdge, slack: f32| PulseWidthCheckReport {
            pin: pin.to_string(),
            edge,
            required_ns: 0.0,
            actual_ns: slack,
            slack_ns: slack,
        };
        let setup = vec![
            row("u/CLK", PulseEdge::High, 0.4),
            row("u/CLK", PulseEdge::Low, -0.1),
        ];
        let hold = vec![
            row("u/CLK", PulseEdge::High, 0.3),
            row("u/CLK", PulseEdge::Low, 0.2),
        ];
        let merged = merge_pulse_rows(setup, hold).expect("merge");
        assert_eq!(merged[0].slack_ns, 0.3, "high: hold wins");
        assert_eq!(merged[1].slack_ns, -0.1, "low: setup wins");

        let other = vec![row("v/CLK", PulseEdge::High, 0.3)];
        assert!(merge_pulse_rows(
            vec![row("u/CLK", PulseEdge::High, 0.4)],
            other
        )
        .is_err());
    }

    #[test]
    fn test_merge_skip_reports_element_wise_max() {
        let a = SkipReport {
            unmapped_cells: 3,
            unsupported_arcs: BTreeMap::from([("Clear".to_string(), 5)]),
            skipped_endpoints: vec![SkippedEndpoint {
                pin: "x/Q".to_string(),
                kind: EndpointKind::Check,
                reason: SkipReason::NoClock,
                related: Some("clk".to_string()),
            }],
            ..Default::default()
        };
        let b = SkipReport {
            unmapped_cells: 7,
            unsupported_arcs: BTreeMap::from([("Clear".to_string(), 2), ("Preset".to_string(), 1)]),
            skipped_endpoints: vec![SkippedEndpoint {
                pin: "x/Q".to_string(),
                kind: EndpointKind::Check,
                reason: SkipReason::NoClock,
                related: Some("clk".to_string()),
            }],
            ..Default::default()
        };
        let merged = merge_skip_reports(a, b);
        assert_eq!(merged.unmapped_cells, 7);
        assert_eq!(merged.unsupported_arcs["Clear"], 5);
        assert_eq!(merged.unsupported_arcs["Preset"], 1);
        assert_eq!(merged.skipped_endpoints.len(), 1);
    }

    /// `tests/designs/sta_exceptions`: a three-flop DEF (muxed `u_ff1/D`,
    /// a second mux driving `y2` from the same two FF sources, split clock
    /// buffers) plus 111 SDC scenarios.
    /// `exceptions.reference.json` was emitted by the committed
    /// `generate_ref.sh` running OpenSTA inside the pinned LibreLane image.
    /// Four scenarios carry `error` records: a combined `-setup X -hold Y`
    /// on one command trips OpenSTA's Tcl wrapper (`mc_setup2_hold1`), and
    /// the `-both` (`mc_both`), `-early` (`md_early`) and `-datapath_only`
    /// (`md_datapath`) qualifiers are rejected `STA-0563`; our parser
    /// accepts all of them, so they are checked for diagnostics instead of
    /// numbers.
    #[test]
    fn test_sta_exceptions_match_opensta() {
        let lib_dir = sky130_hd();
        let dir = design_dir("sta_exceptions");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("exceptions.def"), &tech, Some(&lib)).expect("DEF");
        let reference: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.join("exceptions.reference.json")).expect("golden file"),
        )
        .expect("golden JSON");
        let scenarios = reference["scenarios"].as_object().expect("scenarios");

        const FIELDS: [&str; 8] = [
            "max_arr_ns",
            "max_req_ns",
            "max_slack_ns",
            "max_slew_ns",
            "min_arr_ns",
            "min_req_ns",
            "min_slack_ns",
            "min_slew_ns",
        ];
        let mut compared = 0;
        for (name, golden) in scenarios {
            if golden.get("error").is_some() {
                continue;
            }
            let sdc = load_sdc(&dir.join(format!("{name}.sdc"))).expect("SDC");
            let (report, _, _) =
                run_sta_detail(&design, &lib, &sdc, None, true).expect("partitioned STA");
            compared += 1;
            let ref_endpoints = golden["endpoints"].as_array().expect("endpoints");
            assert_eq!(
                report.endpoints.len(),
                ref_endpoints.len(),
                "{name}: endpoint count"
            );
            for r in ref_endpoints {
                let pin = r["pin"].as_str().unwrap();
                let ep = report
                    .endpoints
                    .iter()
                    .find(|e| e.pin == pin)
                    .unwrap_or_else(|| panic!("{name}: missing endpoint {pin}"));
                let ours = [
                    ep.max_arr_ns,
                    ep.max_req_ns,
                    ep.max_slack_ns,
                    ep.max_slew_ns,
                    ep.min_arr_ns,
                    ep.min_req_ns,
                    ep.min_slack_ns,
                    ep.min_slew_ns,
                ];
                for (i, field) in FIELDS.iter().enumerate() {
                    let expected = r[*field].as_f64().unwrap();
                    let (ok, rel) = within_tolerance(ours[i], expected, 0.01, 0.005);
                    assert!(
                        ok,
                        "{name} {pin}.{field}: ours={} ref={expected} (rel {rel:.2e})",
                        ours[i]
                    );
                }
            }
            for (ours, key) in [
                (report.wns_max_ns, "wns_max_ns"),
                (report.tns_max_ns, "tns_max_ns"),
                (report.wns_min_ns, "wns_min_ns"),
                (report.tns_min_ns, "tns_min_ns"),
            ] {
                let expected = golden[key].as_f64().unwrap();
                let (ok, _) = within_tolerance(ours, expected, 0.01, 0.005);
                assert!(ok, "{name} {key}: ours={ours} ref={expected}");
            }
            assert_eq!(
                report.setup_violations as u64,
                golden["setup_violations"].as_u64().unwrap(),
                "{name}: setup violations"
            );
            assert_eq!(
                report.hold_violations as u64,
                golden["hold_violations"].as_u64().unwrap(),
                "{name}: hold violations"
            );
        }
        let expected = scenarios
            .values()
            .filter(|g| g.get("error").is_none())
            .count();
        assert_eq!(
            compared, expected,
            "every non-error scenario must be compared"
        );
    }

    /// Exception diagnostics the golden cannot express: false-path endpoint
    /// removal is counted (and exempt from `is_clean`), unresolvable selector
    /// names/sides are counted, and `-both` lands in `ignored_commands`.
    #[test]
    fn test_sta_exception_diagnostics() {
        let lib_dir = sky130_hd();
        let dir = design_dir("sta_exceptions");
        let lib =
            load_liberty(&lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib")).expect("Liberty");
        let mut interner = StringInterner::new();
        let tech = load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0;
        let design = parse_def(&dir.join("exceptions.def"), &tech, Some(&lib)).expect("DEF");

        let run = |sdc_name: &str| {
            let sdc = load_sdc(&dir.join(format!("{sdc_name}.sdc"))).expect("SDC");
            run_sta_detail(&design, &lib, &sdc, None, false).expect("STA")
        };

        // u_ff0 -> u_ff1 false path covers only the u_ff0 launch source: the
        // mux adds u_ff2 and sel launches, so u_ff1/D stays (partial removal,
        // counted only when every candidate drops).
        let (report, _, _) = run("fp_uf0_uf1");
        assert_eq!(report.skipped.false_path_endpoints, 0);
        assert!(report.endpoints.iter().any(|e| e.pin == "u_ff1/D"));

        // `-to [get_ports y]` covers the only launch reaching y, so the
        // endpoint drops entirely — full-removal coverage.
        let (report, _, _) = run("fp_to_y");
        assert_eq!(report.skipped.false_path_endpoints, 1);
        assert!(!report.endpoints.iter().any(|e| e.pin == "y"));

        // `-from [all_inputs]`-style port false paths remove both port-launched
        // endpoints; the register-launched one survives.
        let (report, _, _) = run("fp_from_din");
        assert_eq!(report.skipped.false_path_endpoints, 2);
        assert!(report.endpoints.iter().any(|e| e.pin == "u_ff1/D"));

        // Unknown instance names count once per name and once per emptied
        // selector side; the resolvable multicycle still applies.
        let (report, _, _) = run("exc_diag");
        assert_eq!(report.skipped.unresolved_exception_selectors, 4);
        assert_eq!(report.skipped.false_path_endpoints, 0);
        let ff1 = report
            .endpoints
            .iter()
            .find(|e| e.pin == "u_ff1/D")
            .expect("u_ff1/D");
        // The `-setup 2` shift applies to the u_ff0->u_ff1 candidate, but the
        // report keeps the min-slack candidate across launch sources: an
        // unshifted u_ff2/sel clock check beats the shifted 4.550057.
        assert!((ff1.max_req_ns - 2.550057).abs() < 0.005);

        // `-both` is valid SDC OpenSTA rejects; it is counted ignored, not applied.
        let (report, _, _) = run("mc_both");
        assert_eq!(
            report
                .skipped
                .sdc_ignored_commands
                .get("set_multicycle_path"),
            Some(&1)
        );
        assert!(!report.skipped.is_clean());

        // `set_load` load selectors: a missing net name is counted as an
        // unresolved load selector while the resolvable net still applies;
        // pin/cell/fanout selections count under `sdc_ignored_commands`.
        let (report, _, _) = run("ld_badnet");
        assert_eq!(report.skipped.unresolved_load_selectors, 1);
        assert!(!report.skipped.is_clean());
        let (report, _, _) = run("ld_net");
        assert_eq!(report.skipped.unresolved_load_selectors, 0);
        assert!(!report.skipped.sdc_ignored_commands.contains_key("set_load"));
        for (name, want) in [
            ("ld_pins", 1),
            ("ld_pin_sink", 1),
            ("ld_pin_drv", 1),
            ("ld_badpin", 1),
            ("ld_cells", 1),
            ("ld_afo", 1),
            ("ld_afi", 1),
        ] {
            let (report, _, _) = run(name);
            assert_eq!(
                report.skipped.sdc_ignored_commands.get("set_load"),
                Some(&want),
                "{name}: set_load ignored count"
            );
        }
        // `set_fanout_load` is accepted and inert in OpenSTA (STA-0461);
        // counted ignored, timing identical to base.
        let (report, _, _) = run("ld_fo");
        assert_eq!(
            report.skipped.sdc_ignored_commands.get("set_fanout_load"),
            Some(&1)
        );
    }
}
