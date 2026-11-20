//! SAIF (Switching Activity Interchange Format) reader for power annotation.
//!
//! SAIF carries per-signal aggregates — state durations (`T0`/`T1`/`TX`/`TZ`/`TB`),
//! a clean-toggle count (`TC`) and an ignore count (`IG`) — inside an `INSTANCE`
//! scope hierarchy, instead of VCD's per-edge stream. [`SaifActivity::resolve`]
//! maps a selected scope onto the design's flat net names and produces the same
//! [`VcdActivity`] `estimate_power` consumes: per-net (rise, fall) edge counts
//! plus the window duration.
//!
//! Measured OpenSTA 3.0.16 semantics this file does *not* copy (see
//! docs/results.md 2026-11-19): OpenSTA resolves `NET`-group names to pins of
//! the enclosing instance scope and to top-level ports at the top scope, drops
//! unmatched scopes/signals silently, and annotates nothing from `PORT` groups.
//! This reader resolves `NET` and `PORT` groups identically (a real emitter may
//! put pin data under `PORT`), resolves top-scope names to design nets *and*
//! ports, and counts every unmatched scope/signal in [`SaifDiagnostics`].
//!
//! Duty (`T0`/`T1`) and x/z/bus durations (`TX`/`TZ`/`TB`) are parsed in
//! [`SaifFields`] and carried into [`SignalActivity::t0_s`],
//! [`SignalActivity::t1_s`] and [`SignalActivity::other_s`]: the power model
//! consumes them as the state probabilities a `when` weight needs — including
//! state-weighted `leakage_power` (docs/results.md).

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

use crate::design_db::DesignDb;
use crate::interner::{CellId, NetId, PinId};
use crate::readers::vcd::{SignalActivity, VcdActivity};

/// Per-signal SAIF activity fields (durations in TIMESCALE units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SaifFields {
    pub t0: f64,
    pub t1: f64,
    pub tx: f64,
    pub tz: f64,
    pub tb: f64,
    pub tc: u64,
    pub ig: u64,
}

/// One scope in the SAIF hierarchy (`INSTANCE` node).
#[derive(Clone, Debug, Default)]
pub struct SaifScope {
    pub name: String,
    /// `NET` and `PORT` signal groups flattened; names raw, escapes stripped.
    pub signals: Vec<(String, SaifFields)>,
    pub instances: Vec<SaifScope>,
}

/// Switching activity read from a SAIF file.
#[derive(Clone, Debug, Default)]
pub struct SaifActivity {
    /// Top-level `INSTANCE` scopes.
    pub roots: Vec<SaifScope>,
    /// `DURATION * TIMESCALE`, seconds.
    pub duration_s: f64,
    /// Seconds per TIMESCALE tick.
    pub timescale_s: f64,
}

/// What scope resolution could not map; reported, never silently dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaifDiagnostics {
    /// `INSTANCE` scopes matching no design cell (their subtrees are skipped).
    pub unmatched_scopes: usize,
    /// Signal names matching no design net, port, or instance pin.
    pub unmatched_signals: usize,
}

enum Sexpr {
    Atom(String),
    List(Vec<Sexpr>),
}

