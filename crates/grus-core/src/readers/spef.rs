use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

/// One `*D_NET`: ground capacitors per node and resistors between nodes, in pF and kohm.
/// A coupling capacitor (`*CAP` line with two nodes) is grounded on its first node with the full
/// value, and on its second node with half the value in the net that owns that node.
#[derive(Clone, Debug, Default)]
pub struct SpefNet {
    pub total_cap_pf: f32,
    pub caps: Vec<(String, f32)>,
    pub resistors: Vec<(String, String, f32)>,
}

/// A `*D_NET` under parse: its name, its parasitics, and the (second node, value) of each coupling cap.
type NetParse = (String, SpefNet, Vec<(String, f32)>);

#[derive(Clone, Debug, Default)]
pub struct Spef {
    pub nets: HashMap<String, SpefNet>,
}

/// Drops the backslash escapes SPEF puts before bus brackets and the hierarchy delimiter.
pub fn unescape_spef_name(name: &str) -> String {
    name.replace('\\', "")
}

fn unit_scale(value: &str, unit: &str, base: &[(&str, f32)]) -> Result<f32> {
    let v: f32 = value.parse().with_context(|| format!("bad SPEF unit value `{value}`"))?;
    let u = unit.to_ascii_uppercase();
    base.iter()
        .find(|(name, _)| *name == u)
        .map(|&(_, scale)| v * scale)
        .with_context(|| format!("unsupported SPEF unit `{unit}`"))
}

