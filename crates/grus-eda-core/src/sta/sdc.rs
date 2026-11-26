use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum PortSel {
    AllInputs {
        no_clocks: bool,
    },
    AllOutputs,
    Ports(Vec<String>),
    /// `remove_from_collection` on a predicate selection: the base minus `excluded` names.
    Except {
        base: Box<PortSel>,
        excluded: Vec<String>,
    },
}

impl PortSel {
    /// `sdc` is needed because `-no_clocks` excludes every clock source, which the selector cannot see.
    pub fn matches(&self, port: &str, is_input: bool, sdc: &Sdc) -> bool {
        match self {
            PortSel::AllInputs { no_clocks } => {
                is_input && !(*no_clocks && sdc.is_clock_port(port))
            }
            PortSel::AllOutputs => !is_input,
            PortSel::Ports(names) => names.iter().any(|n| n == port),
            PortSel::Except { base, excluded } => {
                base.matches(port, is_input, sdc) && !excluded.iter().any(|n| n == port)
            }
        }
    }

    /// The selection minus the named ports. An explicit list filters in place; a predicate
    /// base (`all_inputs`, `all_outputs`, or an existing `Except`) gains the names as
    /// exclusions, which `matches` applies on top.
    fn without(self, removed: &[String]) -> PortSel {
        if removed.is_empty() {
            return self;
        }
        match self {
            PortSel::Ports(mut names) => {
                names.retain(|n| !removed.contains(n));
                PortSel::Ports(names)
            }
            PortSel::Except { base, mut excluded } => {
                excluded.extend(removed.iter().cloned());
                PortSel::Except { base, excluded }
            }
            other => PortSel::Except {
                base: Box::new(other),
                excluded: removed.to_vec(),
            },
        }
    }
}

/// Cell-space object selection for timing exceptions (`get_cells`,
/// `all_registers`), symbolic until the design is known: instance names
/// resolve against the DEF after parsing.
#[derive(Clone, Debug)]
pub enum CellSel {
    /// Explicit instance names (`get_cells`, `get_instance`, `get_inst`).
    Cells(Vec<String>),
    /// `all_registers`: every sequential cell instance.
    AllRegisters,
    /// `remove_from_collection` on a predicate selection: the base minus
    /// `excluded` instance names.
    Except {
        base: Box<CellSel>,
        excluded: Vec<String>,
    },
}

impl CellSel {
    /// The selection minus the named instances. Mirrors `PortSel::without`.
    fn without(self, removed: &[String]) -> CellSel {
        if removed.is_empty() {
            return self;
        }
        match self {
            CellSel::Cells(mut names) => {
                names.retain(|n| !removed.contains(n));
                CellSel::Cells(names)
            }
            CellSel::Except { base, mut excluded } => {
                excluded.extend(removed.iter().cloned());
                CellSel::Except { base, excluded }
            }
            other => CellSel::Except {
                base: Box::new(other),
                excluded: removed.to_vec(),
            },
        }
    }
}

/// A `-from`/`-to` object selection of a timing exception, kept symbolic:
/// ports, cells, pins, or bare names resolved against both namespaces once
/// the design is loaded.
#[derive(Clone, Debug)]
pub enum ObjSel {
    Ports(PortSel),
    Cells(CellSel),
    /// `get_pins` or bare `instance/pin` specs. Measured OpenSTA semantics:
    /// only sequential cells' clock pins (and latch enables) are launch
    /// objects, and only endpoint pins are capture objects — a `Q`/`D` pin
    /// spec resolves but covers nothing.
    Pins(Vec<String>),
    /// `get_clocks`/`all_clocks` collections, resolved to `Sdc::clocks`
    /// row indices at parse time (the `InterClockUnc::from`/`to`
    /// convention). Unknown names drop out at resolution — measured
    /// OpenSTA semantics (`t2_selunk`): `[get_clocks nope]` warns and
    /// the selector covers the empty set, so an all-miss list reaches
    /// the resolver empty and is diagnosed there.
    Clocks(Vec<usize>),
    /// Bare words (`-from u_ff0`): matched against port and cell names at
    /// resolution time. An empty list came from an empty selector and must
    /// be diagnosed at resolution, not treated as absent.
    Names(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionKind {
    FalsePath,
    Multicycle,
}

/// Which side of the check a `set_max_delay`/`set_min_delay` constraint applies to:
/// `Max` replaces the setup (late) clock check, `Min` the hold (early) one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathDelayKind {
    Max,
    Min,
}

/// One `set_max_delay`/`set_min_delay` command, in file order. `from`/`to` are
/// `None` when the flag was absent (every launch source / every endpoint).
/// `value_ns` is the signed positional delay; overlapping rules of the same
/// kind fold last-wins in file order (measured on OpenSTA, `md_overlap*`).
#[derive(Clone, Debug)]
pub struct PathDelay {
    pub kind: PathDelayKind,
    pub from: Option<ObjSel>,
    pub to: Option<ObjSel>,
    pub value_ns: f32,
}

/// One `set_false_path`/`set_multicycle_path` command, in file order.
/// `from`/`to` are `None` when the flag was absent (every launch source /
/// every endpoint). Multicycle counts follow the measured OpenSTA
/// convention: `setup_n` defaults to 1 and `hold_n` to 0 when absent, and
/// `hold_n` accepts any integer (0 = default, negative tightens); `setup_n`
/// requires >= 1.
#[derive(Clone, Debug)]
pub struct TimingException {
    pub kind: ExceptionKind,
    pub from: Option<ObjSel>,
    pub to: Option<ObjSel>,
    pub setup_n: Option<u32>,
    pub hold_n: Option<i32>,
}

/// One `set_clock_uncertainty -from/-to` rule, in file order. Both sides are
/// required — OpenSTA bails on a lone flag (`STA-0421`) — and a side that
/// resolves to an empty collection makes the rule a no-op, so stored rows
/// are never empty.
#[derive(Clone, Debug)]
pub struct InterClockUnc {
    /// `-from` rows (launch side).
    pub from: Vec<usize>,
    /// `-to` rows (capture side).
    pub to: Vec<usize>,
    /// `-setup` value — `None` when the command gave `-hold`.
    pub setup_ns: Option<f32>,
    /// `-hold` value — `None` when the command gave `-setup`.
    pub hold_ns: Option<f32>,
}

/// Index into `Sdc::clocks`. Private: `DelayRule` is private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClockId(usize);

/// Which analysis corner a delay applies to. `Both` is a plain `set_input_delay`; `Min`/`Max`
/// come from the `-min`/`-max` flags and feed the early/late checks respectively.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DelaySense {
    Both,
    Min,
    Max,
}

impl DelaySense {
    /// The corners `self` keeps after a rule covering `covered` lands on the
    /// same anchor — `None` when nothing remains. A `Both` entry under a
    /// single-corner rule degrades to the uncovered corner rather than
    /// dropping (measured `t2_clkdup`: `0.5 -clock ckb` then `-max 0.3 -clock
    /// ckb` keeps the 0.5 min check).
    fn without(self, covered: DelaySense) -> Option<DelaySense> {
        match (self, covered) {
            (DelaySense::Both, DelaySense::Min) => Some(DelaySense::Max),
            (DelaySense::Both, DelaySense::Max) => Some(DelaySense::Min),
            (s, c) if s == c || c == DelaySense::Both => None,
            (s, _) => Some(s),
        }
    }
}

#[derive(Clone, Debug)]
struct DelayRule {
    sel: PortSel,
    value_ns: f32,
    /// `-clock` anchor row; `None` = no declared anchor — the entry
    /// produces no timed check on either corner (measured `t2_noclk`).
    clock: Option<ClockId>,
    sense: DelaySense,
    /// `-add_delay`: append to the port's entry list instead of pruning it.
    add: bool,
}

/// One delay entry surviving on a port: the declared value and the `-clock`
/// anchor. An anchored entry is its own check on the anchor clock's grid — a
/// corner can carry several under different clocks (measured `t2_clkcor2`: a
/// `-max -clock cka` rule followed by `-max -clock ckb -add_delay` reports
/// the port once per anchor and the golden keeps the worst slack). Entries
/// sharing the anchor share per-corner slots: a later plain rule overwrites
/// the corner(s) it covers (measured `t2_clkdup`).
#[derive(Clone, Copy, Debug)]
struct DelayEntry {
    value_ns: f32,
    clock: Option<ClockId>,
    sense: DelaySense,
}

/// `set_driving_cell`: the cell and its output pin that drive a port, with the input transition
/// the driver sees for each input polarity. The driver itself is ideal: it contributes slew,
/// not delay.
#[derive(Clone, Debug)]
pub struct DrivingCell {
    pub cell: String,
    pub pin: String,
    pub in_trans_rise: f32,
    pub in_trans_fall: f32,
}

/// Delay classes `set_timing_derate` selects with `-cell_delay`/`-net_delay`/`-cell_check`.
pub const DERATE_CELL: usize = 0;
pub const DERATE_NET: usize = 1;
pub const DERATE_CHECK: usize = 2;

/// One analysis corner of `set_timing_derate`: factors indexed by
/// `[kind][clock_path][transition]` — `clock_path` is 0 for clock-network delays and 1 for
/// data-path delays, `transition` is RISE (0) / FALL (1). A command overwrites only the
/// subset its flags select; the rest keep their previous value (measured on OpenSTA:
/// repeated commands are last-wins per category).
#[derive(Clone, Copy, Debug)]
pub struct DerateCorner {
    pub factors: [[[f32; 2]; 2]; 3],
}

impl Default for DerateCorner {
    fn default() -> Self {
        Self {
            factors: [[[1.0; 2]; 2]; 3],
        }
    }
}

/// Object sets `set_load` can annotate, resolved once the design is known.
/// Measured on OpenSTA (`docs/results.md` 2026-11-19): a net selection sets the
/// net's wire capacitance attribute, a domain separate from pin capacitance;
/// `get_pins`/`get_cells` objects are rejected outright (`STA-0100`), so those
/// commands are counted in `ignored_commands` instead of stored here.
#[derive(Clone, Debug)]
pub enum LoadSel {
    Ports(PortSel),
    /// `get_nets` name list; resolved against `DesignDb::net_names` at STA time.
    Nets(Vec<String>),
}

/// One `set_load` command, in file order. Net selections are kept symbolic
/// until the design is known; for the same measured target a later command
/// replaces the earlier annotation.
#[derive(Clone, Debug)]
pub struct LoadRule {
    pub sel: LoadSel,
    pub value_pf: f32,
}

/// One `create_clock` row: a design clock when `port` names a source port, a virtual
/// I/O clock when `port` is `None`.
#[derive(Clone, Debug, Default)]
pub struct Clock {
    /// `-name`, or the source port name when `-name` is absent — the handle
    /// `get_clocks`/`all_clocks` resolve.
    pub name: String,
    /// Source port of a design clock; `None` for a virtual clock.
    pub port: Option<String>,
    pub period_ns: f32,
    /// `set_clock_latency` — ideal network latency; a propagated clock ignores it.
    pub latency_ns: f32,
    /// `set_clock_latency -source`; sums with the network latency for a non-propagated
    /// clock, is the only latency applied when propagated.
    pub source_latency_ns: f32,
    /// `set_clock_uncertainty -setup`: subtracted from required times.
    pub setup_uncertainty_ns: f32,
    /// `set_clock_uncertainty -hold`: added to required times.
    pub hold_uncertainty_ns: f32,
    /// `set_clock_transition`: slew seeded on clock pins; 0 without the command.
    pub transition_ns: f32,
    /// `create_clock -waveform {rise fall}` — edge instants within the
    /// period. `None` is the default 50% duty cycle `[0, P/2]`; see
    /// `Clock::edge_offset`.
    pub waveform_ns: Option<[f32; 2]>,
    /// `set_propagated_clock` on this domain.
    pub propagated: bool,
}

impl Clock {
    /// Time within the period of the edge with index parity `p` (0 =
    /// rise, 1 = fall): the `-waveform` instant, or `p · P/2` for the
    /// default 50% duty cycle.
    pub fn edge_offset(&self, p: usize) -> f32 {
        self.waveform_ns.map_or(0.5 * self.period_ns * p as f32, |w| w[p])
    }
}

/// A `set_clock_groups` exclusivity mode. All three cut the same
/// launch/capture pairs on the committed probe (`t2_grpl`/`t2_grpp` =
/// `t2_grp`) — the model keeps the declared mode but shares the eval
/// path; `docs/multi-clock.md` records the measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockGroupKind {
    Asynchronous,
    LogicallyExclusive,
    PhysicallyExclusive,
}

/// One `set_clock_groups` command's resolved clock rows, per `-group`
/// in command order. A clock is cut from every row not in a group with
/// it — a lone `-group` cuts its members against all other clocks
/// (measured `t2_grp1`), so group membership, not pairwise group
/// matching, is what matters.
#[derive(Clone, Debug)]
pub struct ClockGroup {
    pub kind: ClockGroupKind,
    /// Resolved `Sdc::clocks` rows per `-group` argument.
    pub groups: Vec<Vec<usize>>,
}

/// The SDC subset the benchmark flow uses. Commands are applied in file order, so a later
/// command overrides an earlier one on the same port.
#[derive(Clone, Debug, Default)]
pub struct Sdc {
    /// One row per `create_clock`, in file order. Eval is single-domain: the first
    /// port-bearing row is the design clock.
    pub clocks: Vec<Clock>,
    /// Early/late derating of propagated delays (`set_timing_derate`), per delay kind,
    /// clock/data path and transition. Unset entries stay at 1.0 (no derate).
    pub derate_early: DerateCorner,
    pub derate_late: DerateCorner,
    input_delays: Vec<DelayRule>,
    output_delays: Vec<DelayRule>,
    input_transitions: Vec<(PortSel, f32)>,
    loads: Vec<LoadRule>,
    driving_cells: Vec<(PortSel, DrivingCell)>,
    /// Timing exceptions (`set_false_path`, `set_multicycle_path`) in file
    /// order, with symbolic selectors resolved against the design at STA time.
    /// Commands with unsupported qualifiers or unresolvable selectors are not
    /// stored here; they are counted in `ignored_commands` instead.
    pub exceptions: Vec<TimingException>,
    /// Path-delay constraints (`set_max_delay`, `set_min_delay`) in file
    /// order, same symbolic-selector treatment as `exceptions`.
    pub path_delays: Vec<PathDelay>,
    /// `set_clock_uncertainty -from/-to` rules, in file order.
    pub inter_clock_uncertainty: Vec<InterClockUnc>,
    /// `set_clock_groups` declarations, in file order. `-allow_paths`
    /// commands store nothing — measured no-op (`t2_grpa` kept all
    /// eight endpoints).
    pub clock_groups: Vec<ClockGroup>,
    /// Options the parser accepts but does not apply, counted by flag or by descriptive key.
    /// `-lib_cell` and `-pin` restrict a delay to one cell or pin; the model has no per-arc
    /// endpoint matching, so the restriction is dropped and the delay applies to every
    /// selected port.
    pub ignored_options: BTreeMap<String, usize>,
    /// Commands accepted but not applied to the timing model (`set_max_*`,
    /// `set_fanout_load`, unresolvable exception/delay forms), by command name.
    pub ignored_commands: BTreeMap<String, usize>,
    /// Top-level commands evaluated while parsing; the denominator for `ignored_options`.
    pub commands: usize,
}

/// Folds the rules matching `port` in file order into the surviving entry
/// list. A rule without `-add_delay` *replaces*: it removes every entry
/// anchored to a different `-clock` — port-wide, both corners (measured
/// `t2_clkcor`: a `-clock`-less `-min` after `-max -clock cka` leaves no
/// anchored entry on either corner, so the port times nowhere; `t2_clkcor2`
/// in the opposite order leaves only the later anchored entry) — and
/// overwrites the corner(s) it covers on same-anchor entries, one
/// (clock, corner) slot per anchor (measured `t2_clkdup`: `-max 0.7` then
/// `-max 0.4` on `cka` reports the 0.4 check; a `Both` entry under a `-max`
/// rule keeps only its min half). `-add_delay` appends without pruning
/// (measured `t2_clkcor2`'s `yb`). `-min`/`-max` rules cover one corner;
/// the unflagged form writes both.
fn delay_at(rules: &[DelayRule], port: &str, is_input: bool, sdc: &Sdc) -> Vec<DelayEntry> {
    let mut entries = Vec::new();
    for r in rules.iter().filter(|r| r.sel.matches(port, is_input, sdc)) {
        if !r.add {
            entries = entries
                .into_iter()
                .filter_map(|e: DelayEntry| {
                    if e.clock != r.clock {
                        return None; // different anchor: pruned port-wide (t2_clkcor)
                    }
                    e.sense.without(r.sense).map(|sense| DelayEntry { sense, ..e })
                })
                .collect();
        }
        entries.push(DelayEntry {
            value_ns: r.value_ns,
            clock: r.clock,
            sense: r.sense,
        });
    }
    entries
}

/// Splits an entry list into per-corner anchored `(row, value)` lists —
/// `[min, max]`; `DelaySense::Both` entries land on both corners. Entries
/// without `-clock` produce no check (`t2_noclk`) and are dropped here.
fn corner_entries(entries: Vec<DelayEntry>) -> [Vec<(usize, f32)>; 2] {
    let mut out: [Vec<(usize, f32)>; 2] = [Vec::new(), Vec::new()];
    for e in entries {
        if let Some(clk) = e.clock {
            if matches!(e.sense, DelaySense::Both | DelaySense::Min) {
                out[0].push((clk.0, e.value_ns));
            }
            if matches!(e.sense, DelaySense::Both | DelaySense::Max) {
                out[1].push((clk.0, e.value_ns));
            }
        }
    }
    out
}

/// `(min, max)` fold over an entry list — the min corner keeps the smallest
/// surviving value, the max corner the largest (the `-add_delay` widen
/// measured on gcd: `1.0 -add_delay` then `0.4 -add_delay` gives `(0.4,
/// 1.0)`). That fold is also each corner's worst check: an input's hold
/// slack shrinks with an earlier arrival and an output's with a smaller
/// delay, so both min sides take `min`; both max sides take `max`. Every
/// anchored entry still checks with its own value; the fold serves the
/// `set_max_delay`/`set_min_delay` rewrites and diagnostics. `(0.0, 0.0)`
/// for a corner with no covering entry.
fn corner_values(entries: Vec<DelayEntry>) -> (f32, f32) {
    let (mut min_v, mut max_v) = (f32::INFINITY, f32::NEG_INFINITY);
    for e in entries {
        if matches!(e.sense, DelaySense::Both | DelaySense::Min) {
            min_v = min_v.min(e.value_ns);
        }
        if matches!(e.sense, DelaySense::Both | DelaySense::Max) {
            max_v = max_v.max(e.value_ns);
        }
    }
    (
        if min_v.is_finite() { min_v } else { 0.0 },
        if max_v.is_finite() { max_v } else { 0.0 },
    )
}

fn last_value(rules: &[(PortSel, f32)], port: &str, is_input: bool, sdc: &Sdc) -> Option<f32> {
    rules
        .iter()
        .rev()
        .find(|(sel, _)| sel.matches(port, is_input, sdc))
        .map(|&(_, v)| v)
}

