//! Value-change-dump (IEEE 1364) reader for switching-activity annotation.
//!
//! Produces per-signal rise/fall transition counts and per-state durations
//! over the dumped window. Vector signals are expanded to per-bit names
//! (`data[3]`) so they line up with the per-bit net names DEF uses. Only
//! clean 0<->1 edges count one toggle each; transitions involving x/z update
//! the last state and count in `xtrans` (OpenSTA weighs each such edge as
//! half a toggle). Durations charge similarly: time in 0/1 lands in
//! `t0_s`/`t1_s` and x/z/never-defined time lands in `other_s`, so
//! `t1/(t0+t1+other)` is the time-in-1 probability a `when` weight consumes. Every declared signal
//! earns a `signals` entry — a constant dumped net is annotated with zero
//! edges and a measured duty. `signals` keys stay flat leaf names for
//! compatibility; `pin_signals` additionally keys every declaration by its
//! `$scope` path (`tb/dut/u_t/Z`), which is the namespace `resolve_activity`
//! matches against design pins.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

/// Edge counts and state durations for one dumped signal (or one bit of a
/// vector signal).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SignalActivity {
    pub rise: u64,
    pub fall: u64,
    /// Edges where either endpoint is x/z. OpenSTA's activity model weighs
    /// each as half a toggle, so the effective transition count is
    /// `rise + fall + xtrans / 2`.
    pub xtrans: u64,
    /// Seconds in state 0 / state 1 within the dump window; x/z and
    /// never-defined time lands in `other_s`.
    pub t0_s: f64,
    pub t1_s: f64,
    /// Seconds spent in x/z or never defined. `t0_s + t1_s + other_s == 0`
    /// means the signal was never defined — the probability is unknown.
    pub other_s: f64,
}

impl SignalActivity {
    pub fn toggles(&self) -> u64 {
        self.rise + self.fall
    }

    /// Effective toggle count under OpenSTA's weighting (x/z edges count half).
    pub fn eff_toggles(&self) -> f64 {
        self.rise as f64 + self.fall as f64 + self.xtrans as f64 * 0.5
    }

    /// Time-in-1 probability over the whole annotated window, `None` when the
    /// signal was never defined (`t1/(t0+t1+other)`).
    pub fn prob_one(&self) -> Option<f64> {
        let total = self.t0_s + self.t1_s + self.other_s;
        (total > 0.0).then(|| self.t1_s / total)
    }
}

