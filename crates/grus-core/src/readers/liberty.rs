use crate::interner::{LibPinId, PinDir, SymbolId};
use crate::lib_db::{
    LibCell, LibDb, LibPin, LibTimingArc, NldmTable, TableAxisVar, TimingSense, TimingType,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

const CACHE_MAGIC: [u8; 8] = *b"GRUSLIB1";

#[derive(Serialize, Deserialize)]
struct CacheFile {
    magic: [u8; 8],
    source_size: u64,
    source_xxh3: u64,
    db: LibDb,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Token<'a> {
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
                }
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> Option<Token<'a>> {
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
        _ => TimingType::Other,
    }
}

fn parse_pin_direction(s: &str) -> PinDir {
    let clean = s.trim_matches('"').trim();
    match clean {
        "input" => PinDir::Input,
        "output" => PinDir::Output,
        "inout" => PinDir::Inout,
        "internal" => PinDir::Internal,
        _ => PinDir::Input,
    }
}

fn parse_time_unit(s: &str) -> f32 {
    let clean = s.trim_matches('"').trim();
    if let Some(num_str) = clean.strip_suffix("ps") {
        num_str.trim().parse::<f32>().unwrap_or(1000.0) * 0.001
    } else if let Some(num_str) = clean.strip_suffix("ns") {
        num_str.trim().parse::<f32>().unwrap_or(1.0)
    } else if let Some(num_str) = clean.strip_suffix("us") {
        num_str.trim().parse::<f32>().unwrap_or(1.0) * 1000.0
    } else {
        clean.parse::<f32>().unwrap_or(1.0)
    }
}

fn parse_float_list(s: &str, scale: f32, out: &mut Vec<f32>) {
    for part in s.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty()
            && let Ok(v) = trimmed.parse::<f32>() {
                out.push(v * scale);
            }
    }
}

/// Parses a Liberty (`.lib`) formatted byte slice into a `LibDb`.
pub fn parse_liberty_bytes(bytes: &[u8]) -> Result<LibDb> {
    let mut lexer = Lexer::new(bytes);
    let mut db = LibDb::new();
    db.time_unit_ns = 1.0;
    db.cap_unit_pf = 1.0;

    let mut templates: HashMap<String, TemplateDef> = HashMap::new();

    // Top-level must start with library ( <name> ) { ... }
    while let Some(tok) = lexer.next_token() {
        if let Token::Ident("library") = tok
            && let Some(Token::LParen) = lexer.next_token()
                && let Some(name_tok) = lexer.next_token() {
                    let lib_name = match name_tok {
                        Token::Ident(s) | Token::Str(s) => s.to_string(),
                        _ => "unnamed".to_string(),
                    };
                    db.library_name = lib_name;
                    let _ = lexer.next_token(); // RParen
                    if let Some(Token::LBrace) = lexer.next_token() {
                        parse_library_body(&mut lexer, &mut db, &mut templates)?;
                    }
                }
    }

    db.strings.rebuild_map();
    db.rebuild_cell_by_name();

    Ok(db)
}

fn parse_library_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    templates: &mut HashMap<String, TemplateDef>,
) -> Result<()> {
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => break,
            Token::Ident("time_unit") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Str(s) | Token::Ident(s)) = lexer.next_token() {
                        db.time_unit_ns = parse_time_unit(s);
                    }
            }
            Token::Ident("capacitive_load_unit") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    let mut val = 1.0f32;
                    let mut unit_str = "pf";
                    if let Some(tok_val) = lexer.next_token()
                        && let Token::Ident(s) | Token::Str(s) = tok_val {
                            val = s.parse::<f32>().unwrap_or(1.0);
                        }
                    if let Some(Token::Comma) = lexer.next_token()
                        && let Some(tok_unit) = lexer.next_token()
                            && let Token::Ident(s) | Token::Str(s) = tok_unit {
                                unit_str = s.trim_matches('"');
                            }
                    if unit_str.eq_ignore_ascii_case("ff") {
                        db.cap_unit_pf = val * 0.001;
                    } else {
                        db.cap_unit_pf = val;
                    }
                }
            }
            Token::Ident("lu_table_template") => {
                if let Some(Token::LParen) = lexer.next_token()
                    && let Some(Token::Ident(tpl_name) | Token::Str(tpl_name)) = lexer.next_token() {
                        let tname = tpl_name.to_string();
                        let _ = lexer.next_token(); // RParen
                        if let Some(Token::LBrace) = lexer.next_token() {
                            let tpl = parse_template_body(lexer, db)?;
                            templates.insert(tname, tpl);
                        }
                    }
            }
            Token::Ident("cell") => {
                if let Some(Token::LParen) = lexer.next_token()
                    && let Some(Token::Ident(cell_name) | Token::Str(cell_name)) = lexer.next_token() {
                        let cname_id = db.strings.intern(cell_name);
                        let _ = lexer.next_token(); // RParen
                        if let Some(Token::LBrace) = lexer.next_token() {
                            parse_cell_body(lexer, db, cname_id, templates)?;
                        }
                    }
            }
            Token::Ident(_) => {
                // Check if this is a group with { ... } or attribute with ;
                skip_statement_or_group(lexer);
            }
            _ => {}
        }
    }
    Ok(())
}

