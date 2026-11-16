use crate::design_db::{DesignDb, GCellGridDef, Orient, PlacementRow, PlacementStatus, TrackGrid};
use crate::interner::{CellId, LibPinId, MacroPinId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum DefToken<'a> {
    Word(&'a str),
    Semi,
    LParen,
    RParen,
    Eof,
}

pub(crate) struct DefLexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> DefLexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0, line: 1 }
    }

    /// One-based line of the read position, for error messages. Counted incrementally:
    /// rescanning the prefix on every call made large DEFs quadratic.
    pub fn line(&self) -> usize {
        self.line
    }

    pub fn next_token(&mut self) -> DefToken<'a> {
        let bytes = self.input.as_bytes();
        let len = bytes.len();
        while self.pos < len {
            let b = bytes[self.pos];
            if b.is_ascii_whitespace() {
                if b == b'\n' {
                    self.line += 1;
                }
                self.pos += 1;
                continue;
            }
            if b == b'#' {
                self.pos += 1;
                while self.pos < len && bytes[self.pos] != b'\n' {
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
                let c = bytes[self.pos];
                if c.is_ascii_whitespace() || c == b';' || c == b'(' || c == b')' || c == b'#' {
                    break;
                }
                self.pos += 1;
            }
            // Every boundary is an ASCII delimiter or the end of input, so the slice is on char boundaries.
            return DefToken::Word(&self.input[start..self.pos]);
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

fn parse_orient(s: &str, line: usize) -> Result<Orient> {
    Ok(match s {
        "N" => Orient::N,
        "S" => Orient::S,
        "W" => Orient::W,
        "E" => Orient::E,
        "FN" => Orient::FN,
        "FS" => Orient::FS,
        "FW" => Orient::FW,
        "FE" => Orient::FE,
        _ => bail!("DEF line {line}: unknown orientation `{s}`"),
    })
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

/// Words of one `;`-terminated statement. Parentheses are dropped: they only punctuate
/// coordinates whose position is fixed by the statement shape.
fn read_statement<'a>(lexer: &mut DefLexer<'a>) -> Result<Vec<&'a str>> {
    let mut words = Vec::new();
    loop {
        match lexer.next_token() {
            DefToken::Semi => return Ok(words),
            DefToken::Word(w) => words.push(w),
            DefToken::LParen | DefToken::RParen => {}
            DefToken::Eof => bail!(
                "DEF line {}: statement is not terminated by `;` (truncated file?)",
                lexer.line()
            ),
        }
    }
}

/// Words inside a `( ... )` group whose `(` has already been consumed.
fn read_group<'a>(lexer: &mut DefLexer<'a>) -> Result<Vec<&'a str>> {
    let mut words = Vec::new();
    loop {
        match lexer.next_token() {
            DefToken::RParen => return Ok(words),
            DefToken::Word(w) => words.push(w),
            other => bail!("DEF line {}: unexpected {other:?} inside `( ... )`", lexer.line()),
        }
    }
}

fn number<T: FromStr>(word: &str, field: &str, line: usize) -> Result<T> {
    word.parse::<T>()
        .map_err(|_| anyhow!("DEF line {line}: {field} `{word}` is not a number"))
}

/// `X` is true, `Y` is false; the DEF grammar allows nothing else.
fn axis(word: &str, section: &str, line: usize) -> Result<bool> {
    match word {
        "X" => Ok(true),
        "Y" => Ok(false),
        _ => bail!("DEF line {line}: {section} axis must be X or Y, found `{word}`"),
    }
}

