use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
enum PortSel {
    AllInputs { no_clocks: bool },
    AllOutputs,
    Ports(Vec<String>),
}

impl PortSel {
    /// `clock_port` is needed because `-no_clocks` excludes the clock source, which the selector cannot see.
    fn matches(&self, port: &str, is_input: bool, clock_port: Option<&str>) -> bool {
        match self {
            PortSel::AllInputs { no_clocks } => is_input && !(*no_clocks && clock_port == Some(port)),
            PortSel::AllOutputs => !is_input,
            PortSel::Ports(names) => names.iter().any(|n| n == port),
        }
    }
}

/// Which clock a delay is referenced to: the design clock, or the virtual I/O clock.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ClockRef {
    Design,
    Virtual,
}

/// Which analysis corner a delay applies to. `Both` is a plain `set_input_delay`; `Min`/`Max`
/// come from the `-min`/`-max` flags and feed the early/late checks respectively.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DelaySense {
    Both,
    Min,
    Max,
}

#[derive(Clone, Debug)]
struct DelayRule {
    sel: PortSel,
    value_ns: f32,
    clock: ClockRef,
    sense: DelaySense,
    /// `-add_delay`: accumulate onto the port's current delay instead of replacing it.
    add: bool,
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
        Self { factors: [[[1.0; 2]; 2]; 3] }
    }
}

/// The SDC subset the benchmark flow uses. Commands are applied in file order, so a later
/// command overrides an earlier one on the same port.
#[derive(Clone, Debug, Default)]
pub struct Sdc {
    pub clock_name: Option<String>,
    pub clock_port: Option<String>,
    pub period_ns: f32,
    /// Setup-side clock uncertainty: subtracted from required times.
    pub setup_uncertainty_ns: f32,
    /// Hold-side clock uncertainty: added to required times.
    pub hold_uncertainty_ns: f32,
    pub clock_latency_ns: f32,
    pub io_clock_latency_ns: f32,
    /// `set_propagated_clock` on the design clock: clock arrival is propagated through the
    /// clock network instead of every clock pin seeing the ideal clock latency.
    pub propagated_clock: bool,
    /// `set_clock_latency -source` on the design clock: added on top of the propagated
    /// arrival, and summed with the network latency when the clock is not propagated.
    pub clock_source_latency_ns: f32,
    /// `set_clock_latency -source` on the virtual I/O clock; a virtual clock is never
    /// propagated, so it always sums with `io_clock_latency_ns`.
    pub io_clock_source_latency_ns: f32,
    /// Slew seeded on clock pins (`set_clock_transition`); zero without the command.
    pub clock_transition_ns: f32,
    /// Early/late derating of propagated delays (`set_timing_derate`), per delay kind,
    /// clock/data path and transition. Unset entries stay at 1.0 (no derate).
    pub derate_early: DerateCorner,
    pub derate_late: DerateCorner,
    input_delays: Vec<DelayRule>,
    output_delays: Vec<DelayRule>,
    input_transitions: Vec<(PortSel, f32)>,
    loads: Vec<(PortSel, f32)>,
    driving_cells: Vec<(PortSel, DrivingCell)>,
    /// Options the parser accepts but does not apply, counted by flag or by descriptive key.
    /// `-lib_cell` and `-pin` restrict a delay to one cell or pin; the model has no per-arc
    /// endpoint matching, so the restriction is dropped and the delay applies to every
    /// selected port.
    pub ignored_options: BTreeMap<String, usize>,
    /// Commands accepted but not applied to the timing model (`set_max_*`, a
    /// `set_clock_transition` on a virtual clock), by command name.
    pub ignored_commands: BTreeMap<String, usize>,
    /// Top-level commands evaluated while parsing; the denominator for `ignored_options`.
    pub commands: usize,
}