fn skip_statement_or_group(lexer: &mut Lexer) {
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::Semi => break,
            Token::LBrace => {
                lexer.skip_balanced_braces();
                break;
            }
            _ => {}
        }
    }
}

fn parse_template_body(lexer: &mut Lexer, db: &LibDb) -> Result<TemplateDef> {
    let mut tpl = TemplateDef::default();
    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => break,
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
            Token::Ident("index_1") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    let scale = match tpl.var1 {
                        TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
                        _ => db.time_unit_ns,
                    };
                    while let Some(tok_arg) = lexer.next_token() {
                        match tok_arg {
                            Token::RParen => break,
                            Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut tpl.index_1),
                            _ => {}
                        }
                    }
                }
            }
            Token::Ident("index_2") => {
                if let Some(Token::LParen) = lexer.next_token() {
                    let scale = match tpl.var2 {
                        TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
                        _ => db.time_unit_ns,
                    };
                    while let Some(tok_arg) = lexer.next_token() {
                        match tok_arg {
                            Token::RParen => break,
                            Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut tpl.index_2),
                            _ => {}
                        }
                    }
                }
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            _ => {}
        }
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

    // Track cell-local pins
    let mut local_pin_names: HashMap<SymbolId, LibPinId> = HashMap::new();

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => break,
            Token::Ident("area") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        area = s.parse::<f32>().unwrap_or(0.0);
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
                            let local_id = LibPinId((db.pins.len() as u32 - pin_offset) as u16);
                            e.insert(local_id);
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
                            let local_id = LibPinId((db.pins.len() as u32 - pin_offset) as u16);
                            e.insert(local_id);
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
                if let Some(Token::LParen) = lexer.next_token()
                    && let Some(Token::Ident(pin_name) | Token::Str(pin_name)) = lexer.next_token() {
                        let pin_sym = db.strings.intern(pin_name);
                        let _ = lexer.next_token(); // RParen
                        if let Some(Token::LBrace) = lexer.next_token() {
                            let local_id = LibPinId((db.pins.len() as u32 - pin_offset) as u16);
                            local_pin_names.insert(pin_sym, local_id);
                            parse_pin_body(
                                lexer,
                                db,
                                pin_sym,
                                local_id,
                                pin_offset,
                                &local_pin_names,
                                templates,
                            )?;
                        }
                    }
            }
            Token::Ident("test_cell")
            | Token::Ident("pg_pin")
            | Token::Ident("leakage_power")
            | Token::Ident("internal_power")
            | Token::Ident("dynamic_current") => {
                skip_statement_or_group(lexer);
            }
            Token::LBrace => {
                lexer.skip_balanced_braces();
            }
            _ => {}
        }
    }

    let pin_count = (db.pins.len() as u32 - pin_offset) as u16;
    let arc_count = (db.arcs.len() as u32 - arc_offset) as u16;

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

