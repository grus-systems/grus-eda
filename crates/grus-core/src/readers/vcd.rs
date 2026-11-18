//! Value-change-dump (IEEE 1364) reader for switching-activity annotation.
//!
//! Produces per-signal rise/fall transition counts over the dumped window. Vector
//! signals are expanded to per-bit names (`data[3]`) so they line up with the
//! per-bit net names DEF uses. Only clean 0<->1 edges are counted; transitions
//! involving x/z update the last state without counting, since a glitch through
//! x is not a defined logic edge. Scope paths are ignored: DEF net names are flat
//! leaf names, and directly-connected ports share their driver's transitions.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Edge counts for one dumped signal (or one bit of a vector signal).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalActivity {
    pub rise: u64,
    pub fall: u64,
}

impl SignalActivity {
    pub fn toggles(&self) -> u64 {
        self.rise + self.fall
    }
}

/// Switching activity read from a VCD file.
#[derive(Clone, Debug, Default)]
pub struct VcdActivity {
    /// Keyed by leaf signal name (`clk`, `_000_`) or per-bit vector name (`req_msg[3]`).
    pub signals: HashMap<String, SignalActivity>,
    /// Dumped window in seconds, from the first to the last timestamp.
    pub duration_s: f64,
    /// Seconds per timescale tick.
    pub timescale_s: f64,
}

/// `$timescale 1ns $end` / `$timescale 10 ps $end` / `$timescale 1ns/1ps $end`.
/// Returns seconds per tick; the `/precision` part is ignored.
fn parse_timescale(text: &str) -> Result<f64> {
    let Some(body) = text
        .split("$timescale")
        .nth(1)
        .and_then(|s| s.split("$end").next())
    else {
        bail!("VCD has no $timescale directive");
    };
    let spec = body.split('/').next().unwrap_or("").trim();
    let split = spec
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(spec.len());
    let (num, unit) = spec.split_at(split);
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "s" => 1.0,
        "ms" => 1e-3,
        "us" => 1e-6,
        "ns" => 1e-9,
        "ps" => 1e-12,
        "fs" => 1e-15,
        _ => bail!("unsupported VCD timescale unit `{unit}`"),
    };
    let ticks: f64 = num
        .trim()
        .parse()
        .with_context(|| format!("bad VCD timescale `{spec}`"))?;
    Ok(ticks * scale)
}

/// A declared signal: identifier code plus bit layout for vectors.
struct SignalDecl {
    name: String,
    /// (left, right) index bounds for vectors; `None` for scalars.
    range: Option<(i64, i64)>,
    nbits: usize,
}

/// Parses VCD text into per-signal edge counts.
pub fn parse_vcd(text: &str) -> Result<VcdActivity> {
    let timescale_s = parse_timescale(text)?;
    let mut decls: HashMap<String, SignalDecl> = HashMap::new();
    // Per-identifier last definite state, one slot per bit.
    let mut last_state: HashMap<String, Vec<u8>> = HashMap::new();
    let mut signals: HashMap<String, SignalActivity> = HashMap::new();
    let mut first_ts: Option<u64> = None;
    let mut last_ts: u64 = 0;

    let mut in_defs = true;
    // Multi-line header items ($var, $comment, ...) accumulate until their $end.
    let mut pending = String::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if !pending.is_empty() {
            pending.push(' ');
            pending.push_str(line);
            if line.contains("$end") {
                let item = std::mem::take(&mut pending);
                handle_header_item(&item, &mut decls, &mut last_state)?;
            }
            continue;
        }
        if in_defs {
            if line.starts_with("$enddefinitions") {
                in_defs = false;
                continue;
            }
            if line.starts_with('$') {
                if line.contains("$end") {
                    handle_header_item(line, &mut decls, &mut last_state)?;
                } else {
                    pending.push_str(line);
                }
            }
            continue;
        }
        // Body: timestamps, value changes, and $-commands.
        if let Some(ts_str) = line.strip_prefix('#') {
            if let Ok(ts) = ts_str.trim().parse::<u64>() {
                if first_ts.is_none() {
                    first_ts = Some(ts);
                }
                last_ts = ts;
            }
            continue;
        }
        if line.starts_with('$') {
            continue; // $dumpvars/$dumpoff/$dumpon/$end/$finish
        }
        if let Some((v, id)) = parse_scalar_change(line) {
            apply_change(v, id, &decls, &mut last_state, &mut signals);
        } else if let Some((bits, id)) = parse_vector_change(line) {
            apply_vector_change(bits, id, &decls, &mut last_state, &mut signals);
        }
        // Anything else (r<real>, malformed) is ignored.
    }

    let duration_s = match first_ts {
        Some(f) => (last_ts.saturating_sub(f)) as f64 * timescale_s,
        None => 0.0,
    };
    Ok(VcdActivity {
        signals,
        duration_s,
        timescale_s,
    })
}

