use crate::design_db::{DesignDb, GCellGridDef, Orient, PlacementRow, PlacementStatus, TrackGrid};
use crate::interner::{CellId, LibPinId, MacroPinId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum DefToken<'a> {
    Word(&'a str),
    Semi,
    LParen,
    RParen,
    Eof,
}

pub(crate) struct DefLexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> DefLexer<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub fn next_token(&mut self) -> DefToken<'a> {
        let len = self.input.len();
        while self.pos < len {
            let b = self.input[self.pos];
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'#' {
                self.pos += 1;
                while self.pos < len && self.input[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if b == b';' {
                self.pos += 1;
                return DefToken::Semi;
            }
            if b == b'(' {
                self.pos += 1;
                return DefToken::LParen;
            }
            if b == b')' {
                self.pos += 1;
                return DefToken::RParen;
            }

            let start = self.pos;
            while self.pos < len {
                let c = self.input[self.pos];
                if c.is_ascii_whitespace() || c == b';' || c == b'(' || c == b')' || c == b'#' {
                    break;
                }
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
            return DefToken::Word(s);
        }
        DefToken::Eof
    }
}

pub fn unescape_def_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse_orient(s: &str) -> Orient {
    match s {
        "N" => Orient::N,
        "S" => Orient::S,
        "W" => Orient::W,
        "E" => Orient::E,
        "FN" => Orient::FN,
        "FS" => Orient::FS,
        "FW" => Orient::FW,
        "FE" => Orient::FE,
        _ => Orient::N,
    }
}

pub fn orient_str(o: Orient) -> &'static str {
    match o {
        Orient::N => "N",
        Orient::S => "S",
        Orient::W => "W",
        Orient::E => "E",
        Orient::FN => "FN",
        Orient::FS => "FS",
        Orient::FW => "FW",
        Orient::FE => "FE",
    }
}

fn skip_to_semi(lexer: &mut DefLexer) {
    loop {
        let t = lexer.next_token();
        if t == DefToken::Semi || t == DefToken::Eof {
            break;
        }
    }
}

pub fn parse_def_bytes(
    bytes: &[u8],
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let mut lexer = DefLexer::new(bytes);
    let mut db = DesignDb::new();
    db.strings = tech_db.strings.clone();
    db.dbu_per_micron = if tech_db.dbu_per_micron > 0 {
        tech_db.dbu_per_micron
    } else {
        1000
    };

    let mut cell_name_to_id: HashMap<String, CellId> = HashMap::new();
    let mut io_pin_name_to_id: HashMap<String, PinId> = HashMap::new();

    loop {
        let tok = lexer.next_token();
        match tok {
            DefToken::Eof => break,
            DefToken::Word("END") => {
                let _sec = lexer.next_token();
            }
            DefToken::Word("DESIGN") => {
                if let DefToken::Word(name) = lexer.next_token() {
                    db.design_name = name.to_string();
                }
                skip_to_semi(&mut lexer);
            }
            DefToken::Word("UNITS") => {
                let mut dbu = 1000;
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Semi || t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word("MICRONS") = t
                        && let DefToken::Word(dstr) = lexer.next_token() {
                            dbu = dstr.parse::<i32>().unwrap_or(1000);
                        }
                }
                db.dbu_per_micron = dbu;
            }
            DefToken::Word("DIEAREA") => {
                let mut coords = Vec::new();
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Semi || t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word(w) = t
                        && let Ok(v) = w.parse::<i32>() {
                            coords.push(v);
                        }
                }
                if coords.len() >= 4 {
                    db.die_area = RectDbu::new(coords[0], coords[1], coords[2], coords[3]);
                }
            }
            DefToken::Word("ROW") => {
                // ROW row_name site_name orig_x orig_y orient DO num_x BY num_y STEP step_x step_y ;
                let mut row_name = String::new();
                let mut site_name = String::new();
                let mut orig_x = 0;
                let mut orig_y = 0;
                let mut orient = Orient::N;
                let mut num_x = 0;
                let mut step_x = 0;

                let mut words = Vec::new();
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Semi || t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word(w) = t {
                        words.push(w);
                    }
                }

                if words.len() >= 5 {
                    row_name = words[0].to_string();
                    site_name = words[1].to_string();
                    orig_x = words[2].parse().unwrap_or(0);
                    orig_y = words[3].parse().unwrap_or(0);
                    orient = parse_orient(words[4]);

                    let mut i = 5;
                    while i < words.len() {
                        if words[i] == "DO" && i + 1 < words.len() {
                            num_x = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else if words[i] == "STEP" && i + 1 < words.len() {
                            step_x = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }

                db.rows.push(PlacementRow {
                    row_name: db.strings.intern(&row_name),
                    site_name: db.strings.intern(&site_name),
                    orig_x_dbu: orig_x,
                    orig_y_dbu: orig_y,
                    orient,
                    num_x,
                    step_x_dbu: step_x,
                });
            }
            DefToken::Word("TRACKS") => {
                // TRACKS ( X | Y ) start DO num STEP step LAYER layers... ;
                let mut is_vertical = true;
                let mut start = 0;
                let mut num_tracks = 0;
                let mut step = 0;
                let mut layers = Vec::new();

                let mut words = Vec::new();
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Semi || t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word(w) = t {
                        words.push(w);
                    }
                }

                if words.len() >= 6 {
                    is_vertical = words[0] == "X";
                    start = words[1].parse().unwrap_or(0);
                    let mut i = 2;
                    while i < words.len() {
                        if words[i] == "DO" && i + 1 < words.len() {
                            num_tracks = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else if words[i] == "STEP" && i + 1 < words.len() {
                            step = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else if words[i] == "LAYER" {
                            for lname in &words[i + 1..] {
                                if let Some(&lid) = tech_db
                                    .strings
                                    .get(lname)
                                    .and_then(|s| tech_db.layer_by_name.get(&s))
                                {
                                    layers.push(lid);
                                }
                            }
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }

                db.tracks.push(TrackGrid {
                    is_vertical_tracks: is_vertical,
                    start_dbu: start,
                    num_tracks,
                    step_dbu: step,
                    layers,
                });
            }
            DefToken::Word("GCELLGRID") => {
                // GCELLGRID ( X | Y ) start DO num STEP step ;
                let mut is_x = true;
                let mut start = 0;
                let mut num = 0;
                let mut step = 0;

                let mut words = Vec::new();
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Semi || t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word(w) = t {
                        words.push(w);
                    }
                }

                if words.len() >= 6 {
                    is_x = words[0] == "X";
                    start = words[1].parse().unwrap_or(0);
                    let mut i = 2;
                    while i < words.len() {
                        if words[i] == "DO" && i + 1 < words.len() {
                            num = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else if words[i] == "STEP" && i + 1 < words.len() {
                            step = words[i + 1].parse().unwrap_or(0);
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }

                db.gcell_grids.push(GCellGridDef {
                    is_x,
                    start_dbu: start,
                    num_boundaries: num,
                    step_dbu: step,
                });
            }
            DefToken::Word("COMPONENTS") => {
                db.cell_pin_offsets.push(0);

                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word("END") = t {
                        let _sec = lexer.next_token();
                        break;
                    }
                    if let DefToken::Word("-") = t {
                        // - inst_name macro_name [attributes] ;
                        let inst_token = match lexer.next_token() {
                            DefToken::Word(w) => unescape_def_name(w),
                            _ => continue,
                        };
                        let macro_token = match lexer.next_token() {
                            DefToken::Word(w) => w.to_string(),
                            _ => continue,
                        };

                        let mut status = PlacementStatus::Unplaced;
                        let mut x = 0;
                        let mut y = 0;
                        let mut orient = Orient::N;

                        let mut comp_words = Vec::new();
                        loop {
                            let ct = lexer.next_token();
                            if ct == DefToken::Semi || ct == DefToken::Eof {
                                break;
                            }
                            if let DefToken::Word(w) = ct {
                                comp_words.push(w);
                            }
                        }

                        let mut i = 0;
                        while i < comp_words.len() {
                            match comp_words[i] {
                                "PLACED" => {
                                    status = PlacementStatus::Placed;
                                    if i + 3 < comp_words.len() {
                                        x = comp_words[i + 1].parse().unwrap_or(0);
                                        y = comp_words[i + 2].parse().unwrap_or(0);
                                        orient = parse_orient(comp_words[i + 3]);
                                        i += 4;
                                        continue;
                                    }
                                }
                                "FIXED" => {
                                    status = PlacementStatus::Fixed;
                                    if i + 3 < comp_words.len() {
                                        x = comp_words[i + 1].parse().unwrap_or(0);
                                        y = comp_words[i + 2].parse().unwrap_or(0);
                                        orient = parse_orient(comp_words[i + 3]);
                                        i += 4;
                                        continue;
                                    }
                                }
                                "COVER" => {
                                    status = PlacementStatus::Cover;
                                    if i + 3 < comp_words.len() {
                                        x = comp_words[i + 1].parse().unwrap_or(0);
                                        y = comp_words[i + 2].parse().unwrap_or(0);
                                        orient = parse_orient(comp_words[i + 3]);
                                        i += 4;
                                        continue;
                                    }
                                }
                                "UNPLACED" => {
                                    status = PlacementStatus::Unplaced;
                                }
                                _ => {}
                            }
                            i += 1;
                        }

                        let macro_id = match tech_db
                            .strings
                            .get(&macro_token)
                            .and_then(|s| tech_db.macro_by_name.get(&s).copied())
                        {
                            Some(mid) => mid,
                            None => continue,
                        };
                        let mac = &tech_db.macros[macro_id.0 as usize];

                        let lib_cell_id = lib_db.and_then(|lib| {
                            lib.strings
                                .get(&macro_token)
                                .and_then(|s| lib.cell_by_name.get(&s).copied())
                        });

                        let cell_id = CellId(db.cell_names.len() as u32);
                        cell_name_to_id.insert(inst_token.clone(), cell_id);
                        db.cell_names.push(db.strings.intern(&inst_token));
                        db.cell_macros.push(macro_id);
                        db.cell_lib_ids.push(lib_cell_id);
                        db.cell_status.push(status);
                        db.cell_x_dbu.push(x);
                        db.cell_y_dbu.push(y);
                        db.cell_orient.push(orient);

                        let mac_pins = tech_db.macro_pins(mac);
                        for (pin_idx, mpin) in mac_pins.iter().enumerate() {
                            let pin_name = tech_db.strings.resolve(mpin.name);
                            let pin_id = PinId(db.pin_names.len() as u32);
                            let is_driver = matches!(mpin.dir, PinDir::Output);

                            let lib_pin_id = lib_db.and_then(|lib| {
                                if let Some(lcid) = lib_cell_id {
                                    let lcell = &lib.cells[lcid.0 as usize];
                                    let lpins = lib.cell_pins(lcell);
                                    lpins
                                        .iter()
                                        .position(|lp| lib.strings.resolve(lp.name) == pin_name)
                                        .map(|idx| {
                                            LibPinId((lcell.pin_offset as usize + idx) as u16)
                                        })
                                } else {
                                    None
                                }
                            });

                            db.pin_names.push(mpin.name);
                            db.pin_dirs.push(mpin.dir);
                            db.pin_cells.push(Some(cell_id));
                            db.pin_macro_pins
                                .push(Some(MacroPinId(mac.pin_offset + pin_idx as u32)));
                            db.pin_lib_pins.push(lib_pin_id);
                            db.pin_nets.push(None); // linked during NETS
                            db.pin_offset_x_dbu.push(mpin.offset_x_dbu);
                            db.pin_offset_y_dbu.push(mpin.offset_y_dbu);
                            db.pin_is_driver.push(is_driver);
                            db.cell_pin_ids.push(pin_id);
                        }

                        db.cell_pin_offsets.push(db.cell_pin_ids.len() as u32);
                    }
                }
            }
            DefToken::Word("PINS") => {
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word("END") = t {
                        let _sec = lexer.next_token();
                        break;
                    }
                    if let DefToken::Word("-") = t {
                        let pin_name = match lexer.next_token() {
                            DefToken::Word(w) => unescape_def_name(w),
                            _ => continue,
                        };

                        let mut dir = PinDir::Inout;
                        let mut x = 0;
                        let mut y = 0;
                        let mut layer_off_x = 0;
                        let mut layer_off_y = 0;
                        let mut is_special = false;

                        let mut pin_words = Vec::new();
                        loop {
                            let pt = lexer.next_token();
                            if pt == DefToken::Semi || pt == DefToken::Eof {
                                break;
                            }
                            if let DefToken::Word(w) = pt {
                                pin_words.push(w);
                            }
                        }

                        let mut i = 0;
                        while i < pin_words.len() {
                            match pin_words[i] {
                                "SPECIAL" => {
                                    is_special = true;
                                }
                                "DIRECTION" if i + 1 < pin_words.len() => {
                                    dir = match pin_words[i + 1] {
                                        "INPUT" => PinDir::Input,
                                        "OUTPUT" => PinDir::Output,
                                        _ => PinDir::Inout,
                                    };
                                    i += 1;
                                }
                                "LAYER" if i + 5 < pin_words.len() => {
                                    let x1: i32 = pin_words[i + 2].parse().unwrap_or(0);
                                    let y1: i32 = pin_words[i + 3].parse().unwrap_or(0);
                                    let x2: i32 = pin_words[i + 4].parse().unwrap_or(0);
                                    let y2: i32 = pin_words[i + 5].parse().unwrap_or(0);
                                    layer_off_x = (x1 + x2) / 2;
                                    layer_off_y = (y1 + y2) / 2;
                                    i += 5;
                                }
                                "PLACED" | "FIXED" | "COVER" if i + 2 < pin_words.len() => {
                                    x = pin_words[i + 1].parse().unwrap_or(0);
                                    y = pin_words[i + 2].parse().unwrap_or(0);
                                    i += 2;
                                }
                                _ => {}
                            }
                            i += 1;
                        }

                        let sym = db.strings.intern(&pin_name);
                        let pin_id = PinId(db.pin_names.len() as u32);
                        let is_driver = matches!(dir, PinDir::Input | PinDir::Inout);

                        db.pin_names.push(sym);
                        db.pin_dirs.push(dir);
                        db.pin_cells.push(None);
                        db.pin_macro_pins.push(None);
                        db.pin_lib_pins.push(None);
                        db.pin_nets.push(None);
                        db.pin_offset_x_dbu.push(x + layer_off_x);
                        db.pin_offset_y_dbu.push(y + layer_off_y);
                        db.pin_is_driver.push(is_driver);

                        if !is_special {
                            io_pin_name_to_id.insert(pin_name, pin_id);
                            db.io_pins.push(pin_id);
                        }
                    }
                }
            }
            DefToken::Word("SPECIALNETS") => {
                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word("END") = t {
                        let _sec = lexer.next_token();
                        break;
                    }
                    if let DefToken::Word("-") = t {
                        // Parse special net routed segments until semicolon
                        let mut words = Vec::new();
                        loop {
                            let st = lexer.next_token();
                            if st == DefToken::Semi || st == DefToken::Eof {
                                break;
                            }
                            if let DefToken::Word(w) = st {
                                words.push(w);
                            }
                        }

                        let mut last_x = 0;
                        let mut last_y = 0;
                        let mut i = 0;
                        while i < words.len() {
                            if (words[i] == "ROUTED" || words[i] == "NEW") && i + 2 < words.len() {
                                let lname = words[i + 1];
                                let width: i32 = words[i + 2].parse().unwrap_or(0);
                                i += 3;

                                if let Some(&lid) = tech_db
                                    .strings
                                    .get(lname)
                                    .and_then(|s| tech_db.layer_by_name.get(&s))
                                {
                                    // Look for segment coordinates `( x1 y1 ) ( x2 y2 )`
                                    while i < words.len() && words[i] != "NEW" && words[i] != "ROUTED" {
                                        if words[i] == "+" && i + 2 < words.len() && words[i + 1] == "SHAPE" {
                                            i += 3;
                                            continue;
                                        }
                                        if words[i] == "+SHAPE" || words[i] == "SHAPE" {
                                            i += 2;
                                            continue;
                                        }
                                        if i + 3 < words.len() {
                                            let is_c1 = words[i] == "*" || words[i].parse::<i32>().is_ok();
                                            let is_c2 = words[i + 1] == "*" || words[i + 1].parse::<i32>().is_ok();
                                            let is_c3 = words[i + 2] == "*" || words[i + 2].parse::<i32>().is_ok();
                                            let is_c4 = words[i + 3] == "*" || words[i + 3].parse::<i32>().is_ok();

                                            if is_c1 && is_c2 && is_c3 && is_c4 {
                                                let x1 = if words[i] == "*" { last_x } else { words[i].parse().unwrap_or(last_x) };
                                                let y1 = if words[i + 1] == "*" { last_y } else { words[i + 1].parse().unwrap_or(last_y) };
                                                let x2 = if words[i + 2] == "*" { x1 } else { words[i + 2].parse().unwrap_or(x1) };
                                                let y2 = if words[i + 3] == "*" { y1 } else { words[i + 3].parse().unwrap_or(y1) };

                                                last_x = x2;
                                                last_y = y2;
                                                i += 4;

                                                if width > 0 {
                                                    let half = width / 2;
                                                    let rect = if y1 == y2 {
                                                        RectDbu::new(x1.min(x2), y1 - half, x1.max(x2), y1 + half)
                                                    } else if x1 == x2 {
                                                        RectDbu::new(x1 - half, y1.min(y2), x1 + half, y1.max(y2))
                                                    } else {
                                                        RectDbu::new(x1.min(x2) - half, y1.min(y2) - half, x1.max(x2) + half, y1.max(y2) + half)
                                                    };
                                                    db.special_wire_rects.push((lid, rect));
                                                }
                                                continue;
                                            }
                                        }
                                        i += 1;
                                    }
                                    continue;
                                }
                            }
                            i += 1;
                        }
                    }
                }
            }
            DefToken::Word("NETS") => {
                let mut net_pins_list: Vec<Vec<PinId>> = Vec::new();
                let mut net_names_vec: Vec<String> = Vec::new();

                loop {
                    let t = lexer.next_token();
                    if t == DefToken::Eof {
                        break;
                    }
                    if let DefToken::Word("END") = t {
                        let _sec = lexer.next_token();
                        break;
                    }
                    if let DefToken::Word("-") = t {
                        let net_token = match lexer.next_token() {
                            DefToken::Word(w) => unescape_def_name(w),
                            _ => continue,
                        };

                        let net_id = NetId(net_names_vec.len() as u32);
                        net_names_vec.push(net_token);
                        let mut current_pins: Vec<PinId> = Vec::new();

                        // Parse connection pairs `( ... )` until `;`
                        loop {
                            let nt = lexer.next_token();
                            if nt == DefToken::Semi || nt == DefToken::Eof {
                                break;
                            }
                            if nt == DefToken::LParen {
                                let mut conn_words = Vec::new();
                                loop {
                                    let ct = lexer.next_token();
                                    if ct == DefToken::RParen || ct == DefToken::Eof {
                                        break;
                                    }
                                    if let DefToken::Word(w) = ct {
                                        conn_words.push(w);
                                    }
                                }

                                if conn_words.len() >= 2 {
                                    if conn_words[0] == "PIN" {
                                        let pname = unescape_def_name(conn_words[1]);
                                        if let Some(&pid) = io_pin_name_to_id.get(&pname) {
                                            current_pins.push(pid);
                                            db.pin_nets[pid.0 as usize] = Some(net_id);
                                        }
                                    } else {
                                        let iname = unescape_def_name(conn_words[0]);
                                        let mpin_name = conn_words[1];

                                        if let Some(&cid) = cell_name_to_id.get(&iname) {
                                            let cell_pins = db.cell_pins(cid);
                                            for &pid in cell_pins {
                                                let pname = db.strings.resolve(db.pin_names[pid.0 as usize]);
                                                if pname == mpin_name {
                                                    current_pins.push(pid);
                                                    db.pin_nets[pid.0 as usize] = Some(net_id);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        net_pins_list.push(current_pins);
                    }
                }

                // Build Net SoA & CSR
                let num_nets = net_names_vec.len();
                db.net_names.reserve(num_nets);
                db.net_pin_offsets.reserve(num_nets + 1);

                for name in &net_names_vec {
                    db.net_names.push(db.strings.intern(name));
                }

                for mut pins in net_pins_list {
                    let offset = db.net_pin_ids.len() as u32;
                    db.net_pin_offsets.push(offset);

                    // Invariant: driver pin placed first
                    if let Some(drv_pos) = pins.iter().position(|&p| db.pin_is_driver[p.0 as usize]) {
                        pins.swap(0, drv_pos);
                    }
                    db.net_pin_ids.extend(pins);
                }
                db.net_pin_offsets.push(db.net_pin_ids.len() as u32);
            }
            _ => {}
        }
    }

    Ok(db)
}

pub fn parse_def(
    path: &Path,
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open DEF file at {:?}", path))?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    parse_def_bytes(&contents, tech_db, lib_db)
}

pub fn write_def(
    db: &DesignDb,
    tech_db: &TechDb,
    template_def_path: &Path,
    out_def_path: &Path,
) -> Result<()> {
    let mut file = File::open(template_def_path)
        .with_context(|| format!("Failed to open template DEF at {:?}", template_def_path))?;
    let mut template = String::new();
    file.read_to_string(&mut template)?;

    let comp_start_idx = template
        .find("COMPONENTS ")
        .context("COMPONENTS section not found in template DEF")?;
    let comp_end_idx = template[comp_start_idx..]
        .find("END COMPONENTS")
        .context("END COMPONENTS not found in template DEF")?
        + comp_start_idx;

    // Advance to after `END COMPONENTS\n`
    let after_end_components = match template[comp_end_idx..].find('\n') {
        Some(pos) => comp_end_idx + pos + 1,
        None => template.len(),
    };

    let before = &template[..comp_start_idx];
    let after = &template[after_end_components..];

    let mut out_str = String::with_capacity(template.len() + 1024);
    out_str.push_str(before);
    out_str.push_str(&format!("COMPONENTS {} ;\n", db.num_cells()));

    for c in 0..db.num_cells() {
        let name = db.strings.resolve(db.cell_names[c]);
        let macro_id = db.cell_macros[c];
        let macro_name = tech_db.strings.resolve(tech_db.macros[macro_id.0 as usize].name);

        let status = match db.cell_status[c] {
            PlacementStatus::Placed => "PLACED",
            PlacementStatus::Fixed => "FIXED",
            PlacementStatus::Cover => "COVER",
            PlacementStatus::Unplaced => "UNPLACED",
        };
        let orient = orient_str(db.cell_orient[c]);
        let x = db.cell_x_dbu[c];
        let y = db.cell_y_dbu[c];

        if db.cell_status[c] == PlacementStatus::Unplaced {
            out_str.push_str(&format!("    - {} {} + UNPLACED ;\n", name, macro_name));
        } else {
            out_str.push_str(&format!(
                "    - {} {} + {} ( {} {} ) {} ;\n",
                name, macro_name, status, x, y, orient
            ));
        }
    }

    out_str.push_str("END COMPONENTS\n");
    out_str.push_str(after);

    let mut out_file = File::create(out_def_path)
        .with_context(|| format!("Failed to create output DEF at {:?}", out_def_path))?;
    out_file.write_all(out_str.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, sky130_hd};
    use crate::interner::StringInterner;
    use crate::readers::lef::load_tech_db;

    #[test]
    fn test_gcd_placed_def() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let def_path = design_dir("gcd").join("gcd.placed.def");

        if !tlef_path.is_file() {
            panic!("PDK tlef file missing at {:?}", tlef_path);
        }
        if !lef_path.is_file() {
            panic!("PDK lef file missing at {:?}", lef_path);
        }
        if !def_path.is_file() {
            panic!("Design DEF file missing at {:?}", def_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(db.num_cells(), ref_json["instances"].as_u64().unwrap() as usize);
        assert_eq!(db.num_rows(), ref_json["rows"].as_u64().unwrap() as usize);
        assert_eq!(db.tracks.len(), ref_json["def_tracks_statements"].as_u64().unwrap() as usize);
        assert_eq!(db.num_nets(), ref_json["signal_nets"].as_u64().unwrap() as usize);
        assert_eq!(db.io_pins.len(), ref_json["bterms_signal"].as_u64().unwrap() as usize);
        assert_eq!(db.cell_pin_ids.len(), ref_json["def_inst_pins"].as_u64().unwrap() as usize);
        assert_eq!(db.net_pin_ids.len(), ref_json["connected_net_pins"].as_u64().unwrap() as usize);

        let hpwl = db.compute_hpwl_dbu(&tech_db);
        assert_eq!(hpwl, ref_json["placed_hpwl_dbu"].as_i64().unwrap(), "HPWL must equal OpenROAD getTermBBox sum");

        // Verify write_def roundtrip and re-parse
        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_gcd_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path)
            .expect("Failed to write DEF template");
        let db_roundtrip = parse_def(&out_def_path, &tech_db, None)
            .expect("Failed to re-parse written DEF");
        assert_eq!(db_roundtrip.num_cells(), db.num_cells());
        assert_eq!(db_roundtrip.num_nets(), db.num_nets());
        assert_eq!(db_roundtrip.compute_hpwl_dbu(&tech_db), hpwl);
        let _ = std::fs::remove_file(out_def_path);
    }

    /// Every instance pin of gcd with the cells cycled through all 8 orientations must
    /// land exactly where OpenROAD's `dbITerm::getAvgXY` puts it.
    #[test]
    fn test_gcd_orientations_match_openroad_pins() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let gcd_dir = design_dir("gcd");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e));
        let db = parse_def(&gcd_dir.join("gcd.orient.def"), &tech_db, None)
            .expect("Failed to parse gcd.orient.def");

        let mut ours = std::collections::HashMap::new();
        for &pin in &db.net_pin_ids {
            let p = pin.0 as usize;
            if let Some(cell) = db.pin_cells[p] {
                let name = format!(
                    "{}/{}",
                    db.strings.resolve(db.cell_names[cell.0 as usize]),
                    db.strings.resolve(db.pin_names[p])
                );
                ours.insert(name, db.pin_pos_dbu(pin, &tech_db));
            }
        }

        let golden = std::fs::read_to_string(gcd_dir.join("gcd.orient.pins.txt"))
            .expect("Failed to read gcd.orient.pins.txt");
        let mut checked = 0;
        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let want = (f[1].parse::<i32>().unwrap(), f[2].parse::<i32>().unwrap());
            assert_eq!(ours.get(f[0]), Some(&want), "pin {}", f[0]);
            checked += 1;
        }
        assert_eq!(checked, ours.len(), "golden file must cover every signal instance pin");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(gcd_dir.join("gcd.reference.json")).unwrap()).unwrap();
        assert_eq!(db.compute_hpwl_dbu(&tech_db), ref_json["orient_hpwl_dbu"].as_i64().unwrap());
    }
}