/// Folds the rules matching `port` in file order into `(min, max, clock)`. A rule without
/// `-add_delay` replaces its corner(s); with `-add_delay` it widens them instead — the min
/// corner keeps the smaller value, the max corner the larger (measured against OpenSTA:
/// `1.0 -add_delay` then `0.4 -add_delay` gives `(0.4, 1.0)`, and a later plain rule resets
/// the fold). `-min`/`-max` rules touch one corner only. `None` when no rule matches, so
/// callers keep their "no constraint" defaults.
fn delay_at(rules: &[DelayRule], port: &str, is_input: bool, clock_port: Option<&str>) -> Option<(f32, f32, ClockRef)> {
    let (mut min_v, mut max_v, mut min_set, mut max_set, mut clock, mut matched) =
        (0.0, 0.0, false, false, ClockRef::Design, false);
    for r in rules.iter().filter(|r| r.sel.matches(port, is_input, clock_port)) {
        matched = true;
        clock = r.clock;
        let apply = |cur: &mut f32, set: &mut bool, widen: fn(f32, f32) -> f32| {
            *cur = if r.add && *set { widen(*cur, r.value_ns) } else { r.value_ns };
            *set = true;
        };
        match r.sense {
            DelaySense::Both => {
                apply(&mut min_v, &mut min_set, f32::min);
                apply(&mut max_v, &mut max_set, f32::max);
            }
            DelaySense::Min => apply(&mut min_v, &mut min_set, f32::min),
            DelaySense::Max => apply(&mut max_v, &mut max_set, f32::max),
        }
    }
    matched.then_some((min_v, max_v, clock))
}

fn last_value(rules: &[(PortSel, f32)], port: &str, is_input: bool, clock_port: Option<&str>) -> Option<f32> {
    rules.iter().rev().find(|(sel, _)| sel.matches(port, is_input, clock_port)).map(|&(_, v)| v)
}

impl Sdc {
    fn latency_ns(&self, clock: ClockRef) -> f32 {
        match clock {
            // A propagated clock ignores the ideal network latency; `-source` latency is
            // what remains, and it is what I/O delays are referenced against. OpenSTA sums
            // source and network latency when the clock is not propagated (measured:
            // `set_clock_latency 1.0` + `-source 0.2` reports 1.20 ns ideal delay).
            ClockRef::Design if self.propagated_clock => self.clock_source_latency_ns,
            ClockRef::Design => self.clock_source_latency_ns + self.clock_latency_ns,
            ClockRef::Virtual => self.io_clock_latency_ns + self.io_clock_source_latency_ns,
        }
    }

    /// Latency every clock pin sees in ideal-clock mode: `-source` and network latencies
    /// sum (OpenSTA reports 1.20 ns for `set_clock_latency 1.0` + `-source 0.2`).
    pub fn design_clock_latency_ns(&self) -> f32 {
        self.clock_source_latency_ns + self.clock_latency_ns
    }

    /// Derate factor for one propagated delay. `late` selects the max-analysis corner,
    /// `kind` is a `DERATE_*` constant, `clock_path` marks delays on the clock network,
    /// and `transition` is RISE/FALL.
    pub fn derate(&self, late: bool, kind: usize, clock_path: bool, transition: usize) -> f32 {
        let corner = if late { &self.derate_late } else { &self.derate_early };
        corner.factors[kind][usize::from(!clock_path)][transition]
    }

    /// `(min, max)` input delay in ns; both equal the plain value when no `-min`/`-max` was given.
    pub fn input_delay_ns(&self, port: &str) -> (f32, f32) {
        delay_at(&self.input_delays, port, true, self.clock_port.as_deref()).map_or((0.0, 0.0), |(min_v, max_v, _)| (min_v, max_v))
    }

    /// Latency of the clock that the port's input delay is referenced to.
    pub fn input_clock_latency_ns(&self, port: &str) -> f32 {
        delay_at(&self.input_delays, port, true, self.clock_port.as_deref()).map_or(0.0, |(_, _, clock)| self.latency_ns(clock))
    }

    /// `(min, max)` output delay in ns.
    pub fn output_delay_ns(&self, port: &str) -> (f32, f32) {
        delay_at(&self.output_delays, port, false, self.clock_port.as_deref()).map_or((0.0, 0.0), |(min_v, max_v, _)| (min_v, max_v))
    }