/// SAIF tokens: `(`, `)`, quoted strings, and atoms. `\` escapes the next
/// character inside both atoms and quoted strings, so `a\[0\]` reads as
/// `a[0]` (DEF bus-bit form) and `\"` cannot end a string early.
fn tokenize(text: &str) -> Result<Vec<Sexpr>> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Vec<Sexpr>> = vec![Vec::new()];
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'(' => {
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                let Some(list) = stack.pop() else {
                    bail!("SAIF: unmatched `)`");
                };
                if stack.is_empty() {
                    bail!("SAIF: unmatched `)`");
                }
                stack.last_mut().unwrap().push(Sexpr::List(list));
                i += 1;
            }
            b'"' => {
                // `\` escapes the next byte inside quotes, same convention
                // as atoms: \" and \\ cannot terminate the string early.
                // Bytes accumulate verbatim and decode as UTF-8 — a
                // per-byte `char` cast would mojibake every multibyte
                // name (the input is `&str`, so decoding cannot fail).
                let mut s = Vec::new();
                i += 1;
                loop {
                    if i == bytes.len() {
                        bail!("SAIF: unterminated quoted string");
                    }
                    match bytes[i] {
                        b'"' => break,
                        b'\\' => {
                            i += 1;
                            if i == bytes.len() {
                                bail!("SAIF: unterminated quoted string");
                            }
                            s.push(bytes[i]);
                        }
                        b => s.push(b),
                    }
                    i += 1;
                }
                stack.last_mut().unwrap().push(Sexpr::Atom(
                    String::from_utf8(s).context("SAIF: non-UTF-8 string")?,
                ));
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let mut atom = Vec::new();
                while i < bytes.len() {
                    let b = bytes[i];
                    if b.is_ascii_whitespace() || b == b'(' || b == b')' {
                        break;
                    }
                    if b == b'\\' {
                        i += 1;
                        if i == bytes.len() {
                            break;
                        }
                        atom.push(bytes[i]);
                    } else {
                        atom.push(b);
                    }
                    i += 1;
                }
                stack.last_mut().unwrap().push(Sexpr::Atom(
                    String::from_utf8(atom).context("SAIF: non-UTF-8 atom")?,
                ));
            }
        }
    }
    if stack.len() != 1 {
        bail!("SAIF: unclosed `(`");
    }
    Ok(stack.pop().unwrap())
}

fn head_name(list: &[Sexpr]) -> Option<&str> {
    match list.first() {
        Some(Sexpr::Atom(a)) => Some(a.as_str()),
        _ => None,
    }
}

/// `(TIMESCALE <int> <unit>)` — same unit set as the VCD reader.
fn parse_saif_timescale(items: &[Sexpr]) -> Result<f64> {
    let (Some(Sexpr::Atom(num)), Some(Sexpr::Atom(unit))) = (items.get(1), items.get(2)) else {
        bail!("malformed SAIF TIMESCALE");
    };
    let scale = match unit.to_ascii_lowercase().as_str() {
        "s" => 1.0,
        "ms" => 1e-3,
        "us" => 1e-6,
        "ns" => 1e-9,
        "ps" => 1e-12,
        "fs" => 1e-15,
        _ => bail!("unsupported SAIF timescale unit `{unit}`"),
    };
    // The grammar says <int>, same triage as DURATION.
    let ticks: u64 = match num.parse::<u64>() {
        Ok(t) => t,
        Err(_) => match num.parse::<f64>() {
            Ok(f) if f < 0.0 => bail!("negative SAIF timescale `{num}`"),
            Ok(_) => bail!("SAIF timescale must be an integer, found `{num}`"),
            Err(_) => bail!("bad SAIF timescale `{num}`"),
        },
    };
    Ok(ticks as f64 * scale)
}

/// One signal entry `(<name> (FIELD value) ...)` inside a `NET`/`PORT` group.
/// `TC`/`IG` must be integral (OpenSTA rejects all floats; float durations are
/// allowed); unknown field names are ignored.
fn parse_signal(items: &[Sexpr]) -> Result<(String, SaifFields)> {
    let Some(name) = head_name(items) else {
        bail!("SAIF signal entry has no name");
    };
    let mut f = SaifFields::default();
    for item in &items[1..] {
        let Sexpr::List(field) = item else {
            continue;
        };
        let (Some(Sexpr::Atom(key)), Some(Sexpr::Atom(val))) =
            (field.first(), field.get(1))
        else {
            continue;
        };
        let number = || -> Result<f64> {
            val.parse::<f64>()
                .with_context(|| format!("bad SAIF {key} value `{val}`"))
        };
        match key.as_str() {
            "T0" => f.t0 = number()?,
            "T1" => f.t1 = number()?,
            "TX" => f.tx = number()?,
            "TZ" => f.tz = number()?,
            "TB" => f.tb = number()?,
            "TC" | "IG" => {
                let v = number()?;
                if v < 0.0 {
                    bail!("negative SAIF {key} value `{val}` for signal `{name}`");
                }
                if v.fract() != 0.0 {
                    bail!("non-integral SAIF {key} value `{val}` for signal `{name}`");
                }
                if key == "TC" {
                    f.tc = v as u64;
                } else {
                    f.ig = v as u64;
                }
            }
            _ => {}
        }
    }
    Ok((name.to_string(), f))
}

