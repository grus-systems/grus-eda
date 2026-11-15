use anyhow::{Context, Result, bail};
use std::collections::HashMap;
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

#[derive(Clone, Debug)]
struct DelayRule {
    sel: PortSel,
    value_ns: f32,
    clock: ClockRef,
}

/// The SDC subset the benchmark flow uses. Commands are applied in file order, so a later
/// command overrides an earlier one on the same port.
#[derive(Clone, Debug, Default)]
pub struct Sdc {
    pub clock_name: Option<String>,
    pub clock_port: Option<String>,
    pub period_ns: f32,
    pub uncertainty_ns: f32,
    pub clock_latency_ns: f32,
    pub io_clock_latency_ns: f32,
    input_delays: Vec<DelayRule>,
    output_delays: Vec<DelayRule>,
    input_transitions: Vec<(PortSel, f32)>,
    loads: Vec<(PortSel, f32)>,
}

fn last_delay<'a>(rules: &'a [DelayRule], port: &str, is_input: bool, clock_port: Option<&str>) -> Option<&'a DelayRule> {
    rules.iter().rev().find(|r| r.sel.matches(port, is_input, clock_port))
}

fn last_value(rules: &[(PortSel, f32)], port: &str, is_input: bool, clock_port: Option<&str>) -> Option<f32> {
    rules.iter().rev().find(|(sel, _)| sel.matches(port, is_input, clock_port)).map(|&(_, v)| v)
}

impl Sdc {
    fn latency_ns(&self, clock: ClockRef) -> f32 {
        match clock {
            ClockRef::Design => self.clock_latency_ns,
            ClockRef::Virtual => self.io_clock_latency_ns,
        }
    }

    pub fn input_delay_ns(&self, port: &str) -> f32 {
        last_delay(&self.input_delays, port, true, self.clock_port.as_deref()).map_or(0.0, |r| r.value_ns)
    }

    /// Latency of the clock that the port's input delay is referenced to.
    pub fn input_clock_latency_ns(&self, port: &str) -> f32 {
        last_delay(&self.input_delays, port, true, self.clock_port.as_deref()).map_or(0.0, |r| self.latency_ns(r.clock))
    }

    pub fn output_delay_ns(&self, port: &str) -> f32 {
        last_delay(&self.output_delays, port, false, self.clock_port.as_deref()).map_or(0.0, |r| r.value_ns)
    }

    /// Latency of the clock that the port's output delay is referenced to.
    pub fn output_clock_latency_ns(&self, port: &str) -> f32 {
        last_delay(&self.output_delays, port, false, self.clock_port.as_deref()).map_or(0.0, |r| self.latency_ns(r.clock))
    }

    pub fn input_transition_ns(&self, port: &str) -> f32 {
        last_value(&self.input_transitions, port, true, self.clock_port.as_deref()).unwrap_or(0.0)
    }

