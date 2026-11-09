use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
enum PortSel {
    AllInputs,
    AllOutputs,
    Ports(Vec<String>),
}

impl PortSel {
    fn matches(&self, port: &str, is_input: bool) -> bool {
        match self {
            PortSel::AllInputs => is_input,
            PortSel::AllOutputs => !is_input,
            PortSel::Ports(names) => names.iter().any(|n| n == port),
        }
    }
}

/// The SDC subset the benchmark flow uses. Commands are applied in file order, so a later
/// command overrides an earlier one on the same port.
#[derive(Clone, Debug, Default)]
pub struct Sdc {
    pub clock_name: Option<String>,
    pub clock_port: Option<String>,
    pub period_ns: f32,
    pub uncertainty_ns: f32,
    input_delays: Vec<(PortSel, f32)>,
    output_delays: Vec<(PortSel, f32)>,
    input_transitions: Vec<(PortSel, f32)>,
    loads: Vec<(PortSel, f32)>,
}

fn last_match(rules: &[(PortSel, f32)], port: &str, is_input: bool) -> Option<f32> {
    rules.iter().rev().find(|(sel, _)| sel.matches(port, is_input)).map(|&(_, v)| v)
}

impl Sdc {
    pub fn input_delay_ns(&self, port: &str) -> f32 {
        last_match(&self.input_delays, port, true).unwrap_or(0.0)
    }

    pub fn output_delay_ns(&self, port: &str) -> f32 {
        last_match(&self.output_delays, port, false).unwrap_or(0.0)
    }

    pub fn input_transition_ns(&self, port: &str) -> f32 {
        last_match(&self.input_transitions, port, true).unwrap_or(0.0)
    }

    pub fn output_load_pf(&self, port: &str) -> f32 {
        last_match(&self.loads, port, false).unwrap_or(0.0)
    }
}

pub fn load_sdc(path: &Path) -> Result<Sdc> {
    let text = std::fs::read_to_string(path).with_context(|| format!("Failed to read SDC {:?}", path))?;
    parse_sdc(&text)
}

pub fn parse_sdc(text: &str) -> Result<Sdc> {
    let mut sdc = Sdc::default();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("");
        let spaced = line.replace(['[', ']', '{', '}'], " ");
        let tokens: Vec<&str> = spaced.split_whitespace().collect();
        let Some((&command, args)) = tokens.split_first() else {
            continue;
        };
        let at = || format!("SDC line {}: `{}`", line_no + 1, raw.trim());
        match command {
            "create_clock" => {
                sdc.period_ns = flag_value(args, "-period")
                    .with_context(|| format!("{}: missing -period", at()))?
                    .parse()
                    .with_context(|| format!("{}: bad -period", at()))?;
                sdc.clock_name = flag_value(args, "-name").map(str::to_string);
                if let Some(PortSel::Ports(ports)) = port_selector(args) {
                    sdc.clock_port = ports.into_iter().next();
                }
            }
            "set_input_delay" => sdc.input_delays.push(value_and_ports(args, &at())?),
            "set_output_delay" => sdc.output_delays.push(value_and_ports(args, &at())?),
            "set_input_transition" => sdc.input_transitions.push(value_and_ports(args, &at())?),
            "set_load" => sdc.loads.push(value_and_ports(args, &at())?),
            "set_clock_uncertainty" => {
                sdc.uncertainty_ns = positionals(args)
                    .first()
                    .and_then(|t| t.parse().ok())
                    .with_context(|| format!("{}: missing numeric value", at()))?;
            }
            _ => bail!("{}: unsupported command `{}`", at(), command),
        }
    }
    if sdc.period_ns <= 0.0 {
        bail!("SDC defines no clock period");
    }
    Ok(sdc)
}

/// Flags that take a value, so that their value is not mistaken for a positional argument.
const VALUE_FLAGS: [&str; 5] = ["-clock", "-name", "-period", "-lib_cell", "-pin"];

fn flag_value<'a>(args: &[&'a str], flag: &str) -> Option<&'a str> {
    args.iter().position(|&a| a == flag).and_then(|i| args.get(i + 1)).copied()
}

fn positionals<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if VALUE_FLAGS.contains(&args[i]) {
            i += 2;
        } else if args[i].starts_with('-') && args[i].parse::<f32>().is_err() {
            i += 1;
        } else {
            out.push(args[i]);
            i += 1;
        }
    }
    out
}

fn port_selector(args: &[&str]) -> Option<PortSel> {
    if args.contains(&"all_inputs") {
        return Some(PortSel::AllInputs);
    }
    if args.contains(&"all_outputs") {
        return Some(PortSel::AllOutputs);
    }
    let after = args.iter().position(|&a| a == "get_ports")?;
    let names = positionals(&args[after + 1..]).into_iter().map(str::to_string).collect();
    Some(PortSel::Ports(names))
}

fn value_and_ports(args: &[&str], at: &str) -> Result<(PortSel, f32)> {
    let pos = positionals(args);
    let value = pos
        .iter()
        .find_map(|t| t.parse::<f32>().ok())
        .with_context(|| format!("{}: missing numeric value", at))?;
    let sel = port_selector(args).with_context(|| format!("{}: missing port selector", at))?;
    Ok((sel, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GCD: &str = "create_clock -name core_clk -period 5.0 [get_ports clk]\n\
        set_input_delay 1.0 -clock core_clk [all_inputs -no_clocks]\n\
        set_output_delay 1.0 -clock core_clk [all_outputs]\n\
        set_input_transition 0.15 [all_inputs]\n\
        set_load 0.05 [all_outputs]\n";

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
}