impl Sdc {
    /// First port-bearing row — the domain eval times against. `None` on an empty or
    /// virtual-only table (pre-`finish` state).
    pub fn design_clock(&self) -> Option<&Clock> {
        self.clocks.iter().find(|c| c.port.is_some())
    }

    /// Number of design (port-bearing) clock rows. The eval gate.
    pub fn design_clock_count(&self) -> usize {
        self.clocks.iter().filter(|c| c.port.is_some()).count()
    }

    /// A row by handle (`name`), or by source port. Used by tests and later tickets;
    /// parse-side code uses `resolve_clock`, which matches names only.
    pub fn clock(&self, name: &str) -> Option<&Clock> {
        self.clocks
            .iter()
            .find(|c| c.name == name || c.port.as_deref() == Some(name))
    }

    /// `port` is a source port of any design clock — the `-no_clocks` predicate and the
    /// data-seeding exclusion.
    pub fn is_clock_port(&self, port: &str) -> bool {
        self.clocks.iter().any(|c| c.port.as_deref() == Some(port))
    }

    /// The design clock's handle (`-name`, or the source port name).
    pub fn clock_name(&self) -> Option<&str> {
        self.design_clock().map(|c| c.name.as_str())
    }

    /// The design clock's source port.
    pub fn clock_port(&self) -> Option<&str> {
        self.design_clock().and_then(|c| c.port.as_deref())
    }

    /// The design clock's period; 0.0 when no design row exists.
    pub fn period_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.period_ns)
    }

    /// The design clock's `set_clock_uncertainty -setup` value.
    pub fn setup_uncertainty_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.setup_uncertainty_ns)
    }

    /// The design clock's `set_clock_uncertainty -hold` value.
    pub fn hold_uncertainty_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.hold_uncertainty_ns)
    }

    /// Uncertainty effective for a `launch -> capture` check: the last
    /// `-from`/`-to` rule matching both sides supplies that side's value — a
    /// **replacement** for the capture row's own field, not additive and not
    /// symmetric (measured `t2_uncx`: `-setup 0.30 -from cka -to ckb`
    /// replaces `ckb`'s 0.20 for A→B only). No match → the capture row's
    /// value. Applies uniformly, same-domain included (`-from cka -to cka`
    /// fires on `cka`'s own checks).
    pub fn uncertainty_ns(&self, launch: usize, capture: usize, hold: bool) -> f32 {
        for r in self.inter_clock_uncertainty.iter().rev() {
            let side_match = r.from.contains(&launch) && r.to.contains(&capture);
            let v = if hold { r.hold_ns } else { r.setup_ns };
            if side_match && let Some(v) = v {
                return v;
            }
        }
        self.clocks.get(capture).map_or(0.0, |c| {
            if hold {
                c.hold_uncertainty_ns
            } else {
                c.setup_uncertainty_ns
            }
        })
    }

    /// The design clock's `set_clock_latency` (network) value.
    pub fn clock_latency_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.latency_ns)
    }

    /// The design clock's `set_clock_latency -source` value.
    pub fn clock_source_latency_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.source_latency_ns)
    }

    /// `set_propagated_clock` on the design clock: clock arrival is propagated through
    /// the clock network instead of every clock pin seeing the ideal clock latency.
    pub fn propagated_clock(&self) -> bool {
        self.design_clock().is_some_and(|c| c.propagated)
    }

    /// The design clock's `set_clock_transition` slew; zero without the command.
    pub fn clock_transition_ns(&self) -> f32 {
        self.design_clock().map_or(0.0, |c| c.transition_ns)
    }

    /// Effective latency an I/O delay is referenced to: `source_latency` when the row is
    /// propagated, else `latency + source_latency`. `None` resolves to the design clock.
    /// OpenSTA sums source and network latency when the clock is not propagated
    /// (measured: `set_clock_latency 1.0` + `-source 0.2` reports 1.20 ns ideal delay).
    fn latency_ns(&self, clock: Option<ClockId>) -> f32 {
        let Some(c) = clock.map_or_else(|| self.design_clock(), |id| self.clocks.get(id.0))
        else {
            return 0.0;
        };
        // A propagated clock ignores the ideal network latency; `-source` latency is
        // what remains, and it is what I/O delays are referenced against.
        if c.propagated {
            c.source_latency_ns
        } else {
            c.latency_ns + c.source_latency_ns
        }
    }

    /// Latency every clock pin sees in ideal-clock mode: `-source` and network latencies
    /// sum (OpenSTA reports 1.20 ns for `set_clock_latency 1.0` + `-source 0.2`).
    pub fn design_clock_latency_ns(&self) -> f32 {
        self.design_clock()
            .map_or(0.0, |c| c.latency_ns + c.source_latency_ns)
    }

    /// Derate factor for one propagated delay. `late` selects the max-analysis corner,
    /// `kind` is a `DERATE_*` constant, `clock_path` marks delays on the clock network,
    /// and `transition` is RISE/FALL.
    pub fn derate(&self, late: bool, kind: usize, clock_path: bool, transition: usize) -> f32 {
        let corner = if late {
            &self.derate_late
        } else {
            &self.derate_early
        };
        corner.factors[kind][usize::from(!clock_path)][transition]
    }

    /// Index of the first design (port-bearing) clock row — the
    /// bookkeeping fallback [`Sdc::output_clock_row`] resolves to on a
    /// corner with no declared anchor. 0 on an empty or virtual-only
    /// table.
    fn design_row(&self) -> usize {
        self.clocks
            .iter()
            .position(|c| c.port.is_some())
            .unwrap_or(0)
    }

    /// The port's surviving `set_input_delay` entries per corner — `[min,
    /// max]` lists of `(clock row, delay value)`, one per anchored entry in
    /// file order. Each entry produces its own check on the anchor clock's
    /// grid (measured `t2_clkcor2`); an entry without `-clock` produces none
    /// (`t2_noclk`), so corners without an anchored entry stay empty.
    pub fn declared_input_clock_rows(&self, port: &str) -> [Vec<(usize, f32)>; 2] {
        corner_entries(delay_at(&self.input_delays, port, true, self))
    }

    /// Same as [`Sdc::declared_input_clock_rows`] for `set_output_delay`.
    pub fn declared_output_clock_rows(&self, port: &str) -> [Vec<(usize, f32)>; 2] {
        corner_entries(delay_at(&self.output_delays, port, false, self))
    }

    /// First anchored `set_output_delay -clock` row on `port`'s `corner` (0
    /// = min, 1 = max), else the `design_row` bookkeeping fallback — used
    /// where a check rewrite still needs one capture row on a port that can
    /// anchor on several (measured `t2_clkcor2`).
    pub fn output_clock_row(&self, port: &str, corner: usize) -> usize {
        self.declared_output_clock_rows(port)[corner]
            .first()
            .map_or_else(|| self.design_row(), |&(r, _)| r)
    }

    /// `(min, max)` input delay in ns — the fold over each corner's
    /// surviving entries (see `corner_values`); both equal the plain value
    /// when no `-min`/`-max` was given.
    pub fn input_delay_ns(&self, port: &str) -> (f32, f32) {
        corner_values(delay_at(&self.input_delays, port, true, self))
    }

    /// `(min, max)` output delay in ns — the same per-corner fold.
    pub fn output_delay_ns(&self, port: &str) -> (f32, f32) {
        corner_values(delay_at(&self.output_delays, port, false, self))
    }

    /// Effective latency of one `clocks` row — the I/O-delay reference of
    /// an anchored entry.
    pub fn clock_row_latency_ns(&self, row: usize) -> f32 {
        self.latency_ns(Some(ClockId(row)))
    }

    /// `(min, max)` latency of the port's declared input-delay anchors —
    /// the first anchored entry's clock per corner, 0.0 when the corner
    /// has none. Eval reads per-entry latencies through
    /// [`Sdc::clock_row_latency_ns`]; this accessor serves diagnostics.
    pub fn input_clock_latency_ns(&self, port: &str) -> (f32, f32) {
        let rows = self.declared_input_clock_rows(port);
        (
            rows[0].first().map_or(0.0, |&(r, _)| self.clock_row_latency_ns(r)),
            rows[1].first().map_or(0.0, |&(r, _)| self.clock_row_latency_ns(r)),
        )
    }

    /// `(min, max)` latency of the port's declared output-delay anchors —
    /// same shape as [`Sdc::input_clock_latency_ns`].
    pub fn output_clock_latency_ns(&self, port: &str) -> (f32, f32) {
        let rows = self.declared_output_clock_rows(port);
        (
            rows[0].first().map_or(0.0, |&(r, _)| self.clock_row_latency_ns(r)),
            rows[1].first().map_or(0.0, |&(r, _)| self.clock_row_latency_ns(r)),
        )
    }

    pub fn input_transition_ns(&self, port: &str) -> f32 {
        last_value(&self.input_transitions, port, true, self).unwrap_or(0.0)
    }

    pub fn output_load_pf(&self, port: &str) -> f32 {
        self.loads
            .iter()
            .rev()
            .find(|r| match &r.sel {
                LoadSel::Ports(sel) => sel.matches(port, false, self),
                LoadSel::Nets(_) => false,
            })
            .map(|r| r.value_pf)
            .unwrap_or(0.0)
    }

    /// `set_load` rules on `get_nets` collections, in file order. Net names are
    /// symbolic until resolved against the design's net table.
    pub fn net_loads(&self) -> impl Iterator<Item = (&Vec<String>, f32)> {
        self.loads.iter().filter_map(|r| match &r.sel {
            LoadSel::Nets(names) => Some((names, r.value_pf)),
            LoadSel::Ports(_) => None,
        })
    }

    /// The `set_driving_cell` rule covering `port`, if any.
    pub fn driving_cell(&self, port: &str, is_input: bool) -> Option<&DrivingCell> {
        self.driving_cells
            .iter()
            .rev()
            .find(|(sel, _)| sel.matches(port, is_input, self))
            .map(|(_, d)| d)
    }
}

pub fn load_sdc(path: &Path) -> Result<Sdc> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read SDC {:?}", path))?;
    parse_sdc(&text)
}

pub fn parse_sdc(text: &str) -> Result<Sdc> {
    let mut interp = Interp::new(text);
    interp.script(false)?;
    interp.finish()
}

/// A Tcl value. Selections keep their type, so that `set clk_port [get_ports ...]` can carry a
/// port list through a variable into a later command.
#[derive(Clone, Debug)]
enum Value {
    Str(String),
    Ports(PortSel),
    Clocks(Vec<String>),
    Cells(CellSel),
    /// `get_pins`: `instance/pin` specs, symbolic until the design is known.
    Pins(Vec<String>),
    /// `get_nets`: net names, symbolic until the design is known.
    Nets(Vec<String>),
    /// A selection the model cannot represent (`select_*`, a
    /// malformed selector). Selector combinators propagate it; exception
    /// commands count it as ignored instead of failing the whole SDC.
    Opaque,
}

fn empty() -> Value {
    Value::Str(String::new())
}

/// Evaluates the SDC as a Tcl subset: `set`, `$var`, `[cmd]` and quoting, `expr`, and the constraint
/// commands. Anything outside that fails, so no construct is ever silently skipped.
struct Interp {
    src: Vec<char>,
    pos: usize,
    vars: HashMap<String, Value>,
    sdc: Sdc,
}

impl Interp {
    fn new(text: &str) -> Self {
        Interp {
            src: text.chars().collect(),
            pos: 0,
            vars: HashMap::new(),
            sdc: Sdc::default(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn skip_while(&mut self, pred: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&pred) {
            self.pos += 1;
        }
    }

    fn line(&self) -> usize {
        1 + self.src[..self.pos].iter().filter(|&&c| c == '\n').count()
    }

    fn error<T>(&self, msg: impl std::fmt::Display) -> Result<T> {
        bail!("SDC line {}: {}", self.line(), msg)
    }

    /// Evaluates commands up to the end of input, or up to the `]` closing a command substitution.
    /// Returns the result of the last command.
    fn script(&mut self, in_brackets: bool) -> Result<Value> {
        let mut result = empty();
        loop {
            self.skip_while(|c| c.is_whitespace() || c == ';');
            match self.peek() {
                None if in_brackets => return self.error("unterminated `[`"),
                None => return Ok(result),
                Some(']') if in_brackets => {
                    self.pos += 1;
                    return Ok(result);
                }
                Some('#') => self.skip_while(|c| c != '\n'),
                Some(_) => {
                    let words = self.words(in_brackets)?;
                    result = self.run(words)?;
                    if !in_brackets {
                        self.sdc.commands += 1;
                    }
                }
            }
        }
    }

    /// The words of one command, ending at a newline, `;`, end of input, or an enclosing `]`.
    fn words(&mut self, in_brackets: bool) -> Result<Vec<Value>> {
        let mut words = Vec::new();
        loop {
            self.skip_while(|c| c == ' ' || c == '\t' || c == '\r');
            match self.peek() {
                None | Some('\n') | Some(';') => break,
                Some(']') if in_brackets => break,
                Some(_) => words.push(self.word(in_brackets)?),
            }
        }
        Ok(words)
    }

    fn word(&mut self, in_brackets: bool) -> Result<Value> {
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let text = self.braced()?;
                self.word_end(in_brackets)?;
                Ok(Value::Str(text))
            }
            Some('"') => {
                self.pos += 1;
                let value = self.pieces(true, in_brackets)?;
                self.word_end(in_brackets)?;
                Ok(value)
            }
            _ => self.pieces(false, in_brackets),
        }
    }