    /// Latency of the clock that the port's output delay is referenced to.
    pub fn output_clock_latency_ns(&self, port: &str) -> f32 {
        delay_at(&self.output_delays, port, false, self.clock_port.as_deref()).map_or(0.0, |(_, _, clock)| self.latency_ns(clock))
    }

    pub fn input_transition_ns(&self, port: &str) -> f32 {
        last_value(&self.input_transitions, port, true, self.clock_port.as_deref()).unwrap_or(0.0)
    }

    pub fn output_load_pf(&self, port: &str) -> f32 {
        last_value(&self.loads, port, false, self.clock_port.as_deref()).unwrap_or(0.0)
    }

    /// The `set_driving_cell` rule covering `port`, if any.
    pub fn driving_cell(&self, port: &str, is_input: bool) -> Option<&DrivingCell> {
        self.driving_cells
            .iter()
            .rev()
            .find(|(sel, _)| sel.matches(port, is_input, self.clock_port.as_deref()))
            .map(|(_, d)| d)
    }
}

pub fn load_sdc(path: &Path) -> Result<Sdc> {
    let text = std::fs::read_to_string(path).with_context(|| format!("Failed to read SDC {:?}", path))?;
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
    virtual_clocks: Vec<(String, f32)>,
}

impl Interp {
    fn new(text: &str) -> Self {
        Interp { src: text.chars().collect(), pos: 0, vars: HashMap::new(), sdc: Sdc::default(), virtual_clocks: Vec::new() }
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
        self.command(name, args).with_context(|| format!("SDC line {line}: `{name}`"))
    }