fn parse_instance(items: &[Sexpr]) -> Result<SaifScope> {
    let Some(Sexpr::Atom(name)) = items.get(1) else {
        bail!("SAIF INSTANCE has no name");
    };
    let mut scope = SaifScope {
        name: name.clone(),
        ..Default::default()
    };
    for item in &items[2..] {
        let Sexpr::List(list) = item else {
            continue;
        };
        match head_name(list) {
            // NET and PORT groups resolve identically; OpenSTA 3.0.16 ignores
            // PORT, but a real emitter may put pin data there (documented
            // divergence, docs/results.md).
            Some("NET") | Some("PORT") => {
                for sig in &list[1..] {
                    if let Sexpr::List(sig_items) = sig {
                        scope.signals.push(parse_signal(sig_items)?);
                    }
                }
            }
            Some("INSTANCE") => scope.instances.push(parse_instance(list)?),
            // VIRTUAL_INSTANCE (OpenSTA hard-errors; we are lenient) and any
            // unknown item: parser-level skip, like the VCD reader.
            _ => {}
        }
    }
    Ok(scope)
}

/// Parses SAIF text into the scope tree plus header times.
pub fn parse_saif(text: &str) -> Result<SaifActivity> {
    let top = tokenize(text)?;
    // The file is either a single `(SAIFILE ...)` list or a bare item list.
    let items: &[Sexpr] = match top.as_slice() {
        [Sexpr::List(l)] if head_name(l) == Some("SAIFILE") => &l[1..],
        _ => &top,
    };
    let mut timescale_s = None;
    let mut duration = None;
    let mut roots = Vec::new();
    for item in items {
        let Sexpr::List(list) = item else {
            continue;
        };
        match head_name(list) {
            Some("TIMESCALE") => timescale_s = Some(parse_saif_timescale(list)?),
            Some("DURATION") => {
                let Some(Sexpr::Atom(v)) = list.get(1) else {
                    bail!("malformed SAIF DURATION");
                };
                // The grammar says <int> and OpenSTA rejects (DURATION
                // 1990.0); any float literal or negative is a hard error.
                // The f64 triage names a numeric-but-non-integer literal
                // separately from a non-numeric token.
                duration = Some(match v.parse::<u64>() {
                    Ok(d) => d,
                    Err(_) => match v.parse::<f64>() {
                        Ok(f) if f < 0.0 => bail!("negative SAIF DURATION `{v}`"),
                        Ok(_) => bail!("SAIF DURATION must be an integer, found `{v}`"),
                        Err(_) => bail!("bad SAIF DURATION `{v}`"),
                    },
                });
            }
            Some("INSTANCE") => roots.push(parse_instance(list)?),
            _ => {}
        }
    }
    let timescale_s = timescale_s.context("SAIF has no TIMESCALE")?;
    let duration = duration.context("SAIF has no DURATION")?;
    let duration_s = duration as f64 * timescale_s;
    if duration_s <= 0.0 {
        bail!("SAIF DURATION is not positive ({duration} ticks)");
    }
    Ok(SaifActivity {
        roots,
        duration_s,
        timescale_s,
    })
}

/// Loads and parses a SAIF file.
pub fn load_saif(path: &Path) -> Result<SaifActivity> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read SAIF {:?}", path))?;
    parse_saif(&text)
}

/// Splits a SAIF toggle count into (rise, fall) edge counts. Even `tc` splits
/// evenly; odd `tc` means start != end and the unpaired last edge is a rise iff
/// the signal started low — inferred from the majority-time state (`t0 >= t1`).
/// The inference can be wrong on duty-skewed signals, bounded at ±1 edge per
/// net (docs/results.md).
fn split_tc(f: &SaifFields) -> (u64, u64) {
    let half = f.tc / 2;
    if f.tc.is_multiple_of(2) {
        (half, half)
    } else if f.t0 >= f.t1 {
        (half + 1, half)
    } else {
        (half, half + 1)
    }
}