    /// Text of a `{...}` word, taken literally with nested braces counted. Called after the `{`.
    fn braced(&mut self) -> Result<String> {
        let mut depth = 1;
        let mut text = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(text);
                    }
                }
                _ => {}
            }
            text.push(c);
        }
        self.error("unterminated `{`")
    }

    fn word_end(&self, in_brackets: bool) -> Result<()> {
        match self.peek() {
            None => Ok(()),
            Some(c) if c.is_whitespace() || c == ';' => Ok(()),
            Some(']') if in_brackets => Ok(()),
            Some(_) => self.error("extra characters after a quoted or braced word"),
        }
    }

    /// Literal text with `$var` and `[cmd]` substitutions, bare or inside quotes. A word that is a
    /// single substitution keeps its value type; anything mixed is concatenated as text.
    fn pieces(&mut self, quoted: bool, in_brackets: bool) -> Result<Value> {
        let mut parts: Vec<Value> = Vec::new();
        let mut literal = String::new();
        loop {
            let Some(c) = self.peek() else {
                if quoted {
                    return self.error("unterminated `\"`");
                }
                break;
            };
            if quoted && c == '"' {
                self.pos += 1;
                break;
            }
            if !quoted && (c.is_whitespace() || c == ';' || (c == ']' && in_brackets)) {
                break;
            }
            self.pos += 1;
            let part = match c {
                '\\' => return self.error("backslash escapes are outside the SDC subset"),
                '$' => self.variable()?,
                '[' => self.script(true)?,
                _ => {
                    literal.push(c);
                    continue;
                }
            };
            if !literal.is_empty() {
                parts.push(Value::Str(std::mem::take(&mut literal)));
            }
            parts.push(part);
        }
        if !literal.is_empty() {
            parts.push(Value::Str(literal));
        }
        if parts.len() == 1 {
            return Ok(parts.remove(0));
        }
        let mut text = String::new();
        for part in parts {
            match part {
                Value::Str(s) => text.push_str(&s),
                _ => return self.error("a selection cannot be concatenated with text"),
            }
        }
        Ok(Value::Str(text))
    }

    /// The variable name after a `$`: letters, digits and underscores.
    fn variable(&mut self) -> Result<Value> {
        let start = self.pos;
        self.skip_while(|c| c.is_ascii_alphanumeric() || c == '_');
        let name: String = self.src[start..self.pos].iter().collect();
        if name.is_empty() {
            return self.error("`$` must be followed by a variable name");
        }
        match self.vars.get(&name) {
            Some(value) => Ok(value.clone()),
            None => self.error(format!("undefined variable `${name}`")),
        }
    }

    fn run(&mut self, words: Vec<Value>) -> Result<Value> {
        let (name, args) = match words.split_first() {
            Some((Value::Str(name), args)) => (name.as_str(), args),
            _ => return self.error("a command name must be a plain word"),
        };
        let line = self.line();
        self.command(name, args)
            .with_context(|| format!("SDC line {line}: `{name}`"))
    }

    fn command(&mut self, name: &str, args: &[Value]) -> Result<Value> {
        match name {
            "current_design" => Ok(empty()),
            "set" => self.set(args),
            "expr" => expr(args),
            "get_ports" => Ok(match plain_names(args) {
                Ok(names) => Value::Ports(PortSel::Ports(names)),
                Err(_) => Value::Opaque,
            }),
            // `[get_clocks]` with no argument is the `*` match — every
            // clock (measured `t2_eclk2`). `[get_clocks {}]` is a valid
            // empty collection: `set_clock_*` commands apply to zero rows,
            // silently (`t2_eclk`). Unknown names drop out at use — see
            // `clock_rows` (OpenSTA's `STA-0351` warn is unmodelled).
            "get_clocks" => Ok(match args {
                [] => Value::Clocks(self.all_clock_names()),
                _ => match clock_names_arg(args) {
                    Some(names) => Value::Clocks(names),
                    None => Value::Opaque,
                },
            }),
            "get_cells" | "get_instance" | "get_inst" => Ok(self.cells_sel(args)),
            "get_pins" => Ok(match plain_names(args) {
                Ok(names) => Value::Pins(names),
                Err(_) => Value::Opaque,
            }),
            "get_nets" | "get_net" => Ok(match plain_names(args) {
                Ok(names) => Value::Nets(names),
                Err(_) => Value::Opaque,
            }),
            "all_registers" => Ok(match args {
                [] => Value::Cells(CellSel::AllRegisters),
                _ => Value::Opaque,
            }),
            "all_inputs" => Ok(match args {
                [] => Value::Ports(PortSel::AllInputs { no_clocks: false }),
                [Value::Str(flag)] if flag == "-no_clocks" => {
                    Value::Ports(PortSel::AllInputs { no_clocks: true })
                }
                _ => Value::Opaque,
            }),
            "all_outputs" => Ok(match args {
                [] => Value::Ports(PortSel::AllOutputs),
                _ => Value::Opaque,
            }),
            "all_clocks" => Ok(Value::Clocks(self.all_clock_names())),
            "remove_from_collection" => self.remove_from_collection(args),
            "set_false_path" => {
                self.timing_exception(name, args, ExceptionKind::FalsePath)?;
                Ok(empty())
            }
            "set_multicycle_path" => {
                self.timing_exception(name, args, ExceptionKind::Multicycle)?;
                Ok(empty())
            }
            "set_max_delay" => {
                self.path_delay(name, args, PathDelayKind::Max)?;
                Ok(empty())
            }
            "set_min_delay" => {
                self.path_delay(name, args, PathDelayKind::Min)?;
                Ok(empty())
            }
            "create_clock" => {
                self.create_clock(args)?;
                Ok(empty())
            }
            "set_clock_groups" => {
                self.set_clock_groups(args)?;
                Ok(empty())
            }
            "set_clock_latency" => {
                self.set_clock_latency(args)?;
                Ok(empty())
            }
            "set_clock_uncertainty" => {
                self.set_clock_uncertainty(args)?;
                Ok(empty())
            }
            "set_clock_transition" => {
                self.set_clock_transition(args)?;
                Ok(empty())
            }
            "set_units" => {
                self.set_units(args)?;
                Ok(empty())
            }
            "set_timing_derate" => {
                self.set_timing_derate(args)?;
                Ok(empty())
            }
            "set_driving_cell" => {
                self.set_driving_cell(args)?;
                Ok(empty())
            }
            "set_input_delay" => {
                let rule = self.delay_rule(args)?;
                self.sdc.input_delays.push(rule);
                Ok(empty())
            }
            "set_output_delay" => {
                let rule = self.delay_rule(args)?;
                self.sdc.output_delays.push(rule);
                Ok(empty())
            }
            "set_input_transition" => {
                let rule = value_and_ports(
                    args,
                    &PORT_VALUE_FLAGS,
                    &IGNORED_VALUE_FLAGS,
                    &[],
                    &mut self.sdc.ignored_options,
                )?;
                self.sdc.input_transitions.push(rule);
                Ok(empty())
            }
            "set_load" => {
                match self.load_rule(name, args)? {
                    Some(rule) => self.sdc.loads.push(rule),
                    None => {
                        *self
                            .sdc
                            .ignored_commands
                            .entry(name.to_string())
                            .or_default() += 1
                    }
                }
                Ok(empty())
            }
            // OpenSTA accepts `set_fanout_load` but warns it is not supported
            // (STA-0461) and applies nothing: accept the value+object shape so a
            // stock `write_sdc` file parses, and count the command — it cannot
            // change a reported number, so it joins the `is_clean` exemption.
            "set_fanout_load" => {
                self.fanout_load(args)?;
                *self
                    .sdc
                    .ignored_commands
                    .entry(name.to_string())
                    .or_default() += 1;
                Ok(empty())
            }
            // Design rules the timing model does not check, and propagated clocks it cannot
            // propagate: accepted so a stock `write_sdc` file parses, counted in
            // `ignored_commands` so the skip summary still shows them.
            "set_max_transition" | "set_max_capacitance" | "set_max_fanout" => {
                self.design_rule(args)?;
                *self
                    .sdc
                    .ignored_commands
                    .entry(name.to_string())
                    .or_default() += 1;
                Ok(empty())
            }
            "set_propagated_clock" | "unset_propagated_clock" => {
                self.propagated_clock(name, args)?;
                Ok(empty())
            }
            // Selector commands outside the modelled set evaluate to an opaque
            // value: inside an exception argument the command degrades to
            // `ignored_commands` instead of aborting the whole SDC; anywhere
            // else it still fails on the type mismatch downstream.
            _ if is_object_selector(name) => Ok(Value::Opaque),
            _ => bail!("unsupported command `{name}`"),
        }
    }

    fn set(&mut self, args: &[Value]) -> Result<Value> {
        let [Value::Str(name), value] = args else {
            bail!("set takes a variable name and a value");
        };
        self.vars.insert(name.clone(), value.clone());
        Ok(value.clone())
    }

    /// `create_clock -name N -period P [-waveform {rise fall}] [source]`. A clock with a source port is a design
    /// clock; one without is a virtual I/O clock that I/O delays can be referenced to.
    /// Every clock — design or virtual — appends a row to `Sdc::clocks`.
    fn create_clock(&mut self, args: &[Value]) -> Result<()> {
        let name = flag_value(args, "-name")?.map(str::to_string);
        let period_ns = parse_f32(flag_value(args, "-period")?.context("missing -period")?)?;
        let waveform_ns = flag_value(args, "-waveform")?
            .map(|text| {
                let instants = text
                    .split_whitespace()
                    .map(parse_f32)
                    .collect::<Result<Vec<f32>>>()?;
                // Measured OpenSTA bails: an odd instant count (STA-0371)
                // and non-increasing instants (STA-0372). Lists past two
                // instants parse there (`t2_wave4`) but the eval model is
                // built on the rise/fall pair — named bail instead of a
                // silently wrong lattice (`docs/multi-clock.md`).
                if instants.len() % 2 == 1 {
                    bail!("create_clock -waveform takes an even number of edge instants");
                }
                if instants.len() > 2 {
                    bail!("create_clock -waveform with more than two instants is unsupported");
                }
                if instants.windows(2).any(|w| w[1] <= w[0]) {
                    bail!("create_clock -waveform instants must be strictly increasing");
                }
                if instants.len() != 2 {
                    bail!("create_clock -waveform takes two edge instants");
                }
                // OpenSTA accepts instants past the period (`t2_wavebig`
                // `{0 2.5}`) — the waveform repeats per period, so the
                // instants wrap. The wrapped pair is kept as-is even when
                // it reorders or collapses: measured `t2_waverot` `{1.9
                // 2.1}` on a 2.0 period becomes `{1.9, 0.1}` (an 80%-high
                // phase-shifted lattice) and `t2_wavez` `{0 2.0}` becomes
                // `{0, 0}` — edge 1 no later than edge 0 is a legal stored
                // state, so no post-wrap check applies.
                Ok([
                    instants[0].rem_euclid(period_ns),
                    instants[1].rem_euclid(period_ns),
                ])
            })
            .transpose()?;
        match positionals(
            args,
            &CLOCK_VALUE_FLAGS,
            &[],
            &[],
            &mut self.sdc.ignored_options,
        )?
        .as_slice()
        {
            [] => {
                let Some(name) = name else {
                    bail!("a virtual clock needs -name");
                };
                self.sdc.clocks.push(Clock {
                    name,
                    port: None,
                    period_ns,
                    waveform_ns,
                    ..Clock::default()
                });
            }
            [Value::Ports(PortSel::Ports(ports))] => {
                let [port] = ports.as_slice() else {
                    bail!("create_clock takes one source port");
                };
                self.sdc.clocks.push(Clock {
                    name: name.unwrap_or_else(|| port.clone()),
                    port: Some(port.clone()),
                    period_ns,
                    waveform_ns,
                    ..Clock::default()
                });
            }
            _ => bail!("unsupported create_clock argument"),
        }
        Ok(())
    }

    /// `set_clock_groups -asynchronous|-logically_exclusive|-physically_exclusive
    /// [-name n] [-allow_paths] -group <clocks> [-group <clocks>...]`.
    /// The mode flag is mandatory — OpenSTA aborts without it (`STA-0400`,
    /// measured `t2_grpn`). `-allow_paths` parses but declares no
    /// exclusion (measured `t2_grpa`: all endpoints stay timed). Group
    /// arguments are `get_clocks`/`all_clocks` collections or bare
    /// name lists: collections filter like `clock_rows` (an all-miss
    /// group resolves empty — the `clock_rows` posture measured on
    /// `t2_selunk`), while a bare name list still bails on an unknown
    /// clock (`t2_grpb` shows bare names resolve).
    fn set_clock_groups(&mut self, args: &[Value]) -> Result<()> {
        let mut kind = None;
        let mut groups = Vec::new();
        let mut allow_paths = false;
        let mut i = 0;
        while i < args.len() {
            match &args[i] {
                Value::Str(s)
                    if matches!(
                        s.as_str(),
                        "-asynchronous" | "-logically_exclusive" | "-physically_exclusive"
                    ) =>
                {
                    if kind.is_some() {
                        bail!("set_clock_groups takes one mode flag");
                    }
                    kind = Some(match s.as_str() {
                        "-asynchronous" => ClockGroupKind::Asynchronous,
                        "-logically_exclusive" => ClockGroupKind::LogicallyExclusive,
                        _ => ClockGroupKind::PhysicallyExclusive,
                    });
                    i += 1;
                }
                Value::Str(s) if s == "-name" => {
                    if matches!(args.get(i + 1), Some(Value::Str(_))) {
                        i += 2;
                    } else {
                        bail!("set_clock_groups -name needs a value");
                    }
                }
                Value::Str(s) if s == "-allow_paths" => {
                    allow_paths = true;
                    i += 1;
                }
                Value::Str(s) if s == "-group" => {
                    let group = match args.get(i + 1) {
                        // A collection filters like `clock_rows`: unknown
                        // names drop out, `[get_clocks {}]` is the empty
                        // group — posture follows the measured
                        // `set_clock_latency` case (`t2_eclk`).
                        Some(Value::Clocks(names)) => self.clock_rows(names),
                        // `-group cka` and `-group {cka ckb}` arrive as
                        // plain/braced words; `-group {}` is the empty
                        // group (measured `t2_grpe`: parses, still cuts).
                        Some(Value::Str(text)) => text
                            .split_whitespace()
                            .map(|n| self.resolve_clock(n).map(|id| id.0))
                            .collect::<Result<Vec<_>>>()?,
                        _ => bail!("set_clock_groups -group takes clock objects"),
                    };
                    groups.push(group);
                    i += 2;
                }
                Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                    bail!("unsupported option `{s}`")
                }
                _ => bail!("set_clock_groups takes a mode flag and -group clock lists"),
            }
        }
        if kind.is_none() {
            bail!(
                "set_clock_groups needs -asynchronous, -logically_exclusive or -physically_exclusive"
            );
        }
        if !allow_paths {
            self.sdc.clock_groups.push(ClockGroup {
                kind: kind.unwrap(),
                groups,
            });
        }
        Ok(())
    }

    /// `Sdc::clocks` rows for a `get_clocks`/`all_clocks` name list.
    /// Unknown names drop out silently — OpenSTA warns and leaves them
    /// out of the collection, so an all-miss list resolves to the empty
    /// set (measured `t2_selunk`) and `resolve_clock`'s bail does not
    /// apply here.
    fn clock_rows(&self, names: &[String]) -> Vec<usize> {
        names
            .iter()
            .filter_map(|n| self.sdc.clocks.iter().position(|c| c.name == *n))
            .collect()
    }

    /// `set_clock_uncertainty [-setup|-hold] <ns> [-from <clocks>] [-to <clocks>] [<clocks>]`.
    /// OpenSTA requires the clock list; the single-value form is kept for hand-written SDCs
    /// and sets both checks. At most one of `-setup`/`-hold` is accepted, matching OpenSTA,
    /// which treats a second flag as objects. `-from`/`-to` select clock objects and store
    /// an inter-clock rule ([`Sdc::inter_clock_uncertainty`]) instead of writing rows; each
    /// takes clock objects only and may appear once.
    fn set_clock_uncertainty(&mut self, args: &[Value]) -> Result<()> {
        let mut setup = false;
        let mut hold = false;
        let mut from: Option<Vec<usize>> = None;
        let mut to: Option<Vec<usize>> = None;
        let mut rest = Vec::with_capacity(args.len());
        let mut i = 0;
        while i < args.len() {
            match &args[i] {
                Value::Str(s) if s.as_str() == "-setup" || s.as_str() == "-hold" => {
                    let slot = if s.as_str() == "-setup" {
                        &mut setup
                    } else {
                        &mut hold
                    };
                    if *slot {
                        bail!("duplicate {s}");
                    }
                    *slot = true;
                    i += 1;
                }
                Value::Str(s) if s.as_str() == "-from" || s.as_str() == "-to" => {
                    let slot = if s.as_str() == "-from" {
                        &mut from
                    } else {
                        &mut to
                    };
                    if slot.is_some() {
                        bail!("set_clock_uncertainty takes each of -from/-to once");
                    }
                    let rows = match args.get(i + 1) {
                        // A collection filters like `clock_rows` — unknown
                        // names drop out and an empty list resolves empty
                        // (measured `t2_eclk`'s `-to [get_clocks {}]`).
                        Some(Value::Clocks(names)) => self.clock_rows(names),
                        // `-from cka`: a bare name (write_sdc form), possibly a
                        // whitespace-separated list.
                        Some(Value::Str(name)) => name
                            .split_whitespace()
                            .map(|n| self.resolve_clock(n).map(|id| id.0))
                            .collect::<Result<Vec<_>>>()?,
                        Some(_) => bail!("set_clock_uncertainty -from/-to take clock objects"),
                        None => bail!("set_clock_uncertainty {s} needs a value"),
                    };
                    *slot = Some(rows);
                    i += 2;
                }
                Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                    bail!("unsupported option `{s}`")
                }
                value => {
                    rest.push(value);
                    i += 1;
                }
            }
        }
        if setup && hold {
            bail!("set_clock_uncertainty takes at most one of -setup/-hold");
        }
        let (value, targets) = match rest.as_slice() {
            [Value::Str(value)] => (value.as_str(), None),
            // A `get_clocks` collection filters — unknown names drop out and
            // an empty list applies to zero rows (`clock_rows`, `t2_eclk`).
            [Value::Str(value), Value::Clocks(names)] => {
                (value.as_str(), Some(self.clock_rows(names)))
            }
            // `set_clock_uncertainty 0.25 clk`: write_sdc emits the clock as a bare name.
            [Value::Str(value), Value::Str(name)] => {
                (value.as_str(), Some(vec![self.resolve_clock(name)?.0]))
            }
            _ => bail!("set_clock_uncertainty takes a value and optional clock names"),
        };
        let value_ns = parse_f32(value)?;
        // OpenSTA refuses a lone side outright — `STA-0421 -from/-to must be
        // used together` aborts `read_sdc` (measured `t2_eclk3`/`t2_eclk4`).
        if from.is_some() != to.is_some() {
            bail!("set_clock_uncertainty -from/-to must be used together");
        }
        if from.is_some() || to.is_some() {
            if targets.is_some() {
                // OpenSTA treats the positional list and `-to` as overlapping;
                // refuse the ambiguous form loudly.
                bail!("set_clock_uncertainty cannot name clocks with -from/-to");
            }
            // A flag supplied with a collection that resolved empty covers no
            // clock pair, so the rule is a no-op (measured `t2_eclk`: `-from
            // cka -to [get_clocks {}]` warns nothing and changes nothing).
            if from.as_ref().is_some_and(|f| f.is_empty())
                || to.as_ref().is_some_and(|t| t.is_empty())
            {
                return Ok(());
            }
            self.sdc.inter_clock_uncertainty.push(InterClockUnc {
                from: from.unwrap_or_default(),
                to: to.unwrap_or_default(),
                setup_ns: (!hold).then_some(value_ns),
                hold_ns: (!setup).then_some(value_ns),
            });
            return Ok(());
        }
        let ids = match targets {
            None => vec![self.design_clock_id()?],
            Some(rows) => rows.into_iter().map(ClockId).collect(),
        };
        for id in ids {
            let row = &mut self.sdc.clocks[id.0];
            if !hold {
                row.setup_uncertainty_ns = value_ns;
            }
            if !setup {
                row.hold_uncertainty_ns = value_ns;
            }
        }
        Ok(())
    }

    /// `set_clock_latency [-source] <ns> [get_clocks <name>]`. Without `-source` the value
    /// is the ideal network latency, which a propagated clock ignores; `-source` latency is
    /// added on top of the propagated arrival. `-early`/`-late` are refused: the model has a
    /// single latency per clock.
    fn set_clock_latency(&mut self, args: &[Value]) -> Result<()> {
        let mut source = false;
        let mut rest = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                Value::Str(s) if s == "-source" => {
                    if source {
                        bail!("duplicate -source");
                    }
                    source = true;
                }
                Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                    bail!("unsupported option `{s}`")
                }
                value => rest.push(value),
            }
        }
        let (value, targets) = match rest.as_slice() {
            [Value::Str(value)] => (value, vec![self.design_clock_id()?]),
            // A `get_clocks` collection filters — unknown names drop out and
            // an empty list applies to zero rows (`clock_rows`, `t2_eclk`).
            [Value::Str(value), Value::Clocks(names)] => (
                value,
                self.clock_rows(names).into_iter().map(ClockId).collect(),
            ),
            // `set_clock_latency 0.5 clk`: write_sdc emits the clock as a bare name.
            [Value::Str(value), Value::Str(name)] => {
                (value, vec![self.resolve_clock(name)?])
            }
            _ => bail!("set_clock_latency takes a value and optional clock names"),
        };
        let latency = parse_f32(value)?;
        for id in targets {
            let row = &mut self.sdc.clocks[id.0];
            if source {
                row.source_latency_ns = latency;
            } else {
                row.latency_ns = latency;
            }
        }
        Ok(())
    }

    /// `set_propagated_clock|unset_propagated_clock <clock objects>`. The flag is set on
    /// every named design-clock row; unsetting a clock that was never propagated is a
    /// no-op. A virtual clock cannot be propagated: naming it in `set` is an error, in
    /// `unset` it is skipped. Collection arguments (`get_clocks`/`get_ports`
    /// results) filter — an empty collection applies to zero rows, silently
    /// (measured `t2_eclk`); literal names still error when unknown.
    fn propagated_clock(&mut self, command: &str, args: &[Value]) -> Result<()> {
        let mut names = Vec::new();
        for arg in args {
            match arg {
                Value::Str(s) if s.starts_with('-') => {
                    bail!("{command} takes clock objects, not `{s}`")
                }
                Value::Str(s) => names.extend(s.split_whitespace().map(|n| (n.to_string(), true))),
                Value::Clocks(ns) => names.extend(ns.iter().map(|n| (n.clone(), false))),
                Value::Ports(PortSel::Ports(ns)) => {
                    names.extend(ns.iter().map(|n| (n.clone(), false)))
                }
                _ => bail!("{command} takes clock objects"),
            }
        }
        if args.is_empty() {
            bail!("{command} takes clock objects");
        }
        for (clock, literal) in &names {
            // `get_ports {clk}` names the clock's source port; the clock handle is `-name`.
            let id = match self.resolve_clock(clock) {
                Ok(id) => id,
                Err(e) => match self
                    .sdc
                    .clocks
                    .iter()
                    .position(|c| c.port.as_deref() == Some(clock.as_str()))
                {
                    Some(i) => ClockId(i),
                    // A collection name was filtered by its selector
                    // already — drop it; a literal name errors.
                    None if !literal => continue,
                    None => return Err(e),
                },
            };
            if self.sdc.clocks[id.0].port.is_none() {
                if command == "set_propagated_clock" {
                    bail!("set_propagated_clock does not apply to the virtual clock `{clock}`")
                }
                continue;
            }
            self.sdc.clocks[id.0].propagated = command == "set_propagated_clock";
        }
        Ok(())
    }

    /// `set_clock_transition <ns> [get_clocks <name>...]`. The slew lands on every named
    /// clock's row, virtual rows included.
    fn set_clock_transition(&mut self, args: &[Value]) -> Result<()> {
        let (value, ids) = match args {
            [Value::Str(v)] => (v, self.clock_rows(&self.all_clock_names())),
            // A `get_clocks` collection filters — unknown names drop out and
            // an empty list applies to zero rows (`clock_rows`, `t2_eclk`).
            [Value::Str(v), Value::Clocks(names)] => (v, self.clock_rows(names)),
            [Value::Str(v), Value::Str(name)] => (v, vec![self.resolve_clock(name)?.0]),
            _ => bail!("set_clock_transition takes a value and optional clock names"),
        };
        let transition = parse_f32(value)?;
        for row in ids {
            self.sdc.clocks[row].transition_ns = transition;
        }
        Ok(())
    }

    /// `set_units [-time ns] [-capacitance pF] [-resistance ...] ...`. Every number the model
    /// reads is already in ns and pF, so a declaration in those units is a no-op. `-time` or
    /// `-capacitance` in any other unit would silently rescale every value, so it is an error
    /// rather than a skipped option; the remaining unit keys name quantities the timing model
    /// never reads and are accepted without effect.
    fn set_units(&mut self, args: &[Value]) -> Result<()> {
        const IGNORED: [&str; 5] = ["-resistance", "-voltage", "-current", "-power", "-distance"];
        let mut i = 0;
        while i < args.len() {
            let Value::Str(flag) = &args[i] else {
                bail!("set_units takes -flag unit pairs");
            };
            let Some(Value::Str(unit)) = args.get(i + 1) else {
                bail!("{flag} needs a unit");
            };
            match flag.as_str() {
                "-time" if unit != "ns" => bail!("set_units -time {unit}: the model reads ns"),
                "-capacitance" if unit != "pF" => {
                    bail!("set_units -capacitance {unit}: the model reads pF")
                }
                "-time" | "-capacitance" => {}
                f if IGNORED.contains(&f) => {}
                other => bail!("unsupported option `{other}`"),
            }
            i += 2;
        }
        Ok(())
    }

    /// `set_timing_derate -early|-late [-rise|-fall] [-clock|-data]
    /// [-net_delay|-cell_delay|-cell_check] <derate> [objects]`. At least one of `-early`/`-late`
    /// is required — both may appear on one command, which applies the derate to both corners —
    /// and the derate value is positional. Each command overwrites only the
    /// categories its flags select; a bare corner flag covers cell and net delays on both
    /// clock and data paths and both transitions but not cell checks (measured on OpenSTA).
    /// Cell checks respond to `-data` only, never `-clock`. A second positional word would
    /// be the object scope, which the model cannot honour, so it is rejected.
    fn set_timing_derate(&mut self, args: &[Value]) -> Result<()> {
        let mut early = false;
        let mut late = false;
        let mut rise = false;
        let mut fall = false;
        let mut clock = false;
        let mut data = false;
        let mut kinds = [false; 3];
        let mut any_kind = false;
        let mut derate = None;
        for arg in args {
            let Value::Str(word) = arg else {
                bail!("set_timing_derate takes flag/value words");
            };
            match word.as_str() {
                "-early" => {
                    if early {
                        bail!("duplicate -early");
                    }
                    early = true;
                }
                "-late" => {
                    if late {
                        bail!("duplicate -late");
                    }
                    late = true;
                }
                "-rise" => rise = true,
                "-fall" => fall = true,
                "-clock" => clock = true,
                "-data" => data = true,
                "-net_delay" => {
                    kinds[DERATE_NET] = true;
                    any_kind = true;
                }
                "-cell_delay" => {
                    kinds[DERATE_CELL] = true;
                    any_kind = true;
                }
                "-cell_check" => {
                    kinds[DERATE_CHECK] = true;
                    any_kind = true;
                }
                _ if word.starts_with('-') => bail!("unsupported option `{word}`"),
                _ => {
                    if derate.is_some() {
                        bail!("set_timing_derate takes one derate value");
                    }
                    derate = Some(parse_f32(word)?);
                }
            }
        }
        let Some(value) = derate else {
            bail!("set_timing_derate needs a derate value");
        };
        if !early && !late {
            bail!("set_timing_derate needs -early or -late");
        }
        let mut selected = kinds;
        if !any_kind {
            selected[DERATE_CELL] = true;
            selected[DERATE_NET] = true;
        }
        for late_corner in [false, true] {
            if (late_corner && !late) || (!late_corner && !early) {
                continue;
            }
            let corner = if late_corner {
                &mut self.sdc.derate_late
            } else {
                &mut self.sdc.derate_early
            };
            for (kind, &sel) in selected.iter().enumerate() {
                if !sel {
                    continue;
                }
                for (clock_path, row) in corner.factors[kind].iter_mut().enumerate() {
                    let is_clock = clock_path == 0;
                    if kind == DERATE_CHECK && is_clock {
                        continue;
                    }
                    if clock != data && is_clock != clock {
                        continue;
                    }
                    for (transition, cell) in row.iter_mut().enumerate() {
                        if rise != fall && (transition == 0) != rise {
                            continue;
                        }
                        *cell = value;
                    }
                }
            }
        }
        Ok(())
    }

    /// `set_driving_cell -lib_cell C -pin P [-input_transition_rise r]
    /// [-input_transition_fall f] <sel>`. Unlike the delay commands, `-lib_cell`/`-pin` are the
    /// driver's identity here, not restrictions to drop.
    fn set_driving_cell(&mut self, args: &[Value]) -> Result<()> {
        const FLAGS: [&str; 4] = [
            "-lib_cell",
            "-pin",
            "-input_transition_rise",
            "-input_transition_fall",
        ];
        let pos = positionals(args, &FLAGS, &[], &[], &mut self.sdc.ignored_options)?;
        let [Value::Ports(sel)] = pos.as_slice() else {
            bail!("set_driving_cell takes a port selector");
        };
        let in_trans = |flag: &str| -> Result<f32> {
            flag_value(args, flag)?
                .map(parse_f32)
                .transpose()
                .map(|v| v.unwrap_or(0.0))
        };
        self.sdc.driving_cells.push((
            sel.clone(),
            DrivingCell {
                cell: flag_value(args, "-lib_cell")?
                    .context("set_driving_cell needs -lib_cell")?
                    .to_string(),
                pin: flag_value(args, "-pin")?
                    .context("set_driving_cell needs -pin")?
                    .to_string(),
                in_trans_rise: in_trans("-input_transition_rise")?,
                in_trans_fall: in_trans("-input_transition_fall")?,
            },
        ));
        Ok(())
    }

    /// `set_max_transition|set_max_capacitance|set_max_fanout <value> [current_design]`:
    /// design rules the timing model does not check. Only the leading number and the empty
    /// `[current_design]` result are accepted; the command is counted as ignored.
    fn design_rule(&mut self, args: &[Value]) -> Result<()> {
        let [Value::Str(v), rest @ ..] = args else {
            bail!("expected a numeric limit");
        };
        parse_f32(v)?;
        for arg in rest {
            match arg {
                Value::Str(s) if s.is_empty() => {}
                _ => bail!("unexpected argument in design-rule command"),
            }
        }
        Ok(())
    }

    /// Clock names `all_clocks` resolves to at this point in the file, in file order.
    fn all_clock_names(&self) -> Vec<String> {
        self.sdc.clocks.iter().map(|c| c.name.clone()).collect()
    }

    /// `get_cells`/`get_instance`/`get_inst`: explicit instance names. Any
    /// flag or non-name argument makes the selection opaque — exception
    /// commands then count it ignored rather than failing the whole SDC.
    fn cells_sel(&self, args: &[Value]) -> Value {
        match plain_names(args) {
            Ok(names) => Value::Cells(CellSel::Cells(names)),
            Err(_) => Value::Opaque,
        }
    }

    /// `remove_from_collection <collection> <object>...`: the collection minus the named
    /// objects, keeping the collection's type. Ports subtract through `PortSel::without`
    /// (an explicit list filters, a predicate base records exclusions); cells subtract
    /// through `CellSel::without` the same way; clocks and plain name lists subtract by
    /// name. The objects to remove must be enumerable — a predicate selection like
    /// `all_inputs` used as an object cannot be named, so it is an error rather than a
    /// silent no-op. An opaque value anywhere makes the result opaque.
    fn remove_from_collection(&mut self, args: &[Value]) -> Result<Value> {
        let [collection, objects @ ..] = args else {
            bail!("remove_from_collection takes a collection and objects");
        };
        if objects.is_empty() {
            bail!("remove_from_collection takes objects to remove");
        }
        if matches!(collection, Value::Opaque) || objects.iter().any(|a| matches!(a, Value::Opaque))
        {
            return Ok(Value::Opaque);
        }
        // Pin and net specs only subtract from same-namespace collections:
        // mixing namespaces (`remove_from_collection [get_pins ...] [get_cells
        // ...]`) cannot be honoured name-wise, so it degrades to opaque like
        // any other unrepresentable selection.
        let pins_in_objects = objects.iter().any(|a| matches!(a, Value::Pins(_)));
        let nets_in_objects = objects.iter().any(|a| matches!(a, Value::Nets(_)));
        let foreign_collection = |a: &&Value| {
            matches!(a, Value::Cells(_) | Value::Ports(_) | Value::Clocks(_))
        };
        if (pins_in_objects && !matches!(collection, Value::Pins(_)))
            || (nets_in_objects && !matches!(collection, Value::Nets(_)))
            || (matches!(collection, Value::Pins(_))
                && objects
                    .iter()
                    .any(|a| foreign_collection(&a) || matches!(a, Value::Nets(_))))
            || (matches!(collection, Value::Nets(_))
                && objects
                    .iter()
                    .any(|a| foreign_collection(&a) || matches!(a, Value::Pins(_))))
        {
            return Ok(Value::Opaque);
        }
        let mut removed: Vec<String> = Vec::new();
        for arg in objects {
            match arg {
                Value::Ports(PortSel::Ports(names)) => removed.extend(names.iter().cloned()),
                Value::Ports(_) => {
                    bail!("remove_from_collection cannot subtract a predicate selection")
                }
                Value::Cells(CellSel::Cells(names)) => removed.extend(names.iter().cloned()),
                Value::Cells(_) => {
                    bail!("remove_from_collection cannot subtract a predicate selection")
                }
                Value::Clocks(names) => removed.extend(names.iter().cloned()),
                Value::Pins(names) => removed.extend(names.iter().cloned()),
                Value::Nets(names) => removed.extend(names.iter().cloned()),
                Value::Str(text) => removed.extend(text.split_whitespace().map(str::to_string)),
                Value::Opaque => unreachable!(),
            }
        }
        removed.sort_unstable();
        removed.dedup();
        match collection {
            Value::Ports(sel) => Ok(Value::Ports(sel.clone().without(&removed))),
            Value::Cells(sel) => Ok(Value::Cells(sel.clone().without(&removed))),
            Value::Pins(specs) => Ok(Value::Pins(
                specs
                    .iter()
                    .filter(|s| !removed.contains(*s))
                    .cloned()
                    .collect(),
            )),
            Value::Nets(specs) => Ok(Value::Nets(
                specs
                    .iter()
                    .filter(|s| !removed.contains(*s))
                    .cloned()
                    .collect(),
            )),
            Value::Clocks(names) => Ok(Value::Clocks(
                names
                    .iter()
                    .filter(|n| !removed.contains(*n))
                    .cloned()
                    .collect(),
            )),
            Value::Str(text) => Ok(Value::Str(
                text.split_whitespace()
                    .filter(|n| !removed.iter().any(|r| r.as_str() == *n))
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            Value::Opaque => unreachable!(),
        }
    }

    /// `set_false_path`/`set_multicycle_path`: a timing exception with
    /// symbolic `-from`/`-to` selectors, resolved against the design at STA
    /// time. An unsupported qualifier (`-through`, `-rise`, ...) or an
    /// unresolvable selector (clock objects, opaque selections) drops the
    /// whole command into `ignored_commands` — a half-applied exception
    /// would silently change reported timing. Multicycle counts follow the
    /// measured OpenSTA convention (`docs/timing-exceptions.md`): a bare
    /// positional `N` or `-setup N` sets the setup count (>= 1), `-hold H`
    /// accepts any integer, and `-setup X -hold Y` on one command is valid
    /// SDC (OpenSTA rejects it) and is accepted here. `-both` is counted
    /// ignored: OpenSTA rejects it too, so its hold-side semantics cannot
    /// be verified. `-setup`/`-hold`/`-both` on `set_false_path` are
    /// accepted no-ops.
    fn timing_exception(&mut self, name: &str, args: &[Value], kind: ExceptionKind) -> Result<()> {
        if args.iter().any(
            |a| matches!(a, Value::Str(s) if EXCEPTION_UNSUPPORTED_FLAGS.contains(&s.as_str())),
        ) {
            *self
                .sdc
                .ignored_commands
                .entry(name.to_string())
                .or_default() += 1;
            return Ok(());
        }
        let mut from = None;
        let mut to = None;
        let mut setup_n = None;
        let mut hold_n = None;
        let mut i = 0;
        while i < args.len() {
            match &args[i] {
                Value::Str(s) if s == "-from" || s == "-to" => {
                    let flag = s.clone();
                    let slot = if s == "-from" { &mut from } else { &mut to };
                    if slot.is_some() {
                        bail!("{name} takes each of -from/-to once");
                    }
                    let sel = match args.get(i + 1) {
                        None => bail!("{name} {flag} needs a value"),
                        Some(Value::Str(v)) if v.starts_with('-') && !is_negative_number(v) => {
                            bail!("{name} {flag} needs a value, found `{v}`")
                        }
                        Some(
                            v
                            @ (Value::Ports(_) | Value::Cells(_) | Value::Pins(_) | Value::Str(_)),
                        ) => obj_sel(v),
                        // Clock objects resolve to rows now; only opaque
                        // selections still cannot be honoured and count the
                        // command ignored.
                        Some(Value::Clocks(names)) => {
                            Some(ObjSel::Clocks(self.clock_rows(names)))
                        }
                        Some(_) => None,
                    };
                    match sel {
                        Some(sel) => *slot = Some(sel),
                        None => {
                            *self
                                .sdc
                                .ignored_commands
                                .entry(name.to_string())
                                .or_default() += 1;
                            return Ok(());
                        }
                    }
                    i += 2;
                }
                Value::Str(s) if s == "-both" && kind == ExceptionKind::Multicycle => {
                    // `-both N` is valid SDC but OpenSTA rejects it (STA-0563),
                    // so its hold-side semantics cannot be verified against the
                    // reference tool — count the command ignored rather than
                    // guess a mapping.
                    *self
                        .sdc
                        .ignored_commands
                        .entry(name.to_string())
                        .or_default() += 1;
                    return Ok(());
                }
                Value::Str(s)
                    if kind == ExceptionKind::Multicycle && (s == "-setup" || s == "-hold") =>
                {
                    // The count may already have been given positionally
                    // (`set_multicycle_path 2 -setup` is accepted SDC), so a
                    // non-numeric next token makes the flag a no-op marker;
                    // any real count error still surfaces at the end check.
                    match args.get(i + 1) {
                        Some(Value::Str(v)) if v.parse::<i64>().is_ok() => {
                            let n = multicycle_count(v)?;
                            if s == "-setup" {
                                setup_n = Some(setup_count(n, name, s)?);
                            } else {
                                hold_n = Some(hold_count(n, name, s)?);
                            }
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
                Value::Str(s)
                    if kind == ExceptionKind::FalsePath
                        && (s == "-setup" || s == "-hold" || s == "-both") =>
                {
                    // Accepted no-ops; consume an optional following number so
                    // `-setup 2` does not surface as a stray positional.
                    i += 1;
                    if matches!(args.get(i), Some(Value::Str(n)) if n.parse::<i64>().is_ok()) {
                        i += 1;
                    }
                }
                Value::Str(s) if s == "-quiet" => {
                    i += 1;
                }
                Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                    bail!("unsupported option `{s}`")
                }
                Value::Str(s) => {
                    if kind != ExceptionKind::Multicycle {
                        bail!("{name} takes no positional arguments, found `{s}`");
                    }
                    setup_n = Some(setup_count(multicycle_count(s)?, name, "positional")?);
                    i += 1;
                }
                _ => bail!("{name} takes flags and -from/-to selectors"),
            }
        }
        if kind == ExceptionKind::Multicycle && setup_n.is_none() && hold_n.is_none() {
            bail!("{name} needs a multicycle value (-setup/-hold/-both or a positional N)");
        }
        self.sdc.exceptions.push(TimingException {
            kind,
            from,
            to,
            setup_n,
            hold_n,
        });
        Ok(())
    }

    /// `set_max_delay`/`set_min_delay`: a path-delay constraint with a signed
    /// positional value and symbolic `-from`/`-to` selectors, resolved against
    /// the design at STA time like timing exceptions. Qualifiers the model
    /// cannot honour (`-rise`/`-fall`/`-through`/`-early`/`-clock`/...) drop
    /// the whole command into `ignored_commands` — a half-applied delay
    /// constraint would silently change reported timing. Measured OpenSTA
    /// acceptance is recorded in `docs/path-delays.md`.
    fn path_delay(&mut self, name: &str, args: &[Value], kind: PathDelayKind) -> Result<()> {
        if args.iter().any(
            |a| matches!(a, Value::Str(s) if PATH_DELAY_UNSUPPORTED_FLAGS.contains(&s.as_str())),
        ) {
            *self
                .sdc
                .ignored_commands
                .entry(name.to_string())
                .or_default() += 1;
            return Ok(());
        }
        let mut from = None;
        let mut to = None;
        let mut value_ns = None;
        let mut i = 0;
        while i < args.len() {
            match &args[i] {
                Value::Str(s) if s == "-from" || s == "-to" => {
                    let flag = s.clone();
                    let slot = if s == "-from" { &mut from } else { &mut to };
                    if slot.is_some() {
                        bail!("{name} takes each of -from/-to once");
                    }
                    let sel = match args.get(i + 1) {
                        None => bail!("{name} {flag} needs a value"),
                        Some(Value::Str(v)) if v.starts_with('-') && !is_negative_number(v) => {
                            bail!("{name} {flag} needs a value, found `{v}`")
                        }
                        Some(
                            v
                            @ (Value::Ports(_) | Value::Cells(_) | Value::Pins(_) | Value::Str(_)),
                        ) => obj_sel(v),
                        Some(Value::Clocks(names)) => {
                            Some(ObjSel::Clocks(self.clock_rows(names)))
                        }
                        Some(_) => None,
                    };
                    match sel {
                        Some(sel) => *slot = Some(sel),
                        None => {
                            *self
                                .sdc
                                .ignored_commands
                                .entry(name.to_string())
                                .or_default() += 1;
                            return Ok(());
                        }
                    }
                    i += 2;
                }
                Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                    bail!("unsupported option `{s}`")
                }
                Value::Str(s) => {
                    if value_ns.is_some() {
                        bail!("{name} takes one delay value, found `{s}`");
                    }
                    value_ns = Some(parse_f32(s)?);
                    i += 1;
                }
                _ => bail!("{name} takes a delay value and -from/-to selectors"),
            }
        }
        let Some(value_ns) = value_ns else {
            bail!("{name} needs a delay value");
        };
        self.sdc.path_delays.push(PathDelay {
            kind,
            from,
            to,
            value_ns,
        });
        Ok(())
    }

    /// `set_load <pf> <object collection>`. Port and net collections become
    /// symbolic [`LoadRule`]s; the other object domains OpenSTA rejects
    /// (`get_pins`, `get_cells` — `STA-0100`) or does not know
    /// (`all_fanouts` — opaque here) return `None` so the caller counts the
    /// whole command ignored rather than half-applying it. `-pin_load` is
    /// consumed for every domain and `-wire_load` for nets: OpenSTA warns
    /// those are not allowed on net objects but applies the same value
    /// anyway, so accepting them reproduces the measured outcome. `-wire_load`
    /// on ports writes the attached net's wire capacitance — a different
    /// domain than the port pin load — so that command is counted instead.
    /// Corner/transition/subtractive qualifiers (`-min`, `-max`, `-rise`,
    /// `-fall`, `-subtract_pin_load`) are accepted-and-counted: applying them
    /// to both corners would silently diverge.
    fn load_rule(&mut self, name: &str, args: &[Value]) -> Result<Option<LoadRule>> {
        if args.iter().any(
            |a| matches!(a, Value::Str(s) if LOAD_UNSUPPORTED_FLAGS.contains(&s.as_str())),
        ) {
            return Ok(None);
        }
        let pos = positionals(
            args,
            &PORT_VALUE_FLAGS,
            &IGNORED_VALUE_FLAGS,
            &LOAD_BOOL_FLAGS,
            &mut self.sdc.ignored_options,
        )?;
        let [Value::Str(value), object] = pos.as_slice() else {
            bail!("{name} takes a numeric value and an object collection");
        };
        let value_pf = parse_f32(value)?;
        let sel = match object {
            Value::Ports(sel) => {
                // `-wire_load` on a port targets the attached net's wire
                // capacitance, not the port pin load the model applies.
                if has_flag(args, "-wire_load") {
                    return Ok(None);
                }
                LoadSel::Ports(sel.clone())
            }
            Value::Nets(names) => LoadSel::Nets(names.clone()),
            Value::Pins(_) | Value::Cells(_) | Value::Clocks(_) | Value::Opaque => {
                return Ok(None)
            }
            Value::Str(_) => {
                bail!("{name} takes a numeric value and an object collection")
            }
        };
        Ok(Some(LoadRule { sel, value_pf }))
    }

    /// `set_fanout_load <n> <object collection>`: validated like `set_load`
    /// (same unsupported-qualifier list; the command is inert in OpenSTA, so
    /// every accepted shape is counted ignored by the caller anyway).
    fn fanout_load(&mut self, args: &[Value]) -> Result<()> {
        if args.iter().any(
            |a| matches!(a, Value::Str(s) if LOAD_UNSUPPORTED_FLAGS.contains(&s.as_str())),
        ) {
            return Ok(());
        }
        let pos = positionals(
            args,
            &PORT_VALUE_FLAGS,
            &IGNORED_VALUE_FLAGS,
            &LOAD_BOOL_FLAGS,
            &mut self.sdc.ignored_options,
        )?;
        let [Value::Str(value), _object] = pos.as_slice() else {
            bail!("set_fanout_load takes a numeric value and an object collection");
        };
        parse_f32(value)?;
        Ok(())
    }

    fn delay_rule(&mut self, args: &[Value]) -> Result<DelayRule> {
        let sense = match (has_flag(args, "-min"), has_flag(args, "-max")) {
            (true, true) => bail!("a delay takes at most one of -min/-max"),
            (true, false) => DelaySense::Min,
            (false, true) => DelaySense::Max,
            (false, false) => DelaySense::Both,
        };
        let add = has_flag(args, "-add_delay");
        let (sel, value_ns) = value_and_ports(
            args,
            &DELAY_VALUE_FLAGS,
            &IGNORED_VALUE_FLAGS,
            &DELAY_BOOL_FLAGS,
            &mut self.sdc.ignored_options,
        )?;
        let clock = match flag_arg(args, "-clock")? {
            None => None,
            Some(Value::Str(name)) => Some(self.resolve_clock(name)?),
            Some(Value::Clocks(names)) => match names.as_slice() {
                [name] => Some(self.resolve_clock(name)?),
                _ => bail!("-clock takes one clock"),
            },
            Some(_) => bail!("-clock needs a clock name"),
        };
        Ok(DelayRule {
            sel,
            value_ns,
            clock,
            sense,
            add,
        })
    }

    /// The row a clock handle resolves to. The match is by `name` only — `create_clock`
    /// stores `name.unwrap_or(port)`, so the port-name fallback is covered — and stays
    /// deliberately absent of a port match: `set_clock_latency 1.0 clk` on a `-name`d
    /// clock is `unknown clock`, matching OpenSTA (the port form is honoured only by
    /// `propagated_clock`).
    fn resolve_clock(&self, name: &str) -> Result<ClockId> {
        match self.sdc.clocks.iter().position(|c| c.name == name) {
            Some(i) => Ok(ClockId(i)),
            None => bail!("unknown clock `{name}`"),
        }
    }

    /// The design clock's row — the target of a `set_clock_*` command without a clock
    /// argument. Bails when no port-bearing row has been declared yet; such an SDC would
    /// fail `finish` anyway.
    fn design_clock_id(&self) -> Result<ClockId> {
        self.sdc
            .clocks
            .iter()
            .position(|c| c.port.is_some())
            .map(ClockId)
            .ok_or_else(|| anyhow::anyhow!("no design clock declared yet"))
    }

    fn finish(self) -> Result<Sdc> {
        if self.sdc.design_clock_count() == 0 {
            bail!("SDC defines no clock period");
        }
        for clock in &self.sdc.clocks {
            if clock.period_ns <= 0.0 {
                bail!(
                    "clock `{}` has period {}; a clock period must be positive",
                    clock.name,
                    clock.period_ns
                );
            }
        }
        Ok(self.sdc)
    }
}

fn expr(args: &[Value]) -> Result<Value> {
    let mut text = String::new();
    for arg in args {
        let Value::Str(s) = arg else {
            bail!("expr takes numbers and operators");
        };
        text.push_str(s);
        text.push(' ');
    }
    let value = eval_arith(&text).with_context(|| format!("unsupported expr `{}`", text.trim()))?;
    Ok(Value::Str(value.to_string()))
}

/// `get_clocks` name arguments: bare words only. Unlike `plain_names`, an
/// empty name list is a valid empty collection — `[get_clocks {}]` selects
/// no clocks and `set_clock_*` commands apply to zero rows, silently
/// (measured `t2_eclk`). `None` on a flag or nested selection keeps the
/// opaque form.
fn clock_names_arg(args: &[Value]) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for arg in args {
        let Value::Str(text) = arg else {
            return None;
        };
        if text.starts_with('-') {
            return None;
        }
        names.extend(text.split_whitespace().map(str::to_string));
    }
    Some(names)
}

/// Names from a selector's arguments. Each word may hold a whitespace-separated list, as `{a b}` does.
fn plain_names(args: &[Value]) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for arg in args {
        let Value::Str(text) = arg else {
            bail!("expected names, found a selection");
        };
        if text.starts_with('-') {
            bail!("unsupported option `{text}`");
        }
        names.extend(text.split_whitespace().map(str::to_string));
    }
    if names.is_empty() {
        bail!("no names given");
    }
    Ok(names)
}