/// Switching activity read from a VCD file.
#[derive(Clone, Debug, Default)]
pub struct VcdActivity {
    /// Keyed by leaf signal name (`clk`, `_000_`) or per-bit vector name (`req_msg[3]`).
    /// Compat/debug view — `resolve_activity` consumes `pin_signals`.
    pub signals: HashMap<String, SignalActivity>,
    /// Per-pin annotation candidates, keyed by the declaration's `$scope`
    /// chain joined with `/` plus the leaf name (`tb/dut/_d_/Q`,
    /// `tb/dut/n_and`). Every declaration yields an entry carrying its
    /// own stream; declarations sharing an identifier code (Icarus
    /// reuses codes in sibling scopes) get an alias entry copying the
    /// retained shallowest stream's activity. Per-declaration, not the
    /// `signals` flat-name view — same-named declarations (`Y` on every
    /// dumped cell) must not fold into each other.
    pub pin_signals: HashMap<String, SignalActivity>,
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
#[derive(Clone)]
struct SignalDecl {
    name: String,
    /// `$scope` chain at declaration joined with `/` plus the leaf name
    /// (`tb/dut/_d_/Q`) — the hierarchical key `pin_signals` emits.
    path: String,
    /// (left, right) index bounds for vectors; `None` for scalars.
    range: Option<(i64, i64)>,
    nbits: usize,
    /// `$scope` nesting depth at declaration; Icarus reuses identifier codes in
    /// sibling scopes, so the shallowest declaration names the shared stream.
    depth: usize,
}

/// Per-identifier waveform state: last value per bit, the timestamp it last
/// changed, ticks accumulated per state (0 / 1 / x|z|never-seen), and per-bit
/// edge counts. `when` power weights need time-in-state, which edge counts
/// alone do not carry. `last` uses 3 for "no value written yet" so a first
/// write counts as initialization, not a half-weighted x/z edge. `sa` is the
/// declaration's own stream — `pin_signals` reads it: `signals` folds
/// same-named declarations into one flat entry, so a pin's count there is
/// the sum of every like-named declaration (gcd.vcd's per-cell `Y`/`X`/`Q`
/// scopes), not the pin's.
#[derive(Default)]
struct IdActivity {
    last: Vec<u8>,
    since: Vec<u64>,
    dur: Vec<[u64; 3]>,
    sa: Vec<SignalActivity>,
}

impl IdActivity {
    fn with_bits(nbits: usize) -> Self {
        Self {
            last: vec![3; nbits],
            since: vec![0; nbits],
            dur: vec![[0; 3]; nbits],
            sa: vec![SignalActivity::default(); nbits],
        }
    }
}

/// Header-parse state shared by `$var`/`$scope`/`$upscope` items.
struct VcdHeaders {
    decls: HashMap<String, SignalDecl>,
    /// Every declaration in file order, including id-sharing ones shadowed
    /// out of `decls` — `pin_signals` emits an alias entry for each.
    decl_list: Vec<(String, SignalDecl)>,
    /// Per-identifier waveform state, one slot per bit.
    activity: HashMap<String, IdActivity>,
    /// `$scope` name chain — `pin_signals` path prefix.
    scopes: Vec<String>,
    scope_depth: usize,
}

/// Parses VCD text into per-signal edge counts.
pub fn parse_vcd(text: &str) -> Result<VcdActivity> {
    let timescale_s = parse_timescale(text)?;
    let mut headers = VcdHeaders {
        decls: HashMap::new(),
        decl_list: Vec::new(),
        activity: HashMap::new(),
        scopes: Vec::new(),
        scope_depth: 0,
    };
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
                handle_header_item(&item, &mut headers)?;
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
                    handle_header_item(line, &mut headers)?;
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
            apply_change(
                v,
                id,
                &headers.decls,
                &mut headers.activity,
                &mut signals,
                last_ts,
            );
        } else if let Some((bits, id)) = parse_vector_change(line) {
            apply_vector_change(
                bits,
                id,
                &headers.decls,
                &mut headers.activity,
                &mut signals,
                last_ts,
            );
        }
        // Anything else (r<real>, malformed) is ignored.
    }

    // Close every declared signal's last open state interval and fold
    // durations into `signals` — including edge-free signals, so a dumped
    // constant net reads as annotated with a measured duty. The same
    // durations land on the declaration's own `sa` stream.
    for (id, decl) in &headers.decls {
        let Some(act) = headers.activity.get_mut(id) else {
            continue;
        };
        for (pos, (&state, &since)) in act.last.iter().zip(&act.since).enumerate() {
            let mut dur = act.dur[pos];
            dur[(state.min(2)) as usize] += last_ts.saturating_sub(since);
            let name = match decl.range {
                Some((left, right)) => {
                    let bit_idx = if left >= right {
                        left - pos as i64
                    } else {
                        left + pos as i64
                    };
                    format!("{}[{}]", decl.name, bit_idx)
                }
                None => decl.name.clone(),
            };
            act.sa[pos].t0_s = dur[0] as f64 * timescale_s;
            act.sa[pos].t1_s = dur[1] as f64 * timescale_s;
            act.sa[pos].other_s = dur[2] as f64 * timescale_s;
            let e = signals.entry(name).or_default();
            e.t0_s = dur[0] as f64 * timescale_s;
            e.t1_s = dur[1] as f64 * timescale_s;
            e.other_s = dur[2] as f64 * timescale_s;
        }
    }

    // `pin_signals`: one entry per declaration, keyed by its scope path;
    // id-sharing declarations alias the retained shallowest stream
    // bit-for-bit (same pos maps the retained decl's own bit expansion).
    let mut pin_signals: HashMap<String, SignalActivity> = HashMap::new();
    for (id, decl) in &headers.decl_list {
        let Some(retained) = headers.decls.get(id) else {
            continue;
        };
        let Some(act) = headers.activity.get(id) else {
            continue;
        };
        for (pos, &sa) in act
            .sa
            .iter()
            .enumerate()
            .take(decl.nbits.min(retained.nbits))
        {
            let pin_name = match decl.range {
                Some((left, right)) => {
                    let bit_idx = if left >= right {
                        left - pos as i64
                    } else {
                        left + pos as i64
                    };
                    format!("{}[{}]", decl.path, bit_idx)
                }
                None => decl.path.clone(),
            };
            pin_signals.insert(pin_name, sa);
        }
    }

    let duration_s = match first_ts {
        Some(f) => (last_ts.saturating_sub(f)) as f64 * timescale_s,
        None => 0.0,
    };
    Ok(VcdActivity {
        signals,
        pin_signals,
        duration_s,
        timescale_s,
    })
}

