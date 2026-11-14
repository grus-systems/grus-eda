use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

/// One `*D_NET`: ground capacitors per node and resistors between nodes, in pF and kohm.
/// Coupling capacitors (`*CAP` lines with two nodes) are lumped to ground on the first node.
#[derive(Clone, Debug, Default)]
pub struct SpefNet {
    pub total_cap_pf: f32,
    pub caps: Vec<(String, f32)>,
    pub resistors: Vec<(String, String, f32)>,
}

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
    let mut spef = Spef::default();
    let mut current: Option<(String, SpefNet)> = None;
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
            "*C_UNIT" if tokens.len() >= 3 => {
                c_scale = unit_scale(tokens[1], tokens[2], &[("PF", 1.0), ("FF", 1e-3), ("NF", 1e3)])
                    .with_context(at)?;
            }
            "*R_UNIT" if tokens.len() >= 3 => {
                r_scale = unit_scale(tokens[1], tokens[2], &[("KOHM", 1.0), ("OHM", 1e-3), ("MOHM", 1e3)])
                    .with_context(at)?;
            }
            "*T_UNIT" if tokens.len() >= 3 => {
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
                current = Some((resolve(tokens[1], &name_map), net));
                section = Section::None;
            }
            "*CONN" => section = Section::Conn,
            "*CAP" => section = Section::Cap,
            "*RES" => section = Section::Res,
            "*END" => {
                if let Some((name, net)) = current.take() {
                    spef.nets.insert(name, net);
                }
                section = Section::None;
            }
            _ if in_name_map && first.starts_with('*') && tokens.len() >= 2 => {
                name_map.insert(first.to_string(), unescape_spef_name(tokens[1]));
            }
            _ => {
                let Some((_, net)) = current.as_mut() else { continue };
                match section {
                    Section::Cap if tokens.len() == 3 => {
                        net.caps.push((resolve(tokens[1], &name_map), tokens[2].parse::<f32>().with_context(at)? * c_scale));
                    }
                    Section::Cap if tokens.len() == 4 => {
                        net.caps.push((resolve(tokens[1], &name_map), tokens[3].parse::<f32>().with_context(at)? * c_scale));
                    }
                    Section::Res if tokens.len() == 4 => {
                        net.resistors.push((
                            resolve(tokens[1], &name_map),
                            resolve(tokens[2], &name_map),
                            tokens[3].parse::<f32>().with_context(at)? * r_scale,
                        ));
                    }
                    _ => {}
                }
            }
        }
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
}
