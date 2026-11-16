use crate::interner::{LibPinId, PinDir, SymbolId};
use crate::lib_db::{
    LibCell, LibDb, LibPin, LibTimingArc, NldmTable, TableAxisVar, TimingSense, TimingType,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use xxhash_rust::xxh3::xxh3_64;

const CACHE_MAGIC: [u8; 8] = *b"GRUSLIB2";
// Bump when the serialized LibDb layout changes (e.g. new TimingType variants shift
// discriminant values and old caches would deserialize `Other` arcs as real types).
const CACHE_VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    magic: [u8; 8],
    version: u32,
    source_size: u64,
    source_xxh3: u64,
    db: LibDb,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Token<'a> {
    Ident(&'a str),
    Str(&'a str),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Colon,
    Semi,
    Comma,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                self.pos += 1;
            } else if b == b'\\' && self.pos + 1 < self.src.len() && (self.src[self.pos + 1] == b'\n' || self.src[self.pos + 1] == b'\r') {
                self.pos += 2;
                if self.pos < self.src.len() && self.src[self.pos] == b'\n' {
                    self.pos += 1;
                }
            } else if b == b'/' && self.pos + 1 < self.src.len() && self.src[self.pos + 1] == b'/' {
                self.pos += 2;
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else if b == b'/' && self.pos + 1 < self.src.len() && self.src[self.pos + 1] == b'*' {
                self.pos += 2;
                while self.pos + 1 < self.src.len() && !(self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/') {
                    self.pos += 1;
                }
                if self.pos + 1 < self.src.len() {
                    self.pos += 2;
                } else {
                    // Unterminated comment: consume to EOF so its last byte is
                    // not emitted as an identifier token.
                    self.pos = self.src.len();
                }
            } else {
                break;
            }
        }
    }

    pub(crate) fn next_token(&mut self) -> Option<Token<'a>> {
        self.skip_whitespace_and_comments();
        if self.pos >= self.src.len() {
            return None;
        }

        let b = self.src[self.pos];
        match b {
            b'(' => {
                self.pos += 1;
                Some(Token::LParen)
            }
            b')' => {
                self.pos += 1;
                Some(Token::RParen)
            }
            b'{' => {
                self.pos += 1;
                Some(Token::LBrace)
            }
            b'}' => {
                self.pos += 1;
                Some(Token::RBrace)
            }
            b':' => {
                self.pos += 1;
                Some(Token::Colon)
            }
            b';' => {
                self.pos += 1;
                Some(Token::Semi)
            }
            b',' => {
                self.pos += 1;
                Some(Token::Comma)
            }
            b'"' => {
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.src.len() {
                    if self.src[self.pos] == b'\\' && self.pos + 1 < self.src.len() {
                        self.pos += 2;
                    } else if self.src[self.pos] == b'"' {
                        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
                        self.pos += 1;
                        return Some(Token::Str(text));
                    } else {
                        self.pos += 1;
                    }
                }
                let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
                Some(Token::Str(text))
            }
            _ => {
                let start = self.pos;
                while self.pos < self.src.len() {
                    let c = self.src[self.pos];
                    if c.is_ascii_whitespace()
                        || c == b'('
                        || c == b')'
                        || c == b'{'
                        || c == b'}'
                        || c == b':'
                        || c == b';'
                        || c == b','
                        || c == b'"'
                        || (c == b'/' && self.pos + 1 < self.src.len() && (self.src[self.pos + 1] == b'/' || self.src[self.pos + 1] == b'*'))
                        || (c == b'\\' && self.pos + 1 < self.src.len() && (self.src[self.pos + 1] == b'\n' || self.src[self.pos + 1] == b'\r'))
                    {
                        break;
                    }
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
                Some(Token::Ident(text))
            }
        }
    }

    pub fn skip_balanced_braces(&mut self) {
        let mut depth = 1;
        while let Some(tok) = self.next_token() {
            match tok {
                Token::LBrace => depth += 1,
                Token::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Default, Clone, Debug)]
struct TemplateDef {
    var1: TableAxisVar,
    var2: TableAxisVar,
    index_1: Vec<f32>,
    index_2: Vec<f32>,
}

fn parse_axis_var(s: &str) -> TableAxisVar {
    let clean = s.trim_matches('"').trim();
    match clean {
        "input_net_transition" => TableAxisVar::InputNetTransition,
        "total_output_net_capacitance" => TableAxisVar::TotalOutputNetCapacitance,
        "constrained_pin_transition" => TableAxisVar::ConstrainedPinTransition,
        "related_pin_transition" => TableAxisVar::RelatedPinTransition,
        _ => TableAxisVar::Other,
    }
}

fn parse_timing_sense(s: &str) -> TimingSense {
    let clean = s.trim_matches('"').trim();
    match clean {
        "positive_unate" => TimingSense::PositiveUnate,
        "negative_unate" => TimingSense::NegativeUnate,
        "non_unate" => TimingSense::NonUnate,
        _ => TimingSense::NonUnate,
    }
}

fn parse_timing_type(s: &str) -> TimingType {
    let clean = s.trim_matches('"').trim();
    match clean {
        "combinational" => TimingType::Combinational,
        "combinational_rise" => TimingType::CombinationalRise,
        "combinational_fall" => TimingType::CombinationalFall,
        "rising_edge" => TimingType::RisingEdge,
        "falling_edge" => TimingType::FallingEdge,
        "setup_rising" => TimingType::SetupRising,
        "hold_rising" => TimingType::HoldRising,
        "setup_falling" => TimingType::SetupFalling,
        "hold_falling" => TimingType::HoldFalling,
        "preset" => TimingType::Preset,
        "clear" => TimingType::Clear,
        "recovery_rising" => TimingType::RecoveryRising,
        "removal_rising" => TimingType::RemovalRising,
        "recovery_falling" => TimingType::RecoveryFalling,
        "removal_falling" => TimingType::RemovalFalling,
        _ => TimingType::Other,
    }
}

fn parse_pin_direction(s: &str) -> Result<PinDir> {
    let clean = s.trim_matches('"').trim();
    match clean {
        "input" => Ok(PinDir::Input),
        "output" => Ok(PinDir::Output),
        "inout" => Ok(PinDir::Inout),
        "internal" => Ok(PinDir::Internal),
        _ => bail!("liberty pin direction `{clean}` is not input, output, inout or internal"),
    }
}

/// A numeric attribute value. A value that does not parse is a malformed library, not zero.
fn number_value(s: &str, attribute: &str) -> Result<f32> {
    s.trim()
        .parse::<f32>()
        .with_context(|| format!("liberty {attribute} value `{s}` is not a number"))
}

/// `time_unit` such as `1ns` or `100ps`, converted to ns. A bare number is taken as ns.
fn parse_time_unit(s: &str) -> Result<f32> {
    let clean = s.trim_matches('"').trim();
    let (num_str, scale) = if let Some(num_str) = clean.strip_suffix("ps") {
        (num_str, 0.001)
    } else if let Some(num_str) = clean.strip_suffix("ns") {
        (num_str, 1.0)
    } else if let Some(num_str) = clean.strip_suffix("us") {
        (num_str, 1000.0)
    } else {
        (clean, 1.0)
    };
    Ok(number_value(num_str, "time_unit")? * scale)
}

fn parse_float_list(s: &str, scale: f32, out: &mut Vec<f32>) -> Result<()> {
    for part in s.split(',') {
        // A backslash line continuation inside a quoted list is layout, not part of a value.
        let trimmed = part.trim_matches(|c: char| c.is_whitespace() || c == '\\');
        if !trimmed.is_empty() {
            out.push(number_value(trimmed, "table index or value")? * scale);
        }
    }
    Ok(())
}

/// Parses a Liberty (`.lib`) formatted byte slice into a `LibDb`.
pub fn parse_liberty_bytes(bytes: &[u8]) -> Result<LibDb> {
    let mut lexer = Lexer::new(bytes);
    let mut db = LibDb::new();
    db.time_unit_ns = 1.0;
    db.cap_unit_pf = 1.0;

    let mut templates: HashMap<String, TemplateDef> = HashMap::new();

    // Top-level must start with library ( <name> ) { ... }
    let mut saw_library = false;
    while let Some(tok) = lexer.next_token() {
        if !matches!(tok, Token::Ident("library")) || !matches!(lexer.next_token(), Some(Token::LParen)) {
            continue;
        }
        db.library_name = read_paren_header(&mut lexer).unwrap_or_else(|| "unnamed".to_string());
        if let Some(Token::LBrace) = lexer.next_token() {
            parse_library_body(&mut lexer, &mut db, &mut templates)?;
            saw_library = true;
        }
    }
    if !saw_library {
        bail!("Liberty input has no `library (name) {{ ... }}` body");
    }

    db.strings.rebuild_map();
    db.rebuild_cell_by_name();
    u16::try_from(db.cells.len()).context("Liberty cell count exceeds u16")?;

    Ok(db)
}

fn parse_library_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    templates: &mut HashMap<String, TemplateDef>,
) -> Result<()> {
    let mut closed = false;
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => {
                closed = true;
                break;
            }
            Token::Ident("time_unit") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Str(s) | Token::Ident(s)) = lexer.next_token() {
                        db.time_unit_ns = parse_time_unit(s)?;
                    }
            }
            Token::Ident("capacitive_load_unit") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    let mut val = 1.0f32;
                    let mut unit_str = "pf";
                    if let Some(tok_val) = lexer.next_token()
                        && let Token::Ident(s) | Token::Str(s) = tok_val {
                            val = number_value(s, "capacitive_load_unit")?;
                        }
                    if let Some(Token::Comma) = lexer.next_token()
                        && let Some(tok_unit) = lexer.next_token()
                            && let Token::Ident(s) | Token::Str(s) = tok_unit {
                                unit_str = s.trim_matches('"');
                            }
                    db.cap_unit_pf = match unit_str.to_ascii_lowercase().as_str() {
                        "pf" => val,
                        "ff" => val * 0.001,
                        "nf" => val * 1_000.0,
                        "uf" => val * 1_000_000.0,
                        _ => bail!("unsupported capacitive_load_unit {unit_str:?}"),
                    };
                }
            }
            Token::Ident("lu_table_template") => {
                if matches!(lexer.next_token(), Some(Token::LParen)) {
                    let tname = read_paren_header(lexer);
                    if let Some(Token::LBrace) = lexer.next_token() {
                        match tname {
                            Some(tname) => {
                                let tpl = parse_template_body(lexer, db)?;
                                templates.insert(tname, tpl);
                            }
                            None => lexer.skip_balanced_braces(),
                        }
                    }
                }
            }
            Token::Ident("cell") => {
                if matches!(lexer.next_token(), Some(Token::LParen)) {
                    let cname = read_paren_header(lexer);
                    if let Some(Token::LBrace) = lexer.next_token() {
                        match cname {
                            Some(cname) => {
                                let cname_id = db.strings.intern(&cname);
                                parse_cell_body(lexer, db, cname_id, templates)?;
                            }
                            None => lexer.skip_balanced_braces(),
                        }
                    }
                }
            }
            // Check if this is a group with { ... } or attribute with ;
            Token::Ident(_) if skip_statement_or_group(lexer) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    if !closed {
        bail!("unterminated Liberty library body (missing closing brace)");
    }
    Ok(())
}