/// Handles one complete header item (`$end` already included in `item`).
fn handle_header_item(
    item: &str,
    decls: &mut HashMap<String, SignalDecl>,
    last_state: &mut HashMap<String, Vec<u8>>,
) -> Result<()> {
    let item = item.trim();
    if item.starts_with("$var") {
        parse_var_decl(item, decls, last_state)
    } else {
        Ok(()) // $scope/$upscope/$comment/$date/$version/$timescale: not needed per-signal
    }
}

/// `$var <type> <size> <id> <name> [<msb> <lsb>] $end`
fn parse_var_decl(
    item: &str,
    decls: &mut HashMap<String, SignalDecl>,
    last_state: &mut HashMap<String, Vec<u8>>,
) -> Result<()> {
    let body = item
        .trim_start_matches("$var")
        .trim_end_matches("$end")
        .trim();
    // First three fields: type, size, id — none can contain spaces.
    let bytes = body.as_bytes();
    let mut pos = 0usize;
    let mut fields = [""; 3];
    for f in &mut fields {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        *f = &body[start..pos];
    }
    if fields.iter().any(|f| f.is_empty()) {
        bail!("malformed $var declaration `{item}`");
    }
    let id = fields[2];
    let rest = body[pos..].trim();
    // rest = `<name>` or `<name> [<msb>:<lsb>]` / `<name> <msb> <lsb>`.
    // An escaped identifier (`\...`) runs to the first whitespace, so brackets
    // inside it are literal name characters (`\dpath.a_lt_b$in1[9]` is a scalar,
    // not a range); only tokens after that whitespace can be a range.
    let (name_part, range) = if rest.starts_with('\\') {
        match rest.find(char::is_whitespace) {
            Some(ws) => {
                let (nm, tail) = rest.split_at(ws);
                (nm, split_name_range(tail.trim()).1)
            }
            None => (rest, None),
        }
    } else {
        split_name_range(rest)
    };
    let name = unescape_vcd_name(name_part.trim_end());
    if name.is_empty() {
        bail!("VCD $var has empty name in `{item}`");
    }
    let nbits = match range {
        Some((a, b)) => (a - b).unsigned_abs() as usize + 1,
        None => 1,
    };
    decls.insert(id.to_string(), SignalDecl { name, range, nbits });
    last_state.insert(id.to_string(), vec![2u8; nbits]);
    Ok(())
}

/// Splits `name [msb:lsb]` / `name msb lsb` / `name` into (name, range).
fn split_name_range(s: &str) -> (&str, Option<(i64, i64)>) {
    let t = s.trim_end();
    // A range, when present, is the last two integer tokens (with `:` or `[`/`]`
    // separators). Scan integer tokens from the end.
    let mut nums: Vec<(usize, usize, i64)> = Vec::new(); // (start, end, value)
    let bytes = t.as_bytes();
    let mut i = bytes.len();
    while i > 0 && nums.len() < 2 {
        // skip separators: whitespace, ':', '[', ']'
        while i > 0
            && (bytes[i - 1].is_ascii_whitespace()
                || bytes[i - 1] == b':'
                || bytes[i - 1] == b'['
                || bytes[i - 1] == b']')
        {
            i -= 1;
        }
        let mut j = i;
        while j > 0 && (bytes[j - 1].is_ascii_digit() || bytes[j - 1] == b'-') {
            j -= 1;
        }
        if j == i {
            break; // no more integer tokens
        }
        if let Ok(v) = t[j..i].parse::<i64>() {
            nums.push((j, i, v));
        }
        i = j;
    }
    if nums.len() == 2 {
        // nums[0] = last token (lsb), nums[1] = msb
        let (msb_start, _msb_end, msb) = nums[1];
        let (_lsb_start, _lsb_end, lsb) = nums[0];
        // name ends before the whitespace/'['/':' preceding the msb token
        let mut ne = msb_start;
        while ne > 0
            && (bytes[ne - 1].is_ascii_whitespace()
                || bytes[ne - 1] == b'['
                || bytes[ne - 1] == b':')
        {
            ne -= 1;
        }
        return (&t[..ne], Some((msb, lsb)));
    }
    (t, None)
}