/// Splits `+ KEYWORD args...` attributes; the args of each keyword run up to the next `+`.
fn split_attributes<'w, 'a>(
    words: &'w [&'a str],
    line: usize,
) -> Result<Vec<(&'a str, &'w [&'a str])>> {
    let mut out = Vec::new();
    let mut rest = words;
    while let Some((&first, tail)) = rest.split_first() {
        if first != "+" {
            bail!("DEF line {line}: expected `+` before `{first}`");
        }
        let Some((&keyword, after)) = tail.split_first() else {
            bail!("DEF line {line}: `+` without an attribute keyword");
        };
        let end = after.iter().position(|&w| w == "+").unwrap_or(after.len());
        out.push((keyword, &after[..end]));
        rest = &after[end..];
    }
    Ok(out)
}

/// Consumes the name that follows an `END` and checks that it closes `section`.
fn end_section(lexer: &mut DefLexer, section: &str) -> Result<()> {
    match lexer.next_token() {
        DefToken::Word(name) if name == section => Ok(()),
        other => bail!(
            "DEF line {}: expected `END {section}`, found {other:?}",
            lexer.line()
        ),
    }
}

/// Reads the `SECTION <count> ;` header of a counted section.
fn section_count(lexer: &mut DefLexer, section: &str) -> Result<usize> {
    let words = read_statement(lexer)?;
    let line = lexer.line();
    match words.as_slice() {
        [count] => number(count, &format!("{section} count"), line),
        _ => bail!("DEF line {line}: {section} header must be `{section} <count> ;`"),
    }
}

/// Skips a section the database does not model, through its `END <section>`.
/// Every token is skipped by design, so there is no content to validate.
fn skip_section(lexer: &mut DefLexer, section: &str) -> Result<()> {
    loop {
        match lexer.next_token() {
            DefToken::Word("END") => return end_section(lexer, section),
            DefToken::Eof => bail!("DEF: {section} is not terminated by `END {section}` (truncated file?)"),
            _ => {}
        }
    }
}

/// Runs `entry` on each `- ...;` record of a counted section, through its `END <section>`,
/// and checks the record count against the header.
fn for_each_record<'a>(
    lexer: &mut DefLexer<'a>,
    section: &str,
    mut entry: impl FnMut(&[&'a str], usize) -> Result<()>,
) -> Result<()> {
    let declared = section_count(lexer, section)?;
    let mut found = 0;
    loop {
        match lexer.next_token() {
            DefToken::Word("-") => {
                let words = read_statement(lexer)?;
                entry(&words, lexer.line())?;
                found += 1;
            }
            DefToken::Word("END") => {
                end_section(lexer, section)?;
                break;
            }
            DefToken::Eof => bail!("DEF: {section} is not terminated by `END {section}` (truncated file?)"),
            other => bail!(
                "DEF line {}: unexpected {other:?} in {section}; records start with `-`",
                lexer.line()
            ),
        }
    }
    if declared != found {
        bail!(
            "DEF line {}: {section} header declares {declared} records but {found} were read (truncated or edited file?)",
            lexer.line()
        );
    }
    Ok(())
}

fn add_component(
    db: &mut DesignDb,
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
    cell_name_to_id: &mut HashMap<String, CellId>,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [inst, macro_name, attributes @ ..] = words else {
        bail!("DEF line {line}: component record needs an instance name and a macro name");
    };
    let inst_token = unescape_def_name(inst);

    let mut status = PlacementStatus::Unplaced;
    let mut x = 0;
    let mut y = 0;
    let mut orient = Orient::N;
    for (keyword, args) in split_attributes(attributes, line)? {
        match keyword {
            "PLACED" | "FIXED" | "COVER" => {
                let [px, py, o] = args else {
                    bail!("DEF line {line}: component {inst} `+ {keyword}` needs `( x y ) orient`");
                };
                x = number(px, "component x", line)?;
                y = number(py, "component y", line)?;
                orient = parse_orient(o, line)?;
                status = match keyword {
                    "PLACED" => PlacementStatus::Placed,
                    "FIXED" => PlacementStatus::Fixed,
                    _ => PlacementStatus::Cover,
                };
            }
            "UNPLACED" => {
                if !args.is_empty() {
                    bail!("DEF line {line}: component {inst} `+ UNPLACED` takes no arguments");
                }
                status = PlacementStatus::Unplaced;
            }
            // Attributes the database does not model (source, weights, halos, regions,
            // foreign cells, properties). Their arguments are skipped with them.
            "SOURCE" | "WEIGHT" | "HALO" | "ROUTEHALO" | "REGION" | "EEQMASTER" | "FOREIGN"
            | "PROPERTY" => {}
            other => bail!("DEF line {line}: unknown component attribute `+ {other}`"),
        }
    }

    let macro_id = tech_db
        .strings
        .get(macro_name)
        .and_then(|s| tech_db.macro_by_name.get(&s).copied())
        .with_context(|| {
            format!("DEF line {line}: component {inst} uses macro {macro_name}, which the LEF does not define")
        })?;
    let mac = &tech_db.macros[macro_id.0 as usize];

    let lib_cell_id = lib_db.and_then(|lib| {
        lib.strings
            .get(macro_name)
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
                    .map(|idx| LibPinId((lcell.pin_offset as usize + idx) as u16))
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
    Ok(())
}

fn add_pin(
    db: &mut DesignDb,
    io_pin_name_to_id: &mut HashMap<String, PinId>,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [name, attributes @ ..] = words else {
        bail!("DEF line {line}: PINS record without a pin name");
    };
    let pin_name = unescape_def_name(name);

    let mut dir = PinDir::Inout;
    let mut x = 0;
    let mut y = 0;
    let mut layer_off_x = 0;
    let mut layer_off_y = 0;
    let mut is_special = false;
    for (keyword, args) in split_attributes(attributes, line)? {
        match keyword {
            "SPECIAL" => is_special = true,
            "DIRECTION" => {
                dir = match args {
                    [d] => match *d {
                        "INPUT" => PinDir::Input,
                        "OUTPUT" => PinDir::Output,
                        "INOUT" => PinDir::Inout,
                        other => bail!("DEF line {line}: pin {pin_name} has unsupported DIRECTION `{other}`"),
                    },
                    _ => bail!("DEF line {line}: pin {pin_name} DIRECTION takes one value"),
                };
            }
            "LAYER" => {
                let [_, x1, y1, x2, y2] = args else {
                    bail!("DEF line {line}: pin {pin_name} `+ LAYER` needs `layer ( x1 y1 ) ( x2 y2 )`");
                };
                let x1: i32 = number(x1, "pin LAYER x1", line)?;
                let y1: i32 = number(y1, "pin LAYER y1", line)?;
                let x2: i32 = number(x2, "pin LAYER x2", line)?;
                let y2: i32 = number(y2, "pin LAYER y2", line)?;
                layer_off_x = (x1 + x2) / 2;
                layer_off_y = (y1 + y2) / 2;
            }
            "PLACED" | "FIXED" | "COVER" => {
                let [px, py, o] = args else {
                    bail!("DEF line {line}: pin {pin_name} `+ {keyword}` needs `( x y ) orient`");
                };
                x = number(px, "pin x", line)?;
                y = number(py, "pin y", line)?;
                // The pin orientation is validated but not stored: pin shapes come from the LEF.
                parse_orient(o, line)?;
            }
            // Attributes the database does not model: net name, expressions, sensitivity,
            // use (power/signal, derived from the LEF), and port-shape markers.
            "NET" | "NETEXPR" | "SUPPLYSENSITIVITY" | "GROUNDSENSITIVITY" | "USE" | "PORT" => {}
            other => bail!("DEF line {line}: unknown pin attribute `+ {other}`"),
        }
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
    Ok(())
}

fn add_special_net(
    db: &mut DesignDb,
    tech_db: &TechDb,
    words: &[&str],
    line: usize,
) -> Result<()> {
    // Wire segments: `ROUTED|NEW layer width ( x1 y1 ) ( x2 y2 )`, where `*` repeats the previous coordinate.
    let mut last_x = 0;
    let mut last_y = 0;
    let mut i = 0;
    while i < words.len() {
        if (words[i] == "ROUTED" || words[i] == "NEW") && i + 2 < words.len() {
            let lname = words[i + 1];
            let width: i32 = number(words[i + 2], "SPECIALNETS wire width", line)?;
            i += 3;

            if let Some(&lid) = tech_db
                .strings
                .get(lname)
                .and_then(|s| tech_db.layer_by_name.get(&s))
            {
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
                            let x1 = if words[i] == "*" { last_x } else { number(words[i], "SPECIALNETS x", line)? };
                            let y1 = if words[i + 1] == "*" { last_y } else { number(words[i + 1], "SPECIALNETS y", line)? };
                            let x2 = if words[i + 2] == "*" { x1 } else { number(words[i + 2], "SPECIALNETS x", line)? };
                            let y2 = if words[i + 3] == "*" { y1 } else { number(words[i + 3], "SPECIALNETS y", line)? };

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
                                    RectDbu::new(
                                        x1.min(x2) - half,
                                        y1.min(y2) - half,
                                        x1.max(x2) + half,
                                        y1.max(y2) + half,
                                    )
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
    Ok(())
}

pub fn parse_def_bytes(
    bytes: &[u8],
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let text = std::str::from_utf8(bytes).context("DEF is not valid UTF-8")?;
    let mut lexer = DefLexer::new(text);
    let mut db = DesignDb::new();
    db.strings = tech_db.strings.clone();
    db.dbu_per_micron = if tech_db.dbu_per_micron > 0 {
        tech_db.dbu_per_micron
    } else {
        1000
    };

    let mut cell_name_to_id: HashMap<String, CellId> = HashMap::new();
    let mut io_pin_name_to_id: HashMap<String, PinId> = HashMap::new();
    let mut saw_end_design = false;

    loop {
        let tok = lexer.next_token();
        match tok {
            DefToken::Eof => break,
            DefToken::Word("END") => {
                end_section(&mut lexer, "DESIGN")?;
                saw_end_design = true;
            }
            DefToken::Word("DESIGN") => match read_statement(&mut lexer)?.as_slice() {
                [name] => db.design_name = name.to_string(),
                _ => bail!("DEF line {}: DESIGN takes exactly one name", lexer.line()),
            },
            DefToken::Word("UNITS") => match read_statement(&mut lexer)?.as_slice() {
                ["DISTANCE", "MICRONS", dbu] => {
                    db.dbu_per_micron = number(dbu, "UNITS MICRONS", lexer.line())?;
                }
                _ => bail!("DEF line {}: UNITS must be `UNITS DISTANCE MICRONS <dbu> ;`", lexer.line()),
            },
            // Header statements. The database takes names as written and the technology from
            // the LEF, so these describe conventions it does not need.
            DefToken::Word("VERSION" | "DIVIDERCHAR" | "BUSBITCHARS" | "TECHNOLOGY") => {
                read_statement(&mut lexer)?;
            }
            DefToken::Word("DIEAREA") => {
                let words = read_statement(&mut lexer)?;
                let line = lexer.line();
                let coords = words
                    .iter()
                    .map(|w| number::<i32>(w, "DIEAREA coordinate", line))
                    .collect::<Result<Vec<i32>>>()?;
                match coords.as_slice() {
                    [x1, y1, x2, y2, ..] => db.die_area = RectDbu::new(*x1, *y1, *x2, *y2),
                    _ => bail!("DEF line {line}: DIEAREA needs two points"),
                }
            }
            DefToken::Word("ROW") => {
                // ROW name site orig_x orig_y orient DO num_x BY num_y STEP step_x step_y
                let words = read_statement(&mut lexer)?;
                let line = lexer.line();
                let [name, site, orig_x, orig_y, orient, "DO", num_x, "BY", num_y, "STEP", step_x, step_y] =
                    words.as_slice()
                else {
                    bail!(
                        "DEF line {line}: ROW must be `ROW name site x y orient DO n BY m STEP sx sy ;` ({} words given)",
                        words.len()
                    );
                };
                // num_y and step_y must be numbers but the database has no field for them.
                number::<i32>(num_y, "ROW BY count", line)?;
                number::<i32>(step_y, "ROW step y", line)?;
                db.rows.push(PlacementRow {
                    row_name: db.strings.intern(name),
                    site_name: db.strings.intern(site),
                    orig_x_dbu: number(orig_x, "ROW origin x", line)?,
                    orig_y_dbu: number(orig_y, "ROW origin y", line)?,
                    orient: parse_orient(orient, line)?,
                    num_x: number(num_x, "ROW count", line)?,
                    step_x_dbu: number(step_x, "ROW step x", line)?,
                });
            }
            DefToken::Word("TRACKS") => {
                // TRACKS X|Y start DO num STEP step [LAYER layer...]
                let words = read_statement(&mut lexer)?;
                let line = lexer.line();
                let (dir, start, num, step, layer_names): (&str, &str, &str, &str, &[&str]) =
                    match words.as_slice() {
                        [dir, start, "DO", num, "STEP", step, "LAYER", layers @ ..] => {
                            (dir, start, num, step, layers)
                        }
                        [dir, start, "DO", num, "STEP", step] => (dir, start, num, step, &[]),
                        _ => bail!("DEF line {line}: TRACKS must be `TRACKS X|Y start DO n STEP step [LAYER ...] ;`"),
                    };
                let mut layers = Vec::new();
                for lname in layer_names {
                    // Only layers the tech DB has as routing layers can carry tracks here.
                    if let Some(&lid) = tech_db
                        .strings
                        .get(lname)
                        .and_then(|s| tech_db.layer_by_name.get(&s))
                    {
                        layers.push(lid);
                    }
                }
                db.tracks.push(TrackGrid {
                    is_vertical_tracks: axis(dir, "TRACKS", line)?,
                    start_dbu: number(start, "TRACKS start", line)?,
                    num_tracks: number(num, "TRACKS count", line)?,
                    step_dbu: number(step, "TRACKS step", line)?,
                    layers,
                });
            }
            DefToken::Word("GCELLGRID") => {
                // GCELLGRID X|Y start DO num STEP step
                let words = read_statement(&mut lexer)?;
                let line = lexer.line();
                let [dir, start, "DO", num, "STEP", step] = words.as_slice() else {
                    bail!("DEF line {line}: GCELLGRID must be `GCELLGRID X|Y start DO n STEP step ;`");
                };
                db.gcell_grids.push(GCellGridDef {
                    is_x: axis(dir, "GCELLGRID", line)?,
                    start_dbu: number(start, "GCELLGRID start", line)?,
                    num_boundaries: number(num, "GCELLGRID count", line)?,
                    step_dbu: number(step, "GCELLGRID step", line)?,
                });
            }
            DefToken::Word("COMPONENTS") => {
                db.cell_pin_offsets.push(0);
                for_each_record(&mut lexer, "COMPONENTS", |words, line| {
                    add_component(&mut db, tech_db, lib_db, &mut cell_name_to_id, words, line)
                })?;
            }
            DefToken::Word("PINS") => {
                for_each_record(&mut lexer, "PINS", |words, line| {
                    add_pin(&mut db, &mut io_pin_name_to_id, words, line)
                })?;
            }
            DefToken::Word("SPECIALNETS") => {
                for_each_record(&mut lexer, "SPECIALNETS", |words, line| {
                    add_special_net(&mut db, tech_db, words, line)
                })?;
            }
            // Vias and non-default rules are not in the database: they come from the tech
            // LEF and only constrain routing, which the STA model does not read.
            DefToken::Word("VIAS") => skip_section(&mut lexer, "VIAS")?,
            DefToken::Word("NONDEFAULTRULES") => skip_section(&mut lexer, "NONDEFAULTRULES")?,
            DefToken::Word("NETS") => {
                let declared = section_count(&mut lexer, "NETS")?;
                let mut net_pins_list: Vec<Vec<PinId>> = Vec::new();
                let mut net_names_vec: Vec<String> = Vec::new();

                loop {
                    match lexer.next_token() {
                        DefToken::Word("-") => {
                            let net_name = match lexer.next_token() {
                                DefToken::Word(w) => unescape_def_name(w),
                                other => bail!(
                                    "DEF line {}: expected a net name after `-`, found {other:?}",
                                    lexer.line()
                                ),
                            };
                            let net_id = NetId(net_names_vec.len() as u32);
                            net_names_vec.push(net_name.clone());
                            let mut current_pins: Vec<PinId> = Vec::new();

                            // Connections `( comp pin )` come first; then `+` attributes and routed
                            // wiring follow. Those are not modeled, so they are skipped.
                            let mut in_attributes = false;
                            loop {
                                match lexer.next_token() {
                                    DefToken::Semi => break,
                                    DefToken::Word("+") => in_attributes = true,
                                    DefToken::Word(_) if in_attributes => {}
                                    DefToken::LParen if in_attributes => {
                                        read_group(&mut lexer)?;
                                    }
                                    DefToken::LParen => {
                                        let group = read_group(&mut lexer)?;
                                        let line = lexer.line();
                                        let [comp, pin] = group.as_slice() else {
                                            bail!("DEF line {line}: connection of net {net_name} must be `( component pin )`");
                                        };
                                        if *comp == "PIN" {
                                            let pname = unescape_def_name(pin);
                                            let &pid = io_pin_name_to_id.get(&pname).with_context(|| {
                                                format!("DEF line {line}: net {net_name} connects to PIN {pname}, which PINS does not define")
                                            })?;
                                            current_pins.push(pid);
                                            db.pin_nets[pid.0 as usize] = Some(net_id);
                                        } else {
                                            let iname = unescape_def_name(comp);
                                            let &cid = cell_name_to_id.get(&iname).with_context(|| {
                                                format!("DEF line {line}: net {net_name} connects to component {iname}, which COMPONENTS does not define")
                                            })?;
                                            let pid = db
                                                .cell_pins(cid)
                                                .iter()
                                                .copied()
                                                .find(|&pid| db.strings.resolve(db.pin_names[pid.0 as usize]) == *pin)
                                                .with_context(|| {
                                                    format!("DEF line {line}: net {net_name} connects to {iname}/{pin}, which the macro does not have")
                                                })?;
                                            current_pins.push(pid);
                                            db.pin_nets[pid.0 as usize] = Some(net_id);
                                        }
                                    }
                                    other => bail!(
                                        "DEF line {}: unexpected {other:?} in net {net_name}; connections must be `( component pin )`",
                                        lexer.line()
                                    ),
                                }
                            }

                            net_pins_list.push(current_pins);
                        }
                        DefToken::Word("END") => {
                            end_section(&mut lexer, "NETS")?;
                            break;
                        }
                        DefToken::Eof => bail!("DEF: NETS is not terminated by `END NETS` (truncated file?)"),
                        other => bail!(
                            "DEF line {}: unexpected {other:?} in NETS; records start with `-`",
                            lexer.line()
                        ),
                    }
                }
                if declared != net_names_vec.len() {
                    bail!(
                        "DEF line {}: NETS header declares {declared} nets but {} were read (truncated or edited file?)",
                        lexer.line(),
                        net_names_vec.len()
                    );
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
            DefToken::Word(keyword) => {
                bail!("DEF line {}: unknown section or statement `{keyword}`", lexer.line())
            }
            other => bail!("DEF line {}: unexpected {other:?} at top level", lexer.line()),
        }
    }

    if !saw_end_design {
        bail!("DEF: missing `END DESIGN` (truncated file?)");
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
        .with_context(|| format!("Failed to parse DEF file at {:?}", path))
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
    use crate::test_support::{design_dir, require, sky130_hd};
    use crate::interner::StringInterner;
    use crate::readers::lef::{load_tech_db, parse_lef_bytes};

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

    #[test]
    fn test_aes_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("aes").join("aes.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("aes").join("aes.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read aes.reference.json"))
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

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_aes_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path)
            .expect("Failed to write DEF template");
        let db_roundtrip = parse_def(&out_def_path, &tech_db, None)
            .expect("Failed to re-parse written DEF");
        assert_eq!(db_roundtrip.num_cells(), db.num_cells());
        assert_eq!(db_roundtrip.num_nets(), db.num_nets());
        assert_eq!(db_roundtrip.compute_hpwl_dbu(&tech_db), hpwl);
        let _ = std::fs::remove_file(out_def_path);
    }

    /// Every instance pin of aes with the cells cycled through all 8 orientations must
    /// land exactly where OpenROAD's `dbITerm::getAvgXY` puts it.
    #[test]
    fn test_aes_orientations_match_openroad_pins() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let aes_dir = design_dir("aes");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e));
        let db = parse_def(&require(aes_dir.join("aes.orient.def")), &tech_db, None)
            .expect("Failed to parse aes.orient.def");

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

        let golden = std::fs::read_to_string(require(aes_dir.join("aes.orient.pins.txt")))
            .expect("Failed to read aes.orient.pins.txt");
        let mut checked = 0;
        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let want = (f[1].parse::<i32>().unwrap(), f[2].parse::<i32>().unwrap());
            assert_eq!(ours.get(f[0]), Some(&want), "pin {}", f[0]);
            checked += 1;
        }
        assert_eq!(checked, ours.len(), "golden file must cover every signal instance pin");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(require(aes_dir.join("aes.reference.json"))).unwrap()).unwrap();
        assert_eq!(db.compute_hpwl_dbu(&tech_db), ref_json["orient_hpwl_dbu"].as_i64().unwrap());
    }

    #[test]
    fn test_jpeg_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("jpeg").join("jpeg.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("jpeg").join("jpeg.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read jpeg.reference.json"))
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

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_jpeg_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path)
            .expect("Failed to write DEF template");
        let db_roundtrip = parse_def(&out_def_path, &tech_db, None)
            .expect("Failed to re-parse written DEF");
        assert_eq!(db_roundtrip.num_cells(), db.num_cells());
        assert_eq!(db_roundtrip.num_nets(), db.num_nets());
        assert_eq!(db_roundtrip.compute_hpwl_dbu(&tech_db), hpwl);
        let _ = std::fs::remove_file(out_def_path);
    }

    /// Every instance pin of jpeg with the cells cycled through all 8 orientations must
    /// land exactly where OpenROAD's `dbITerm::getAvgXY` puts it.
    #[test]
    fn test_jpeg_orientations_match_openroad_pins() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let jpeg_dir = design_dir("jpeg");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e));
        let db = parse_def(&require(jpeg_dir.join("jpeg.orient.def")), &tech_db, None)
            .expect("Failed to parse jpeg.orient.def");

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

        let golden = std::fs::read_to_string(require(jpeg_dir.join("jpeg.orient.pins.txt")))
            .expect("Failed to read jpeg.orient.pins.txt");
        let mut checked = 0;
        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let want = (f[1].parse::<i32>().unwrap(), f[2].parse::<i32>().unwrap());
            assert_eq!(ours.get(f[0]), Some(&want), "pin {}", f[0]);
            checked += 1;
        }
        assert_eq!(checked, ours.len(), "golden file must cover every signal instance pin");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(require(jpeg_dir.join("jpeg.reference.json"))).unwrap()).unwrap();
        assert_eq!(db.compute_hpwl_dbu(&tech_db), ref_json["orient_hpwl_dbu"].as_i64().unwrap());
    }

    #[test]
    fn test_ibex_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("ibex").join("ibex.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("ibex").join("ibex.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read ibex.reference.json"))
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

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_ibex_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path)
            .expect("Failed to write DEF template");
        let db_roundtrip = parse_def(&out_def_path, &tech_db, None)
            .expect("Failed to re-parse written DEF");
        assert_eq!(db_roundtrip.num_cells(), db.num_cells());
        assert_eq!(db_roundtrip.num_nets(), db.num_nets());
        assert_eq!(db_roundtrip.compute_hpwl_dbu(&tech_db), hpwl);
        let _ = std::fs::remove_file(out_def_path);
    }

    /// Every instance pin of ibex with the cells cycled through all 8 orientations must
    /// land exactly where OpenROAD's `dbITerm::getAvgXY` puts it.
    #[test]
    fn test_ibex_orientations_match_openroad_pins() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let ibex_dir = design_dir("ibex");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e));
        let db = parse_def(&require(ibex_dir.join("ibex.orient.def")), &tech_db, None)
            .expect("Failed to parse ibex.orient.def");

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

        let golden = std::fs::read_to_string(require(ibex_dir.join("ibex.orient.pins.txt")))
            .expect("Failed to read ibex.orient.pins.txt");
        let mut checked = 0;
        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split_whitespace().collect();
            let want = (f[1].parse::<i32>().unwrap(), f[2].parse::<i32>().unwrap());
            assert_eq!(ours.get(f[0]), Some(&want), "pin {}", f[0]);
            checked += 1;
        }
        assert_eq!(checked, ours.len(), "golden file must cover every signal instance pin");

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(require(ibex_dir.join("ibex.reference.json"))).unwrap()).unwrap();
        assert_eq!(db.compute_hpwl_dbu(&tech_db), ref_json["orient_hpwl_dbu"].as_i64().unwrap());
    }

    const MINI_LEF: &str = r#"