/// Reads a `( name ... )` header up to the closing paren and returns the first
/// identifier — the declared name — if one appears. Extra arguments are consumed
/// so the following `{` stays aligned even for `()` or multi-argument headers.
fn read_paren_header(lexer: &mut Lexer) -> Option<String> {
    let mut name = None;
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RParen => break,
            Token::Ident(s) | Token::Str(s) if name.is_none() => name = Some(s.to_string()),
            _ => {}
        }
    }
    name
}

/// Skips an unknown attribute's value tokens up to `;` or a `{...}` group.
/// Returns `true` when the skip ran into the enclosing group's closing `}` —
/// the caller must treat its own block as closed. Without this an
/// unterminated attribute would consume the `}` and desynchronize the parse.
fn skip_statement_or_group(lexer: &mut Lexer) -> bool {
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::Semi => return false,
            Token::LBrace => {
                lexer.skip_balanced_braces();
                return false;
            }
            Token::RBrace => return true,
            _ => {}
        }
    }
    false
}

fn parse_template_body(lexer: &mut Lexer, db: &LibDb) -> Result<TemplateDef> {
    let mut tpl = TemplateDef::default();
    let mut closed = false;
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => {
                closed = true;
                break;
            }
            Token::Ident("variable_1") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Str(s) | Token::Ident(s)) = lexer.next_token() {
                        tpl.var1 = parse_axis_var(s);
                    }
            }
            Token::Ident("variable_2") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Str(s) | Token::Ident(s)) = lexer.next_token() {
                        tpl.var2 = parse_axis_var(s);
                    }
            }
            // Index values are stored raw: `index_*` may precede `variable_*`, so the
            // unit scale is applied once the body closes and both are known.
            Token::Ident("index_1") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    while let Some(tok_arg) = lexer.next_token() {
                        match tok_arg {
                            Token::RParen => break,
                            Token::Str(s) | Token::Ident(s) => parse_float_list(s, 1.0, &mut tpl.index_1)?,
                            _ => {}
                        }
                    }
                }
            }
            Token::Ident("index_2") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    while let Some(tok_arg) = lexer.next_token() {
                        match tok_arg {
                            Token::RParen => break,
                            Token::Str(s) | Token::Ident(s) => parse_float_list(s, 1.0, &mut tpl.index_2)?,
                            _ => {}
                        }
                    }
                }
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            // Unknown attributes are skipped whole so a value cannot alias a template
            // keyword (`custom : index_1;`) and corrupt the axis indices.
            Token::Ident(_) if skip_statement_or_group(lexer) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    if !closed {
        bail!("unterminated Liberty template body (missing closing brace)");
    }
    let axis_unit = |v: TableAxisVar| match v {
        TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
        _ => db.time_unit_ns,
    };
    for v in &mut tpl.index_1 {
        *v *= axis_unit(tpl.var1);
    }
    for v in &mut tpl.index_2 {
        *v *= axis_unit(tpl.var2);
    }
    Ok(tpl)
}