/// VCD escaped identifiers start with `\` and run to whitespace.
fn unescape_vcd_name(name: &str) -> String {
    name.strip_prefix('\\').unwrap_or(name).to_string()
}

/// Scalar change line: `<0|1|x|z|X|Z><id>` with no space before the id.
fn parse_scalar_change(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let v = chars.next()?;
    if !matches!(v, '0' | '1' | 'x' | 'X' | 'z' | 'Z') {
        return None;
    }
    let id = chars.as_str();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some((v, id))
}

/// Vector change line: `b<bits> <id>` / `B<bits> <id>`.
fn parse_vector_change(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix(['b', 'B'])?;
    let sp = rest.find(char::is_whitespace)?;
    let (bits, id) = rest.split_at(sp);
    let id = id.trim();
    if id.is_empty() || bits.is_empty() {
        return None;
    }
    Some((bits, id))
}

fn bit_state(c: u8) -> u8 {
    match c {
        b'0' => 0,
        b'1' => 1,
        _ => 2, // x, X, z, Z, ?
    }
}

fn record_edge(name: &str, old: u8, new: u8, signals: &mut HashMap<String, SignalActivity>) {
    if old == new || old == 2 || new == 2 {
        return;
    }
    let e = signals.entry(name.to_string()).or_default();
    if old == 0 && new == 1 {
        e.rise += 1;
    } else {
        e.fall += 1;
    }
}

fn apply_change(
    v: char,
    id: &str,
    decls: &HashMap<String, SignalDecl>,
    last_state: &mut HashMap<String, Vec<u8>>,
    signals: &mut HashMap<String, SignalActivity>,
) {
    let Some(decl) = decls.get(id) else {
        return;
    };
    if decl.range.is_some() {
        return; // scalar write to a vector id is malformed
    }
    let states = last_state.entry(id.to_string()).or_default();
    if states.is_empty() {
        states.push(2);
    }
    let old = states[0];
    states[0] = bit_state(v as u8);
    record_edge(&decl.name.clone(), old, states[0], signals);
}

fn apply_vector_change(
    bits: &str,
    id: &str,
    decls: &HashMap<String, SignalDecl>,
    last_state: &mut HashMap<String, Vec<u8>>,
    signals: &mut HashMap<String, SignalActivity>,
) {
    let Some(decl) = decls.get(id) else {
        return;
    };
    let Some((left, right)) = decl.range else {
        return; // vector write to a scalar id is malformed
    };
    let chars: Vec<u8> = bits.bytes().collect();
    if chars.len() != decl.nbits {
        return;
    }
    let states = last_state.entry(id.to_string()).or_default();
    if states.len() != decl.nbits {
        states.resize(decl.nbits, 2);
    }
    for (pos, &ch) in chars.iter().enumerate() {
        // chars[0] is the leftmost = first index of the declared range; for an
        // ascending range (`[0:3]`) indices increase with pos, for descending
        // (`[3:0]`) they decrease.
        let bit_idx = if left >= right {
            left - pos as i64
        } else {
            left + pos as i64
        };
        let old = states[pos];
        let new = bit_state(ch);
        states[pos] = new;
        if old != new && old != 2 && new != 2 {
            record_edge(&format!("{}[{}]", decl.name, bit_idx), old, new, signals);
        }
    }
}