impl SaifActivity {
    fn scope_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        fn walk(scope: &SaifScope, prefix: &str, paths: &mut Vec<String>) {
            let path = format!("{prefix}/{}", scope.name);
            paths.push(path.trim_start_matches('/').to_string());
            for child in &scope.instances {
                walk(child, &path, paths);
            }
        }
        for root in &self.roots {
            walk(root, "", &mut paths);
        }
        paths
    }

    /// Finds the scope `resolve` annotates. `Some("tb/dut")` walks `INSTANCE`
    /// names; a missing component is a hard error listing the available paths.
    /// `None` selects the outermost scope containing signal groups, descending
    /// through single-child chains; zero or several candidates is an error.
    fn select_scope(&self, scope: Option<&str>) -> Result<&SaifScope> {
        match scope {
            Some(path) => {
                let mut node: Option<&SaifScope> = None;
                for (depth, part) in path.trim_matches('/').split('/').enumerate() {
                    let children: &[SaifScope] = match node {
                        None => &self.roots,
                        Some(n) => &n.instances,
                    };
                    node = Some(children.iter().find(|s| s.name == part).ok_or_else(|| {
                        let comp = path.trim_matches('/').split('/').nth(depth).unwrap_or(part);
                        anyhow::anyhow!(
                            "SAIF scope `{path}`: no INSTANCE named `{comp}` (available: {})",
                            self.scope_paths().join(", ")
                        )
                    })?);
                }
                node.context("SAIF scope path is empty")
            }
            None => {
                let mut candidates = Vec::new();
                for root in &self.roots {
                    let mut node = root;
                    while node.signals.is_empty() && node.instances.len() == 1 {
                        node = &node.instances[0];
                    }
                    if !node.signals.is_empty() {
                        candidates.push(node);
                    }
                }
                match candidates.len() {
                    1 => Ok(candidates[0]),
                    _ => bail!(
                        "SAIF scope is ambiguous ({candidates} candidates); pass --saif-scope \
                         (available: {})",
                        self.scope_paths().join(", "),
                        candidates = candidates.len()
                    ),
                }
            }
        }
    }

    /// Resolves the hierarchy to per-net edge counts for `design`, producing
    /// the `VcdActivity` `estimate_power` consumes. `scope` is a `/`-joined
    /// `INSTANCE` path from a SAIF root; `None` selects the outermost scope
    /// containing signal groups.
    pub fn resolve(
        &self,
        design: &DesignDb,
        scope: Option<&str>,
    ) -> Result<(VcdActivity, SaifDiagnostics)> {
        let selected = self.select_scope(scope)?;

        let net_of: HashMap<&str, NetId> = design
            .net_names
            .iter()
            .enumerate()
            .map(|(i, &s)| (design.strings.resolve(s), NetId(i as u32)))
            .collect();
        let cell_of: HashMap<&str, CellId> = design
            .cell_names
            .iter()
            .enumerate()
            .map(|(i, &s)| (design.strings.resolve(s), CellId(i as u32)))
            .collect();
        let port_net_of: HashMap<&str, Option<NetId>> = design
            .io_pins
            .iter()
            .map(|&p| {
                (
                    design.strings.resolve(design.pin_names[p.0 as usize]),
                    design.pin_nets[p.0 as usize],
                )
            })
            .collect();

        let mut diag = SaifDiagnostics::default();
        let mut signals: HashMap<String, SignalActivity> = HashMap::new();

        // Top-scope signal names resolve to a design net by name, else to the
        // net of a top-level port. Nested INSTANCE names resolve to design
        // cells whose signal names are pin names on that instance.
        fn annotate(
            signals: &mut HashMap<String, SignalActivity>,
            design: &DesignDb,
            net: NetId,
            f: &SaifFields,
            timescale_s: f64,
        ) {
            let name = design
                .strings
                .resolve(design.net_names[net.0 as usize])
                .to_string();
            let (rise, fall) = split_tc(f);
            signals.insert(
                name,
                SignalActivity {
                    rise,
                    fall,
                    xtrans: 0,
                    t0_s: f.t0 * timescale_s,
                    t1_s: f.t1 * timescale_s,
                    other_s: (f.tx + f.tz + f.tb) * timescale_s,
                },
            );
        }

        for (name, f) in &selected.signals {
            let net = net_of
                .get(name.as_str())
                .copied()
                .or_else(|| port_net_of.get(name.as_str()).copied().flatten());
            match net {
                Some(net) => annotate(&mut signals, design, net, f, self.timescale_s),
                None => diag.unmatched_signals += 1,
            }
        }

        fn resolve_instance(
            scope: &SaifScope,
            design: &DesignDb,
            cell_of: &HashMap<&str, CellId>,
            signals: &mut HashMap<String, SignalActivity>,
            diag: &mut SaifDiagnostics,
            timescale_s: f64,
        ) {
            let Some(&cell) = cell_of.get(scope.name.as_str()) else {
                diag.unmatched_scopes += 1;
                return;
            };
            let pin_of: HashMap<&str, PinId> = design
                .cell_pins(cell)
                .iter()
                .map(|&p| (design.strings.resolve(design.pin_names[p.0 as usize]), p))
                .collect();
            for (name, f) in &scope.signals {
                let net = pin_of
                    .get(name.as_str())
                    .and_then(|&p| design.pin_nets[p.0 as usize]);
                match net {
                    Some(net) => annotate(signals, design, net, f, timescale_s),
                    None => diag.unmatched_signals += 1,
                }
            }
            for child in &scope.instances {
                resolve_instance(child, design, cell_of, signals, diag, timescale_s);
            }
        }
        for inst in &selected.instances {
            resolve_instance(
                inst,
                design,
                &cell_of,
                &mut signals,
                &mut diag,
                self.timescale_s,
            );
        }

        Ok((
            VcdActivity {
                signals,
                duration_s: self.duration_s,
                timescale_s: self.timescale_s,
            },
            diag,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "(SAIFILE (SAIFVERSION \"2.0\") (DIRECTION \"backward\") \
                          (DIVIDER / ) (TIMESCALE 1 ns) (DURATION 100) ";

    fn saif(body: &str) -> String {
        format!("{HEADER}{body})")
    }

    fn sig(name: &str, tc: u64, t0: u64, t1: u64) -> String {
        format!("({name} (T0 {t0}) (T1 {t1}) (TX 0) (TZ 0) (TB 0) (TC {tc}) (IG 0))")
    }

    #[test]
    fn test_parse_header_and_scopes() {
        let text = saif(&format!(
            "(INSTANCE tb (INSTANCE dut (NET {} {}) (INSTANCE _u_ (NET {}))))",
            sig("clk", 4, 50, 50),
            sig("a", 2, 60, 40),
            sig("D", 2, 60, 40),
        ));
        let act = parse_saif(&text).expect("parse");
        assert!((act.timescale_s - 1e-9).abs() < 1e-20);
        assert!((act.duration_s - 100e-9).abs() < 1e-20);
        assert_eq!(act.roots.len(), 1);
        let tb = &act.roots[0];
        assert_eq!(tb.name, "tb");
        let dut = &tb.instances[0];
        assert_eq!(dut.signals.len(), 2);
        assert_eq!(dut.signals[0].0, "clk");
        assert_eq!(dut.signals[0].1.tc, 4);
        assert_eq!(dut.instances[0].name, "_u_");
        assert_eq!(dut.instances[0].signals.len(), 1);
    }

    #[test]
    fn test_port_group_collected() {
        // PORT groups carry pin data in some emitters; we collect them like NET.
        let text = saif(&format!(
            "(INSTANCE dut (PORT {}) (NET {}))",
            sig("A", 2, 10, 90),
            sig("clk", 4, 50, 50),
        ));
        let act = parse_saif(&text).expect("parse");
        assert_eq!(act.roots[0].signals.len(), 2);
        assert_eq!(act.roots[0].signals[0].0, "A");
        assert_eq!(act.roots[0].signals[1].0, "clk");
    }

    #[test]
    fn test_escaped_names() {
        let text = saif(&format!(
            "(INSTANCE dut (NET {}))",
            sig("data\\[0\\]", 2, 10, 90),
        ));
        let act = parse_saif(&text).expect("parse");
        assert_eq!(act.roots[0].signals[0].0, "data[0]");
    }

    #[test]
    fn test_fractional_tc_is_error() {
        let text = saif(
            "(INSTANCE dut (NET (clk (T0 0) (T1 0) (TC 4.5))))",
        );
        let err = match parse_saif(&text) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("fractional TC accepted"),
        };
        assert!(err.contains("non-integral SAIF TC value `4.5`"), "{err}");
    }

    #[test]
    fn test_negative_tc_is_error() {
        let text = saif("(INSTANCE dut (NET (clk (T0 0) (T1 0) (TC -3))))");
        let err = match parse_saif(&text) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("negative TC accepted"),
        };
        assert!(err.contains("negative SAIF TC value `-3`"), "{err}");
    }

    #[test]
    fn test_fractional_duration_is_error() {
        // The grammar says (DURATION <int>) and OpenSTA rejects a float
        // literal — even an integral-valued one. Each rejection names the
        // actual kind: non-integer numeric, negative, non-numeric.
        for (v, expected) in [
            ("1980.5", "SAIF DURATION must be an integer, found `1980.5`"),
            ("1980.0", "SAIF DURATION must be an integer, found `1980.0`"),
            ("-5", "negative SAIF DURATION `-5`"),
            ("abc", "bad SAIF DURATION `abc`"),
        ] {
            let text = format!("(SAIFILE (TIMESCALE 1 ns) (DURATION {v}) (INSTANCE dut))");
            let err = match parse_saif(&text) {
                Err(e) => format!("{e:#}"),
                Ok(_) => panic!("DURATION {v} accepted"),
            };
            assert!(err.contains(expected), "DURATION {v}: {err}");
        }
    }

    #[test]
    fn test_utf8_net_name() {
        // A multibyte name survives verbatim — the tokenizer used to map
        // each UTF-8 byte to its own char and mojibake the name.
        let text = saif("(INSTANCE dut (NET (büße (T0 0) (T1 0) (TC 4))))");
        let act = parse_saif(&text).expect("parse");
        assert_eq!(act.roots[0].signals[0].0, "büße");
    }

    #[test]
    fn test_quoted_string_escapes() {
        // `\` escapes the next byte inside quotes, same as in atoms:
        // \" stays literal, \\ is one backslash — neither ends the string.
        let items =
            tokenize("(VERSION \"a \\\"b\\\" c\" (VERSION \"x\\\\y\"))").expect("tokenize");
        let Sexpr::List(top) = &items[0] else {
            panic!("expected list");
        };
        // The escaped quotes stay inside the first string; the sibling list
        // parses as a sibling, not trailing junk.
        let Sexpr::Atom(q) = &top[1] else {
            panic!("expected quoted atom");
        };
        assert_eq!(q, "a \"b\" c");
        let Sexpr::List(inner) = &top[2] else {
            panic!("expected nested list");
        };
        let Sexpr::Atom(bs) = &inner[1] else {
            panic!("expected quoted atom");
        };
        assert_eq!(bs, "x\\y");
    }

    #[test]
    fn test_missing_duration_is_error() {
        let text = "(SAIFILE (TIMESCALE 1 ns) (INSTANCE dut))";
        assert!(parse_saif(text).is_err());
    }

    #[test]
    fn test_missing_timescale_is_error() {
        let text = "(SAIFILE (DURATION 100) (INSTANCE dut))";
        assert!(parse_saif(text).is_err());
    }

    #[test]
    fn test_virtual_instance_skipped() {
        let text = saif(&format!(
            "(INSTANCE dut (VIRTUAL_INSTANCE x y) (NET {}))",
            sig("clk", 4, 50, 50),
        ));
        let act = parse_saif(&text).expect("parse");
        assert_eq!(act.roots[0].signals.len(), 1);
    }

    #[test]
    fn test_split_tc() {
        let even = SaifFields {
            tc: 200,
            ..Default::default()
        };
        assert_eq!(split_tc(&even), (100, 100));
        // Odd count: majority-time state taken as the start state; a low start
        // makes the unpaired last edge a rise.
        let low = SaifFields {
            tc: 199,
            t0: 1000.0,
            t1: 990.0,
            ..Default::default()
        };
        assert_eq!(split_tc(&low), (100, 99));
        let high = SaifFields {
            tc: 199,
            t0: 990.0,
            t1: 1000.0,
            ..Default::default()
        };
        assert_eq!(split_tc(&high), (99, 100));
        // Duty tie resolves to rise (the start-low assumption); mb's clk hits
        // this case under the truncated [10,1990] window.
        let tie = SaifFields {
            tc: 199,
            t0: 990.0,
            t1: 990.0,
            ..Default::default()
        };
        assert_eq!(split_tc(&tie), (100, 99));
    }

    #[test]
    fn test_auto_scope_single_chain() {
        let text = saif(&format!(
            "(INSTANCE tb (INSTANCE dut (NET {})))",
            sig("clk", 4, 50, 50),
        ));
        let act = parse_saif(&text).expect("parse");
        assert_eq!(act.select_scope(None).unwrap().name, "dut");
        assert_eq!(act.select_scope(Some("tb/dut")).unwrap().name, "dut");
    }

    /// Minimal mb-shaped design: nets clk/a/q/y, ports clk/a/y, cell `_d_`
    /// with pins D/CLK/Q on nets a/clk/q, cell `_i_` with A/Y on q/y.
    fn mini_design() -> DesignDb {
        use crate::interner::*;
        let mut d = DesignDb::default();
        for n in ["clk", "a", "q", "y"] {
            d.net_names.push(d.strings.intern(n));
        }
        for n in ["_d_", "_i_"] {
            d.cell_names.push(d.strings.intern(n));
        }
        // pins: 0 clk port, 1 a port, 2 y port, 3 _d_/D, 4 _d_/CLK, 5 _d_/Q,
        //       6 _i_/A, 7 _i_/Y
        for (n, cell, net) in [
            ("clk", None, 0usize),
            ("a", None, 1),
            ("y", None, 3),
            ("D", Some(0), 1),
            ("CLK", Some(0), 0),
            ("Q", Some(0), 2),
            ("A", Some(1), 2),
            ("Y", Some(1), 3),
        ] {
            d.pin_names.push(d.strings.intern(n));
            d.pin_cells.push(cell.map(CellId));
            d.pin_nets.push(Some(NetId(net as u32)));
        }
        d.io_pins = vec![PinId(0), PinId(1), PinId(2)];
        d.cell_pin_offsets = vec![0, 3, 5];
        d.cell_pin_ids = vec![PinId(3), PinId(4), PinId(5), PinId(6), PinId(7)];
        d
    }

    #[test]
    fn test_resolve_nets_ports_pins_and_drops() {
        let text = saif(&format!(
            "(INSTANCE tb (INSTANCE dut (NET {} {} {} {} {}) \
             (INSTANCE _d_ (NET {} {} {})) (INSTANCE ghost (NET {}))))",
            // Mirrors the committed mb.saif (window [10,1990]): clk/CLK carry
            // the odd TC=199 with a duty tie -> (100,99).
            sig("clk", 199, 990, 990),
            sig("a", 4, 1180, 800),
            sig("q", 4, 1180, 800),
            sig("y", 4, 800, 1180),
            sig("dut_sig", 4, 100, 100), // matches no net or port
            sig("D", 4, 1180, 800),
            sig("CLK", 199, 990, 990),
            sig("Q", 4, 1180, 800),
            sig("ghost_sig", 2, 10, 10),
        ));
        let act = parse_saif(&text).expect("parse");
        let design = mini_design();
        let (vcd, diag) = act.resolve(&design, None).expect("resolve");
        assert_eq!((vcd.signals["clk"].rise, vcd.signals["clk"].fall), (100, 99));
        assert_eq!((vcd.signals["a"].rise, vcd.signals["a"].fall), (2, 2));
        assert_eq!((vcd.signals["q"].rise, vcd.signals["q"].fall), (2, 2));
        assert_eq!((vcd.signals["y"].rise, vcd.signals["y"].fall), (2, 2));
        assert_eq!(diag.unmatched_scopes, 1); // ghost
        assert_eq!(diag.unmatched_signals, 1); // dut_sig
        // Explicit scope path resolves the same subtree.
        let (vcd2, _) = act.resolve(&design, Some("tb/dut")).expect("resolve");
        assert_eq!(vcd2.signals, vcd.signals);
    }

    #[test]
    fn test_auto_scope_ambiguous_errors() {
        let text = saif(&format!(
            "(INSTANCE a (NET {})) (INSTANCE b (NET {}))",
            sig("clk", 4, 50, 50),
            sig("clk", 4, 50, 50),
        ));
        let act = parse_saif(&text).expect("parse");
        assert!(act.select_scope(None).is_err());
        assert!(act.select_scope(Some("tb/dut")).is_err());
    }
}