    pub fn output_load_pf(&self, port: &str) -> f32 {
        last_value(&self.loads, port, false, self.clock_port.as_deref()).unwrap_or(0.0)
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
            "create_clock" => {
                self.create_clock(args)?;
                Ok(empty())
            }
            "set_clock_latency" => {
                self.set_clock_latency(args)?;
                Ok(empty())
            }
            "set_clock_uncertainty" => {
                self.sdc.uncertainty_ns = single_number(args)?;
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
                let rule = value_and_ports(args)?;
                self.sdc.input_transitions.push(rule);
                Ok(empty())
            }
            "set_load" => {
                let rule = value_and_ports(args)?;
                self.sdc.loads.push(rule);
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
        match positionals(args)?.as_slice() {
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

    fn set_clock_latency(&mut self, args: &[Value]) -> Result<()> {
        let [Value::Str(value), Value::Clocks(names)] = args else {
            bail!("set_clock_latency takes a value and [get_clocks <name>]");
        };
        let [name] = names.as_slice() else {
            bail!("set_clock_latency takes one clock");
        };
        let latency = parse_f32(value)?;
        match self.resolve_clock(name)? {
            ClockRef::Design => self.sdc.clock_latency_ns = latency,
            ClockRef::Virtual => self.sdc.io_clock_latency_ns = latency,
        }
        Ok(())
    }

    fn delay_rule(&self, args: &[Value]) -> Result<DelayRule> {
        let (sel, value_ns) = value_and_ports(args)?;
        let clock = match flag_value(args, "-clock")? {
            Some(name) => self.resolve_clock(name)?,
            None => ClockRef::Design,
        };
        Ok(DelayRule { sel, value_ns, clock })
    }

    fn resolve_clock(&self, name: &str) -> Result<ClockRef> {
        if self.sdc.clock_name.as_deref() == Some(name) {
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

/// Flags that take a value, so that their value is not mistaken for a positional argument.
const VALUE_FLAGS: [&str; 5] = ["-clock", "-name", "-period", "-lib_cell", "-pin"];

/// The value after `flag`, or None when the flag is absent. A flag with no value is an error.
fn flag_value<'a>(args: &'a [Value], flag: &str) -> Result<Option<&'a str>> {
    let Some(i) = args.iter().position(|a| matches!(a, Value::Str(s) if s == flag)) else {
        return Ok(None);
    };
    match args.get(i + 1) {
        Some(Value::Str(v)) => Ok(Some(v.as_str())),
        _ => bail!("{flag} needs a value"),
    }
}

/// `-1.5` or `-.5`: a dash followed by a decimal number. `-inf` is not one.
fn is_negative_number(text: &str) -> bool {
    text.strip_prefix('-')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit() || c == '.') && rest.parse::<f32>().is_ok())
}

/// The arguments that are neither a value flag nor the value of one. Any other flag is an error:
/// skipping it would drop a meaning-changing option such as `-max`, `-add_delay` or `-setup`.
fn positionals(args: &[Value]) -> Result<Vec<&Value>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match &args[i] {
            Value::Str(s) if VALUE_FLAGS.contains(&s.as_str()) => i += 2,
            Value::Str(s) if s.starts_with('-') && !is_negative_number(s) => bail!("unsupported option `{s}`"),
            value => {
                out.push(value);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn single_number(args: &[Value]) -> Result<f32> {
    let [Value::Str(value)] = positionals(args)?.as_slice() else {
        bail!("expected a single numeric value");
    };
    parse_f32(value)
}

fn value_and_ports(args: &[Value]) -> Result<(PortSel, f32)> {
    let [Value::Str(value), Value::Ports(sel)] = positionals(args)?.as_slice() else {
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
        assert_eq!(sdc.input_delay_ns("req_val"), 1.0);
        assert_eq!(sdc.output_delay_ns("resp_val"), 1.0);
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
        assert_eq!(sdc.input_delay_ns("instr_rdata_i"), 1.6);
        assert_eq!(sdc.input_delay_ns("clk_i"), 0.0, "-no_clocks must exclude the clock port");
        assert_eq!(sdc.input_clock_latency_ns("instr_rdata_i"), 1.095);
        assert_eq!(sdc.output_delay_ns("instr_req_o"), 1.6);
        assert_eq!(sdc.output_clock_latency_ns("instr_req_o"), 1.095);
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
            assert_eq!(sdc.input_delay_ns("req_val"), io_ns);
            assert_eq!(sdc.output_delay_ns("resp_val"), io_ns);
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
    fn rejects_flags_that_change_the_meaning() {
        let cases = [
            "set_input_delay -max 1.0 -clock core_clk [all_inputs]",
            "set_input_delay 1.0 -add_delay -clock core_clk [all_inputs]",
            "set_output_delay -min 1.0 -clock core_clk [all_outputs]",
            "set_clock_uncertainty -setup 0.1",
            "set_clock_uncertainty -hold 0.1",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
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
            "set_clock_uncertainty 0.1 [get_clocks core_clk]",
        ];
        for snippet in cases {
            let text = format!("{GCD}{snippet}\n");
            assert!(parse_sdc(&text).is_err(), "accepted: {snippet}");
        }
    }

    #[test]
    fn keeps_negative_values_and_the_known_flags() {
        let sdc = parse_sdc(&format!("{GCD}set_input_delay -0.5 -clock core_clk [all_inputs -no_clocks]\n")).unwrap();
        assert_eq!(sdc.input_delay_ns("req_val"), -0.5);
        assert!(parse_sdc(&format!("{GCD}set_clock_uncertainty 0.1\n")).is_ok());
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