    fn command(&mut self, name: &str, args: &[Value]) -> Result<Value> {
        match name {
            "current_design" => Ok(empty()),
            "set" => self.set(args),
            "expr" => expr(args),
            "get_ports" => Ok(Value::Ports(PortSel::Ports(plain_names(args)?))),
            "get_clocks" => Ok(Value::Clocks(plain_names(args)?)),
            "all_inputs" => match args {
                [] => Ok(Value::Ports(PortSel::AllInputs { no_clocks: false })),
                [Value::Str(flag)] if flag == "-no_clocks" => Ok(Value::Ports(PortSel::AllInputs { no_clocks: true })),
                _ => bail!("all_inputs takes at most -no_clocks"),
            },
            "all_outputs" => match args {
                [] => Ok(Value::Ports(PortSel::AllOutputs)),
                _ => bail!("all_outputs takes no arguments"),
            },
            "all_clocks" => Ok(Value::Clocks(self.all_clock_names())),
            "create_clock" => {
                self.create_clock(args)?;
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
                let rule = value_and_ports(args, &PORT_VALUE_FLAGS, &IGNORED_VALUE_FLAGS, &[], &mut self.sdc.ignored_options)?;
                self.sdc.input_transitions.push(rule);
                Ok(empty())
            }
            "set_load" => {
                let rule = value_and_ports(args, &PORT_VALUE_FLAGS, &IGNORED_VALUE_FLAGS, &["-pin_load"], &mut self.sdc.ignored_options)?;
                self.sdc.loads.push(rule);
                Ok(empty())
            }
            // Design rules the timing model does not check, and propagated clocks it cannot
            // propagate: accepted so a stock `write_sdc` file parses, counted in
            // `ignored_commands` so the skip summary still shows them.
            "set_max_transition" | "set_max_capacitance" | "set_max_fanout" => {
                self.design_rule(args)?;
                *self.sdc.ignored_commands.entry(name.to_string()).or_default() += 1;
                Ok(empty())
            }
            "set_propagated_clock" | "unset_propagated_clock" => {
                self.propagated_clock(name, args)?;
                Ok(empty())
            }
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

    /// `create_clock -name N -period P [source]`. A clock with a source port is the design clock;
    /// one without is the virtual I/O clock that the ibex SDC references its I/O delays to.
    fn create_clock(&mut self, args: &[Value]) -> Result<()> {
        let name = flag_value(args, "-name")?.map(str::to_string);
        let period_ns = parse_f32(flag_value(args, "-period")?.context("missing -period")?)?;
        match positionals(args, &CLOCK_VALUE_FLAGS, &[], &[], &mut self.sdc.ignored_options)?.as_slice() {
            [] => {
                let Some(name) = name else {
                    bail!("a virtual clock needs -name");
                };
                self.virtual_clocks.push((name, period_ns));
            }
            [Value::Ports(PortSel::Ports(ports))] => {
                let [port] = ports.as_slice() else {
                    bail!("create_clock takes one source port");
                };
                // A second design clock would overwrite the first one's port, name and period.
                if self.sdc.clock_port.is_some() {
                    bail!("a second design clock is not supported");
                }
                self.sdc.clock_name = name;
                self.sdc.clock_port = Some(port.clone());
                self.sdc.period_ns = period_ns;
            }
            _ => bail!("unsupported create_clock argument"),
        }
        Ok(())
    }

    /// `set_clock_uncertainty [-setup|-hold] <ns> [get_clocks <name>]`. OpenSTA requires the clock
    /// list; the single-value form is kept for hand-written SDCs and sets both checks. At most one
    /// of `-setup`/`-hold` is accepted, matching OpenSTA, which treats a second flag as objects.
    fn set_clock_uncertainty(&mut self, args: &[Value]) -> Result<()> {
        let mut setup = false;
        let mut hold = false;
        let mut rest = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                Value::Str(s) if s.as_str() == "-setup" || s.as_str() == "-hold" => {
                    let slot = if s.as_str() == "-setup" { &mut setup } else { &mut hold };
                    if *slot {
                        bail!("duplicate {s}");
                    }
                    *slot = true;
                }
                value => rest.push(value),
            }
        }
        if setup && hold {
            bail!("set_clock_uncertainty takes at most one of -setup/-hold");
        }
        let value_ns = match rest.as_slice() {
            [Value::Str(value)] => parse_f32(value)?,
            [Value::Str(value), Value::Clocks(names)] => {
                for name in names {
                    self.resolve_clock(name)?;
                }
                parse_f32(value)?
            }
            // `set_clock_uncertainty 0.25 clk`: write_sdc emits the clock as a bare name.
            [Value::Str(value), Value::Str(name)] => {
                self.resolve_clock(name)?;
                parse_f32(value)?
            }
            _ => bail!("set_clock_uncertainty takes a value and optional clock names"),
        };
        match (setup, hold) {
            (false, false) => {
                self.sdc.setup_uncertainty_ns = value_ns;
                self.sdc.hold_uncertainty_ns = value_ns;
            }
            (true, false) => self.sdc.setup_uncertainty_ns = value_ns,
            (false, true) => self.sdc.hold_uncertainty_ns = value_ns,
            (true, true) => unreachable!(),
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
        let (value, clock) = match rest.as_slice() {
            [Value::Str(value)] => (value, None),
            [Value::Str(value), Value::Clocks(names)] => match names.as_slice() {
                [name] => (value, Some(name.as_str())),
                _ => bail!("set_clock_latency takes one clock"),
            },
            // `set_clock_latency 0.5 clk`: write_sdc emits the clock as a bare name.
            [Value::Str(value), Value::Str(name)] => (value, Some(name.as_str())),
            _ => bail!("set_clock_latency takes a value and optional clock names"),
        };
        let latency = parse_f32(value)?;
        let clock = clock.map(|n| self.resolve_clock(n)).transpose()?.unwrap_or(ClockRef::Design);
        match clock {
            ClockRef::Design => {
                if source {
                    self.sdc.clock_source_latency_ns = latency;
                } else {
                    self.sdc.clock_latency_ns = latency;
                }
            }
            // A virtual clock is never propagated, so source and network latency sum for
            // the I/O checks that read `io_clock_latency_ns`.
            ClockRef::Virtual => {
                if source {
                    self.sdc.io_clock_source_latency_ns = latency;
                } else {
                    self.sdc.io_clock_latency_ns = latency;
                }
            }
        }
        Ok(())
    }

    /// `set_propagated_clock|unset_propagated_clock <clock objects>`. Only the design clock
    /// is modelled; unsetting a clock that was never propagated is a no-op. The virtual I/O
    /// clock cannot be propagated: naming it in `set` is an error.
    fn propagated_clock(&mut self, command: &str, args: &[Value]) -> Result<()> {
        let mut names = Vec::new();
        for arg in args {
            match arg {
                Value::Str(s) if s.starts_with('-') => bail!("{command} takes clock objects, not `{s}`"),
                Value::Str(s) => names.extend(s.split_whitespace().map(str::to_string)),
                Value::Clocks(ns) => names.extend(ns.iter().cloned()),
                Value::Ports(PortSel::Ports(ns)) => names.extend(ns.iter().cloned()),
                _ => bail!("{command} takes clock objects"),
            }
        }
        if names.is_empty() {
            bail!("{command} takes clock objects");
        }
        let mut design = false;
        for clock in &names {
            match self.resolve_clock(clock) {
                Ok(ClockRef::Design) => design = true,
                Ok(ClockRef::Virtual) if command == "set_propagated_clock" => {
                    bail!("set_propagated_clock does not apply to the virtual clock `{clock}`")
                }
                Ok(ClockRef::Virtual) => {}
                Err(e) => {
                    // `get_ports {clk}` names the clock's source port; the clock handle is `-name`.
                    if self.sdc.clock_port.as_deref() == Some(clock.as_str()) {
                        design = true;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        if command == "set_propagated_clock" && !design {
            bail!("set_propagated_clock needs the design clock");
        }
        if design {
            self.sdc.propagated_clock = command == "set_propagated_clock";
        }
        Ok(())
    }

    /// `set_clock_transition <ns> [get_clocks <name>...]`. Only the design clock's slew is
    /// modelled; a virtual clock's transition is counted as ignored.
    fn set_clock_transition(&mut self, args: &[Value]) -> Result<()> {
        let (value, names) = match args {
            [Value::Str(v)] => (v, None),
            [Value::Str(v), Value::Clocks(names)] => (v, Some(names.clone())),
            [Value::Str(v), Value::Str(name)] => (v, Some(vec![name.clone()])),
            _ => bail!("set_clock_transition takes a value and optional clock names"),
        };
        let transition = parse_f32(value)?;
        for name in names.unwrap_or_else(|| self.all_clock_names()) {
            match self.resolve_clock(&name)? {
                ClockRef::Design => self.sdc.clock_transition_ns = transition,
                ClockRef::Virtual => {
                    *self.sdc.ignored_options.entry("set_clock_transition (virtual clock)".to_string()).or_default() += 1;
                }
            }
        }
        Ok(())
    }

    /// `set_timing_derate -early|-late [-rise|-fall] [-clock|-data]
    /// [-net_delay|-cell_delay|-cell_check] <derate> [objects]`. Exactly one of `-early`/`-late`
    /// is required and the derate value is positional. Each command overwrites only the
    /// categories its flags select; a bare corner flag covers cell and net delays on both
    /// clock and data paths and both transitions but not cell checks (measured on OpenSTA).
    /// Cell checks respond to `-data` only, never `-clock`. A second positional word would
    /// be the object scope, which the model cannot honour, so it is rejected.
    fn set_timing_derate(&mut self, args: &[Value]) -> Result<()> {
        let mut late = None;
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
                "-early" | "-late" => {
                    if late.is_some() {
                        bail!("set_timing_derate takes exactly one of -early/-late");
                    }
                    late = Some(word == "-late");
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
        let (Some(late), Some(value)) = (late, derate) else {
            bail!("set_timing_derate needs -early or -late and a derate value");
        };
        let mut selected = kinds;
        if !any_kind {
            selected[DERATE_CELL] = true;
            selected[DERATE_NET] = true;
        }
        let corner = if late { &mut self.sdc.derate_late } else { &mut self.sdc.derate_early };
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
        Ok(())
    }

    /// `set_driving_cell -lib_cell C -pin P [-input_transition_rise r]
    /// [-input_transition_fall f] <sel>`. Unlike the delay commands, `-lib_cell`/`-pin` are the
    /// driver's identity here, not restrictions to drop.
    fn set_driving_cell(&mut self, args: &[Value]) -> Result<()> {
        const FLAGS: [&str; 4] = ["-lib_cell", "-pin", "-input_transition_rise", "-input_transition_fall"];
        let pos = positionals(args, &FLAGS, &[], &[], &mut self.sdc.ignored_options)?;
        let [Value::Ports(sel)] = pos.as_slice() else {
            bail!("set_driving_cell takes a port selector");
        };
        let in_trans = |flag: &str| -> Result<f32> {
            flag_value(args, flag)?.map(parse_f32).transpose().map(|v| v.unwrap_or(0.0))
        };
        self.sdc.driving_cells.push((
            sel.clone(),
            DrivingCell {
                cell: flag_value(args, "-lib_cell")?.context("set_driving_cell needs -lib_cell")?.to_string(),
                pin: flag_value(args, "-pin")?.context("set_driving_cell needs -pin")?.to_string(),
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

    /// Clock names `all_clocks` resolves to at this point in the file.
    fn all_clock_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.virtual_clocks.iter().map(|(n, _)| n.clone()).collect();
        if let Some(name) = &self.sdc.clock_name {
            names.push(name.clone());
        } else if let Some(port) = &self.sdc.clock_port {
            names.push(port.clone());
        }
        names
    }

    fn delay_rule(&mut self, args: &[Value]) -> Result<DelayRule> {
        let sense = match (has_flag(args, "-min"), has_flag(args, "-max")) {
            (true, true) => bail!("a delay takes at most one of -min/-max"),
            (true, false) => DelaySense::Min,
            (false, true) => DelaySense::Max,
            (false, false) => DelaySense::Both,
        };
        let add = has_flag(args, "-add_delay");
        let (sel, value_ns) = value_and_ports(args, &DELAY_VALUE_FLAGS, &IGNORED_VALUE_FLAGS, &DELAY_BOOL_FLAGS, &mut self.sdc.ignored_options)?;
        let clock = match flag_arg(args, "-clock")? {
            None => ClockRef::Design,
            Some(Value::Str(name)) => self.resolve_clock(name)?,
            Some(Value::Clocks(names)) => match names.as_slice() {
                [name] => self.resolve_clock(name)?,
                _ => bail!("-clock takes one clock"),
            },
            Some(_) => bail!("-clock needs a clock name"),
        };
        Ok(DelayRule { sel, value_ns, clock, sense, add })
    }

    fn resolve_clock(&self, name: &str) -> Result<ClockRef> {
        // `create_clock` without `-name` leaves the port name as the clock's handle, the same
        // fallback `all_clock_names` uses.
        let design_name = self.sdc.clock_name.as_deref().or(self.sdc.clock_port.as_deref());
        if design_name == Some(name) {
            return Ok(ClockRef::Design);
        }
        if self.virtual_clocks.iter().any(|(n, _)| n == name) {
            return Ok(ClockRef::Virtual);
        }
        bail!("unknown clock `{name}`")
    }

    fn finish(self) -> Result<Sdc> {
        if self.sdc.period_ns <= 0.0 {
            bail!("SDC defines no clock period");
        }
        // The analysis uses one period for the design and the virtual I/O clock.
        for (name, period_ns) in &self.virtual_clocks {
            if *period_ns != self.sdc.period_ns {
                bail!("virtual clock `{name}` has period {period_ns}, not the design period {}", self.sdc.period_ns);
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
    text.parse().with_context(|| format!("`{text}` is not a number"))
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
const CLOCK_VALUE_FLAGS: [&str; 2] = ["-name", "-period"];

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
    let Some(i) = args.iter().position(|a| matches!(a, Value::Str(s) if s == flag)) else {
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
    text.strip_prefix('-')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') && rest.parse::<f32>().is_ok())
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
            Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => bail!("unsupported option `{s}`"),
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
    let [Value::Str(value), Value::Ports(sel)] = positionals(args, value_flags, ignored_flags, bool_flags, ignored)?.as_slice() else {
        bail!("expected a numeric value and a port selector");
    };
    Ok((sel.clone(), parse_f32(value)?))
}

/// Four-operator arithmetic with parentheses over decimal numbers. Values are f64; the caller
/// narrows them. Tcl's integer division is refused rather than emulated.
fn eval_arith(text: &str) -> Option<f64> {
    let mut arith = Arith { src: text.as_bytes(), pos: 0 };
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

    #[test]
    fn parses_the_gcd_constraints() {
        let sdc = parse_sdc(GCD).unwrap();
        assert_eq!(sdc.clock_port.as_deref(), Some("clk"));
        assert_eq!(sdc.period_ns, 5.0);
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
        assert!(parse_sdc("set_false_path -from a -to b\n").is_err());
    }

    #[test]
    fn parses_the_ibex_tcl_sdc() {
        let sdc = parse_sdc(IBEX).unwrap();
        assert_eq!(sdc.period_ns, 8.0);
        assert_eq!(sdc.clock_name.as_deref(), Some("core_clock"));
        assert_eq!(sdc.clock_port.as_deref(), Some("clk_i"));
        assert_eq!(sdc.clock_latency_ns, 1.095);
        assert_eq!(sdc.io_clock_latency_ns, 1.095);
        assert_eq!(sdc.input_delay_ns("instr_rdata_i"), (1.6, 1.6));
        assert_eq!(sdc.input_delay_ns("clk_i"), (0.0, 0.0), "-no_clocks must exclude the clock port");
        assert_eq!(sdc.input_clock_latency_ns("instr_rdata_i"), 1.095);
        assert_eq!(sdc.output_delay_ns("instr_req_o"), (1.6, 1.6));
        assert_eq!(sdc.output_clock_latency_ns("instr_req_o"), 1.095);
    }

    #[test]
    fn counts_ignored_lib_cell_and_pin_options() {
        let sdc = parse_sdc(&format!(
            "{GCD}set_input_delay 1.0 -lib_cell buf_2 -pin A -clock core_clk [all_inputs -no_clocks]\n\
             set_output_delay 2.0 -pin out_pin -clock core_clk [all_outputs]\n"
        ))
        .unwrap();
        // The restrictions are dropped, so the delays still apply to every selected port.
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
            assert_eq!(sdc.clock_name.as_deref(), Some("core_clk"));
            assert_eq!(sdc.clock_port.as_deref(), Some("clk"));
            assert_eq!(sdc.period_ns, period_ns);
            assert_eq!(sdc.input_delay_ns("req_val"), (io_ns, io_ns));
            assert_eq!(sdc.output_delay_ns("resp_val"), (io_ns, io_ns));
            assert_eq!(sdc.input_transition_ns("req_val"), 0.15);
            assert_eq!(sdc.output_load_pf("resp_val"), 0.05);
            assert_eq!(sdc.clock_latency_ns, 0.0);
            assert_eq!(sdc.io_clock_latency_ns, 0.0);
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
            "set_clock_latency 1.0 [get_clocks nope]",
            "set_input_delay 1.0 -clock nope [all_inputs]",
            "if {1} {set_load 0.1 [all_outputs]}",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn rejects_a_virtual_clock_with_another_period() {
        let text = format!("{GCD}create_clock -name vclk -period 4.0\n");
        assert!(parse_sdc(&text).is_err());
    }

    #[test]
    fn rejects_a_second_design_clock() {
        let text = format!("{GCD}create_clock -name core_clk2 -period 5.0 [get_ports other]\n");
        assert!(parse_sdc(&text).is_err());
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
            assert!(parse_sdc(&format!("{GCD}{snippet}\n")).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn add_delay_widens_instead_of_summing() {
        // Measured against OpenSTA on gcd: `-add_delay` keeps every constraint and folds each
        // corner — the min corner takes the smallest value, the max corner the largest — and a
        // later plain rule resets the fold.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay 1.0 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.4 -add_delay -clock core_clk [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.input_delay_ns("a"), (0.4, 1.0));

        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_input_delay 1.0 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.4 -add_delay -clock core_clk [get_ports {{a}}]\n\
             set_input_delay 0.8 -clock core_clk [get_ports {{a}}]\n"
        ))
        .unwrap();
        assert_eq!(sdc.input_delay_ns("a"), (0.8, 0.8));
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
        assert_eq!(sdc.clock_transition_ns, 0.1);
    }

    #[test]
    fn propagated_clock_and_design_rules_reject_flags() {
        for snippet in [
            "set_propagated_clock -bogus [get_ports {clk}]",
            "unset_propagated_clock -x",
            "set_max_transition 0.75 -bogus [current_design]",
            "set_max_fanout 10.0 0.5 [current_design]",
        ] {
            assert!(parse_sdc(&format!("{GCD}{snippet}\n")).is_err(), "accepted: {snippet}");
        }
        // The emitted forms still parse; the propagated clock is modelled, `set_max_*` is counted.
        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_propagated_clock [get_ports {{clk}}]\n\
             set_max_transition 0.75 [current_design]\n"
        ))
        .unwrap();
        assert!(sdc.propagated_clock);
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
        assert!(sdc.propagated_clock);
        assert_eq!(sdc.clock_latency_ns, 1.0);
        assert_eq!(sdc.clock_source_latency_ns, 0.2);

        let sdc = parse_sdc(&format!(
            "{GCD}\
             set_propagated_clock [get_clocks core_clk]\n\
             unset_propagated_clock [get_clocks core_clk]\n"
        ))
        .unwrap();
        assert!(!sdc.propagated_clock);

        for snippet in [
            // The virtual I/O clock cannot be propagated.
            "set_propagated_clock [get_clocks vclk]",
            // No design clock named at all.
            "set_propagated_clock [get_clocks other]",
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
        assert_eq!(sdc.clock_transition_ns, 0.15);
        assert_eq!(sdc.derate_early.factors[DERATE_CELL][1][0], 0.95);
        assert_eq!(sdc.derate_late.factors[DERATE_CELL][1][0], 1.05);
        assert_eq!(sdc.setup_uncertainty_ns, 0.25);
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
            assert!(parse_sdc(&format!("create_clock -name clk -period 2.0 [get_ports clk]\n{bad}")).is_err(), "accepted: {bad}");
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
            "set_clock_uncertainty 0.1 [get_clocks nope]",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn keeps_negative_values_and_the_known_flags() {
        let sdc = parse_sdc(&format!("{GCD}set_input_delay -0.5 -clock core_clk [all_inputs -no_clocks]\n")).unwrap();
        assert_eq!(sdc.input_delay_ns("req_val"), (-0.5, -0.5));
        let sdc = parse_sdc(&format!("{GCD}set_clock_uncertainty 0.1\n")).unwrap();
        assert_eq!(sdc.setup_uncertainty_ns, 0.1);
        assert_eq!(sdc.hold_uncertainty_ns, 0.1);
    }

    #[test]
    fn clock_uncertainty_setup_hold_and_clock_list() {
        let sdc = parse_sdc(&format!("{GCD}set_clock_uncertainty -setup 0.2 [get_clocks core_clk]\n")).unwrap();
        assert_eq!(sdc.setup_uncertainty_ns, 0.2);
        assert_eq!(sdc.hold_uncertainty_ns, 0.0);
        let sdc = parse_sdc(&format!("{GCD}set_clock_uncertainty -hold 0.1 [get_clocks core_clk]\n")).unwrap();
        assert_eq!(sdc.setup_uncertainty_ns, 0.0);
        assert_eq!(sdc.hold_uncertainty_ns, 0.1);
        let sdc = parse_sdc(&format!("{GCD}set_clock_uncertainty 0.3 [get_clocks core_clk]\n")).unwrap();
        assert_eq!(sdc.setup_uncertainty_ns, 0.3);
        assert_eq!(sdc.hold_uncertainty_ns, 0.3);
    }

    #[test]
    fn expr_follows_tcl_for_the_ibex_operands() {
        assert_eq!(eval_arith("8.0 * 0.2 "), Some(8.0 * 0.2));
        assert_eq!(eval_arith("(1.0 + 2.0) * 3 "), Some(9.0));
        assert_eq!(eval_arith("1.0 / 4 "), Some(0.25));
        assert_eq!(eval_arith("8 / 2 "), None);
        assert_eq!(eval_arith("1 + "), None);
    }
}