fn parse_f32(text: &str) -> Result<f32> {
    text.parse()
        .with_context(|| format!("`{text}` is not a number"))
}

/// Qualifiers that make a timing-exception command unsupported: the whole
/// command is counted in `ignored_commands` rather than half-applied.
const EXCEPTION_UNSUPPORTED_FLAGS: [&str; 9] = [
    "-through",
    "-start",
    "-end",
    "-rise",
    "-fall",
    "-edge",
    "-points",
    "-lib_cell",
    "-pin",
];

/// Qualifiers that make a path-delay command unsupported: the whole
/// command is counted in `ignored_commands` rather than half-applied.
/// Superset of the exception list — delay commands additionally take the
/// `-rise_from`/`-fall_from`/`-rise_to`/`-fall_to` edge qualifiers and the
/// delay-specific `-early`/`-late`/`-datapath_only`/`-clock` options OpenSTA
/// either rejects (`STA-0563`) or scopes per edge (unmeasured; `md_through`
/// shows `-through` resolves but cannot be honoured).
const PATH_DELAY_UNSUPPORTED_FLAGS: [&str; 17] = [
    "-through",
    "-start",
    "-end",
    "-rise",
    "-fall",
    "-rise_from",
    "-fall_from",
    "-rise_to",
    "-fall_to",
    "-edge",
    "-points",
    "-lib_cell",
    "-pin",
    "-early",
    "-late",
    "-datapath_only",
    "-clock",
];