/// Handles one complete header item (`$end` already included in `item`).
fn handle_header_item(item: &str, headers: &mut VcdHeaders) -> Result<()> {
    let item = item.trim();
    if item.starts_with("$scope") {
        headers.scope_depth += 1;
        // `$scope <type> <name> $end` — the name is the third field.
        let name = item
            .trim_start_matches("$scope")
            .trim_end_matches("$end")
            .split_whitespace()
            .nth(1)
            .unwrap_or("");
        headers.scopes.push(name.to_string());
        Ok(())
    } else if item.starts_with("$upscope") {
        headers.scope_depth = headers.scope_depth.saturating_sub(1);
        headers.scopes.pop();
        Ok(())
    } else if item.starts_with("$var") {
        parse_var_decl(item, headers)
    } else {
        Ok(()) // $comment/$date/$version/$timescale: not needed per-signal
    }
}

/// `$var <type> <size> <id> <name> [<msb> <lsb>] $end`
fn parse_var_decl(item: &str, headers: &mut VcdHeaders) -> Result<()> {
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
    let id_key = id.to_string();
    let path = if headers.scopes.is_empty() {
        name.clone()
    } else {
        format!("{}/{}", headers.scopes.join("/"), name)
    };
    let decl = SignalDecl {
        name,
        path,
        range,
        nbits,
        depth: headers.scope_depth,
    };
    // Icarus reuses identifier codes across sibling scopes; value changes cannot
    // distinguish them, so one stream feeds whichever declaration we keep. The
    // shallowest scope is the design-level net name (`clk`, not a cell's `CLK`).
    match headers.decls.get(&id_key) {
        Some(existing) if existing.depth <= decl.depth => {}
        _ => {
            headers.decls.insert(id_key.clone(), decl.clone());
            headers
                .activity
                .insert(id_key.clone(), IdActivity::with_bits(nbits));
        }
    }
    headers.decl_list.push((id_key, decl));
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
    if old == new || old == 3 {
        return;
    }
    let e = signals.entry(name.to_string()).or_default();
    if old == 2 || new == 2 {
        e.xtrans += 1;
    } else if old == 0 && new == 1 {
        e.rise += 1;
    } else {
        e.fall += 1;
    }
}

/// Same transition accounting as `record_edge`, onto the declaration's own
/// per-bit stream (`IdActivity::sa`) instead of the flat-name `signals` map.
fn record_id_edge(act: &mut IdActivity, pos: usize, old: u8, new: u8) {
    if old == new || old == 3 {
        return;
    }
    let e = &mut act.sa[pos];
    if old == 2 || new == 2 {
        e.xtrans += 1;
    } else if old == 0 && new == 1 {
        e.rise += 1;
    } else {
        e.fall += 1;
    }
}