fn parse_cell_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    cell_name: SymbolId,
    templates: &HashMap<String, TemplateDef>,
) -> Result<()> {
    let pin_offset = db.pins.len() as u32;
    let arc_offset = db.arcs.len() as u32;
    let mut area = 0.0f32;
    let mut is_sequential = false;
    let mut is_latch = false;

    // Track cell-local pins mapping SymbolId to global LibPinId
    let mut local_pin_names: HashMap<SymbolId, LibPinId> = HashMap::new();
    let mut pending_arcs = Vec::new();
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => {
                closed = true;
                break;
            }
            Token::Ident("area") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        area = number_value(s, "area")?;
                    }
            }
            Token::Ident("ff") => {
                is_sequential = true;
                if let Some(Token::LParen) = lexer.next_token() {
                    // Extract var1 and var2
                    let mut vars = Vec::new();
                    while let Some(arg) = lexer.next_token() {
                        match arg {
                            Token::RParen => break,
                            Token::Ident(s) | Token::Str(s) => vars.push(s.to_string()),
                            _ => {}
                        }
                    }
                    if let Some(Token::LBrace) = lexer.next_token() {
                        lexer.skip_balanced_braces();
                    }
                    for var in vars {
                        let sym = db.strings.intern(&var);
                        if let std::collections::hash_map::Entry::Vacant(e) = local_pin_names.entry(sym) {
                            let global_id = LibPinId(u16::try_from(db.pins.len()).context("Liberty pin count exceeds u16")?);
                            e.insert(global_id);
                            db.pins.push(LibPin {
                                name: sym,
                                dir: PinDir::Internal,
                                cap_rise: 0.0,
                                cap_fall: 0.0,
                                cap_nom: 0.0,
                                is_clock: false,
                                max_cap: None,
                                max_transition: None,
                            });
                        }
                    }
                }
            }
            Token::Ident("latch") => {
                is_latch = true;
                if let Some(Token::LParen) = lexer.next_token() {
                    let mut vars = Vec::new();
                    while let Some(arg) = lexer.next_token() {
                        match arg {
                            Token::RParen => break,
                            Token::Ident(s) | Token::Str(s) => vars.push(s.to_string()),
                            _ => {}
                        }
                    }
                    if let Some(Token::LBrace) = lexer.next_token() {
                        lexer.skip_balanced_braces();
                    }
                    for var in vars {
                        let sym = db.strings.intern(&var);
                        if let std::collections::hash_map::Entry::Vacant(e) = local_pin_names.entry(sym) {
                            let global_id = LibPinId(u16::try_from(db.pins.len()).context("Liberty pin count exceeds u16")?);
                            e.insert(global_id);
                            db.pins.push(LibPin {
                                name: sym,
                                dir: PinDir::Internal,
                                cap_rise: 0.0,
                                cap_fall: 0.0,
                                cap_nom: 0.0,
                                is_clock: false,
                                max_cap: None,
                                max_transition: None,
                            });
                        }
                    }
                }
            }
            Token::Ident("pin") => {
                if matches!(lexer.next_token(), Some(Token::LParen)) {
                    // Drain the header: `pin (A, B)` lists must not leak `,`/`B` into
                    // the body parser. Only a single-name declaration is modelled;
                    // anything else has its group skipped.
                    let mut names = Vec::new();
                    while let Some(t) = lexer.next_token() {
                        match t {
                            Token::RParen => break,
                            Token::Ident(s) | Token::Str(s) => names.push(s.to_string()),
                            _ => {}
                        }
                    }
                    if let Some(Token::LBrace) = lexer.next_token() {
                        if names.len() == 1 {
                            let pin_sym = db.strings.intern(&names[0]);
                            // An `ff`/`latch` state variable may already occupy this name;
                            // reuse its slot so the pin is declared once, in place.
                            let global_id = match local_pin_names.get(&pin_sym) {
                                Some(&id) => id,
                                None => {
                                    let id = LibPinId(u16::try_from(db.pins.len()).context("Liberty pin count exceeds u16")?);
                                    local_pin_names.insert(pin_sym, id);
                                    id
                                }
                            };
                            parse_pin_body(
                                lexer,
                                db,
                                pin_sym,
                                global_id,
                                templates,
                                &mut pending_arcs,
                            )?;
                        } else {
                            lexer.skip_balanced_braces();
                        }
                    }
                }
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            // `test_cell`, `pg_pin`, `leakage_power`, `internal_power`,
            // `dynamic_current` and any other unknown attribute are skipped whole so
            // their values cannot alias keywords (`cell_footprint : latch;` would
            // otherwise set `is_latch`). One arm only: a guard that fails must not
            // fall through to a second skip.
            Token::Ident(_) if skip_statement_or_group(lexer) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    if !closed {
        bail!("unterminated Liberty cell body (missing closing brace)");
    }

    // Deferred resolution of related_pin SymbolIds to global LibPinId. An arc whose
    // `related_pin` is missing or names a pin the cell does not declare (e.g. a skipped
    // `pg_pin`) is dropped: substituting the target pin would fabricate a self-loop arc
    // that levelization reports as a combinational loop.
    for (target_pin_id, rel_sym, sense, ttype, rise, fall, r_trans, f_trans, r_const, f_const) in pending_arcs {
        let Some(&rel_pin_id) = local_pin_names.get(&rel_sym) else {
            continue;
        };
        db.arcs.push(LibTimingArc {
            related_pin: rel_pin_id,
            target_pin: target_pin_id,
            timing_sense: sense,
            timing_type: ttype,
            cell_rise: rise,
            cell_fall: fall,
            rise_transition: r_trans,
            fall_transition: f_trans,
            rise_constraint: r_const,
            fall_constraint: f_const,
        });
    }

    let pin_count = u16::try_from(db.pins.len() - pin_offset as usize).context("cell pin count exceeds u16")?;
    let arc_count = u16::try_from(db.arcs.len() - arc_offset as usize).context("cell arc count exceeds u16")?;

    db.cells.push(LibCell {
        name: cell_name,
        area,
        is_sequential,
        is_latch,
        pin_offset,
        pin_count,
        arc_offset,
        arc_count,
    });

    Ok(())
}