VERSION 5.8 ;
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
SITE unithd
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
END unithd
LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  OFFSET 0.17 ;
  WIDTH 0.14 ;
END met1
MACRO INV
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
  PIN A
    DIRECTION INPUT ;
    PORT
      LAYER met1 ;
        RECT 0.10 0.10 0.20 0.20 ;
    END
  END A
  PIN Y
    DIRECTION OUTPUT ;
    PORT
      LAYER met1 ;
        RECT 0.30 0.10 0.40 0.20 ;
    END
  END Y
END INV
END LIBRARY
"#;

    const MINI_DEF: &str = "\
VERSION 5.8 ;
DIVIDERCHAR \"/\" ;
BUSBITCHARS \"[]\" ;
DESIGN top ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 4600 2720 ) ;
ROW ROW_0 unithd 0 0 N DO 10 BY 1 STEP 460 2720 ;
TRACKS X 170 DO 100 STEP 340 LAYER met1 ;
COMPONENTS 1 ;
- u1 INV + PLACED ( 460 0 ) N ;
END COMPONENTS
PINS 1 ;
- clk + NET clk + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 0 0 ) N ;
END PINS
NETS 1 ;
- n1 ( PIN clk ) ( u1 A ) + USE SIGNAL ;
END NETS
END DESIGN
";

    fn mini_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        parse_lef_bytes(MINI_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("mini LEF must parse");
        tech_db.strings = interner.clone();
        tech_db.rebuild_maps();
        tech_db
    }

    /// Error text for a DEF that must be rejected.
    fn mini_def_error(def: &str) -> String {
        match parse_def_bytes(def.as_bytes(), &mini_tech(), None) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("malformed DEF was accepted:\n{def}"),
        }
    }

    #[test]
    fn test_mini_def_parses_every_section() {
        let db = parse_def_bytes(MINI_DEF.as_bytes(), &mini_tech(), None).expect("mini DEF must parse");
        assert_eq!(db.design_name, "top");
        assert_eq!(db.die_area, RectDbu::new(0, 0, 4600, 2720));
        assert_eq!(db.num_rows(), 1);
        assert_eq!(db.tracks.len(), 1);
        assert_eq!(db.tracks[0].layers.len(), 1);
        assert_eq!(db.num_cells(), 1);
        assert_eq!(db.cell_x_dbu[0], 460);
        assert_eq!(db.io_pins.len(), 1);
        assert_eq!(db.num_nets(), 1);
        assert_eq!(db.net_pin_ids.len(), 2);
    }

    #[test]
    fn test_bad_number_in_placed_component_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("PLACED ( 460 0 )", "PLACED ( 4x6 0 )"));
        assert!(msg.contains("4x6") && msg.contains("line 10"), "{msg}");
    }

    #[test]
    fn test_bad_number_in_die_area_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("( 4600 2720 )", "( 4600 2o72 )"));
        assert!(msg.contains("2o72"), "{msg}");
    }

    #[test]
    fn test_truncated_row_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("ROW ROW_0 unithd 0 0 N DO 10 BY 1 STEP 460 2720 ;", "ROW ROW_0 unithd 0 0 N ;"));
        assert!(msg.contains("ROW must be"), "{msg}");
    }

    #[test]
    fn test_unknown_section_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("DESIGN top ;", "DESIGN top ;\nFOO 1 ;"));
        assert!(msg.contains("FOO"), "{msg}");
    }

    #[test]
    fn test_unknown_component_attribute_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("+ PLACED ( 460 0 ) N ;", "+ PLACED ( 460 0 ) N + BOGUS 1 ;"));
        assert!(msg.contains("BOGUS"), "{msg}");
    }

    #[test]
    fn test_missing_end_design_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("END DESIGN\n", ""));
        assert!(msg.contains("END DESIGN"), "{msg}");
    }

    #[test]
    fn test_component_count_mismatch_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("COMPONENTS 1 ;", "COMPONENTS 2 ;"));
        assert!(msg.contains("COMPONENTS header declares 2"), "{msg}");
    }

    #[test]
    fn test_unknown_macro_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("- u1 INV", "- u1 NAND9"));
        assert!(msg.contains("NAND9"), "{msg}");
    }

    #[test]
    fn test_net_to_unknown_component_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("( u1 A )", "( u9 A )"));
        assert!(msg.contains("u9"), "{msg}");
    }

    #[test]
    fn test_net_to_unknown_macro_pin_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("( u1 A )", "( u1 Q )"));
        assert!(msg.contains("u1/Q"), "{msg}");
    }

    /// Every truncation of the mini DEF must return `Ok` or `Err` without panicking, and only the
    /// full document (through `END DESIGN`) may parse, because the trailer is required.
    #[test]
    fn test_every_truncation_of_mini_def_is_rejected_or_parsed_without_panic() {
        let tech = mini_tech();
        let trailer_end = MINI_DEF.find("END DESIGN").expect("fixture has END DESIGN") + "END DESIGN".len();
        for n in 0..=MINI_DEF.len() {
            let result = parse_def_bytes(&MINI_DEF.as_bytes()[..n], &tech, None);
            if n < trailer_end {
                assert!(result.is_err(), "prefix of {n} bytes was accepted");
            }
        }
    }
}