fn parse_pin_body(
    lexer: &mut Lexer,
    db: &mut LibDb,
    pin_name: SymbolId,
    target_pin_id: LibPinId,
    pin_offset: u32,
    local_pins: &HashMap<SymbolId, LibPinId>,
    templates: &HashMap<String, TemplateDef>,
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

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => break,
            Token::Ident("direction") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        dir = parse_pin_direction(s);
                    }
            }
            Token::Ident("capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap = s.parse::<f32>().unwrap_or(0.0) * db.cap_unit_pf;
                    }
            }
            Token::Ident("rise_capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap_rise = s.parse::<f32>().unwrap_or(0.0) * db.cap_unit_pf;
                        has_rise_cap = true;
                    }
            }
            Token::Ident("fall_capacitance") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        cap_fall = s.parse::<f32>().unwrap_or(0.0) * db.cap_unit_pf;
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
                        max_cap = Some(s.parse::<f32>().unwrap_or(0.0) * db.cap_unit_pf);
                    }
            }
            Token::Ident("max_transition") => {
                if let Some(Token::Colon) = lexer.next_token()
                    && let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
                        max_transition = Some(s.parse::<f32>().unwrap_or(0.0) * db.time_unit_ns);
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
            _ => {}
        }
    }

    if !has_rise_cap {
        cap_rise = cap;
    }
    if !has_fall_cap {
        cap_fall = cap;
    }

    db.pins.push(LibPin {
        name: pin_name,
        dir,
        cap_rise,
        cap_fall,
        cap_nom: cap,
        is_clock,
        max_cap,
        max_transition,
    });

    // Now resolve related_pin SymbolIds into LibPinIds and push arcs
    for (rel_sym, sense, ttype, rise, fall, r_trans, f_trans, r_const, f_const) in pin_arcs {
        let rel_pin_id = if let Some(&id) = local_pins.get(&rel_sym) {
            id
        } else {
            // If related pin was defined later or not yet seen, look for it in db.pins
            let mut found = None;
            for (idx, p) in db.pins[pin_offset as usize..].iter().enumerate() {
                if p.name == rel_sym {
                    found = Some(LibPinId(idx as u16));
                    break;
                }
            }
            found.unwrap_or(LibPinId(0))
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

    while let Some(tok) = lexer.next_token() {
        match tok {
            Token::RBrace => break,
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

    Ok(())
}

fn parse_nldm_table(
    lexer: &mut Lexer,
    db: &LibDb,
    templates: &HashMap<String, TemplateDef>,
) -> Result<Option<NldmTable>> {
    let mut tpl_name = String::new();
    if let Some(Token::LParen) = lexer.next_token() {
        if let Some(Token::Ident(s) | Token::Str(s)) = lexer.next_token() {
            tpl_name = s.to_string();
        }
        let _ = lexer.next_token(); // RParen
    }

    if let Some(Token::LBrace) = lexer.next_token() {
        let tpl_opt = templates.get(&tpl_name).cloned();
        let var1 = tpl_opt.as_ref().map(|t| t.var1).unwrap_or(TableAxisVar::Other);
        let var2 = tpl_opt.as_ref().map(|t| t.var2).unwrap_or(TableAxisVar::Other);

        let mut index_1 = Vec::new();
        let mut index_2 = Vec::new();
        let mut values = Vec::new();

        while let Some(tok) = lexer.next_token() {
            match tok {
                Token::RBrace => break,
                Token::Ident("index_1") => {
                    if let Some(Token::LParen) = lexer.next_token() {
                        let scale = match var1 {
                            TableAxisVar::TotalOutputNetCapacitance => db.cap_unit_pf,
                            _ => db.time_unit_ns,
                        };
                        while let Some(arg) = lexer.next_token() {
                            match arg {
                                Token::RParen => break,
                                Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut index_1),
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
                                Token::Str(s) | Token::Ident(s) => parse_float_list(s, scale, &mut index_2),
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
                                    parse_float_list(s, db.time_unit_ns, &mut values);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Token::LBrace => {
                    lexer.skip_balanced_braces();
                }
                _ => {}
            }
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

        // 1D table
        if index_2.is_empty() && !index_1.is_empty() {
            index_2 = vec![0.0];
        }

        if index_1.is_empty() && index_2.is_empty() && values.is_empty() {
            return Ok(None);
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

/// Resolves the cache path for a given Liberty file.
fn get_cache_paths(lib_path: &Path, xxh3_val: u64) -> (PathBuf, PathBuf) {
    // Primary path: next to the .lib file: <path>.grus.cache
    let primary = lib_path.with_extension("lib.grus.cache");

    // Fallback path: in system temp or target/grus-cache
    let tmp_dir = std::env::temp_dir().join("grus-eda-cache");
    let fallback = tmp_dir.join(format!("{:016x}.libcache", xxh3_val));

    (primary, fallback)
}

/// Loads a Liberty file from disk, using mmap binary cache if valid, or parsing and caching.
pub fn load_liberty(path: &Path) -> Result<LibDb> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open Liberty file at {:?}", path))?;
    let metadata = file.metadata()?;
    let source_size = metadata.len();

    // Memory map the file to compute hash and parse if needed
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    let source_xxh3 = xxh3_64(&mmap);

    let (primary_cache, fallback_cache) = get_cache_paths(path, source_xxh3);

    // Try loading from cache
    for cache_path in [&primary_cache, &fallback_cache] {
        if cache_path.is_file()
            && let Ok(cfile) = File::open(cache_path)
                && let Ok(c_meta) = cfile.metadata()
                    && c_meta.len() > 16
                        && let Ok(c_mmap) = unsafe { memmap2::Mmap::map(&cfile) } {
                            let config = bincode::config::standard();
                            if let Ok((cache_obj, _)) =
                                bincode::serde::decode_from_slice::<CacheFile, _>(&c_mmap, config)
                                && cache_obj.magic == CACHE_MAGIC
                                    && cache_obj.source_size == source_size
                                    && cache_obj.source_xxh3 == source_xxh3
                                {
                                    let mut db = cache_obj.db;
                                    db.strings.rebuild_map();
                                    db.rebuild_cell_by_name();
                                    return Ok(db);
                                }
                        }
    }

    // Cache miss: parse directly from mmap
    let mut db = parse_liberty_bytes(&mmap)?;

    // Serialize and write to cache
    let cache_obj = CacheFile {
        magic: CACHE_MAGIC,
        source_size,
        source_xxh3,
        db: db.clone(),
    };
    let config = bincode::config::standard();
    if let Ok(encoded) = bincode::serde::encode_to_vec(&cache_obj, config) {
        // Try writing to primary, fallback to fallback
        let write_cache = |target: &Path| -> Result<()> {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp_target = target.with_extension("tmp");
            let mut f = File::create(&tmp_target)?;
            f.write_all(&encoded)?;
            f.sync_all()?;
            std::fs::rename(tmp_target, target)?;
            Ok(())
        };

        if write_cache(&primary_cache).is_err() {
            let _ = write_cache(&fallback_cache);
        }
    }

    db.strings.rebuild_map();
    db.rebuild_cell_by_name();
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let default_path = dirs_home().join(".ciel/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib");
        let pdk_env = std::env::var("PDK_ROOT").ok().map(|p| PathBuf::from(p).join("sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let lib_path = pdk_env.filter(|p| p.is_file()).unwrap_or(default_path);

        if !lib_path.is_file() {
            eprintln!("Skipping sky130 integration test: .lib not found at {:?}", lib_path);
            return;
        }

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

        assert_eq!(db.library_name, "sky130_fd_sc_hd__tt_025C_1v80");
        assert_eq!(db.cells.len(), 428, "Expected 428 cells");
        assert_eq!(db.pins.len(), 1897, "Expected 1897 pins matching OpenSTA");
        assert_eq!(db_cached.cells.len(), 428);
        assert_eq!(db_cached.pins.len(), 1897);

        let ff_count = db.cells.iter().filter(|c| c.is_sequential).count();
        let latch_count = db.cells.iter().filter(|c| c.is_latch).count();
        let seq_count = ff_count + latch_count;
        assert_eq!(ff_count, 45, "Expected 45 flip-flop cells");
        assert_eq!(latch_count, 18, "Expected 18 latch cells");
        assert_eq!(seq_count, 63, "Expected 63 total sequential cells matching OpenSTA");

        // Warm cache load time must be < 10 ms in release mode
        if !cfg!(debug_assertions) {
            assert!(
                load_time_2.as_millis() < 10,
                "Warm cache load took too long: {:?}",
                load_time_2
            );
        } else {
            assert!(
                load_time_2.as_millis() < 150,
                "Warm cache load took too long in debug mode: {:?}",
                load_time_2
            );
        }

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
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
    }
}