#[allow(clippy::type_complexity)]
fn parse_pin_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    pin_name: SymbolId,
    target_pin_id: LibPinId,
    templates: &HashMap<String, TemplateDef>,
    pending_arcs: &mut Vec<(
        LibPinId,
        SymbolId,
        TimingSense,
        TimingType,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
    )>,
) -> Result<()> {
    let mut dir = PinDir::Input;
    let mut cap = 0.0f32;
    let mut cap_rise = 0.0f32;
    let mut cap_fall = 0.0f32;
    let mut has_rise_cap = false;
    let mut has_fall_cap = false;
    let mut is_clock = false;
    let mut max_cap = None;
    let mut max_transition = None;

    // Temporary list of timing arcs defined inside this pin
    let mut pin_arcs = Vec::new();
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => {
                closed = true;
                break;
            }
            Token::Ident("direction") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        dir = parse_pin_direction(s)?;
                    }
            }
            Token::Ident("capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap = number_value(s, "capacitance")? * db.cap_unit_pf;
                    }
            }
            Token::Ident("rise_capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap_rise = number_value(s, "rise_capacitance")? * db.cap_unit_pf;
                        has_rise_cap = true;
                    }
            }
            Token::Ident("fall_capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap_fall = number_value(s, "fall_capacitance")? * db.cap_unit_pf;
                        has_fall_cap = true;
                    }
            }
            Token::Ident("clock") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        is_clock = s.trim_matches('"').eq_ignore_ascii_case("true");
                    }
            }
            Token::Ident("max_capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        max_cap = Some(number_value(s, "max_capacitance")? * db.cap_unit_pf);
                    }
            }
            Token::Ident("max_transition") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        max_transition = Some(number_value(s, "max_transition")? * db.time_unit_ns);
                    }
            }
            Token::Ident("timing") => {
                // Consume optional (args)
                while let Some(tok_t) = lexer.next_token() {
                    match tok_t {
                        Token::LBrace => break,
                        Token::RParen => {}
                        _ => {}
                    }
                }
                parse_timing_body(lexer, db, target_pin_id, templates, &mut pin_arcs)?;
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            // Skip unknown pin attributes whole so their values cannot alias keywords.
            Token::Ident(_) if skip_statement_or_group(lexer) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }

    if !has_rise_cap {
        cap_rise = cap;
    }
    if !has_fall_cap {
        cap_fall = cap;
    }

    let pin = LibPin {
        name: pin_name,
        dir,
        cap_rise,
        cap_fall,
        cap_nom: cap,
        is_clock,
        max_cap,
        max_transition,
    };
    // A pin sharing its name with an `ff`/`latch` state variable reuses the placeholder
    // slot pushed earlier; writing in place keeps one entry per name.
    let idx = target_pin_id.0 as usize;
    if idx < db.pins.len() {
        db.pins[idx] = pin;
    } else {
        db.pins.push(pin);
    }

    for (rel_sym, sense, ttype, rise, fall, r_trans, f_trans, r_const, f_const) in pin_arcs {
        pending_arcs.push((
            target_pin_id,
            rel_sym,
            sense,
            ttype,
            rise,
            fall,
            r_trans,
            f_trans,
            r_const,
            f_const,
        ));
    }

    if !closed {
        bail!("unterminated Liberty pin body (missing closing brace)");
    }

    Ok(())
}

#[allow(clippy::type_complexity)]
fn parse_timing_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    _target_pin_id: LibPinId,
    templates: &HashMap<String, TemplateDef>,
    arcs_out: &mut Vec<(
        SymbolId,
        TimingSense,
        TimingType,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
        Option<NldmTable>,
    )>,
) -> Result<()> {
    let mut related_pins_str = String::new();
    let mut sense = TimingSense::NonUnate;
    let mut ttype = TimingType::Combinational;

    let mut cell_rise = None;
    let mut cell_fall = None;
    let mut rise_trans = None;
    let mut fall_trans = None;
    let mut rise_const = None;
    let mut fall_const = None;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => {
                closed = true;
                break;
            }
            Token::Ident("related_pin") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Str(s) | Token::Ident(s)) = lexer.next_token() {
                        related_pins_str = s.to_string();
                    }
            }
            Token::Ident("timing_sense") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        sense = parse_timing_sense(s);
                    }
            }
            Token::Ident("timing_type") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        ttype = parse_timing_type(s);
                    }
            }
            Token::Ident("cell_rise") => {
                cell_rise = parse_nldm_table(lexer, db, templates)?;
            }
            Token::Ident("cell_fall") => {
                cell_fall = parse_nldm_table(lexer, db, templates)?;
            }
            Token::Ident("rise_transition") => {
                rise_trans = parse_nldm_table(lexer, db, templates)?;
            }
            Token::Ident("fall_transition") => {
                fall_trans = parse_nldm_table(lexer, db, templates)?;
            }
            Token::Ident("rise_constraint") => {
                rise_const = parse_nldm_table(lexer, db, templates)?;
            }
            Token::Ident("fall_constraint") => {
                fall_const = parse_nldm_table(lexer, db, templates)?;
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            // Unknown timing attributes (including `when` conditionals, whose arcs this
            // parser does not model) are skipped with their values.
            Token::Ident(_) if skip_statement_or_group(lexer) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }

    let rel_parts: Vec<&str> = related_pins_str.split_whitespace().collect();
    if rel_parts.is_empty() {
        let sym = db.strings.intern("");
        arcs_out.push((
            sym,
            sense,
            ttype,
            cell_rise,
            cell_fall,
            rise_trans,
            fall_trans,
            rise_const,
            fall_const,
        ));
    } else {
        for rel in rel_parts {
            let sym = db.strings.intern(rel.trim_matches('"'));
            arcs_out.push((
                sym,
                sense,
                ttype,
                cell_rise.clone(),
                cell_fall.clone(),
                rise_trans.clone(),
                fall_trans.clone(),
                rise_const.clone(),
                fall_const.clone(),
            ));
        }
    }

    if !closed {
        bail!("unterminated Liberty timing body (missing closing brace)");
    }

    Ok(())
}