fn apply_change(
    v: char,
    id: &str,
    decls: &HashMap<String, SignalDecl>,
    activity: &mut HashMap<String, IdActivity>,
    signals: &mut HashMap<String, SignalActivity>,
    cur_ts: u64,
) {
    let Some(decl) = decls.get(id) else {
        return;
    };
    if decl.range.is_some() {
        return; // scalar write to a vector id is malformed
    }
    let act = activity.entry(id.to_string()).or_default();
    if act.last.is_empty() {
        *act = IdActivity::with_bits(1);
    }
    let old = act.last[0];
    act.dur[0][old.min(2) as usize] += cur_ts.saturating_sub(act.since[0]);
    act.since[0] = cur_ts;
    act.last[0] = bit_state(v as u8);
    record_id_edge(act, 0, old, act.last[0]);
    record_edge(&decl.name.clone(), old, act.last[0], signals);
}

fn apply_vector_change(
    bits: &str,
    id: &str,
    decls: &HashMap<String, SignalDecl>,
    activity: &mut HashMap<String, IdActivity>,
    signals: &mut HashMap<String, SignalActivity>,
    cur_ts: u64,
) {
    let Some(decl) = decls.get(id) else {
        return;
    };
    let Some((left, right)) = decl.range else {
        return; // vector write to a scalar id is malformed
    };
    // A `b` value may print shorter than the declared width: the missing
    // high bits are the value's sign convention — x/z extend themselves
    // (`bx` is the all-x write), 0/1 left-extend with 0 (`b0`, and
    // leading-zero trims like `b100011111011010` on a 16-bit net).
    // Dropping them silently loses the transition AND stalls the state
    // (gcd.vcd's `resp_msg` writes `b0`/`bx` on idle).
    let chars: Vec<u8> = if bits.len() < decl.nbits {
        let pad = match bits.as_bytes()[0] {
            b'x' | b'X' => b'x',
            b'z' | b'Z' => b'z',
            _ => b'0',
        };
        let mut v = Vec::with_capacity(decl.nbits);
        v.extend(std::iter::repeat_n(pad, decl.nbits - bits.len()));
        v.extend_from_slice(bits.as_bytes());
        v
    } else {
        bits.bytes().collect()
    };
    if chars.len() != decl.nbits {
        return;
    }
    let act = activity.entry(id.to_string()).or_default();
    if act.last.len() != decl.nbits {
        *act = IdActivity::with_bits(decl.nbits);
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
        let old = act.last[pos];
        let new = bit_state(ch);
        act.dur[pos][old.min(2) as usize] += cur_ts.saturating_sub(act.since[pos]);
        act.since[pos] = cur_ts;
        act.last[pos] = new;
        if old != new {
            record_id_edge(act, pos, old, new);
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
        // clk: 0 over [0,10) and [20,50] -> 40 ns low, 10 ns high.
        assert!((clk.t0_s - 40e-9).abs() < 1e-20);
        assert!((clk.t1_s - 10e-9).abs() < 1e-20);

        // reset stays 1 the whole window -> zero counted edges but a real
        // duty: declared signals always earn a `signals` entry.
        let reset = act.signals.get("reset").expect("reset");
        assert_eq!(reset.toggles(), 0);
        assert_eq!(reset.t0_s, 0.0);
        assert!((reset.t1_s - 50e-9).abs() < 1e-20);

        // data[3:0]: 0000 -> 1010 -> 0101
        let d3 = act.signals.get("data[3]").expect("data[3]");
        assert_eq!((d3.rise, d3.fall), (1, 1));
        let d2 = act.signals.get("data[2]").expect("data[2]");
        assert_eq!((d2.rise, d2.fall), (1, 0));
        let d1 = act.signals.get("data[1]").expect("data[1]");
        assert_eq!((d1.rise, d1.fall), (1, 1));
        let d0 = act.signals.get("data[0]").expect("data[0]");
        assert_eq!((d0.rise, d0.fall), (1, 0));
        // Per-bit durations: data[3] toggles 0->1 @30 ->0 @40 (t0=40, t1=10);
        // data[0] rises @40 and stays (t0=40, t1=10).
        assert!((d3.t0_s - 40e-9).abs() < 1e-20 && (d3.t1_s - 10e-9).abs() < 1e-20);
        assert!((d0.t0_s - 40e-9).abs() < 1e-20 && (d0.t1_s - 10e-9).abs() < 1e-20);
    }

    #[test]
    fn test_state_durations() {
        // 0 for [0,10), 1 for [10,30), x for [30,40), 0 for [40,60]:
        // x time enters neither bucket; a trailing constant run closes at
        // the last timestamp.
        let vcd = "$timescale 1ns $end\n\
                   $var wire 1 ! a $end\n\
                   $var wire 1 \" b $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n1\"\n#10\n1!\n#30\nx!\n#40\n0!\n#60\n";
        let act = parse_vcd(vcd).expect("parse");
        let a = act.signals.get("a").expect("a");
        // The 1->0 return passes through x: only the rise is clean; the two
        // x-involved edges land in `xtrans` (OpenSTA counts each as 0.5).
        assert_eq!((a.rise, a.fall, a.xtrans), (1, 0, 2));
        assert!((a.t0_s - 30e-9).abs() < 1e-20); // [0,10) + [40,60]
        assert!((a.t1_s - 20e-9).abs() < 1e-20); // [10,30)
        assert!((a.other_s - 10e-9).abs() < 1e-20); // x over [30,40)
        // Constant-1 signal: no edges, full-window t1.
        let b = act.signals.get("b").expect("b");
        assert_eq!(b.toggles(), 0);
        assert_eq!(b.t0_s, 0.0);
        assert!((b.t1_s - 60e-9).abs() < 1e-20);
    }

    #[test]
    fn test_x_z_do_not_count() {
        let vcd = "$timescale 1ns $end\n\
                   $var wire 1 ! a $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n#5\nx!\n#10\n1!\n#15\nz!\n#20\n0!\n";
        let act = parse_vcd(vcd).expect("parse");
        // 0->x, x->1, 1->z, z->0: none are clean 0<->1 edges; all four are
        // x/z-involved, so `xtrans` counts them at half weight each.
        let a = act.signals.get("a").expect("a");
        assert_eq!(a.toggles(), 0);
        assert_eq!(a.xtrans, 4);
        assert_eq!(a.eff_toggles(), 2.0);
        // x time [5,10) + z time [15,20) is not 0/1 state time.
        assert!((a.other_s - 10e-9).abs() < 1e-20);
        assert!((a.t0_s - 5e-9).abs() < 1e-20);
        assert!((a.t1_s - 5e-9).abs() < 1e-20);
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
    fn test_scope_reused_identifier() {
        // Icarus reuses identifier codes in sibling scopes: `!` names the top-level
        // `clk` and every cell's `CLK` pin. Value changes cannot tell them apart;
        // the shallowest declaration supplies the name.
        let vcd = "$timescale 1ns $end\n\
                   $scope module dut $end\n\
                   $var wire 1 ! clk $end\n\
                   $upscope $end\n\
                   $scope module _1_ $end\n\
                   $var wire 1 ! CLK $end\n\
                   $upscope $end\n\
                   $scope module _2_ $end\n\
                   $var wire 1 ! CLK $end\n\
                   $upscope $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n#5\n1!\n#10\n0!\n";
        let act = parse_vcd(vcd).expect("parse");
        let s = act.signals.get("clk").expect("clk");
        assert_eq!((s.rise, s.fall), (1, 1));
        assert!(!act.signals.contains_key("CLK"));
        // Every declaration earns a `pin_signals` entry keyed by scope path;
        // the shadowed `CLK` decls alias the retained shallowest stream.
        for path in ["dut/clk", "_1_/CLK", "_2_/CLK"] {
            let s = act.pin_signals.get(path).unwrap_or_else(|| panic!("{path}"));
            assert_eq!((s.rise, s.fall), (1, 1), "{path}");
        }
    }

    #[test]
    fn test_pin_signals_scoped_decl() {
        // A scoped instance-pin dump (`tb/dut/u_t/Z`) keys `pin_signals` by
        // the full path while `signals` keeps the flat leaf.
        let vcd = "$timescale 1ns $end\n\
                   $scope module tb $end\n\
                   $scope module dut $end\n\
                   $var wire 1 ! a $end\n\
                   $scope module u_t $end\n\
                   $var wire 1 \" Z $end\n\
                   $upscope $end\n\
                   $upscope $end\n\
                   $upscope $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n1\"\n#10\n1!\n0\"\n#20\n";
        let act = parse_vcd(vcd).expect("parse");
        let s = act.pin_signals.get("tb/dut/u_t/Z").expect("scoped pin");
        assert_eq!((s.rise, s.fall), (0, 1));
        assert_eq!(act.pin_signals.get("tb/dut/a").expect("port").rise, 1);
        assert_eq!(act.signals["Z"], act.pin_signals["tb/dut/u_t/Z"]);
    }

    #[test]
    fn test_pin_signals_keep_same_leaf_apart() {
        // Per-cell pin dumps declare the same leaf (`Y`) under different ids
        // in different scopes — `pin_signals` must read each declaration's own
        // stream, not the `signals` flat-name fold of every like-named pin
        // (gcd.vcd, where `signals["Y"]` summed all cell outputs).
        let vcd = "$timescale 1ns $end\n\
                   $scope module dut $end\n\
                   $scope module _1_ $end\n\
                   $var wire 1 ! Y $end\n\
                   $upscope $end\n\
                   $scope module _2_ $end\n\
                   $var wire 1 \" Y $end\n\
                   $upscope $end\n\
                   $upscope $end\n\
                   $enddefinitions $end\n\
                   #0\n0!\n0\"\n#10\n1!\n#20\n0!\n1\"\n#30\n";
        let act = parse_vcd(vcd).expect("parse");
        let y1 = act.pin_signals.get("dut/_1_/Y").expect("_1_/Y");
        let y2 = act.pin_signals.get("dut/_2_/Y").expect("_2_/Y");
        assert_eq!((y1.rise, y1.fall), (1, 1));
        assert_eq!((y2.rise, y2.fall), (1, 0));
        // The flat fold carries the sum — debug view only, not an annotation.
        assert_eq!((act.signals["Y"].rise, act.signals["Y"].fall), (2, 1));
    }

    #[test]
    fn test_short_vector_writes_left_extend() {
        // `b` values narrower than the declared width pad on the left: 0/1
        // writes extend with 0 (`b0`, `b111` on [3:0]), x/z extend
        // themselves (`bx` is the all-x write). Dropping them loses the edge
        // and stalls the state.
        let vcd = "$timescale 1ns $end\n\
                   $var wire 4 # data [3:0] $end\n\
                   $enddefinitions $end\n\
                   #0\nb0000 #\n#10\nb111 #\n#20\nb0 #\n#30\nbx #\n#40\n";
        let act = parse_vcd(vcd).expect("parse");
        for i in 0..3 {
            let s = act.signals.get(&format!("data[{i}]")).expect("bit");
            assert_eq!((s.rise, s.fall, s.xtrans), (1, 1, 1), "data[{i}]");
        }
        // bit3 never toggles (0 pad on `b111`), then x on `bx`.
        let b3 = act.signals.get("data[3]").expect("data[3]");
        assert_eq!((b3.rise, b3.fall, b3.xtrans), (0, 0, 1));
        assert!((b3.other_s - 10e-9).abs() < 1e-20);
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