/// `*12` in a NAME_MAP section or reference. Anything else starting with `*` is a section header
/// (`*PORTS`) or a `*CONN` pin reference (`*I`), so it cannot be read as a NAME_MAP entry.
fn is_name_map_index(token: &str) -> bool {
    token.strip_prefix('*').is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

pub fn load_spef(path: &Path) -> Result<Spef> {
    let text = std::fs::read_to_string(path).with_context(|| format!("Failed to read SPEF {:?}", path))?;
    parse_spef(&text)
}

/// Parses the subset of IEEE 1481 that OpenROAD and common extractors write. Capacitance is
/// converted to pF and resistance to kohm, so that `R * C` is in ns.
pub fn parse_spef(text: &str) -> Result<Spef> {
    #[derive(PartialEq)]
    enum Section {
        None,
        Conn,
        Cap,
        Res,
    }
    let mut c_scale = 1.0f32;
    let mut r_scale = 1.0f32;
    let mut name_map: HashMap<String, String> = HashMap::new();
    let mut in_name_map = false;
    // Finished nets in file order, with the (second node, value) of each coupling cap they list.
    let mut ended: Vec<NetParse> = Vec::new();
    let mut current: Option<NetParse> = None;
    let mut section = Section::None;

    let resolve = |token: &str, map: &HashMap<String, String>| -> String {
        // `*12` or `*12:3` refer to the NAME_MAP; everything else is a literal name.
        if let Some(rest) = token.strip_prefix('*') {
            let (index, suffix) = rest.split_once(':').map_or((rest, None), |(a, b)| (a, Some(b)));
            if let Some(name) = map.get(&format!("*{index}")) {
                return match suffix {
                    Some(s) => format!("{name}:{s}"),
                    None => name.clone(),
                };
            }
        }
        unescape_spef_name(token)
    };

    for (line_no, line) in text.lines().enumerate() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(&first) = tokens.first() else { continue };
        let at = || format!("SPEF line {}", line_no + 1);
        match first {
            "*C_UNIT" | "*R_UNIT" | "*T_UNIT" if tokens.len() < 3 => {
                bail!("{}: `{first}` needs a value and a unit", at());
            }
            "*C_UNIT" => {
                c_scale = unit_scale(tokens[1], tokens[2], &[("PF", 1.0), ("FF", 1e-3), ("NF", 1e3)])
                    .with_context(at)?;
            }
            "*R_UNIT" => {
                r_scale = unit_scale(tokens[1], tokens[2], &[("KOHM", 1.0), ("OHM", 1e-3), ("MOHM", 1e3)])
                    .with_context(at)?;
            }
            "*T_UNIT" => {
                // R [kohm] * C [pF] is in ns, so only a ns time unit is consistent with the model.
                let scale = unit_scale(tokens[1], tokens[2], &[("NS", 1.0), ("PS", 1e-3)]).with_context(at)?;
                if (scale - 1.0).abs() > 1e-6 {
                    bail!("{}: only `1 NS` is supported as *T_UNIT", at());
                }
            }
            "*NAME_MAP" => in_name_map = true,
            "*D_NET" => {
                in_name_map = false;
                if tokens.len() < 3 {
                    bail!("{}: malformed *D_NET", at());
                }
                let net = SpefNet {
                    total_cap_pf: tokens[2].parse::<f32>().with_context(at)? * c_scale,
                    ..Default::default()
                };
                current = Some((resolve(tokens[1], &name_map), net, Vec::new()));
                section = Section::None;
            }
            "*CONN" => section = Section::Conn,
            "*CAP" => section = Section::Cap,
            "*RES" => section = Section::Res,
            "*END" => {
                if let Some(finished) = current.take() {
                    ended.push(finished);
                }
                section = Section::None;
            }
            _ if in_name_map && is_name_map_index(first) => {
                if tokens.len() < 2 {
                    bail!("{}: NAME_MAP entry `{first}` has no name", at());
                }
                name_map.insert(first.to_string(), unescape_spef_name(tokens[1]));
            }
            _ if in_name_map && first.starts_with('*') => in_name_map = false,
            _ => {
                let Some((_, net, couplings)) = current.as_mut() else { continue };
                match section {
                    Section::Cap if tokens.len() == 3 => {
                        net.caps.push((resolve(tokens[1], &name_map), tokens[2].parse::<f32>().with_context(at)? * c_scale));
                    }
                    Section::Cap if tokens.len() == 4 => {
                        let value = tokens[3].parse::<f32>().with_context(at)? * c_scale;
                        net.caps.push((resolve(tokens[1], &name_map), value));
                        couplings.push((resolve(tokens[2], &name_map), value));
                    }
                    Section::Cap => bail!("{}: *CAP entry must have 3 or 4 fields", at()),
                    Section::Res if tokens.len() == 4 => {
                        net.resistors.push((
                            resolve(tokens[1], &name_map),
                            resolve(tokens[2], &name_map),
                            tokens[3].parse::<f32>().with_context(at)? * r_scale,
                        ));
                    }
                    Section::Res => bail!("{}: *RES entry must have 4 fields", at()),
                    // *CONN entries are not modeled, and lines outside any section carry header
                    // fields (*SPEF, *DESIGN, *DATE, ...) that the database does not use.
                    Section::Conn | Section::None => {}
                }
            }
        }
    }
    if current.is_some() {
        bail!("SPEF: *D_NET without *END (truncated file?)");
    }

    // Node owners: resistor endpoints first, then cap first-nodes; the first net in file order wins.
    let mut owner: HashMap<&str, usize> = HashMap::new();
    for (i, (_, net, _)) in ended.iter().enumerate() {
        for (a, b, _) in &net.resistors {
            owner.entry(a.as_str()).or_insert(i);
            owner.entry(b.as_str()).or_insert(i);
        }
    }
    for (i, (_, net, _)) in ended.iter().enumerate() {
        for (node, _) in &net.caps {
            owner.entry(node.as_str()).or_insert(i);
        }
    }
    let halves: Vec<(usize, String, f32)> = ended
        .iter()
        .flat_map(|(_, _, couplings)| couplings)
        .filter_map(|(second, value)| owner.get(second.as_str()).map(|&i| (i, second.clone(), value / 2.0)))
        .collect();
    for (i, node, half) in halves {
        ended[i].1.caps.push((node, half));
    }

    let mut spef = Spef::default();
    for (name, net, _) in ended {
        spef.nets.insert(name, net);
    }
    Ok(spef)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, require};

    const SAMPLE: &str = "*SPEF \"ieee 1481-1999\"\n*C_UNIT 1 FF\n*R_UNIT 1 OHM\n*T_UNIT 1 NS\n\
        *NAME_MAP\n*1 net\\[0\\]\n*2 u1\n\
        *D_NET *1 5\n*CONN\n*I *2:A I\n*CAP\n1 *1:1 2\n2 *1:1 *1:2 1\n*RES\n1 *1:1 *2:A 100\n*END\n";

    #[test]
    fn converts_units_and_resolves_the_name_map() {
        let spef = parse_spef(SAMPLE).unwrap();
        let net = &spef.nets["net[0]"];
        assert!((net.total_cap_pf - 0.005).abs() < 1e-9);
        assert_eq!(net.caps.len(), 2);
        assert_eq!(net.caps[0].0, "net[0]:1");
        assert!((net.caps[0].1 - 0.002).abs() < 1e-9);
        // 1 OHM = 1e-3 kohm
        assert_eq!(net.resistors[0].1, "u1:A");
        assert!((net.resistors[0].2 - 0.1).abs() < 1e-6);
    }

    #[test]
    fn rejects_a_time_unit_other_than_ns() {
        assert!(parse_spef("*T_UNIT 1 PS\n").is_err());
    }

    #[test]
    fn test_aes_routed_spef_present() {
        let path = require(design_dir("aes").join("aes.routed.spef"));
        let spef = load_spef(&path).expect("aes.routed.spef must parse");
        assert!(!spef.nets.is_empty(), "aes.routed.spef has no nets");
    }

    #[test]
    fn test_jpeg_routed_spef_present() {
        let path = require(design_dir("jpeg").join("jpeg.routed.spef"));
        let spef = load_spef(&path).expect("jpeg.routed.spef must parse");
        assert!(!spef.nets.is_empty(), "jpeg.routed.spef has no nets");
    }

    #[test]
    fn test_ibex_routed_spef_present() {
        let path = require(design_dir("ibex").join("ibex.routed.spef"));
        let spef = load_spef(&path).expect("ibex.routed.spef must parse");
        assert!(!spef.nets.is_empty(), "ibex.routed.spef has no nets");
    }

    #[test]
    fn rejects_a_unit_line_without_its_unit() {
        assert!(parse_spef("*C_UNIT 1\n").is_err());
    }

    #[test]
    fn rejects_a_cap_entry_with_a_missing_value() {
        let text = SAMPLE.replace("2 *1:1 *1:2 1\n", "2 *1:1 *1:2\n");
        let msg = format!("{:#}", parse_spef(&text).unwrap_err());
        assert!(msg.contains("SPEF line 13"), "{msg}");
    }

    #[test]
    fn rejects_a_res_entry_with_a_missing_value() {
        let text = SAMPLE.replace("1 *1:1 *2:A 100\n", "1 *1:1 *2:A\n");
        assert!(parse_spef(&text).is_err());
    }

    #[test]
    fn rejects_a_net_without_end() {
        let text = SAMPLE.replace("*END\n", "");
        let msg = format!("{:#}", parse_spef(&text).unwrap_err());
        assert!(msg.contains("without *END"), "{msg}");
    }

    #[test]
    fn rejects_a_name_map_entry_without_a_name() {
        let text = SAMPLE.replace("*2 u1\n", "*2\n");
        assert!(parse_spef(&text).is_err());
    }

    #[test]
    fn rejects_a_numbered_name_map_entry_without_a_name() {
        let err = parse_spef("*NAME_MAP\n*123\n").unwrap_err();
        assert!(format!("{err:#}").contains("NAME_MAP entry `*123` has no name"));
    }

    #[test]
    fn name_map_ends_at_the_ports_section() {
        let text = "*C_UNIT 1 FF\n*R_UNIT 1 OHM\n*NAME_MAP\n*1 net\n*PORTS\nclk I\n\
            *D_NET *1 5\n*CAP\n1 *1:1 2\n*RES\n1 *1:1 *1:2 1\n*END\n";
        let spef = parse_spef(text).unwrap();
        assert!(spef.nets.contains_key("net"));
    }

    #[test]
    fn parses_the_gcd_routed_rcx_spef() {
        let path = require(design_dir("gcd").join("gcd.rcx.spef"));
        let spef = load_spef(&path).expect("gcd.rcx.spef must parse");
        assert_eq!(spef.nets.len(), 269);
        assert!(spef.nets.contains_key("clk"), "NAME_MAP entry *3 must resolve to clk");
    }

    fn cap_at(net: &SpefNet, node: &str) -> f32 {
        net.caps.iter().filter(|(n, _)| n == node).map(|(_, v)| v).sum()
    }

    /// Net A lists couplings to B:1 (owned by B's resistor), to Z:9 (owned by nobody), and to A:2
    /// (owned by A's own resistor). Each second node gets half its value in its owner's net.
    #[test]
    fn coupling_second_node_gets_half_in_its_owning_net() {
        let text = "*C_UNIT 1 FF\n*R_UNIT 1 OHM\n*T_UNIT 1 NS\n\
            *D_NET A 3\n*CAP\n1 A:1 1\n2 A:1 B:1 2\n3 A:1 Z:9 6\n4 A:1 A:2 4\n*RES\n1 A:1 A:2 100\n*END\n\
            *D_NET B 1\n*CAP\n1 B:2 1\n*RES\n1 B:1 B:2 50\n*END\n";
        let spef = parse_spef(text).unwrap();
        let (a, b) = (&spef.nets["A"], &spef.nets["B"]);
        assert!((a.total_cap_pf - 0.003).abs() < 1e-6);
        assert!((cap_at(a, "A:1") - 0.013).abs() < 1e-6, "full coupling values stay on the first node");
        assert!((cap_at(a, "A:2") - 0.002).abs() < 1e-6, "own-net owner gets half of A:2's coupling");
        assert!((cap_at(b, "B:1") - 0.001).abs() < 1e-6, "B owns B:1 and gets half of A's coupling to it");
        assert!((cap_at(b, "B:2") - 0.001).abs() < 1e-6);
        assert_eq!(b.caps.len(), 2);
        assert!(spef.nets.values().all(|n| cap_at(n, "Z:9") == 0.0), "unowned node gets nothing");
    }

    /// Every truncation from the first `*D_NET` through its `*END` must be rejected, and no
    /// truncation may panic.
    #[test]
    fn test_every_truncation_of_sample_is_rejected_or_parsed_without_panic() {
        let net_start = SAMPLE.find("*D_NET").expect("fixture has a net");
        let end = SAMPLE.find("*END").expect("fixture has *END") + "*END".len();
        for n in 0..=SAMPLE.len() {
            let result = parse_spef(&SAMPLE[..n]);
            if n >= net_start + "*D_NET".len() && n < end {
                assert!(result.is_err(), "truncation at {n} bytes was accepted");
            }
        }
    }
}