fn parse_nldm_table(
    lexer: &mut Lexer,
    db: &LibDb,
    templates: &HashMap<String, TemplateDef>,
) -> Result<Option<NldmTable>> {
    let mut tpl_name = String::new();
    if matches!(lexer.next_token(), Some(Token::LParen)) {
        tpl_name = read_paren_header(lexer).unwrap_or_default();
    }

    if let Some(Token::LBrace) = lexer.next_token() {
        let tpl_opt = templates.get(&tpl_name).cloned();
        let var1 = tpl_opt.as_ref().map(|t| t.var1).unwrap_or(TableAxisVar::Other);
        let var2 = tpl_opt.as_ref().map(|t| t.var2).unwrap_or(TableAxisVar::Other);

        let mut index_1 = Vec::new();
        let mut index_2 = Vec::new();
        let mut values = Vec::new();
        let mut closed = false;

        while let Some(tok) = lexer.next_token() {
            match tok {
                Token::RBrace => {
                    closed = true;
                    break;
                }
                Token::Ident("index_1") => {
                    if let Some(Token::LParen) = lexer.next_token() {
                        let scale = match var1 {
                            TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
                            _ => db.time_unit_ns,
                        };
                        while let Some(arg) = lexer.next_token() {
                            match arg {
                                Token::RParen => break,
                                Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut index_1)?,
                                _ => {}
                            }
                        }
                    }
                }
                Token::Ident("index_2") => {
                    if let Some(Token::LParen) = lexer.next_token() {
                        let scale = match var2 {
                            TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
                            _ => db.time_unit_ns,
                        };
                        while let Some(arg) = lexer.next_token() {
                            match arg {
                                Token::RParen => break,
                                Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut index_2)?,
                                _ => {}
                            }
                        }
                    }
                }
                Token::Ident("values") => {
                    if let Some(Token::LParen) = lexer.next_token() {
                        while let Some(arg) = lexer.next_token() {
                            match arg {
                                Token::RParen => break,
                                Token::Str(s) | Token::Ident(s) => {
                                    parse_float_list(s, db.time_unit_ns, &mut values)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Token::LBrace => {
                    lexer.skip_balanced_braces();
                }
                // Unknown attributes are skipped whole so a value cannot alias a table
                // keyword (e.g. `foo : cell_rise;`) and open a phantom table. NLDM entries
                // carry no `;`, so the table's closing brace also terminates the skip.
                Token::Ident(_) => {
                    let mut hit_close = false;
                    while let Some(t) = lexer.next_token() {
                        match t {
                            Token::Semi => break,
                            Token::LBrace => {
                                lexer.skip_balanced_braces();
                                break;
                            }
                            Token::RBrace => {
                                hit_close = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if hit_close {
                        closed = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !closed {
            bail!("unterminated Liberty NLDM table body (missing closing brace)");
        }

        // Fallback to template indices if not specified on instance
        if index_1.is_empty()
            && let Some(tpl) = &tpl_opt {
                index_1 = tpl.index_1.clone();
            }
        if index_2.is_empty()
            && let Some(tpl) = &tpl_opt {
                index_2 = tpl.index_2.clone();
            }

        // 0D scalar table
        if (tpl_name.eq_ignore_ascii_case("scalar") || (index_1.is_empty() && index_2.is_empty())) && values.len() == 1 {
            return Ok(Some(NldmTable {
                var1: TableAxisVar::Other,
                var2: TableAxisVar::Other,
                index_1: vec![0.0],
                index_2: vec![0.0],
                values,
            }));
        }

        // Pad a missing axis symmetrically so `eval_2d` always sees at least one index
        // per dimension; an `index_2`-only table would otherwise underflow `m - 1`.
        if index_1.is_empty() {
            index_1 = vec![0.0];
        }
        if index_2.is_empty() {
            index_2 = vec![0.0];
        }

        if values.is_empty() {
            return Ok(None);
        }
        if values.len() != index_1.len() * index_2.len() {
            bail!("NLDM table has {} values for a {}x{} index grid", values.len(), index_1.len(), index_2.len());
        }

        Ok(Some(NldmTable {
            var1,
            var2,
            index_1,
            index_2,
            values,
        }))
    } else {
        Ok(None)
    }
}

/// Directory for Liberty binary caches. Never the source's own directory: a `.lib` may sit in the read-only PDK tree.
fn cache_dir() -> PathBuf {
    cache_dir_from(
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
        &std::env::temp_dir(),
        std::env::var_os("USER"),
    )
}

/// Pure form of `cache_dir` so each environment combination can be tested without mutating the process environment.
fn cache_dir_from(
    xdg_cache_home: Option<OsString>,
    home: Option<OsString>,
    tmp: &Path,
    user: Option<OsString>,
) -> PathBuf {
    let set = |v: Option<OsString>| v.filter(|v| !v.is_empty());
    if let Some(xdg) = set(xdg_cache_home).map(PathBuf::from).filter(|p| p.is_absolute()) {
        return xdg.join("grus-eda");
    }
    if let Some(home) = set(home) {
        return PathBuf::from(home).join(".cache").join("grus-eda");
    }
    let user = set(user).unwrap_or_else(|| OsString::from("shared"));
    tmp.join(format!("grus-eda-cache-{}", user.to_string_lossy()))
}

fn cache_file(dir: &Path, source_xxh3: u64) -> PathBuf {
    dir.join(format!("{:016x}.libcache", source_xxh3))
}

/// Returns the cached database only if its header matches the source; any other outcome is a miss.
fn read_cache(cache_path: &Path, source_size: u64, source_xxh3: u64) -> Option<LibDb> {
    let cfile = File::open(cache_path).ok()?;
    if cfile.metadata().ok()?.len() <= 16 {
        return None;
    }
    let c_mmap = unsafe { memmap2::Mmap::map(&cfile) }.ok()?;
    let (cache_obj, _) =
        bincode::serde::decode_from_slice::<CacheFile, _>(&c_mmap, bincode::config::standard())
            .ok()?;
    if cache_obj.magic != CACHE_MAGIC
        || cache_obj.version != CACHE_VERSION
        || cache_obj.source_size != source_size
        || cache_obj.source_xxh3 != source_xxh3
    {
        return None;
    }
    let mut db = cache_obj.db;
    db.strings.rebuild_map();
    db.rebuild_cell_by_name();
    Some(db)
}

static CACHE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Writes through a temp name unique to this process and call, then renames into place.
fn write_cache_file(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let seq = CACHE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = target.with_extension(format!("{}.{}.tmp", std::process::id(), seq));
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Loads a Liberty file from disk, using mmap binary cache if valid, or parsing and caching.
pub fn load_liberty(path: &Path) -> Result<LibDb> {
    load_liberty_in(path, &cache_dir())
}

fn load_liberty_in(path: &Path, dir: &Path) -> Result<LibDb> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open Liberty file at {:?}", path))?;
    let metadata = file.metadata()?;
    let source_size = metadata.len();

    // Memory map the file to compute hash and parse if needed
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    let source_xxh3 = xxh3_64(&mmap);

    let cache_path = cache_file(dir, source_xxh3);
    if let Some(db) = read_cache(&cache_path, source_size, source_xxh3) {
        return Ok(db);
    }

    // Cache miss: parse directly from mmap; `parse_liberty_bytes` already rebuilt the
    // lookup maps.
    let db = parse_liberty_bytes(&mmap)?;

    // Serialize and write to cache
    let cache_obj = CacheFile {
        magic: CACHE_MAGIC,
        version: CACHE_VERSION,
        source_size,
        source_xxh3,
        db: db.clone(),
    };
    if let Ok(encoded) = bincode::serde::encode_to_vec(&cache_obj, bincode::config::standard()) {
        // A cache that cannot be written only costs a reparse on the next load.
        let _ = write_cache_file(&cache_path, &encoded);
    }

    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, require, sky130_hd};

    const SAMPLE_LIB: &str = r#"
library (test_lib) {
    time_unit : "1ns";
    capacitive_load_unit (1, pf);

    lu_table_template (del_template) {
        variable_1 : input_net_transition;
        variable_2 : total_output_net_capacitance;
        index_1 ("0.01, 0.1");
        index_2 ("0.001, 0.01");
    }

    cell (inv_sample) {
        area : 1.25;
        pin (A) {
            direction : input;
            capacitance : 0.004;
        }
        pin (Y) {
            direction : output;
            timing () {
                related_pin : "A";
                timing_sense : negative_unate;
                timing_type : combinational;
                cell_rise (del_template) {
                    values ("0.02, 0.05", "0.04, 0.09");
                }
                cell_fall (del_template) {
                    values ("0.02, 0.05", "0.04, 0.09");
                }
            }
        }
    }

    cell (dff_sample) {
        area : 15.0;
        ff ("IQ", "IQ_N") {
            clocked_on : "CLK";
            next_state : "D";
        }
        pin (CLK) {
            direction : input;
            clock : true;
        }
        pin (D) {
            direction : input;
        }
        pin (Q) {
            direction : output;
        }
    }
}
"#;

    #[test]
    fn test_parse_sample_lib() {
        let db = parse_liberty_bytes(SAMPLE_LIB.as_bytes()).expect("parse failed");
        assert_eq!(db.library_name, "test_lib");
        assert_eq!(db.time_unit_ns, 1.0);
        assert_eq!(db.cap_unit_pf, 1.0);
        assert_eq!(db.cells.len(), 2);

        // Check inv_sample
        let inv = &db.cells[0];
        assert_eq!(db.strings.resolve(inv.name), "inv_sample");
        assert_eq!(inv.area, 1.25);
        assert!(!inv.is_sequential);
        assert_eq!(inv.pin_count, 2);
        assert_eq!(inv.arc_count, 1);

        let pins = db.cell_pins(inv);
        assert_eq!(db.strings.resolve(pins[0].name), "A");
        assert_eq!(pins[0].dir, PinDir::Input);
        assert_eq!(pins[0].cap_nom, 0.004);
        assert_eq!(db.strings.resolve(pins[1].name), "Y");
        assert_eq!(pins[1].dir, PinDir::Output);

        let arcs = db.cell_arcs(inv);
        assert_eq!(arcs[0].timing_sense, TimingSense::NegativeUnate);
        let cr = arcs[0].cell_rise.as_ref().expect("cell_rise must exist");
        assert_eq!(cr.values.len(), 4);
        assert_eq!(cr.index_1, vec![0.01, 0.1]);
        assert_eq!(cr.index_2, vec![0.001, 0.01]);

        // Check dff_sample
        let dff = &db.cells[1];
        assert_eq!(db.strings.resolve(dff.name), "dff_sample");
        assert!(dff.is_sequential);
        // IQ, IQ_N, CLK, D, Q = 5 pins
        assert_eq!(dff.pin_count, 5);
        let dff_pins = db.cell_pins(dff);
        assert_eq!(db.strings.resolve(dff_pins[0].name), "IQ");
        assert_eq!(dff_pins[0].dir, PinDir::Internal);
        assert_eq!(db.strings.resolve(dff_pins[1].name), "IQ_N");
        assert_eq!(dff_pins[1].dir, PinDir::Internal);
        assert_eq!(db.strings.resolve(dff_pins[2].name), "CLK");
        assert!(dff_pins[2].is_clock);
    }

    #[test]
    fn test_cache_roundtrip() {
        let temp_dir = std::env::temp_dir().join("grus_test_cache");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_lib_file = temp_dir.join("sample.lib");
        std::fs::write(&test_lib_file, SAMPLE_LIB).expect("write failed");

        // First load: cold parse
        let t0 = std::time::Instant::now();
        let db1 = load_liberty(&test_lib_file).expect("cold load failed");
        let cold_time = t0.elapsed();

        // Second load: warm mmap cache
        let t1 = std::time::Instant::now();
        let db2 = load_liberty(&test_lib_file).expect("warm load failed");
        let warm_time = t1.elapsed();

        assert_eq!(db1.library_name, db2.library_name);
        assert_eq!(db1.cells.len(), db2.cells.len());
        assert_eq!(db1.pins.len(), db2.pins.len());
        assert_eq!(db1.arcs.len(), db2.arcs.len());

        println!("Cold parse time: {:?}, Warm cache load time: {:?}", cold_time, warm_time);
        assert!(warm_time.as_millis() < 50);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sky130_liberty_integration() {
        let lib_path = require(sky130_hd().join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));

        // Remove this file's cache entry to guarantee a true cold parse
        let lib_xxh3 = xxh3_64(&std::fs::read(&lib_path).expect("read sky130 liberty"));
        let _ = std::fs::remove_file(cache_file(&cache_dir(), lib_xxh3));

        // Test cold parse
        let t0 = std::time::Instant::now();
        let db = load_liberty(&lib_path).expect("Failed to load sky130 liberty");
        let load_time_1 = t0.elapsed();

        // Test warm mmap cache load
        let t1 = std::time::Instant::now();
        let db_cached = load_liberty(&lib_path).expect("Failed to load sky130 cached liberty");
        let load_time_2 = t1.elapsed();

        println!(
            "sky130 liberty: load 1 (cold/parse): {:?}, load 2 (warm/cache): {:?}",
            load_time_1, load_time_2
        );

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        let expected_cells = ref_json["lib_cells"].as_u64().expect("lib_cells") as usize;
        let expected_pins = ref_json["lib_pins"].as_u64().expect("lib_pins") as usize;
        let expected_seq = ref_json["lib_sequential_cells"].as_u64().expect("lib_sequential_cells") as usize;

        assert_eq!(db.library_name, "sky130_fd_sc_hd__tt_025C_1v80");
        assert_eq!(db.cells.len(), expected_cells, "Expected lib_cells matching reference");
        assert_eq!(db.pins.len(), expected_pins, "Expected lib_pins matching reference");
        assert_eq!(db_cached.cells.len(), expected_cells);
        assert_eq!(db_cached.pins.len(), expected_pins);

        let ff_count = db.cells.iter().filter(|c| c.is_sequential).count();
        let latch_count = db.cells.iter().filter(|c| c.is_latch).count();
        // OpenSTA's Tcl API exposes no per-cell ff/latch flag, so only the total is a
        // tool-derived reference.
        assert_eq!(ff_count + latch_count, expected_seq, "Expected sequential cells matching reference");

        // Spot-check inv_1
        let inv_sym = db.strings.get("sky130_fd_sc_hd__inv_1").expect("inv_1 symbol not found");
        let inv_id = db.cell_by_name.get(&inv_sym).expect("inv_1 cell not found");
        let inv_cell = &db.cells[inv_id.0 as usize];
        assert_eq!(inv_cell.pin_count, 2);
        assert_eq!(inv_cell.arc_count, 1);
        let inv_pins = db.cell_pins(inv_cell);
        assert_eq!(db.strings.resolve(inv_pins[0].name), "A");
        assert_eq!(db.strings.resolve(inv_pins[1].name), "Y");

        let inv_arcs = db.cell_arcs(inv_cell);
        let inv_fall = inv_arcs[0].cell_fall.as_ref().expect("cell_fall must exist");
        assert_eq!(inv_fall.index_1.len(), 7);
        assert_eq!(inv_fall.index_2.len(), 7);
        assert_eq!(inv_fall.values.len(), 49);
        assert!((inv_fall.index_1[0] - 0.01).abs() < 1e-6);
        assert!((inv_fall.index_2[0] - 0.0005).abs() < 1e-6);
        assert!((inv_fall.values[0] - 0.0143656).abs() < 1e-6);
        assert!((inv_fall.values[48] - 1.2016104).abs() < 1e-6);

        // Spot-check dfxtp_1
        let dfx_sym = db.strings.get("sky130_fd_sc_hd__dfxtp_1").expect("dfxtp_1 symbol not found");
        let dfx_id = db.cell_by_name.get(&dfx_sym).expect("dfxtp_1 cell not found");
        let dfx_cell = &db.cells[dfx_id.0 as usize];
        assert!(dfx_cell.is_sequential);
        assert_eq!(dfx_cell.pin_count, 5); // IQ, IQ_N, CLK, D, Q

        // Spot-check fa_1
        let fa_sym = db.strings.get("sky130_fd_sc_hd__fa_1").expect("fa_1 symbol not found");
        let fa_id = db.cell_by_name.get(&fa_sym).expect("fa_1 cell not found");
        let fa_cell = &db.cells[fa_id.0 as usize];
        assert_eq!(fa_cell.pin_count, 5); // A, B, CIN, COUT, SUM

        // Spot-check dlxtp_1 (D declared before GATE forward reference check)
        let dlx_sym = db.strings.get("sky130_fd_sc_hd__dlxtp_1").expect("dlxtp_1 symbol not found");
        let dlx_id = db.cell_by_name.get(&dlx_sym).expect("dlxtp_1 cell not found");
        let dlx_cell = &db.cells[dlx_id.0 as usize];
        let dlx_arcs = db.cell_arcs(dlx_cell);
        let gate_sym = db.strings.get("GATE").expect("GATE pin not found");
        // Verify that D's timing arc has related_pin == GATE (not IQ)
        let d_sym = db.strings.get("D").expect("D pin not found");
        let d_arc = dlx_arcs.iter().find(|a| db.pins[a.target_pin.0 as usize].name == d_sym)
            .expect("timing arc for D not found");
        assert_eq!(db.pins[d_arc.related_pin.0 as usize].name, gate_sym);
    }

    #[test]
    fn test_forward_referenced_related_pin() {
        let lib_text = r#"
library(test_fwd) {
  cell(latch_fwd) {
    pin(D) {
      direction : input;
      timing() {
        related_pin : "GATE";
        timing_type : "setup_rising";
      }
    }
    pin(GATE) {
      direction : input;
    }
    pin(Q) {
      direction : output;
      timing() {
        related_pin : "GATE";
        timing_type : "combinational";
      }
    }
  }
}
"#;
        let db = parse_liberty_bytes(lib_text.as_bytes()).unwrap();
        assert_eq!(db.cells.len(), 1);
        let cell = &db.cells[0];
        assert_eq!(cell.pin_count, 3);
        assert_eq!(cell.arc_count, 2);
        let arcs = db.cell_arcs(cell);
        let gate_sym = db.strings.get("GATE").unwrap();
        let d_sym = db.strings.get("D").unwrap();
        let q_sym = db.strings.get("Q").unwrap();
        // D arc should have related_pin == GATE (global LibPinId(1))
        let d_arc = &arcs[0];
        assert_eq!(db.pins[d_arc.target_pin.0 as usize].name, d_sym);
        assert_eq!(db.pins[d_arc.related_pin.0 as usize].name, gate_sym);
        assert_eq!(d_arc.related_pin, LibPinId(1));
        // Q arc should have related_pin == GATE (global LibPinId(1))
        let q_arc = &arcs[1];
        assert_eq!(db.pins[q_arc.target_pin.0 as usize].name, q_sym);
        assert_eq!(db.pins[q_arc.related_pin.0 as usize].name, gate_sym);
        assert_eq!(q_arc.related_pin, LibPinId(1));
    }

    #[test]
    fn test_cache_dir_from_env() {
        let tmp = Path::new("/tmp");
        let s = |v: &str| Some(OsString::from(v));
        let xdg = cache_dir_from(s("/xdg"), s("/home/u"), tmp, s("u"));
        assert_eq!(xdg, PathBuf::from("/xdg/grus-eda"));
        let xdg_unset = cache_dir_from(None, s("/home/u"), tmp, s("u"));
        assert_eq!(xdg_unset, PathBuf::from("/home/u/.cache/grus-eda"));
        let xdg_empty = cache_dir_from(s(""), s("/home/u"), tmp, s("u"));
        assert_eq!(xdg_empty, PathBuf::from("/home/u/.cache/grus-eda"));
        let xdg_relative = cache_dir_from(s("rel/cache"), s("/home/u"), tmp, s("u"));
        assert_eq!(xdg_relative, PathBuf::from("/home/u/.cache/grus-eda"));
    }

    #[test]
    fn test_cache_dir_without_home_uses_temp_dir() {
        let tmp = Path::new("/tmp");
        let s = |v: &str| Some(OsString::from(v));
        let with_user = cache_dir_from(None, None, tmp, s("alice"));
        assert_eq!(with_user, PathBuf::from("/tmp/grus-eda-cache-alice"));
        let no_user = cache_dir_from(None, s(""), tmp, None);
        assert_eq!(no_user, PathBuf::from("/tmp/grus-eda-cache-shared"));
    }

    #[test]
    fn test_cache_written_under_cache_dir_only() {
        let root = std::env::temp_dir().join(format!("grus_cache_loc_{}", std::process::id()));
        let src_dir = root.join("src");
        let cache_dir = root.join("cache");
        std::fs::create_dir_all(&src_dir).expect("create source dir");
        let lib = src_dir.join("sample.lib");
        std::fs::write(&lib, SAMPLE_LIB).expect("write failed");

        load_liberty_in(&lib, &cache_dir).expect("load failed");

        let xxh3 = xxh3_64(SAMPLE_LIB.as_bytes());
        assert!(cache_file(&cache_dir, xxh3).is_file(), "cache not written under the cache directory");
        let mut entries: Vec<OsString> = std::fs::read_dir(&src_dir)
            .expect("read source dir")
            .map(|e| e.expect("dir entry").file_name())
            .collect();
        entries.sort();
        assert_eq!(entries, vec![OsString::from("sample.lib")], "cache written next to the .lib");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Error text for a library that must be rejected. The needle must be in the fixture, so a
    /// test cannot pass by editing nothing.
    fn sample_lib_error(needle: &str, replacement: &str) -> String {
        assert!(SAMPLE_LIB.contains(needle), "fixture no longer contains `{needle}`");
        let text = SAMPLE_LIB.replacen(needle, replacement, 1);
        match parse_liberty_bytes(text.as_bytes()) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("malformed library was accepted: `{replacement}`"),
        }
    }

    #[test]
    fn test_non_numeric_cell_area_is_an_error() {
        let msg = sample_lib_error("area : 1.25;", "area : 1.2x;");
        assert!(msg.contains("area") && msg.contains("1.2x"), "{msg}");
    }

    #[test]
    fn test_non_numeric_pin_capacitance_is_an_error() {
        let msg = sample_lib_error("capacitance : 0.004;", "capacitance : 0.00x;");
        assert!(msg.contains("capacitance") && msg.contains("0.00x"), "{msg}");
    }

    #[test]
    fn test_non_numeric_table_value_is_an_error() {
        let msg = sample_lib_error("values (\"0.02, 0.05\"", "values (\"0.02, 0.0x\"");
        assert!(msg.contains("0.0x"), "{msg}");
    }

    #[test]
    fn test_unknown_time_unit_scale_is_an_error() {
        let msg = sample_lib_error("time_unit : \"1ns\";", "time_unit : \"1xns\";");
        assert!(msg.contains("time_unit") && msg.contains("1x"), "{msg}");
    }

    #[test]
    fn test_unknown_pin_direction_is_an_error() {
        let msg = sample_lib_error("direction : input;", "direction : sideways;");
        assert!(msg.contains("sideways"), "{msg}");
    }

    #[test]
    fn test_empty_paren_nldm_table_does_not_truncate() {
        // `cell_rise ()` carries no template name; the parser must still find the
        // table body instead of consuming `{` as the missing `)` and dropping the
        // rest of the file.
        let text = r#"
library (t) {
  cell (c) {
    pin (A) { direction : input; }
    pin (Y) {
      direction : output;
      timing () {
        related_pin : "A";
        timing_type : combinational;
        cell_rise () {
          values ("0.02");
        }
      }
    }
  }
}
"#;
        let db = parse_liberty_bytes(text.as_bytes()).expect("parse failed");
        assert_eq!(db.cells.len(), 1);
        let cr = db.cell_arcs(&db.cells[0])[0].cell_rise.as_ref().expect("cell_rise must exist");
        assert_eq!(cr.values, vec![0.02]);
    }

    #[test]
    fn test_template_index_scaled_after_variables() {
        // `index_*` may precede `variable_*`; the capacitance axis must still use
        // `capacitive_load_unit`, not `time_unit`.
        let text = r#"
library (t) {
  time_unit : "1ns";
  capacitive_load_unit (1, nf);
  lu_table_template (tpl) {
    index_1 ("0.01");
    index_2 ("0.001");
    variable_1 : input_net_transition;
    variable_2 : total_output_net_capacitance;
  }
  cell (c) {
    pin (A) { direction : input; }
    pin (Y) {
      direction : output;
      timing () {
        related_pin : "A";
        timing_type : combinational;
        cell_rise (tpl) {
          values ("0.02");
        }
      }
    }
  }
}
"#;
        let db = parse_liberty_bytes(text.as_bytes()).expect("parse failed");
        let cr = db.cell_arcs(&db.cells[0])[0].cell_rise.as_ref().expect("cell_rise must exist");
        assert_eq!(cr.index_1, vec![0.01]);
        assert_eq!(cr.index_2, vec![1.0]);
    }

    #[test]
    fn test_liberty_without_library_body_is_an_error() {
        assert!(parse_liberty_bytes(b"").is_err());
        assert!(parse_liberty_bytes(b"library (foo);").is_err());
        assert!(parse_liberty_bytes(b"not a liberty file").is_err());
    }
}