/// Loads and parses a VCD file.
pub fn load_vcd(path: &Path) -> Result<VcdActivity> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("Failed to read VCD {:?}", path))?;
    parse_vcd(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mini_vcd() {
        let vcd = r#"$date today $end
$version test $end
$timescale 1ns $end
$scope module tb $end
$var wire 1 ! clk $end
$var wire 1 " reset $end
$var wire 4 # data [3:0] $end
$upscope $end
$enddefinitions $end
#0
0!
1"
b0000 #
#10
1!
#20
0!
#30
b1010 #
#40
b0101 #
#50
$finish
"#;
        let act = parse_vcd(vcd).expect("parse");
        assert!((act.timescale_s - 1e-9).abs() < 1e-20);
        assert!((act.duration_s - 50e-9).abs() < 1e-20);

        let clk = act.signals.get("clk").expect("clk");
        assert_eq!((clk.rise, clk.fall), (1, 1)); // 0->1 @10, 1->0 @20

        // reset stays 1 the whole window -> zero counted edges -> absent entry
        assert_eq!(
            act.signals.get("reset").map(|s| s.toggles()).unwrap_or(0),
            0
        );

        // data[3:0]: 0000 -> 1010 -> 0101
        let d3 = act.signals.get("data[3]").expect("data[3]");
        assert_eq!((d3.rise, d3.fall), (1, 1));
        let d2 = act.signals.get("data[2]").expect("data[2]");
        assert_eq!((d2.rise, d2.fall), (1, 0));
        let d1 = act.signals.get("data[1]").expect("data[1]");
        assert_eq!((d1.rise, d1.fall), (1, 1));
        let d0 = act.signals.get("data[0]").expect("data[0]");
        assert_eq!((d0.rise, d0.fall), (1, 0));
    }

    #[test]
    fn test_x_z_do_not_count() {
        let vcd = "$timescale 1ns $end\n\
                   $var wire 1 ! a $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n#5\nx!\n#10\n1!\n#15\nz!\n#20\n0!\n";
        let act = parse_vcd(vcd).expect("parse");
        // 0->x, x->1, 1->z, z->0: none are clean 0<->1 edges, so the signal
        // never earns a map entry (absent == zero counted activity).
        assert_eq!(act.signals.get("a").map(|s| s.toggles()).unwrap_or(0), 0);
    }

    #[test]
    fn test_ascending_range() {
        // `[0:3]` lists index 0 leftmost: `b1010` raises bits 0 and 2, `b0101`
        // then falls those and raises bits 1 and 3.
        let vcd = "$timescale 1ns $end\n\
                   $var wire 4 # up [0:3] $end\n\
                   $enddefinitions $end\n\
                   #0\nb0000 #\n#10\nb1010 #\n#20\nb0101 #\n";
        let act = parse_vcd(vcd).expect("parse");
        for i in 0..4 {
            let s = act.signals.get(&format!("up[{i}]")).expect("bit");
            let want = if i % 2 == 0 { (1, 1) } else { (1, 0) };
            assert_eq!((s.rise, s.fall), want, "up[{i}]");
        }
    }

    #[test]
    fn test_escaped_name() {
        // `\dpath.a_lt_b$in1[9]` is a scalar whose name literally contains
        // brackets — the `[9]` must not be read as a vector range.
        let vcd = "$timescale 1ns $end\n\
                   $var wire 1 ! \\dpath.a_lt_b$in1[9] $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n#5\n1!\n";
        let act = parse_vcd(vcd).expect("parse");
        let s = act
            .signals
            .get("dpath.a_lt_b$in1[9]")
            .expect("escaped name");
        assert_eq!(s.rise, 1);
        assert!(!act.signals.contains_key("dpath.a_lt_b$in"));
    }

    #[test]
    fn test_ps_timescale() {
        let vcd = "$timescale 100 ps $end\n\
                   $var wire 1 ! a $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n#100\n1!\n";
        let act = parse_vcd(vcd).expect("parse");
        assert!((act.timescale_s - 1e-10).abs() < 1e-20);
        assert_eq!(act.duration_s, 100.0 * 1e-10);
        assert_eq!(act.signals.get("a").expect("a").rise, 1);
    }
}