/// Command names that produce object selections. Ones the model does not
/// implement evaluate to [`Value::Opaque`] rather than a parse error.
fn is_object_selector(name: &str) -> bool {
    name.starts_with("get_") || name.starts_with("select_") || name.starts_with("all_")
}

/// A `-from`/`-to` argument as an [`ObjSel`]; `None` for selections the
/// exception model cannot honour (opaque selections) — the caller
/// counts the command ignored. Clock collections take the
/// [`Parser::clock_rows`] path at the call sites instead.
fn obj_sel(v: &Value) -> Option<ObjSel> {
    match v {
        Value::Ports(sel) => Some(ObjSel::Ports(sel.clone())),
        Value::Cells(sel) => Some(ObjSel::Cells(sel.clone())),
        Value::Pins(specs) => Some(ObjSel::Pins(specs.clone())),
        Value::Str(text) => Some(ObjSel::Names(
            text.split_whitespace().map(str::to_string).collect(),
        )),
        Value::Clocks(_) | Value::Nets(_) | Value::Opaque => None,
    }
}

/// The multicycle count as written: an integer, possibly negative for `-hold`.
fn multicycle_count(text: &str) -> Result<i64> {
    text.parse()
        .with_context(|| format!("`{text}` is not an integer multicycle count"))
}

/// `-setup N`/positional `N`/`-both N`: the setup count must be >= 1.
fn setup_count(n: i64, name: &str, flag: &str) -> Result<u32> {
    if !(1..=i64::from(u32::MAX)).contains(&n) {
        bail!("{name} {flag} needs a positive integer, found {n}");
    }
    Ok(n as u32)
}

/// `-hold H`/`-both N`: any integer is valid (0 = default, negative tightens —
/// measured on OpenSTA).
fn hold_count(n: i64, name: &str, flag: &str) -> Result<i32> {
    i32::try_from(n).with_context(|| format!("{name} {flag} count {n} is out of range"))
}

/// Value flags the parser drops without applying them; each occurrence is counted in
/// `Sdc::ignored_options`. The rest of the accepted flags is read by the command that owns it.
const IGNORED_VALUE_FLAGS: [&str; 2] = ["-lib_cell", "-pin"];

/// Value flags each command accepts, owned or ignored. A flag outside the command's set is an
/// error: silently dropping it (the old shared `VALUE_FLAGS` behaviour) accepted SDC that
/// OpenSTA rejects, e.g. `-clock` on `set_load`.
const DELAY_VALUE_FLAGS: [&str; 3] = ["-clock", "-lib_cell", "-pin"];
/// Boolean flags the delay commands understand; `-min`/`-max` pick the analysis corner and
/// `-add_delay` accumulates. `delay_rule` reads which are present before `positionals` runs.
const DELAY_BOOL_FLAGS: [&str; 3] = ["-add_delay", "-min", "-max"];
const PORT_VALUE_FLAGS: [&str; 2] = ["-lib_cell", "-pin"];
/// Boolean flags `set_load`/`set_fanout_load` consume: `-pin_load` matches the
/// applied port-pin domain, and `-wire_load` is a no-op on net objects in
/// OpenSTA (warning, same value applied). `load_rule` re-checks `-wire_load`
/// for the port domain, where it means something else.
const LOAD_BOOL_FLAGS: [&str; 2] = ["-pin_load", "-wire_load"];
/// `set_load` qualifiers the model cannot honour: corner-, transition-, or
/// subtractively-scoped loads. The whole command is counted in
/// `ignored_commands` rather than applied to both corners.
const LOAD_UNSUPPORTED_FLAGS: [&str; 5] =
    ["-min", "-max", "-rise", "-fall", "-subtract_pin_load"];
const CLOCK_VALUE_FLAGS: [&str; 3] = ["-name", "-period", "-waveform"];

/// The value after `flag`, or None when the flag is absent. A flag with no value is an error.
fn flag_value<'a>(args: &'a [Value], flag: &str) -> Result<Option<&'a str>> {
    match flag_arg(args, flag)? {
        Some(Value::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => bail!("{flag} needs a plain value"),
        None => Ok(None),
    }
}

/// The argument after `flag`, keeping its type: `-clock [get_clocks ...]` yields `Value::Clocks`.
fn flag_arg<'a>(args: &'a [Value], flag: &str) -> Result<Option<&'a Value>> {
    let Some(i) = args
        .iter()
        .position(|a| matches!(a, Value::Str(s) if s == flag))
    else {
        return Ok(None);
    };
    match args.get(i + 1) {
        Some(v @ (Value::Str(_) | Value::Clocks(_))) => Ok(Some(v)),
        _ => bail!("{flag} needs a value"),
    }
}

/// Whether `flag` appears as a bare word. Callers must run `positionals` afterwards, which
/// rejects a flag used as another flag's value, so a hit here is always a real flag.
fn has_flag(args: &[Value], flag: &str) -> bool {
    args.iter().any(|a| matches!(a, Value::Str(s) if s == flag))
}

/// `-1.5` or `-.5`: a dash followed by a decimal number. `-inf` is not one.
fn is_negative_number(text: &str) -> bool {
    text.strip_prefix('-').is_some_and(|rest| {
        rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') && rest.parse::<f32>().is_ok()
    })
}

/// The arguments that are neither a value flag, a boolean flag, nor the value of one. Any
/// other flag is an error: skipping it would drop a meaning-changing option such as `-setup`.
/// `value_flags` is the set the calling command accepts; the ones listed in `ignored_flags`
/// are counted into `ignored`. `bool_flags` take no value and are consumed silently — the
/// caller reads which are present beforehand.
fn positionals<'a>(
    args: &'a [Value],
    value_flags: &[&str],
    ignored_flags: &[&str],
    bool_flags: &[&str],
    ignored: &mut BTreeMap<String, usize>,
) -> Result<Vec<&'a Value>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match &args[i] {
            Value::Str(s) if value_flags.contains(&s.as_str()) => {
                // `-clock [get_clocks ...]` carries a `Value::Clocks`; it is the only flag
                // whose value may be a selection.
                match args.get(i + 1) {
                    Some(Value::Str(v)) if v.starts_with('-') && !is_negative_number(v) => {
                        bail!("{s} needs a value, found `{v}`");
                    }
                    Some(Value::Str(_)) => {}
                    Some(Value::Clocks(_)) if s == "-clock" => {}
                    _ => bail!("{s} needs a value"),
                }
                if ignored_flags.contains(&s.as_str()) {
                    *ignored.entry(s.clone()).or_default() += 1;
                }
                i += 2;
            }
            Value::Str(s) if bool_flags.contains(&s.as_str()) => {
                i += 1;
            }
            Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => {
                bail!("unsupported option `{s}`")
            }
            value => {
                out.push(value);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn value_and_ports(
    args: &[Value],
    value_flags: &[&str],
    ignored_flags: &[&str],
    bool_flags: &[&str],
    ignored: &mut BTreeMap<String, usize>,
) -> Result<(PortSel, f32)> {
    let [Value::Str(value), Value::Ports(sel)] =
        positionals(args, value_flags, ignored_flags, bool_flags, ignored)?.as_slice()
    else {
        bail!("expected a numeric value and a port selector");
    };
    Ok((sel.clone(), parse_f32(value)?))
}

/// Four-operator arithmetic with parentheses over decimal numbers. Values are f64; the caller
/// narrows them. Tcl's integer division is refused rather than emulated.
fn eval_arith(text: &str) -> Option<f64> {
    let mut arith = Arith {
        src: text.as_bytes(),
        pos: 0,
    };
    let (value, _) = arith.expr()?;
    arith.skip_ws();
    (arith.pos == text.len() && value.is_finite()).then_some(value)
}

/// Recursive-descent evaluator. Each result carries whether it is an integer, for the division rule.
struct Arith<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Arith<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn expr(&mut self) -> Option<(f64, bool)> {
        let (mut value, mut is_int) = self.term()?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(op @ (b'+' | b'-')) => op,
                _ => return Some((value, is_int)),
            };
            self.pos += 1;
            let (rhs, rhs_int) = self.term()?;
            value = if op == b'+' { value + rhs } else { value - rhs };
            is_int &= rhs_int;
        }
    }

    fn term(&mut self) -> Option<(f64, bool)> {
        let (mut value, mut is_int) = self.factor()?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(op @ (b'*' | b'/')) => op,
                _ => return Some((value, is_int)),
            };
            self.pos += 1;
            let (rhs, rhs_int) = self.factor()?;
            if op == b'/' && is_int && rhs_int {
                return None;
            }
            value = if op == b'*' { value * rhs } else { value / rhs };
            is_int &= rhs_int;
        }
    }

    fn factor(&mut self) -> Option<(f64, bool)> {
        self.skip_ws();
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let inner = self.expr()?;
            self.skip_ws();
            if self.peek() != Some(b')') {
                return None;
            }
            self.pos += 1;
            return Some(inner);
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).ok()?;
        let value = text.parse::<f64>().ok()?;
        Some((value, !text.contains('.')))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GCD: &str = "create_clock -name core_clk -period 5.0 [get_ports clk]\n\
        set_input_delay 1.0 -clock core_clk [all_inputs -no_clocks]\n\
        set_output_delay 1.0 -clock core_clk [all_outputs]\n\
        set_input_transition 0.15 [all_inputs]\n\
        set_load 0.05 [all_outputs]\n";

    const IBEX: &str = include_str!("../../../../tests/designs/ibex/ibex.sdc");
    const AES: &str = include_str!("../../../../tests/designs/aes/aes.sdc");
    const JPEG: &str = include_str!("../../../../tests/designs/jpeg/jpeg.sdc");
    const PICORV32: &str = include_str!("../../../../tests/designs/picorv32/picorv32.sdc");
    // `write_sdc` output for gcd.propagated.sdc, emitted by OpenSTA inside the pinned
    // LibreLane container (tests/designs/_dump_sdc.sh pattern). It uses only the supported
    // subset: per-port `-add_delay` rules, `-pin_load`, `set_propagated_clock`, derate-free.
    const GCD_WRITE_SDC: &str = include_str!("../../../../tests/designs/gcd/gcd.write_sdc");

    #[test]
    fn parses_the_gcd_constraints() {
        let sdc = parse_sdc(GCD).unwrap();
        assert_eq!(sdc.clock_port(), Some("clk"));
        assert_eq!(sdc.period_ns(), 5.0);
        assert_eq!(sdc.input_delay_ns("req_val"), (1.0, 1.0));
        assert_eq!(sdc.output_delay_ns("resp_val"), (1.0, 1.0));
        assert_eq!(sdc.input_transition_ns("req_val"), 0.15);
        assert_eq!(sdc.output_load_pf("resp_val"), 0.05);
    }

    #[test]
    fn later_commands_override_earlier_ones() {
        let sdc = parse_sdc(&format!("{GCD}set_load 0.2 [get_ports {{a b}}]\n")).unwrap();
        assert_eq!(sdc.output_load_pf("a"), 0.2);
        assert_eq!(sdc.output_load_pf("c"), 0.05);
    }

    #[test]
    fn rejects_unsupported_commands() {
        assert!(parse_sdc("frobnicate the_things\n").is_err());
    }

    #[test]
    fn parses_path_delay_commands() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_max_delay 1.0 -from [get_cells u1] -to [get_cells u2]\n\
             set_min_delay -0.5 -from [all_registers]\n\
             set_max_delay 2.5 -to [get_ports y]\n\
             set_min_delay 0.1\n"
        ))
        .unwrap();
        assert_eq!(sdc.path_delays.len(), 4);
        let pd = &sdc.path_delays;
        assert_eq!(pd[0].kind, PathDelayKind::Max);
        assert_eq!(pd[0].value_ns, 1.0);
        assert!(matches!(
            pd[0].from,
            Some(ObjSel::Cells(CellSel::Cells(ref n))) if *n == ["u1"]
        ));
        assert!(matches!(
            pd[0].to,
            Some(ObjSel::Cells(CellSel::Cells(ref n))) if *n == ["u2"]
        ));
        assert_eq!(pd[1].kind, PathDelayKind::Min);
        assert_eq!(pd[1].value_ns, -0.5);
        assert!(matches!(
            pd[1].from,
            Some(ObjSel::Cells(CellSel::AllRegisters))
        ));
        assert!(pd[1].to.is_none());
        assert!(pd[2].from.is_none());
        assert!(matches!(
            pd[2].to,
            Some(ObjSel::Ports(PortSel::Ports(ref n))) if *n == ["y"]
        ));
        assert!(pd[3].from.is_none() && pd[3].to.is_none());
    }

    #[test]
    fn path_delay_unsupported_qualifiers_are_ignored_not_fatal() {
        for snippet in [
            "set_max_delay 1.0 -through [get_cells u1] -from a -to b",
            "set_min_delay 0.5 -rise -from a",
            "set_max_delay 1.0 -rise_from [get_cells u1] -to b",
            "set_min_delay 0.5 -fall_to [get_ports y]",
            "set_max_delay 1.0 -edge 1 -from a",
            "set_min_delay 0.5 -points u1 -to b",
            "set_max_delay 1.0 -lib_cell BUF -from a",
            "set_min_delay 0.5 -pin u1/D -to b",
            "set_max_delay 1.0 -early -from a",
            "set_min_delay 0.5 -late -from a",
            "set_max_delay 1.0 -datapath_only -from a",
            "set_min_delay 0.5 -clock core_clk -from a",
            "set_max_delay 1.0 -start -from a",
            "set_min_delay 0.5 -end -to b",
        ] {
            let sdc = parse_sdc(&format!("{GCD}{snippet}\n")).unwrap();
            assert!(sdc.path_delays.is_empty(), "stored: {snippet}");
            let counted = sdc
                .ignored_commands
                .get("set_max_delay")
                .copied()
                .unwrap_or(0)
                + sdc
                    .ignored_commands
                    .get("set_min_delay")
                    .copied()
                    .unwrap_or(0);
            assert_eq!(counted, 1, "not counted: {snippet}");
        }
    }

    #[test]
    fn path_delay_needs_a_value() {
        assert!(parse_sdc(&format!("{GCD}set_max_delay -from a -to b\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_min_delay\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_max_delay 1.0 2.0\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_min_delay 0.5 -from\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_max_delay 1.0 -from a -from b\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_min_delay 0.5 -bogus x\n")).is_err());
    }

    #[test]
    fn path_delay_unresolvable_selectors_are_ignored_not_fatal() {
        let snippet = "set_max_delay 1.0 -from [select_cells u1]";
        let sdc = parse_sdc(&format!("{GCD}{snippet}\n")).unwrap();
        assert!(sdc.path_delays.is_empty(), "stored: {snippet}");
        assert!(
            sdc.ignored_commands.contains_key("set_max_delay")
                || sdc.ignored_commands.contains_key("set_min_delay"),
            "not counted: {snippet}"
        );
    }

    #[test]
    fn parses_false_path_exceptions() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_false_path -from [get_cells u1] -to [get_cells {{u2 u3}}]\n\
             set_false_path -from [all_registers] -to [all_outputs]\n\
             set_false_path -from d_in\n"
        ))
        .unwrap();
        assert_eq!(sdc.exceptions.len(), 3);
        let ex = &sdc.exceptions[0];
        assert_eq!(ex.kind, ExceptionKind::FalsePath);
        assert!(matches!(
            ex.from,
            Some(ObjSel::Cells(CellSel::Cells(ref n))) if *n == ["u1"]
        ));
        assert!(matches!(
            ex.to,
            Some(ObjSel::Cells(CellSel::Cells(ref n))) if *n == ["u2", "u3"]
        ));
        assert!(matches!(
            sdc.exceptions[1].from,
            Some(ObjSel::Cells(CellSel::AllRegisters))
        ));
        assert!(matches!(
            sdc.exceptions[1].to,
            Some(ObjSel::Ports(PortSel::AllOutputs))
        ));
        // A bare `-from` name stays symbolic: it resolves against ports and
        // cells at design time.
        assert!(matches!(
            sdc.exceptions[2].from,
            Some(ObjSel::Names(ref n)) if *n == ["d_in"]
        ));
        assert!(sdc.exceptions[2].to.is_none());
    }

    #[test]
    fn parses_multicycle_path_forms() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_multicycle_path 2 -from [get_cells u1] -to [get_cells u2]\n\
             set_multicycle_path -setup 3 -hold 1 -from [all_registers]\n\
             set_multicycle_path -hold -1 -to [get_ports y]\n"
        ))
        .unwrap();
        assert_eq!(sdc.exceptions.len(), 3);
        let ex = &sdc.exceptions;
        // Bare positional N is the setup count.
        assert_eq!((ex[0].setup_n, ex[0].hold_n), (Some(2), None));
        // `-setup X -hold Y` combined is valid SDC (OpenSTA rejects it).
        assert_eq!((ex[1].setup_n, ex[1].hold_n), (Some(3), Some(1)));
        // `-hold` accepts zero and negative counts.
        assert_eq!((ex[2].setup_n, ex[2].hold_n), (None, Some(-1)));
        assert!(ex.iter().all(|e| e.kind == ExceptionKind::Multicycle));
    }

    #[test]
    fn multicycle_both_is_ignored_not_guessed() {
        // `-both` is valid SDC but OpenSTA rejects it (STA-0563): its
        // hold-side semantics cannot be verified, so the command is counted
        // ignored rather than mapped by guesswork.
        let sdc = parse_sdc(&format!("{GCD}set_multicycle_path -both 2\n")).unwrap();
        assert!(sdc.exceptions.is_empty());
        assert_eq!(sdc.ignored_commands["set_multicycle_path"], 1);
    }

    #[test]
    fn multicycle_path_needs_a_count() {
        assert!(parse_sdc(&format!("{GCD}set_multicycle_path -from a -to b\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_multicycle_path -setup 0\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_multicycle_path -setup\n")).is_err());
        assert!(parse_sdc(&format!("{GCD}set_multicycle_path -from a -from b\n")).is_err());
    }

    #[test]
    fn exception_unsupported_qualifiers_are_ignored_not_fatal() {
        for snippet in [
            "set_false_path -through [get_cells u1] -from a -to b",
            "set_false_path -rise -from a -to b",
            "set_multicycle_path -setup 2 -start [get_cells u1] -to b",
            "set_false_path -edge 1 -from a",
        ] {
            let sdc = parse_sdc(&format!("{GCD}{snippet}\n")).unwrap();
            assert!(sdc.exceptions.is_empty(), "stored: {snippet}");
            let counted = sdc
                .ignored_commands
                .get("set_false_path")
                .copied()
                .unwrap_or(0)
                + sdc
                    .ignored_commands
                    .get("set_multicycle_path")
                    .copied()
                    .unwrap_or(0);
            assert_eq!(counted, 1, "not counted: {snippet}");
        }
    }

    #[test]
    fn exception_unresolvable_selectors_are_ignored_not_fatal() {
        for snippet in [
            // Nested selectors the model cannot represent stay opaque.
            "set_multicycle_path -setup 2 -from [select_cells u1]",
            // Pin specs cannot subtract from a port collection.
            "set_false_path -from [remove_from_collection [all_inputs] [get_pins u1/A]]",
        ] {
            let sdc = parse_sdc(&format!("{GCD}{snippet}\n")).unwrap();
            assert!(sdc.exceptions.is_empty(), "stored: {snippet}");
            assert!(
                sdc.ignored_commands.contains_key("set_false_path")
                    || sdc.ignored_commands.contains_key("set_multicycle_path"),
                "not counted: {snippet}"
            );
        }
    }

    #[test]
    fn false_path_setup_hold_flags_are_noops() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_false_path -setup 2 -hold 1 -from [all_inputs]\n"
        ))
        .unwrap();
        assert_eq!(sdc.exceptions.len(), 1);
        assert_eq!(sdc.exceptions[0].kind, ExceptionKind::FalsePath);
        assert_eq!(sdc.exceptions[0].setup_n, None);
        assert!(sdc.exceptions[0].hold_n.is_none());
    }

    #[test]
    fn parses_pin_selectors_in_exceptions() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_false_path -from [get_pins {{u1/CLK u2/CLK}}] -to [get_pins u3/D]\n\
             set_multicycle_path -setup 2 -from u_ff0/CLK -to u_ff1/D\n\
             set_false_path -from [remove_from_collection [get_pins {{u1/CLK u2/CLK}}] [get_pins u1/CLK]] -to b\n"
        ))
        .unwrap();
        assert_eq!(sdc.exceptions.len(), 3);
        assert!(matches!(
            sdc.exceptions[0].from,
            Some(ObjSel::Pins(ref s)) if s.as_slice() == ["u1/CLK", "u2/CLK"]
        ));
        assert!(matches!(
            sdc.exceptions[0].to,
            Some(ObjSel::Pins(ref s)) if *s == ["u3/D"]
        ));
        // Bare `instance/pin` names are pin specs too.
        assert!(matches!(
            sdc.exceptions[1].from,
            Some(ObjSel::Names(ref s)) if s.iter().any(|n| n == "u_ff0/CLK")
        ));
        assert!(matches!(
            sdc.exceptions[2].from,
            Some(ObjSel::Pins(ref s)) if *s == ["u2/CLK"]
        ));
    }

    #[test]
    fn remove_from_collection_on_cells() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_false_path -from [remove_from_collection [all_registers] [get_cells u_keep]] -to [all_outputs]\n"
        ))
        .unwrap();
        assert_eq!(sdc.exceptions.len(), 1);
        assert!(matches!(
            sdc.exceptions[0].from,
            Some(ObjSel::Cells(CellSel::Except { .. }))
        ));
    }

    #[test]
    fn parses_the_ibex_tcl_sdc() {
        let sdc = parse_sdc(IBEX).unwrap();
        assert_eq!(sdc.period_ns(), 8.0);
        assert_eq!(sdc.clock_name(), Some("core_clock"));
        assert_eq!(sdc.clock_port(), Some("clk_i"));
        assert_eq!(sdc.design_clock().unwrap().latency_ns, 1.095);
        assert_eq!(sdc.clock("vclk_core_clock").unwrap().latency_ns, 1.095);
        assert_eq!(sdc.input_delay_ns("instr_rdata_i"), (1.6, 1.6));
        assert_eq!(
            sdc.input_delay_ns("clk_i"),
            (0.0, 0.0),
            "-no_clocks must exclude the clock port"
        );
        assert_eq!(sdc.input_clock_latency_ns("instr_rdata_i"), (1.095, 1.095));
        assert_eq!(sdc.output_delay_ns("instr_req_o"), (1.6, 1.6));
        assert_eq!(sdc.output_clock_latency_ns("instr_req_o"), (1.095, 1.095));
    }

    #[test]
    fn counts_ignored_lib_cell_and_pin_options() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_input_delay 1.0 -lib_cell buf_2 -pin A -clock core_clk [all_inputs -no_clocks]\n\
             set_output_delay 2.0 -pin out_pin -clock core_clk [all_outputs]\n"
        ))
        .unwrap();
        // The restrictions are dropped, so the delays still apply to every
        // selected port — overwriting the same-anchor (clock, corner) slot
        // GCD's rules hold (measured `t2_clkdup`).
        assert_eq!(sdc.input_delay_ns("req_val"), (1.0, 1.0));
        assert_eq!(sdc.output_delay_ns("resp_val"), (2.0, 2.0));
        assert_eq!(sdc.ignored_options["-lib_cell"], 1);
        assert_eq!(sdc.ignored_options["-pin"], 2);
        assert_eq!(sdc.commands, 7);
    }

    #[test]
    fn rejects_flags_the_command_does_not_own() {
        let cases = [
            // `-clock` belongs to the delay commands; on `set_load` it must not be dropped.
            "set_load 0.05 -clock core_clk [all_outputs]",
            "set_input_transition 0.15 -name vclk [all_inputs]",
            "set_input_delay 1.0 -name vclk -clock core_clk [all_inputs]",
            "create_clock -name core_clk -period 5.0 -lib_cell buf [get_ports clk]",
            // A value flag with no value, or whose value is another flag.
            "set_input_delay 1.0 -clock core_clk [all_inputs -no_clocks] -pin",
            "set_input_delay 1.0 -lib_cell -clock core_clk [all_inputs]",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn flat_sdcs_keep_their_values() {
        let cases = [
            (AES, 3.6, 0.72),
            (JPEG, 5.0, 1.0),
            (PICORV32, 8.0, 1.0),
            (GCD, 5.0, 1.0),
        ];
        for (text, period_ns, io_ns) in cases {
            let sdc = parse_sdc(text).unwrap();
            assert_eq!(sdc.clock_name(), Some("core_clk"));
            assert_eq!(sdc.clock_port(), Some("clk"));
            assert_eq!(sdc.period_ns(), period_ns);
            assert_eq!(sdc.input_delay_ns("req_val"), (io_ns, io_ns));
            assert_eq!(sdc.output_delay_ns("resp_val"), (io_ns, io_ns));
            assert_eq!(sdc.input_transition_ns("req_val"), 0.15);
            assert_eq!(sdc.output_load_pf("resp_val"), 0.05);
            assert!(sdc.clocks.iter().all(|c| c.latency_ns == 0.0));
        }
    }

    #[test]
    fn rejects_tcl_outside_the_subset() {
        let cases = [
            "set_load $nope [all_outputs]",
            "set_load [foo] [all_outputs]",
            "set_load 0.1 [all_outputs",
            "set_load 0.1 {unterminated",
            "set_load 0.1 \\\n[all_outputs]",
            "set n [expr 8 / 2]",
            "set_input_delay 1.0 -clock nope [all_inputs]",
            "if {1} {set_load 0.1 [all_outputs]}",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn virtual_clock_keeps_its_own_period() {
        // Virtual periods are free-form (measured OpenSTA behaviour); only a
        // non-positive period is an error, at `finish`.
        let sdc = parse_sdc(&format!("{GCD}create_clock -name vclk -period 4.0\n")).unwrap();
        let vclk = sdc.clock("vclk").unwrap();
        assert_eq!(vclk.period_ns, 4.0);
        assert_eq!(vclk.port, None);
    }

    #[test]
    fn second_design_clock_appends_a_row() {
        let sdc = parse_sdc(&format!(
            "{GCD}create_clock -name core_clk2 -period 5.0 [get_ports other]\n"
        ))
        .unwrap();
        assert_eq!(sdc.clocks.len(), 2);
        let second = &sdc.clocks[1];
        assert_eq!(second.name, "core_clk2");
        assert_eq!(second.port.as_deref(), Some("other"));
        assert_eq!(second.period_ns, 5.0);
        assert_eq!(sdc.design_clock_count(), 2);
    }

    #[test]
    fn delay_min_max_and_add_delay() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay -min 0.5 -clock core_clk [get_ports {{a}}]\n\
             set_input_delay -max 2.0 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_output_delay -min 0.25 -clock core_clk [all_outputs]\n"
        ))
        .unwrap();
        // GCD's `all_inputs` rule contributes 1.0 to both corners first; `-add_delay` widens
        // the max corner to max(1.0, 2.0) = 2.0 rather than summing (OpenSTA semantics).
        assert_eq!(sdc.input_delay_ns("a"), (0.5, 2.0));
        assert_eq!(sdc.output_delay_ns("resp_val"), (0.25, 1.0));
        // `-min` and `-max` together, or `-add_delay` on `set_load`, are still errors.
        for snippet in [
            "set_input_delay -min -max 1.0 -clock core_clk [all_inputs]",
            "set_load -add_delay 0.1 [all_outputs]",
        ] {
            assert!(
                parse_sdc(&format!("{GCD}{snippet}\n")).is_err(),
                "accepted: {snippet}"
            );
        }
    }

    #[test]
    fn add_delay_widens_instead_of_summing() {
        // Measured against OpenSTA on gcd: `-add_delay` keeps every constraint and folds each
        // corner — the min corner takes the smallest value, the max corner the largest.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay 1.0 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.4 -add_delay -clock core_clk [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.input_delay_ns("a"), (0.4, 1.0));

        // A later plain rule prunes entries anchored to a different `-clock`
        // (measured `t2_clkcor`) and overwrites same-anchor slots
        // (measured `t2_clkdup`), so the accumulated `-add_delay` entries are
        // gone and only the plain 0.8 survives.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay 1.0 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.4 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.8 -clock core_clk [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.input_delay_ns("a"), (0.8, 0.8));
    }

    /// The `t2_clkcor*`/`t2_clkdup` measured rule: a port's delay spec is an
    /// entry list — a plain (non-`-add_delay`) rule removes entries anchored
    /// to a different `-clock`, port-wide, and overwrites the corner(s) it
    /// covers on same-anchor entries, then appends its own; `-add_delay`
    /// appends without pruning. Each anchored entry is a check on its own
    /// clock's grid.
    #[test]
    fn delay_anchor_prune_is_port_wide() {
        // `-clock`d `-max` then a bare `-min`: the bare rule clears the
        // cka-anchored entry — no anchored check survives on either corner
        // (`t2_clkcor`'s `d_in` times nowhere in OpenSTA).
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_input_delay -max 0.7 -clock cka [get_ports {{a}}]\n\
            set_input_delay -min 0.1 [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.declared_input_clock_rows("a"), [vec![], vec![]]);
        // Opposite order (`t2_clkcor2`'s `d_in`): the `-clock`d rule prunes the
        // bare one — the max corner alone keeps an anchor.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_input_delay -min 0.1 [get_ports {{a}}]\n\
            set_input_delay -max 0.7 -clock cka [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.declared_input_clock_rows("a"),
            [Vec::<(usize, f32)>::new(), vec![(0, 0.7)]]
        );
        // `-add_delay` appends without pruning: two `-max` entries survive
        // under different anchors (`t2_clkcor2`'s `yb` — OpenSTA reports the
        // port once per anchor) and the earlier `-min @ckb` is gone.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_output_delay -min 0.1 -clock ckb [get_ports {{y}}]\n\
            set_output_delay -max 0.7 -clock cka [get_ports {{y}}]\n\
            set_output_delay -max 0.4 -clock ckb -add_delay [get_ports {{y}}]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.declared_output_clock_rows("y"),
            [Vec::<(usize, f32)>::new(), vec![(0, 0.7), (1, 0.4)]]
        );
        // Same-clock entries survive a plain rule on the other sense
        // (`t2_int`'s `d_in` times on both corners).
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_input_delay -min 0.1 -clock cka [get_ports {{a}}]\n\
            set_input_delay -max 0.7 -clock cka [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.declared_input_clock_rows("a"),
            [vec![(0, 0.1)], vec![(0, 0.7)]]
        );
        // Same-clock *same-sense* duplicates overwrite per (clock, corner)
        // slot — last wins, not worst-fold (measured `t2_clkdup`: `d_in`'s
        // `-min 0.1` then `-min 0.3` reports the 0.3 check).
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_input_delay -min 0.1 -clock cka [get_ports {{a}}]\n\
            set_input_delay -min 0.3 -clock cka [get_ports {{a}}]\n\
            set_input_delay -max 0.7 -clock cka [get_ports {{a}}]\n\
            set_input_delay -max 0.4 -clock cka [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.declared_input_clock_rows("a"),
            [vec![(0, 0.3)], vec![(0, 0.4)]]
        );
        // A `Both` entry under a same-clock `-max` rule degrades to the
        // uncovered corner rather than dropping (`t2_clkdup`'s `yb` keeps
        // the 0.5 min check while the max slot takes 0.3).
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_output_delay 0.5 -clock ckb [get_ports {{y}}]\n\
            set_output_delay -max 0.3 -clock ckb [get_ports {{y}}]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.declared_output_clock_rows("y"),
            [vec![(1, 0.5)], vec![(1, 0.3)]]
        );
    }

    #[test]
    fn clock_reference_falls_back_to_the_source_port_name() {
        // `create_clock` without `-name`: the port name is the clock handle, as `all_clocks`
        // already assumed.
        let sdc = parse_sdc(
            "create_clock -period 5.0 [get_ports clk]\n\
             set_input_delay 1.0 -clock clk [get_ports {a}]\n\
             set_clock_transition 0.1 [all_clocks]\n",
        )
        .unwrap();
        assert_eq!(sdc.input_delay_ns("a"), (1.0, 1.0));
        assert_eq!(sdc.clock_transition_ns(), 0.1);
    }

    #[test]
    fn propagated_clock_and_design_rules_reject_flags() {
        for snippet in [
            "set_propagated_clock -bogus [get_ports {clk}]",
            "unset_propagated_clock -x",
            "set_max_transition 0.75 -bogus [current_design]",
            "set_max_fanout 10.0 0.5 [current_design]",
        ] {
            assert!(
                parse_sdc(&format!("{GCD}{snippet}\n")).is_err(),
                "accepted: {snippet}"
            );
        }
        // The emitted forms still parse; the propagated clock is modelled, `set_max_*` is counted.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_propagated_clock [get_ports {{clk}}]\n\
             set_max_transition 0.75 [current_design]\n"
        ))
        .unwrap();
        assert!(sdc.propagated_clock());
        assert_eq!(sdc.ignored_commands["set_max_transition"], 1);
    }

    #[test]
    fn propagated_clock_flag_and_source_latency() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_clock_latency 1.0 [get_clocks core_clk]\n\
             set_clock_latency -source 0.2 [get_clocks core_clk]\n\
             set_propagated_clock [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert!(sdc.propagated_clock());
        assert_eq!(sdc.clock_latency_ns(), 1.0);
        assert_eq!(sdc.clock_source_latency_ns(), 0.2);

        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_propagated_clock [get_clocks core_clk]\n\
             unset_propagated_clock [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert!(!sdc.propagated_clock());

        for snippet in [
            // The virtual I/O clock cannot be propagated.
            "set_propagated_clock [get_clocks vclk]",
            "set_clock_latency -early 0.1 [get_clocks core_clk]",
            "set_clock_latency -source",
        ] {
            let text = if snippet.contains("vclk") {
                format!("{GCD}create_clock -name vclk -period 5.0\n{snippet}\n")
            } else {
                format!("{GCD}{snippet}\n")
            };
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn parses_the_write_sdc_output() {
        // The shape `write_sdc` emits for a routed design: per-port `-add_delay` rules,
        // `-pin_load`, driving cells, derate, clock transition, and `set_max_*` rules.
        let sdc = parse_sdc(
            "current_design gcd\n\
             create_clock -name clk -period 5.0 [get_ports {clk}]\n\
             set_clock_transition 0.15 [get_clocks {clk}]\n\
             set_clock_uncertainty 0.25 clk\n\
             set_input_delay 1.0 -clock [get_clocks {clk}] -add_delay [get_ports {req_val}]\n\
             set_output_delay 1.0 -clock [get_clocks {clk}] -add_delay [get_ports {resp_val}]\n\
             set_load -pin_load 0.0334 [get_ports {resp_val}]\n\
             set_driving_cell -lib_cell sky130_fd_sc_hd__inv_2 -pin {Y} \
             -input_transition_rise 0.0 -input_transition_fall 0.0 [get_ports {req_val}]\n\
             set_timing_derate -early 0.95\n\
             set_timing_derate -late 1.05\n\
             set_max_transition 0.75 [current_design]\n\
             set_max_capacitance 0.2 [current_design]\n\
             set_max_fanout 10.0 [current_design]\n",
        )
        .unwrap();
        assert_eq!(sdc.clock_transition_ns(), 0.15);
        assert_eq!(sdc.derate_early.factors[DERATE_CELL][1][0], 0.95);
        assert_eq!(sdc.derate_late.factors[DERATE_CELL][1][0], 1.05);
        assert_eq!(sdc.setup_uncertainty_ns(), 0.25);
        assert_eq!(sdc.input_delay_ns("req_val"), (1.0, 1.0));
        assert_eq!(sdc.output_load_pf("resp_val"), 0.0334);
        let drv = sdc.driving_cell("req_val", true).unwrap();
        assert_eq!(drv.cell, "sky130_fd_sc_hd__inv_2");
        assert_eq!(sdc.ignored_commands["set_max_transition"], 1);
        assert_eq!(sdc.ignored_commands["set_max_capacitance"], 1);
        assert_eq!(sdc.ignored_commands["set_max_fanout"], 1);
    }

    #[test]
    fn timing_derate_selectors_and_last_wins() {
        // OpenSTA shape: one corner flag per command, positional value, and each
        // command overwrites only the categories its flags select. `-cell_check` is
        // data-side only; `-rise`/`-fall` pick the transition entering the arc.
        let sdc = parse_sdc(
            "create_clock -name clk -period 2.0 [get_ports clk]\n\
             set_timing_derate -early 0.9 -cell_delay\n\
             set_timing_derate -late 1.1 -net_delay -data\n\
             set_timing_derate -early 0.8 -cell_check\n\
             set_timing_derate -late 1.2 -cell_delay -rise\n\
             set_timing_derate -early 0.7 -cell_delay\n",
        )
        .unwrap();
        let f = &sdc.derate_early.factors;
        for row in f[DERATE_CELL] {
            assert_eq!(row, [0.7, 0.7]);
        }
        for row in f[DERATE_NET] {
            assert_eq!(row, [1.0, 1.0]);
        }
        assert_eq!(f[DERATE_CHECK], [[1.0, 1.0], [0.8, 0.8]]);
        let l = &sdc.derate_late.factors;
        assert_eq!(l[DERATE_CELL][0], [1.2, 1.0]);
        assert_eq!(l[DERATE_CELL][1], [1.2, 1.0]);
        assert_eq!(l[DERATE_NET][0], [1.0, 1.0]);
        assert_eq!(l[DERATE_NET][1], [1.1, 1.1]);
        assert_eq!(l[DERATE_CHECK], [[1.0, 1.0], [1.0, 1.0]]);
        for bad in [
            "set_timing_derate -early 0.9 -late 1.1\n",
            "set_timing_derate -late 1.1 -early 0.9\n",
            "set_timing_derate -early\n",
            "set_timing_derate -early 0.9 0.5\n",
            "set_timing_derate -early 0.9 [get_cells]\n",
            "set_timing_derate 0.9\n",
        ] {
            assert!(
                parse_sdc(&format!(
                    "create_clock -name clk -period 2.0 [get_ports clk]\n{bad}"
                ))
                .is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn timing_derate_combined_early_late() {
        // `-early -late` on one command applies the same derate to both corners.
        let sdc = parse_sdc(
            "create_clock -name clk -period 2.0 [get_ports clk]\n\
             set_timing_derate -early -late -cell_delay 0.9\n\
             set_timing_derate -early -late -net_delay -data -rise 1.1\n",
        )
        .unwrap();
        for corner in [&sdc.derate_early, &sdc.derate_late] {
            for row in corner.factors[DERATE_CELL] {
                assert_eq!(row, [0.9, 0.9]);
            }
            assert_eq!(corner.factors[DERATE_NET][0], [1.0, 1.0]);
            assert_eq!(corner.factors[DERATE_NET][1], [1.1, 1.0]);
        }
        for bad in [
            "set_timing_derate -early -early 0.9\n",
            "set_timing_derate -late -late 0.9\n",
            "set_timing_derate -early -late\n",
        ] {
            assert!(
                parse_sdc(&format!(
                    "create_clock -name clk -period 2.0 [get_ports clk]\n{bad}"
                ))
                .is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn parses_stock_write_sdc_unchanged() {
        // The N2 criterion: a `write_sdc` file emitted by a stock LibreLane/OpenSTA run on a
        // public design parses without error and without dropping any command.
        let sdc = parse_sdc(GCD_WRITE_SDC).unwrap();
        assert_eq!(sdc.period_ns(), 5.0);
        assert!(sdc.propagated_clock());
        assert_eq!(sdc.clock_name(), Some("core_clk"));
        assert_eq!(sdc.input_delay_ns("req_val"), (1.0, 1.0));
        assert_eq!(sdc.output_delay_ns("resp_val"), (1.0, 1.0));
        assert_eq!(sdc.output_load_pf("resp_msg[0]"), 0.05);
        assert!(
            sdc.ignored_commands.is_empty(),
            "{:?}",
            sdc.ignored_commands
        );
    }

    #[test]
    fn set_units_declares_the_model_units() {
        // ns and pF are what every parsed value already means, so they are a no-op; the
        // unit keys the model never reads are accepted too.
        let sdc = parse_sdc(&format!(
            "{GCD}set_units -time ns -capacitance pF -resistance kOhm -voltage V\n"
        ))
        .unwrap();
        assert_eq!(sdc.input_delay_ns("req_val"), (1.0, 1.0));
        for bad in [
            "set_units -time ps\n",
            "set_units -capacitance fF\n",
            "set_units -time\n",
            "set_units -bogus ns\n",
            "set_units ns\n",
        ] {
            assert!(
                parse_sdc(&format!("{GCD}{bad}")).is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn remove_from_collection_subtracts_selections() {
        // The result feeds the same port selectors as the commands it replaces: a predicate
        // base keeps matching through `PortSel::Except`, an explicit list filters by name.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay 2.0 -clock core_clk [remove_from_collection [all_inputs -no_clocks] [get_ports {{req_val}}]]\n\
             set_output_delay 3.0 -clock core_clk [remove_from_collection [get_ports {{resp_val resp_msg[0]}}] [get_ports {{resp_msg[0]}}]]\n"
        ))
        .unwrap();
        // The new rule overwrites the same-anchor (clock, corner) slots
        // GCD's rules hold (measured `t2_clkdup`), so the selected ports
        // report the new value on both corners.
        assert_eq!(sdc.input_delay_ns("reset"), (2.0, 2.0));
        assert_eq!(sdc.input_delay_ns("req_msg[0]"), (2.0, 2.0));
        assert_eq!(
            sdc.input_delay_ns("req_val"),
            (1.0, 1.0),
            "req_val was removed"
        );
        assert_eq!(sdc.output_delay_ns("resp_val"), (3.0, 3.0));
        assert_eq!(
            sdc.output_delay_ns("resp_msg[0]"),
            (1.0, 1.0),
            "resp_msg[0] was removed"
        );
    }

    #[test]
    fn remove_from_collection_rejects_what_it_cannot_name() {
        for bad in [
            // A predicate selection cannot be enumerated into names to subtract.
            "set_input_delay 1.0 [remove_from_collection [all_inputs] [all_outputs]]",
            "set_input_delay 1.0 [remove_from_collection [get_ports {a}]]",
            "set_input_delay 1.0 [remove_from_collection]",
        ] {
            assert!(
                parse_sdc(&format!("{GCD}{bad}\n")).is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn rejects_non_numeric_or_extra_positional_words() {
        let cases = [
            "set_load foo [all_outputs]",
            "set_load foo 0.05 [all_outputs]",
            "set_load 0.05 extra [all_outputs]",
            "set_input_delay 1.0 -clock core_clk clk",
            "set_clock_uncertainty foo 0.1",
            "set_clock_uncertainty 0.1 0.2",
            "set_clock_uncertainty -setup -hold 0.1",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn keeps_negative_values_and_the_known_flags() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_input_delay -0.5 -clock core_clk [all_inputs -no_clocks]\n"
        ))
        .unwrap();
        // The negative value overwrites the same-anchor slot GCD's 1.0
        // holds on both corners (measured `t2_clkdup`).
        assert_eq!(sdc.input_delay_ns("req_val"), (-0.5, -0.5));
        let sdc = parse_sdc(&format!("{GCD}set_clock_uncertainty 0.1\n")).unwrap();
        assert_eq!(sdc.setup_uncertainty_ns(), 0.1);
        assert_eq!(sdc.hold_uncertainty_ns(), 0.1);
    }

    #[test]
    fn clock_uncertainty_setup_hold_and_clock_list() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_clock_uncertainty -setup 0.2 [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert_eq!(sdc.setup_uncertainty_ns(), 0.2);
        assert_eq!(sdc.hold_uncertainty_ns(), 0.0);
        let sdc = parse_sdc(&format!(
            "{GCD}set_clock_uncertainty -hold 0.1 [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert_eq!(sdc.setup_uncertainty_ns(), 0.0);
        assert_eq!(sdc.hold_uncertainty_ns(), 0.1);
        let sdc = parse_sdc(&format!(
            "{GCD}set_clock_uncertainty 0.3 [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert_eq!(sdc.setup_uncertainty_ns(), 0.3);
        assert_eq!(sdc.hold_uncertainty_ns(), 0.3);
    }

    #[test]
    fn set_load_accepts_nets_and_still_last_wins_on_ports() {
        // `get_nets` produces a symbolic net selection stored as a load rule;
        // the port domain keeps its existing last-matching behaviour.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_load 0.10 [get_nets d1]\n\
             set_load 0.20 [get_net {{d2 d3}}]\n\
             set load_nets [get_nets d1]\n\
             set_load 0.30 $load_nets\n\
             set_load 0.99 [get_ports {{resp_val}}]\n"
        ))
        .unwrap();
        let net_rules: Vec<_> = sdc.net_loads().map(|(n, v)| (n.clone(), v)).collect();
        assert_eq!(
            net_rules,
            vec![
                (vec!["d1".to_string()], 0.10),
                (vec!["d2".to_string(), "d3".to_string()], 0.20),
                (vec!["d1".to_string()], 0.30),
            ]
        );
        assert_eq!(sdc.output_load_pf("resp_val"), 0.99);
        assert_eq!(sdc.output_load_pf("resp_msg[0]"), 0.05);
    }

    #[test]
    fn set_load_counts_unsupported_object_domains() {
        for snippet in [
            // OpenSTA rejects these object types (STA-0100) or does not know
            // the cone selectors at all: counted, not applied.
            "set_load 0.10 [get_pins u_ff1/D]",
            "set_load 0.10 [get_cells u_mux]",
            "set_load 0.10 [all_fanouts -from [get_pins u_ff1/Q]]",
            "set_load 0.10 [all_fanins -to [get_pins u_ff1/D]]",
            "set_load 0.10 [all_clocks]",
            "set_load 0.10 [get_clocks core_clk]",
            "set_load 0.10 [get_nets -of_objects [get_pins u_ff1/D]]",
            // Corner/transition-scoped qualifiers are unmodelled.
            "set_load -min 0.10 [get_nets d1]",
            "set_load -max 0.10 [get_ports y]",
            "set_load -rise 0.10 [get_nets d1]",
            "set_load -fall 0.10 [get_ports y]",
            "set_load -subtract_pin_load 0.10 [get_nets d1]",
            // `-wire_load` on a port targets the attached net's wire cap, a
            // different domain than the port pin load.
            "set_load -wire_load 0.20 [get_ports y]",
        ] {
            let sdc = parse_sdc(&format!("{GCD}{snippet}\n")).unwrap();
            assert_eq!(
                sdc.ignored_commands.get("set_load"),
                Some(&1),
                "not counted: {snippet}"
            );
        }
    }

    #[test]
    fn set_load_still_rejects_malformed_forms() {
        for snippet in [
            "set_load foo [get_nets d1]",
            "set_load 0.05 extra [get_nets d1]",
            "set_load 0.05",
            "set_load 0.05 [get_nets d1] [get_nets d2]",
            "set_load -clock core_clk 0.05 [get_nets d1]",
            "set_load -bogus 0.05 [get_nets d1]",
        ] {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn set_load_pin_and_wire_load_flags_apply_on_nets() {
        // Measured: OpenSTA warns these are not allowed on net objects but
        // applies the same value, so they are accepted as no-ops.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_load -pin_load 0.10 [get_nets d1]\n\
             set_load -wire_load 0.20 [get_nets d2]\n"
        ))
        .unwrap();
        assert!(sdc.ignored_commands.is_empty());
        let rules: Vec<_> = sdc.net_loads().map(|(n, v)| (n.clone(), v)).collect();
        assert_eq!(
            rules,
            vec![
                (vec!["d1".to_string()], 0.10),
                (vec!["d2".to_string()], 0.20)
            ]
        );
    }

    #[test]
    fn set_fanout_load_parses_and_counts() {
        // OpenSTA accepts the command but warns it is unsupported (STA-0461)
        // and applies nothing: parsed here, counted ignored.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_fanout_load 4 [get_ports y]\n\
             set_fanout_load 8 [get_nets d1]\n\
             set_fanout_load 2 [all_outputs]\n"
        ))
        .unwrap();
        assert_eq!(sdc.ignored_commands["set_fanout_load"], 3);
        for bad in [
            "set_fanout_load",
            "set_fanout_load 4",
            "set_fanout_load x [get_ports y]",
            "set_fanout_load 4 [get_ports y] [get_ports y2]",
            "set_fanout_load -bogus 4 [get_ports y]",
        ] {
            assert!(
                parse_sdc(&format!("{GCD}{bad}\n")).is_err(),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn remove_from_collection_on_nets() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_load 0.10 [remove_from_collection [get_nets {{d1 d2}}] [get_nets d1]]\n\
             set_load 0.20 [remove_from_collection [get_nets {{d1 d2}}] [get_pins u1/A]]\n"
        ))
        .unwrap();
        let rules: Vec<_> = sdc.net_loads().map(|(n, v)| (n.clone(), v)).collect();
        // Same-namespace subtraction filters by name; the mixed-namespace
        // command degrades to opaque and is counted ignored.
        assert_eq!(rules, vec![(vec!["d2".to_string()], 0.10)]);
        assert_eq!(sdc.ignored_commands["set_load"], 1);
    }

    #[test]
    fn expr_follows_tcl_for_the_ibex_operands() {
        assert_eq!(eval_arith("8.0 * 0.2 "), Some(8.0 * 0.2));
        assert_eq!(eval_arith("(1.0 + 2.0) * 3 "), Some(9.0));
        assert_eq!(eval_arith("1.0 / 4 "), Some(0.25));
        assert_eq!(eval_arith("8 / 2 "), None);
        assert_eq!(eval_arith("1 + "), None);
    }

    /// The clock-table model accepts multiple domains at parse time; the remaining
    /// bail surface (unmodelled qualifiers and commands) still fails with named
    /// errors — the documented break points for the `mc-*` tickets
    /// (`docs/multi-clock.md`).
    #[test]
    fn multi_clock_bails() {
        let with_vclk = format!("{GCD}create_clock -name vclk -period 5.0\n");
        let cases = [
            (
                "set_clock_latency -early 0.5 [get_clocks core_clk]",
                "unsupported option `-early`",
            ),
            (
                "create_clock -waveform {0 0.5 1.0} -period 2.0 [get_ports other]",
                "even number of edge instants",
            ),
            (
                "create_clock -waveform {1.7 0.3} -period 2.0 [get_ports other]",
                "strictly increasing",
            ),
            (
                "create_clock -waveform {0 0.5 1.0 1.5} -period 2.0 [get_ports other]",
                "more than two instants is unsupported",
            ),
            (
                "set_clock_groups -group [get_clocks core_clk] -group [get_clocks vclk]",
                "needs -asynchronous, -logically_exclusive or -physically_exclusive",
            ),
            (
                "set_input_delay 1.0 -clock other [get_ports d_in]",
                "unknown clock `other`",
            ),
            (
                "set_propagated_clock [get_clocks vclk]",
                "set_propagated_clock does not apply to the virtual clock",
            ),
            (
                "create_clock -name bad -period 0 [get_ports other]",
                "has period 0",
            ),
        ];
        for (snippet, want) in cases {
            let err = format!("{:#}", parse_sdc(&format!("{with_vclk}{snippet}\n")).unwrap_err());
            assert!(err.contains(want), "{snippet}: {err}");
        }
    }

    /// A two-design-clock SDC (`t2_int.sdc` minus the port delays): rows in file order.
    const TWO_DESIGN: &str = "create_clock -name cka -period 2.0 [get_ports clk1]\n\
        create_clock -name ckb -period 1.0 [get_ports clk2]\n";

    /// The probe's silent wrong-target bug: uncertainty on a virtual clock must land on
    /// that clock's own row, never on the design clock's.
    #[test]
    fn uncertainty_lands_on_the_named_row() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
            create_clock -name vclk -period 5.0\n\
            set_clock_uncertainty -setup 0.20 [get_clocks vclk]\n"
        ))
        .unwrap();
        assert_eq!(sdc.clock("vclk").unwrap().setup_uncertainty_ns, 0.20);
        assert_eq!(sdc.design_clock().unwrap().setup_uncertainty_ns, 0.0);
    }

    #[test]
    fn multi_name_clock_lists_apply_to_every_row() {
        let sdc = parse_sdc(&format!(
            "{GCD}\
            create_clock -name vclk -period 5.0\n\
            set_clock_latency 0.5 [get_clocks {{core_clk vclk}}]\n\
            set_clock_latency -source 0.1 [get_clocks {{core_clk vclk}}]\n\
            set_clock_transition 0.1 [get_clocks {{core_clk vclk}}]\n"
        ))
        .unwrap();
        for name in ["core_clk", "vclk"] {
            let row = sdc.clock(name).unwrap();
            assert_eq!(row.latency_ns, 0.5, "{name}");
            assert_eq!(row.source_latency_ns, 0.1, "{name}");
            assert_eq!(row.transition_ns, 0.1, "{name}");
        }
        // The virtual row's transition is stored, not counted as ignored.
        assert!(!sdc
            .ignored_options
            .contains_key("set_clock_transition (virtual clock)"));
    }

    #[test]
    fn clock_commands_target_second_domain_rows() {
        // The `t2_unc` shape: per-name uncertainty and latency on two design clocks.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_latency 0.5 [get_clocks {{cka ckb}}]\n\
            set_clock_uncertainty -setup 0.05 [get_clocks cka]\n\
            set_clock_uncertainty -setup 0.20 [get_clocks ckb]\n\
            set_clock_uncertainty -hold 0.10 [get_clocks cka]\n"
        ))
        .unwrap();
        let (a, b) = (sdc.clock("cka").unwrap(), sdc.clock("ckb").unwrap());
        assert_eq!((a.latency_ns, b.latency_ns), (0.5, 0.5));
        assert_eq!((a.setup_uncertainty_ns, a.hold_uncertainty_ns), (0.05, 0.10));
        assert_eq!((b.setup_uncertainty_ns, b.hold_uncertainty_ns), (0.20, 0.0));
    }

    /// `set_clock_uncertainty -from/-to` (inter-clock) parse forms (`t2_uncx`).
    #[test]
    fn set_clock_uncertainty_directional() {
        // Directed pair: one rule, setup side only.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            create_clock -name vclk -period 5.0\n\
            set_clock_uncertainty -setup 0.30 -from [get_clocks cka] -to [get_clocks ckb]\n"
        ))
        .unwrap();
        assert_eq!(sdc.inter_clock_uncertainty.len(), 1);
        let r = &sdc.inter_clock_uncertainty[0];
        assert_eq!(r.from, vec![0]);
        assert_eq!(r.to, vec![1]);
        assert_eq!(r.setup_ns, Some(0.30));
        assert_eq!(r.hold_ns, None);

        // Hold side stores independently; a bare (flag-less) rule sets both.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_uncertainty -hold 0.15 -from [get_clocks ckb] -to [get_clocks cka]\n\
            set_clock_uncertainty 0.40 -from [get_clocks cka] -to [get_clocks ckb]\n"
        ))
        .unwrap();
        assert_eq!(
            sdc.inter_clock_uncertainty[0].hold_ns,
            Some(0.15)
        );
        assert_eq!(sdc.inter_clock_uncertainty[0].setup_ns, None);
        assert_eq!(
            sdc.inter_clock_uncertainty[1].setup_ns,
            Some(0.40)
        );
        assert_eq!(
            sdc.inter_clock_uncertainty[1].hold_ns,
            Some(0.40)
        );

        // A multi-name collection and a bare `write_sdc` name both expand;
        // a lone `-from` or `-to` is an error, not a wildcard (measured
        // `t2_eclk3`/`t2_eclk4`: OpenSTA aborts `STA-0421`).
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_uncertainty -setup 0.1 -from [get_clocks cka] -to cka\n\
            set_clock_uncertainty -setup 0.2 -from ckb -to [get_clocks {{cka ckb}}]\n\
            set_clock_uncertainty -setup 0.3 -from cka -to ckb\n"
        ))
        .unwrap();
        assert_eq!(sdc.inter_clock_uncertainty[0].to, vec![0]);
        assert_eq!(
            sdc.inter_clock_uncertainty[1].to,
            vec![0, 1]
        );
        assert_eq!(sdc.inter_clock_uncertainty[2].from, vec![0]);
        assert_eq!(sdc.inter_clock_uncertainty[2].to, vec![1]);
    }

    #[test]
    fn set_clock_uncertainty_directional_bails() {
        for (snippet, want) in [
            (
                "set_clock_uncertainty -setup 0.3 -from [get_clocks cka] -to [get_clocks ckb] [get_clocks cka]",
                "cannot name clocks with -from/-to",
            ),
            (
                "set_clock_uncertainty -rise_from [get_clocks cka] 0.3 -to [get_clocks ckb]",
                "unsupported option `-rise_from`",
            ),
            (
                "set_clock_uncertainty -setup 0.3 -from [get_clocks cka] -from [get_clocks ckb]",
                "takes each of -from/-to once",
            ),
            (
                "set_clock_uncertainty -setup 0.3 -from [get_clocks cka]",
                "-from/-to must be used together",
            ),
            (
                "set_clock_uncertainty -setup 0.3 -to [get_clocks ckb]",
                "-from/-to must be used together",
            ),
        ] {
            let err = format!(
                "{:#}",
                parse_sdc(&format!("{TWO_DESIGN}{snippet}\n")).unwrap_err()
            );
            assert!(err.contains(want), "{snippet}: {err}");
        }
    }

    /// `Sdc::uncertainty_ns`: the last matching `-from`/`-to` rule *replaces* the
    /// capture row's value for that direction and side only.
    #[test]
    fn uncertainty_ns_directional_lookup() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_uncertainty -setup 0.05 [get_clocks cka]\n\
            set_clock_uncertainty -hold 0.10 [get_clocks cka]\n\
            set_clock_uncertainty -setup 0.20 [get_clocks ckb]\n\
            set_clock_uncertainty -hold 0.40 [get_clocks ckb]\n\
            set_clock_uncertainty -setup 0.30 -from [get_clocks cka] -to [get_clocks ckb]\n"
        ))
        .unwrap();
        // The directed rule replaces ckb's 0.20 for cka->ckb setup only: the
        // reverse direction and both hold sides keep the row values, and the
        // hold side of the same direction is untouched (not additive either).
        assert_eq!(sdc.uncertainty_ns(0, 1, false), 0.30);
        assert_eq!(sdc.uncertainty_ns(1, 0, false), 0.05);
        assert_eq!(sdc.uncertainty_ns(0, 1, true), 0.40);
        assert_eq!(sdc.uncertainty_ns(1, 0, true), 0.10);
        assert_eq!(sdc.uncertainty_ns(0, 0, false), 0.05);

        // File order: the later rule wins for the same directed pair; a
        // multi-row `-to` matches every listed capture row.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_uncertainty -setup 0.30 -from [get_clocks cka] -to [get_clocks ckb]\n\
            set_clock_uncertainty -setup 0.50 -from [get_clocks cka] -to [get_clocks ckb]\n\
            set_clock_uncertainty -hold 0.15 -from [get_clocks ckb] -to [get_clocks {{cka ckb}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.uncertainty_ns(0, 1, false), 0.50);
        assert_eq!(sdc.uncertainty_ns(1, 0, true), 0.15);
        assert_eq!(sdc.uncertainty_ns(1, 1, true), 0.15);
        assert_eq!(sdc.uncertainty_ns(0, 1, true), 0.0);
    }

    #[test]
    fn propagated_clock_is_per_domain() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}set_propagated_clock [get_clocks cka]\n"
        ))
        .unwrap();
        assert!(sdc.clock("cka").unwrap().propagated);
        assert!(!sdc.clock("ckb").unwrap().propagated);

        // `all_clocks` covers every design row; a virtual row still bails.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}set_propagated_clock [all_clocks]\n"
        ))
        .unwrap();
        assert!(sdc.clocks.iter().all(|c| c.propagated));
        let text = format!("{TWO_DESIGN}create_clock -name vclk -period 3.0\n");
        assert!(
            parse_sdc(&format!("{text}set_propagated_clock [all_clocks]\n")).is_err()
        );
    }

    #[test]
    fn delay_clock_references_a_domain_row() {
        // `-clock ckb` resolves to the second row: its latency, not the design clock's.
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_latency 0.6 [get_clocks cka]\n\
            set_clock_latency 0.2 [get_clocks ckb]\n\
            set_output_delay 0.7 -clock ckb [get_ports yb]\n"
        ))
        .unwrap();
        assert_eq!(sdc.output_clock_latency_ns("yb"), (0.2, 0.2));
    }

    /// `get_clocks`/`all_clocks` selectors inside `-from`/`-to` resolve to
    /// `ObjSel::Clocks` rows instead of counting the command ignored; an
    /// all-miss collection resolves empty (measured `t2_selunk`: OpenSTA
    /// warns and the rule covers nothing).
    #[test]
    fn clock_selectors_parse_to_rows() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_false_path -from [get_clocks cka]\n\
            set_max_delay 1.0 -to [get_clocks {{cka ckb}}]\n\
            set_min_delay 0.5 -from [all_clocks]\n\
            set_multicycle_path -setup 2 -from [get_clocks ckb] -to [get_clocks cka]\n\
            set_false_path -from [get_clocks nope]\n"
        ))
        .unwrap();
        assert!(matches!(sdc.exceptions[0].from, Some(ObjSel::Clocks(ref r)) if *r == [0]));
        assert!(matches!(sdc.path_delays[0].to, Some(ObjSel::Clocks(ref r)) if *r == [0, 1]));
        assert!(matches!(sdc.path_delays[1].from, Some(ObjSel::Clocks(ref r)) if *r == [0, 1]));
        assert!(matches!(sdc.exceptions[1].from, Some(ObjSel::Clocks(ref r)) if *r == [1]));
        assert!(matches!(sdc.exceptions[1].to, Some(ObjSel::Clocks(ref r)) if *r == [0]));
        assert!(matches!(sdc.exceptions[2].from, Some(ObjSel::Clocks(ref r)) if r.is_empty()));
        assert!(!sdc.ignored_commands.contains_key("set_false_path"));
    }

    /// `create_clock -waveform` stores the two edge instants on design and
    /// virtual rows; absence keeps `None` (the 50% duty default).
    #[test]
    fn create_clock_waveform_parses() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            create_clock -name vw -period 5.0 -waveform {{0.5 4.0}}\n\
            create_clock -name cw -period 4.0 -waveform {{0.3 1.7}} [get_ports clk3]\n\
            create_clock -name big -period 2.0 -waveform {{0 2.5}}\n"
        ))
        .unwrap();
        assert_eq!(sdc.clock("vw").unwrap().waveform_ns, Some([0.5, 4.0]));
        assert_eq!(sdc.clock("cw").unwrap().waveform_ns, Some([0.3, 1.7]));
        // `t2_wavebig`: instants past the period wrap onto it.
        assert_eq!(sdc.clock("big").unwrap().waveform_ns, Some([0.0, 0.5]));
        assert_eq!(sdc.clock("cka").unwrap().waveform_ns, None);
    }

    /// `set_clock_groups` stores the resolved groups per mode; `-name` is
    /// accepted, `-allow_paths` records no exclusion, `-group` accepts
    /// bare/braced names and an empty group.
    #[test]
    fn set_clock_groups_parses() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_groups -name g -asynchronous -group [get_clocks cka] -group [get_clocks ckb]\n\
            set_clock_groups -logically_exclusive -group cka -group ckb\n\
            set_clock_groups -asynchronous -allow_paths -group [get_clocks cka] -group [get_clocks ckb]\n\
            set_clock_groups -physically_exclusive -group {{}} -group {{cka ckb}}\n"
        ))
        .unwrap();
        assert_eq!(sdc.clock_groups.len(), 3);
        assert_eq!(sdc.clock_groups[0].kind, ClockGroupKind::Asynchronous);
        assert_eq!(sdc.clock_groups[0].groups, vec![vec![0], vec![1]]);
        assert_eq!(sdc.clock_groups[1].kind, ClockGroupKind::LogicallyExclusive);
        assert_eq!(sdc.clock_groups[1].groups, vec![vec![0], vec![1]]);
        assert_eq!(sdc.clock_groups[2].kind, ClockGroupKind::PhysicallyExclusive);
        assert_eq!(sdc.clock_groups[2].groups, vec![vec![], vec![0, 1]]);
    }

    /// A `set_clock_*` without a clock target writes the first port-bearing row at apply
    /// time; issued before the first `create_clock` there is no such row (the single-clock
    /// fields used to absorb the write unconditionally).
    #[test]
    fn clock_commands_before_create_clock_bail() {
        for snippet in ["set_clock_latency 0.5", "set_clock_uncertainty 0.5"] {
            let err = format!("{:#}", parse_sdc(&format!("{snippet}\n")).unwrap_err());
            assert!(err.contains("no design clock declared yet"), "{snippet}: {err}");
        }
    }

    /// `[get_clocks]` with no argument resolves to every clock;
    /// `[get_clocks {}]` is a valid empty collection — the `set_clock_*`
    /// commands apply to zero rows and succeed silently (measured
    /// `t2_eclk`). Unknown names inside a collection filter out like
    /// `clock_rows` (OpenSTA's `STA-0351` warn is unmodelled).
    #[test]
    fn empty_clock_collection_is_a_noop() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            set_clock_latency 0.5 [get_clocks {{}}]\n\
            set_clock_transition 0.1 [get_clocks {{}}]\n\
            set_clock_uncertainty -setup 0.2 [get_clocks {{}}]\n\
            set_clock_uncertainty -setup 0.2 -from [get_clocks {{}}] -to [get_clocks cka]\n\
            set_clock_uncertainty -setup 0.3 -from [get_clocks nope] -to [get_clocks ckb]\n\
            set_propagated_clock [get_clocks {{}}]\n\
            set_propagated_clock [get_clocks no_such]\n\
            set_clock_latency 0.5 [get_clocks no_such]\n\
            set_clock_uncertainty 0.1 [get_clocks no_such]\n\
            set_clock_groups -asynchronous -group [get_clocks no_such] -group [get_clocks cka]\n"
        ))
        .unwrap();
        let cka = sdc.clock("cka").unwrap();
        assert_eq!(cka.latency_ns, 0.0);
        assert_eq!(cka.transition_ns, 0.0);
        assert_eq!(cka.setup_uncertainty_ns, 0.0);
        assert!(!cka.propagated);
        assert!(sdc.inter_clock_uncertainty.is_empty());
        // The all-miss `-group` stores the empty row set like `-group {}`.
        assert_eq!(sdc.clock_groups[0].groups, vec![vec![], vec![0]]);
        // No-argument `[get_clocks]` is the `*` match — every clock.
        let sdc = parse_sdc(&format!("{TWO_DESIGN}set_clock_latency 0.5 [get_clocks]\n")).unwrap();
        assert_eq!(sdc.clock("cka").unwrap().latency_ns, 0.5);
        assert_eq!(sdc.clock("ckb").unwrap().latency_ns, 0.5);
    }

    /// Post-wrap waveform instants keep their wrapped order even when it
    /// reorders or collapses — measured `t2_waverot`/`t2_wavez`: OpenSTA
    /// accepts both and stores the pair as given.
    #[test]
    fn create_clock_waveform_wrapped_order_kept() {
        let sdc = parse_sdc(&format!(
            "{TWO_DESIGN}\
            create_clock -name rot -period 2.0 -waveform {{1.9 2.1}}\n\
            create_clock -name z -period 2.0 -waveform {{0 2.0}}\n"
        ))
        .unwrap();
        let [r0, r1] = sdc.clock("rot").unwrap().waveform_ns.unwrap();
        assert!((r0 - 1.9).abs() < 1e-6 && (r1 - 0.1).abs() < 1e-6 && r1 < r0);
        assert_eq!(sdc.clock("z").unwrap().waveform_ns, Some([0.0, 0.0]));
    }
}
