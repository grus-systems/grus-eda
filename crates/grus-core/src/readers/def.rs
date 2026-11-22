use crate::design_db::{
    DesignDb, GCellGridDef, GeneratedViaGeom, NetGeom, Orient, PdnConn, PdnNet, PdnUse, PdnVia,
    PlacementRow, PlacementStatus, TrackGrid, ViaGeom,
};
use crate::interner::{
    CellId, LayerId, LibPinId, MacroPinId, NetId, PinDir, PinId, StringInterner, SymbolId,
};
use crate::lib_db::LibDb;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result, anyhow, bail};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::str::FromStr;
use xxhash_rust::xxh3::Xxh3DefaultBuilder;

type FastMap<K, V> = HashMap<K, V, Xxh3DefaultBuilder>;

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
        Self {
            input,
            pos: 0,
            line: 1,
        }
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
                if c == b'\\' && self.pos + 1 < len {
                    // An escaped character belongs to the name even when it
                    // is a delimiter; unescape_def_name strips the backslash
                    // once the full word is known.
                    self.pos += 2;
                    continue;
                }
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
    unescape_def_name_cow(s).into_owned()
}

/// Borrowed when the name has no `\` to strip — the common case on real DEFs.
fn unescape_def_name_cow(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') {
        return Cow::Borrowed(s);
    }
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
    Cow::Owned(out)
}

/// Inverse of `unescape_def_name`: prefixes characters that would delimit
/// a DEF word with `\`. Only the escape character, ASCII whitespace, `;`,
/// `(`, `)`, and `#` (a comment opener in this lexer) need escaping.
/// Everything else — including `[N]` bus-bit syntax and `$`/`/` — is written
/// literally so external DEF readers see the same names OpenROAD-flow-scripts
/// emits for synthesized netlists.
pub fn escape_def_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\\' || c.is_ascii_whitespace() || matches!(c, ';' | '(' | ')' | '#') {
            out.push('\\');
        }
        out.push(c);
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
    read_statement_into(lexer, &mut words, false)?;
    Ok(words)
}

/// `read_statement` into a caller-owned buffer, so counted sections can reuse
/// one allocation across every record. `keep_parens` emits `(`/`)` as words —
/// NETS needs them because routed-wire point groups are meaningful
/// (`( x y z )` carries an ignorable third coordinate that flat tokens
/// cannot be disambiguated from the next point's `x`).
fn read_statement_into<'a>(
    lexer: &mut DefLexer<'a>,
    words: &mut Vec<&'a str>,
    keep_parens: bool,
) -> Result<()> {
    loop {
        match lexer.next_token() {
            DefToken::Semi => return Ok(()),
            DefToken::Word(w) => words.push(w),
            DefToken::LParen if keep_parens => words.push("("),
            DefToken::RParen if keep_parens => words.push(")"),
            DefToken::LParen | DefToken::RParen => {}
            DefToken::Eof => bail!(
                "DEF line {}: statement is not terminated by `;` (truncated file?)",
                lexer.line()
            ),
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
/// Iterator form: one attribute per step, no `Vec`, same rejects as the old
/// collecting version.
fn split_attributes<'w, 'a>(words: &'w [&'a str], line: usize) -> AttrIter<'w, 'a> {
    AttrIter { rest: words, line }
}

struct AttrIter<'w, 'a> {
    rest: &'w [&'a str],
    line: usize,
}

impl<'w, 'a> Iterator for AttrIter<'w, 'a> {
    type Item = Result<(&'a str, &'w [&'a str])>;

    fn next(&mut self) -> Option<Self::Item> {
        let (&first, tail) = self.rest.split_first()?;
        if first != "+" {
            self.rest = &[];
            return Some(Err(anyhow!(
                "DEF line {}: expected `+` before `{first}`",
                self.line
            )));
        }
        let Some((&keyword, after)) = tail.split_first() else {
            self.rest = &[];
            return Some(Err(anyhow!(
                "DEF line {}: `+` without an attribute keyword",
                self.line
            )));
        };
        let end = after.iter().position(|&w| w == "+").unwrap_or(after.len());
        self.rest = &after[end..];
        Some(Ok((keyword, &after[..end])))
    }
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

/// Runs `entry` on each `- ...;` record of a counted section, through its `END <section>`,
/// and checks the record count against the header.
fn for_each_record<'a>(
    lexer: &mut DefLexer<'a>,
    section: &str,
    mut entry: impl FnMut(&[&'a str], usize) -> Result<()>,
    keep_parens: bool,
) -> Result<()> {
    let declared = section_count(lexer, section)?;
    let mut found = 0;
    // One scratch buffer for every `- ...;` record of the section.
    let mut words: Vec<&'a str> = Vec::new();
    loop {
        match lexer.next_token() {
            DefToken::Word("-") => {
                words.clear();
                read_statement_into(lexer, &mut words, keep_parens)?;
                entry(&words, lexer.line())?;
                found += 1;
            }
            DefToken::Word("END") => {
                end_section(lexer, section)?;
                break;
            }
            DefToken::Eof => {
                bail!("DEF: {section} is not terminated by `END {section}` (truncated file?)")
            }
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
    cell_name_to_id: &mut FastMap<String, CellId>,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [inst, macro_name, attributes @ ..] = words else {
        bail!("DEF line {line}: component record needs an instance name and a macro name");
    };
    let inst_token = unescape_def_name_cow(inst);

    let mut status = PlacementStatus::Unplaced;
    let mut x = 0;
    let mut y = 0;
    let mut orient = Orient::N;
    for attr in split_attributes(attributes, line) {
        let (keyword, args) = attr?;
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
    db.cell_names.push(db.strings.intern(&inst_token));
    cell_name_to_id.insert(inst_token.into_owned(), cell_id);
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
        db.pin_io_layer.push(None);
        db.pin_io_rects.push(Vec::new());
        db.pin_is_driver.push(is_driver);
        db.cell_pin_ids.push(pin_id);
    }

    db.cell_pin_offsets.push(db.cell_pin_ids.len() as u32);
    Ok(())
}

fn add_pin(
    db: &mut DesignDb,
    tech_db: &TechDb,
    io_pin_name_to_id: &mut FastMap<String, PinId>,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [name, attributes @ ..] = words else {
        bail!("DEF line {line}: PINS record without a pin name");
    };
    let pin_name = unescape_def_name_cow(name);

    let mut dir = PinDir::Inout;
    let mut x = 0;
    let mut y = 0;
    let mut layer = None;
    let mut layer_off_x = 0;
    let mut layer_off_y = 0;
    let mut io_rects = Vec::new();
    let mut is_special = false;
    for attr in split_attributes(attributes, line) {
        let (keyword, args) = attr?;
        match keyword {
            "SPECIAL" => is_special = true,
            "DIRECTION" => {
                dir = match args {
                    [d] => match *d {
                        "INPUT" => PinDir::Input,
                        "OUTPUT" => PinDir::Output,
                        "INOUT" => PinDir::Inout,
                        other => bail!(
                            "DEF line {line}: pin {pin_name} has unsupported DIRECTION `{other}`"
                        ),
                    },
                    _ => bail!("DEF line {line}: pin {pin_name} DIRECTION takes one value"),
                };
            }
            "LAYER" => {
                let [lname, x1, y1, x2, y2] = args else {
                    bail!(
                        "DEF line {line}: pin {pin_name} `+ LAYER` needs `layer ( x1 y1 ) ( x2 y2 )`"
                    );
                };
                layer = Some(
                    tech_db
                        .strings
                        .get(lname)
                        .and_then(|s| tech_db.layer_by_name.get(&s))
                        .copied()
                        .with_context(|| {
                            format!("DEF line {line}: pin {pin_name} `+ LAYER` uses layer {lname}, which the LEF does not define")
                        })?,
                );
                let x1: i32 = number(x1, "pin LAYER x1", line)?;
                let y1: i32 = number(y1, "pin LAYER y1", line)?;
                let x2: i32 = number(x2, "pin LAYER x2", line)?;
                let y2: i32 = number(y2, "pin LAYER y2", line)?;
                layer_off_x = (x1 + x2) / 2;
                layer_off_y = (y1 + y2) / 2;
                if let Some(l) = layer {
                    io_rects.push((
                        l,
                        RectDbu::new(x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)),
                    ));
                }
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
    db.pin_io_layer.push(layer);
    // DEF `+ LAYER` rects are relative to the `+ PLACED` point; store the
    // absolute shapes OpenDB keeps on the bterm.
    db.pin_io_rects.push(
        io_rects
            .into_iter()
            .map(|(l, r)| {
                (
                    l,
                    RectDbu::new(r.xl + x, r.yl + y, r.xh + x, r.yh + y),
                )
            })
            .collect(),
    );
    db.pin_is_driver.push(is_driver);

    if !is_special {
        io_pin_name_to_id.insert(pin_name.into_owned(), pin_id);
        db.io_pins.push(pin_id);
    }
    Ok(())
}

/// A DEF `VIAS` record resolved at parse time. Parse-local — SPECIALNETS via
/// names consult this map first, then `TechDb::via_by_name`.
enum DefVia {
    /// `+ VIARULE` + `+ LAYERS` triple, fully resolved.
    Generated {
        lower: LayerId,
        upper: LayerId,
        cut: SymbolId,
        cut_count: u32,
        geom: GeneratedViaGeom,
    },
    /// `+ VIA <lefViaName>` — the name interned in `db.strings`; resolved
    /// through `tech_db.via_by_name` when SPECIALNETS references it (a name
    /// absent from the LEF simply misses there).
    Fixed(SymbolId),
    /// `+ RECT`/`+ POLYGON` inline via — no resolvable layer stack. Tolerated
    /// when defined; errors only when SPECIALNETS references it.
    Inline,
}

type DefViaMap = FastMap<String, DefVia>;

fn add_def_via(
    db: &mut DesignDb,
    tech_db: &TechDb,
    words: &[&str],
    line: usize,
) -> Result<(String, DefVia)> {
    let [name, attributes @ ..] = words else {
        bail!("DEF line {line}: VIAS record without a via name");
    };
    let via_name = unescape_def_name_cow(name).into_owned();

    let mut viarule = false;
    let mut layers: Option<(LayerId, SymbolId, LayerId)> = None;
    let mut cut_size: Option<(i32, i32)> = None;
    let mut cut_spacing = (0i32, 0i32);
    let mut enclosure = (0i32, 0i32, 0i32, 0i32);
    let mut rowcol = (1u32, 1u32);
    let mut fixed: Option<SymbolId> = None;
    let mut inline = false;
    for attr in split_attributes(attributes, line) {
        let (keyword, args) = attr?;
        match keyword {
            // The rule name is kept only for diagnostics: `+ LAYERS` is
            // authoritative for the stack.
            "VIARULE" => {
                viarule = true;
                if args.len() != 1 {
                    bail!("DEF line {line}: VIAS via {via_name} `+ VIARULE` takes one rule name");
                }
            }
            "LAYERS" => {
                let [bot, cut, top] = args else {
                    bail!(
                        "DEF line {line}: VIAS via {via_name} `+ LAYERS` needs `<bottom> <cut> <top>`"
                    );
                };
                let routing = |lname: &str| -> Result<LayerId> {
                    tech_db
                        .strings
                        .get(lname)
                        .and_then(|s| tech_db.layer_by_name.get(&s))
                        .copied()
                        .with_context(|| {
                            format!("DEF line {line}: VIAS via {via_name} `+ LAYERS` uses layer {lname}, which the LEF does not define as routing")
                        })
                };
                let cut_sym = tech_db.strings.get(cut).with_context(|| {
                    format!("DEF line {line}: VIAS via {via_name} `+ LAYERS` cut layer {cut} is not in the LEF")
                })?;
                if !tech_db.cut_layer_by_name.contains_key(&cut_sym) {
                    bail!(
                        "DEF line {line}: VIAS via {via_name} `+ LAYERS` middle layer {cut} is not a CUT layer"
                    );
                }
                layers = Some((routing(bot)?, cut_sym, routing(top)?));
            }
            "ROWCOL" => {
                let [r, c] = args else {
                    bail!("DEF line {line}: VIAS via {via_name} `+ ROWCOL` needs `<rows> <cols>`");
                };
                let r: u32 = number(r, "VIAS ROWCOL rows", line)?;
                let c: u32 = number(c, "VIAS ROWCOL cols", line)?;
                rowcol = (r, c);
            }
            "CUTSIZE" => {
                let [x, y] = args else {
                    bail!("DEF line {line}: VIAS via {via_name} `+ CUTSIZE` needs two numbers");
                };
                cut_size = Some((
                    number::<i32>(x, "VIAS CUTSIZE x", line)?,
                    number::<i32>(y, "VIAS CUTSIZE y", line)?,
                ));
            }
            "CUTSPACING" => {
                let [x, y] = args else {
                    bail!("DEF line {line}: VIAS via {via_name} `+ CUTSPACING` needs two numbers");
                };
                cut_spacing = (
                    number::<i32>(x, "VIAS CUTSPACING x", line)?,
                    number::<i32>(y, "VIAS CUTSPACING y", line)?,
                );
            }
            "ENCLOSURE" => {
                let [a, b, c, d] = args else {
                    bail!("DEF line {line}: VIAS via {via_name} `+ ENCLOSURE` needs four numbers");
                };
                // OpenDB semantics: `(botX, botY, topX, topY)` symmetric
                // overhangs on the cut array.
                enclosure = (
                    number::<i32>(a, "VIAS ENCLOSURE", line)?,
                    number::<i32>(b, "VIAS ENCLOSURE", line)?,
                    number::<i32>(c, "VIAS ENCLOSURE", line)?,
                    number::<i32>(d, "VIAS ENCLOSURE", line)?,
                );
            }
            "VIA" => {
                let [lef_name] = args else {
                    bail!("DEF line {line}: VIAS via {via_name} `+ VIA` takes one LEF via name");
                };
                fixed = Some(db.strings.intern(&unescape_def_name_cow(lef_name)));
            }
            // Inline-via geometry and attributes the model does not read.
            "RECT" | "POLYGON" => inline = true,
            "PATTERN" | "MASK" | "FILL_SHAPE" | "STYLE" | "PROPERTY" => {}
            other => bail!("DEF line {line}: unknown VIAS attribute `+ {other}`"),
        }
    }

    let via = if let Some((lower, cut, upper)) = layers {
        if !viarule {
            bail!("DEF line {line}: VIAS via {via_name} has `+ LAYERS` without `+ VIARULE`");
        }
        // Cut geometry cannot be derived from `+ VIARULE` alone — the
        // model does not read via rules — so `+ CUTSIZE` is required.
        let Some((cut_w, cut_h)) = cut_size else {
            bail!("DEF line {line}: VIAS via {via_name} `+ VIARULE` generated via has no `+ CUTSIZE`");
        };
        let (lo, hi) = if lower.0 <= upper.0 {
            (lower, upper)
        } else {
            (upper, lower)
        };
        DefVia::Generated {
            lower: lo,
            upper: hi,
            cut,
            cut_count: rowcol.0 * rowcol.1,
            geom: GeneratedViaGeom {
                cut_w_dbu: cut_w,
                cut_h_dbu: cut_h,
                rows: rowcol.0,
                cols: rowcol.1,
                cut_spacing_x_dbu: cut_spacing.0,
                cut_spacing_y_dbu: cut_spacing.1,
                enc_bot_x_dbu: enclosure.0,
                enc_bot_y_dbu: enclosure.1,
                enc_top_x_dbu: enclosure.2,
                enc_top_y_dbu: enclosure.3,
            },
        }
    } else if viarule {
        // The grammar requires `+ LAYERS` on a generated via.
        bail!("DEF line {line}: VIAS via {via_name} has `+ VIARULE` but no `+ LAYERS`");
    } else if let Some(sym) = fixed {
        DefVia::Fixed(sym)
    } else if inline {
        DefVia::Inline
    } else {
        bail!("DEF line {line}: VIAS via {via_name} has no via definition");
    };
    Ok((via_name, via))
}

/// Resolves a DEF via name to `(lower, upper, cut, cut_count, geom)`:
/// the DEF `VIAS` map first, then LEF `VIA` definitions. Name-pattern
/// heuristics are never consulted — a renamed via resolves by its declared
/// layer triple. Shared by SPECIALNETS and NETS wire statements.
fn resolve_def_via(
    tech_db: &TechDb,
    def_vias: &DefViaMap,
    name: &str,
    net_name: &str,
    line: usize,
) -> Result<(LayerId, LayerId, SymbolId, u32, ViaGeom)> {
    let from_lef = |sym: SymbolId| -> Option<(LayerId, LayerId, SymbolId, u32, ViaGeom)> {
        tech_db.via_by_name.get(&sym).map(|&i| {
            let vd = &tech_db.via_defs[i as usize];
            (vd.lower, vd.upper, vd.cut, vd.cut_count, ViaGeom::Lef(i))
        })
    };
    match def_vias.get(name) {
        Some(DefVia::Generated {
            lower,
            upper,
            cut,
            cut_count,
            geom,
        }) => Ok((
            *lower,
            *upper,
            *cut,
            *cut_count,
            ViaGeom::Generated(geom.clone()),
        )),
        Some(DefVia::Inline) => bail!(
            "DEF line {line}: net {net_name} uses via {name}, which defines no resolvable layer stack"
        ),
        Some(DefVia::Fixed(lef_sym)) => from_lef(*lef_sym).with_context(|| {
            format!("DEF line {line}: net {net_name} uses via {name}, which the LEF does not define")
        }),
        None => {
            let sym = tech_db.strings.get(name);
            sym.and_then(from_lef).with_context(|| {
                format!("DEF line {line}: net {net_name} uses via {name}, which neither the VIAS section nor the LEF defines")
            })
        }
    }
}

fn parse_pdn_use(word: &str, net_name: &str, line: usize) -> Result<PdnUse> {
    Ok(match word {
        "POWER" => PdnUse::Power,
        "GROUND" => PdnUse::Ground,
        "SIGNAL" => PdnUse::Signal,
        "CLOCK" => PdnUse::Clock,
        "ANALOG" => PdnUse::Analog,
        "RESET" => PdnUse::Reset,
        "SCAN" => PdnUse::Scan,
        "TIEOFF" => PdnUse::TieOff,
        "RING" => PdnUse::Ring,
        other => bail!("DEF line {line}: SPECIALNETS net {net_name} has unknown `+ USE {other}`"),
    })
}

/// A point coordinate: a number, or `*` repeating the same axis' previous
/// value (`repeat`).
fn wire_coord(word: Option<&&str>, repeat: i32, net_name: &str, line: usize) -> Result<i32> {
    match word {
        Some(&"*") => Ok(repeat),
        Some(&w) => w
            .parse::<i32>()
            .map_err(|_| anyhow!("DEF line {line}: net {net_name} coordinate `{w}` is not a number")),
        None => bail!("DEF line {line}: net {net_name} wire statement ends mid-point"),
    }
}

fn is_coord(w: &str) -> bool {
    w == "*" || w.parse::<i32>().is_ok()
}

/// Mutable accumulators a wire statement writes through: emitted geometry
/// and the running last-point state `*` coordinates read back.
struct WireSink<'a> {
    interner: &'a mut StringInterner,
    wire_rects: &'a mut Vec<(LayerId, RectDbu)>,
    vias: &'a mut Vec<PdnVia>,
    last_x: i32,
    last_y: i32,
    net_name: &'a str,
}

/// The body of a `ROUTED`/`NEW`/`FIXED`/`COVER`/`SHIELD`/`TRUNK` wire
/// statement, `i` pointing at `<layerName>`. Returns the index of the next
/// statement (a `NEW`/`ROUTED`/`+` word, or the end of the record).
fn parse_special_wire(
    tech_db: &TechDb,
    def_vias: &DefViaMap,
    sink: &mut WireSink,
    rest: &[&str],
    mut i: usize,
    line: usize,
) -> Result<usize> {
    let net_name = sink.net_name;
    let lname = rest.get(i).copied().with_context(|| {
        format!("DEF line {line}: SPECIALNETS net {net_name} wire statement without a layer name")
    })?;
    let width: i32 = number(
        rest.get(i + 1).copied().unwrap_or(""),
        "SPECIALNETS wire width",
        line,
    )?;
    i += 2;
    let lid = tech_db
        .strings
        .get(lname)
        .and_then(|s| tech_db.layer_by_name.get(&s))
        .copied()
        .with_context(|| {
            format!("DEF line {line}: SPECIALNETS net {net_name} wire uses layer {lname}, which the LEF does not define as routing")
        })?;

    loop {
        let Some(&w) = rest.get(i) else { return Ok(i) };
        match w {
            "NEW" | "ROUTED" => return Ok(i),
            "+" => {
                let kw = rest.get(i + 1).copied().with_context(|| {
                    format!("DEF line {line}: SPECIALNETS net {net_name} has `+` without an attribute keyword")
                })?;
                match kw {
                    // One argument each; the model keeps none of them.
                    "SHAPE" | "MASK" | "STYLE" | "VOLTAGE" | "NONDEFAULTRULE" => {
                        rest.get(i + 2).with_context(|| {
                            format!("DEF line {line}: SPECIALNETS net {net_name} `+ {kw}` needs an argument")
                        })?;
                        i += 3;
                    }
                    // Anything else is a new `+` statement inside the same
                    // net record (a repeated `+ ROUTED`/`+ FIXED`/..., or a
                    // net-level attribute like `+ USE`): hand it back to the
                    // dispatcher, which owns that keyword set and its errors.
                    _ => return Ok(i),
                }
            }
            _ => {
                // First point of a record: `x y`.
                let x1 = wire_coord(rest.get(i), sink.last_x, net_name, line)?;
                let y1 = wire_coord(rest.get(i + 1), sink.last_y, net_name, line)?;
                i += 2;
                match rest.get(i) {
                    Some(&w2) if is_coord(w2) => {
                        // A second point — a wire segment. `*` repeats the
                        // same axis' previous coordinate (x1/y1 here).
                        let x2 = wire_coord(rest.get(i), x1, net_name, line)?;
                        let y2 = wire_coord(rest.get(i + 1), y1, net_name, line)?;
                        i += 2;
                        sink.last_x = x2;
                        sink.last_y = y2;
                        if width > 0 {
                            // Perpendicular expansion only — PDN mesh and
                            // attach semantics pin this flush form; NETS
                            // wires use square caps (see parse_net_wire).
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
                            sink.wire_rects.push((lid, rect));
                        }
                    }
                    Some(&via_name) if !matches!(via_name, "+" | "NEW" | "ROUTED") => {
                        // A via-stack record at (x1, y1) — the `width == 0`
                        // link between consecutive layers.
                        sink.last_x = x1;
                        sink.last_y = y1;
                        i += 1;
                        let (lower, upper, cut, cut_count, geom) =
                            resolve_def_via(tech_db, def_vias, via_name, net_name, line)?;
                        if lid != lower && lid != upper {
                            bail!(
                                "DEF line {line}: net {net_name} records via {via_name} on layer {lname}, which is neither of its endpoints"
                            );
                        }
                        sink.vias.push(PdnVia {
                            name: sink.interner.intern(&unescape_def_name_cow(via_name)),
                            lower,
                            upper,
                            cut,
                            cut_count,
                            geom,
                            x_dbu: x1,
                            y_dbu: y1,
                        });
                        // After a via name the point list ends; the next word
                        // must open a new statement.
                        match rest.get(i) {
                            None | Some(&"+" | &"NEW" | &"ROUTED") => return Ok(i),
                            Some(&bad) => bail!(
                                "DEF line {line}: unexpected `{bad}` after via {via_name} in SPECIALNETS net {net_name}"
                            ),
                        }
                    }
                    _ => bail!(
                        "DEF line {line}: SPECIALNETS net {net_name} has a one-point wire stub on layer {lname}"
                    ),
                }
            }
        }
    }
}

fn add_special_net(
    db: &mut DesignDb,
    tech_db: &TechDb,
    def_vias: &DefViaMap,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [name, rest @ ..] = words else {
        bail!("DEF line {line}: SPECIALNETS record without a net name");
    };
    let net_name = unescape_def_name_cow(name).into_owned();
    let net_sym = db.strings.intern(&net_name);

    // Connection groups `( comp pin )` up to the first `+`/`NEW`/`ROUTED`
    // word — parentheses were dropped by the statement reader, so groups are
    // exactly two words.
    let mut conns = Vec::new();
    let mut i = 0;
    while i + 1 < rest.len() && !matches!(rest[i], "+" | "NEW" | "ROUTED") {
        let comp = rest[i];
        let pin = rest[i + 1];
        if matches!(comp, "+" | "NEW" | "ROUTED") || matches!(pin, "+" | "NEW" | "ROUTED") {
            bail!("DEF line {line}: SPECIALNETS net {net_name} has a malformed connection group");
        }
        conns.push(if comp == "PIN" {
            PdnConn::TopPin(db.strings.intern(&unescape_def_name_cow(pin)))
        } else if comp == "*" {
            PdnConn::AllInstances(db.strings.intern(&unescape_def_name_cow(pin)))
        } else {
            PdnConn::Instance {
                inst: db.strings.intern(&unescape_def_name_cow(comp)),
                pin: db.strings.intern(&unescape_def_name_cow(pin)),
            }
        });
        i += 2;
    }
    if i < rest.len() && !matches!(rest[i], "+" | "NEW" | "ROUTED") {
        bail!("DEF line {line}: SPECIALNETS net {net_name} has a trailing odd word in its connection list");
    }

    let mut use_kind = None;
    let mut wire_rects = Vec::new();
    let mut net_vias = Vec::new();
    let mut sink = WireSink {
        interner: &mut db.strings,
        wire_rects: &mut wire_rects,
        vias: &mut net_vias,
        last_x: 0,
        last_y: 0,
        net_name: &net_name,
    };

    while i < rest.len() {
        match rest[i] {
            "+" => {
                let kw = rest.get(i + 1).copied().with_context(|| {
                    format!("DEF line {line}: SPECIALNETS net {net_name} has `+` without an attribute keyword")
                })?;
                i += 2;
                match kw {
                    "USE" => {
                        let arg = rest.get(i).copied().with_context(|| {
                            format!("DEF line {line}: SPECIALNETS net {net_name} `+ USE` needs a keyword")
                        })?;
                        let new_use = parse_pdn_use(arg, &net_name, line)?;
                        if use_kind.is_some_and(|u| u != new_use) {
                            bail!(
                                "DEF line {line}: SPECIALNETS net {net_name} has conflicting `+ USE` attributes"
                            );
                        }
                        use_kind = Some(new_use);
                        i += 1;
                    }
                    "SHIELD" => {
                        // `+ SHIELD shieldNetName` prefixes the same wire
                        // body; the shield metal belongs to this net.
                        rest.get(i).copied().with_context(|| {
                            format!("DEF line {line}: SPECIALNETS net {net_name} `+ SHIELD` needs a shield net name")
                        })?;
                        i += 1;
                        while matches!(rest.get(i), Some(&"+"))
                            && matches!(rest.get(i + 1), Some(&("SHAPE" | "MASK" | "STYLE")))
                        {
                            i += 3;
                        }
                        i = parse_special_wire(tech_db, def_vias, &mut sink, rest, i, line)?;
                    }
                    "ROUTED" | "FIXED" | "COVER" | "TRUNK" => {
                        i = parse_special_wire(tech_db, def_vias, &mut sink, rest, i, line)?;
                    }
                    // Attributes the model does not read; `SUBNET` is not one —
                    // subnets carry their own wires this schema cannot model.
                    "SOURCE" | "PATTERN" | "ESTCAP" | "WEIGHT" | "ORIGINAL" | "FIXEDBUMP"
                    | "FREQUENCY" | "VOLTAGE" | "PROPERTY" | "SYNTHESIZED" | "NONDEFAULTRULE"
                    | "XTALK" => {
                        while i < rest.len() && !matches!(rest[i], "+" | "NEW" | "ROUTED") {
                            i += 1;
                        }
                    }
                    "SUBNET" => bail!(
                        "DEF line {line}: SPECIALNETS net {net_name} `+ SUBNET` is not supported"
                    ),
                    other => bail!(
                        "DEF line {line}: unknown SPECIALNETS attribute `+ {other}` on net {net_name}"
                    ),
                }
            }
            "NEW" => {
                i = parse_special_wire(tech_db, def_vias, &mut sink, rest, i + 1, line)?;
            }
            other => bail!("DEF line {line}: unexpected `{other}` in SPECIALNETS net {net_name}"),
        }
    }

    // A repeated net name merges records; a conflicting `+ USE` is an error.
    match db.pdn_nets.iter_mut().find(|n| n.name == net_sym) {
        Some(net) => {
            if let Some(u) = use_kind {
                match net.use_kind {
                    Some(existing) if existing != u => bail!(
                        "DEF line {line}: SPECIALNETS net {net_name} repeats with a conflicting `+ USE`"
                    ),
                    _ => net.use_kind = Some(u),
                }
            }
            net.conns.extend(conns);
            net.wire_rects.extend(wire_rects);
            net.vias.extend(net_vias);
        }
        None => db.pdn_nets.push(PdnNet {
            name: net_sym,
            use_kind,
            conns,
            wire_rects,
            vias: net_vias,
        }),
    }
    Ok(())
}

/// Reads one wire point: `( x y [z] )` or a bare `x y` pair. OpenROAD's
/// defout emits a third parenthesized coordinate (`0` in every committed
/// input) — it is read and ignored; the model is 2-D. Returns
/// `(x, y, next_index)` where `*` resolves against `sink.last_*`.
fn read_wire_point(
    sink: &WireSink,
    rest: &[&str],
    mut i: usize,
    net_name: &str,
    line: usize,
) -> Result<(i32, i32, usize)> {
    if rest.get(i) == Some(&"(") {
        i += 1;
        let mut coords: Vec<&str> = Vec::new();
        while i < rest.len() && rest[i] != ")" {
            if !is_coord(rest[i]) {
                bail!("DEF line {line}: net {net_name} wire point contains `{}`, which is not a coordinate", rest[i]);
            }
            coords.push(rest[i]);
            i += 1;
        }
        if rest.get(i) != Some(&")") {
            bail!("DEF line {line}: net {net_name} wire point is not closed by `)`");
        }
        i += 1;
        if coords.len() < 2 {
            bail!("DEF line {line}: net {net_name} wire point needs at least x and y");
        }
        let x = wire_coord(coords.first(), sink.last_x, net_name, line)?;
        let y = wire_coord(coords.get(1), sink.last_y, net_name, line)?;
        Ok((x, y, i))
    } else {
        let x = wire_coord(rest.get(i), sink.last_x, net_name, line)?;
        let y = wire_coord(rest.get(i + 1), sink.last_y, net_name, line)?;
        Ok((x, y, i + 2))
    }
}

/// The body of a `ROUTED`/`NEW`/`FIXED`/`COVER`/`PLACED` wire statement in
/// a regular `NETS` record, `i` pointing at `<layerName>`. Regular-net
/// paths carry no width token — expansion uses the LEF layer `WIDTH`.
/// Returns the index of the next statement (a `NEW`/`ROUTED`/`+` word, or
/// the end of the record).
fn parse_net_wire(
    tech_db: &TechDb,
    def_vias: &DefViaMap,
    sink: &mut WireSink,
    rest: &[&str],
    mut i: usize,
    line: usize,
    ndr: Option<&FastMap<u8, i32>>,
) -> Result<usize> {
    let net_name = sink.net_name;
    let lname = rest.get(i).copied().with_context(|| {
        format!("DEF line {line}: net {net_name} wire statement without a layer name")
    })?;
    i += 1;
    let lid = tech_db
        .strings
        .get(lname)
        .and_then(|s| tech_db.layer_by_name.get(&s))
        .copied()
        .with_context(|| {
            format!("DEF line {line}: net {net_name} wire uses layer {lname}, which the LEF does not define as routing")
        })?;
    // LEF routing WIDTH is required by `parse_layer`, so it is never 0;
    // `+ NONDEFAULTRULE` overrides it per layer.
    let width = ndr
        .and_then(|m| m.get(&lid.0))
        .copied()
        .unwrap_or(tech_db.routing_layers[lid.0 as usize].width_dbu);
    let half = width / 2;

    let mut have_point = false;
    let mut emitted = false;
    let (mut px, mut py) = (0, 0);
    while let Some(&w) = rest.get(i) {
        match w {
            "+" | "NEW" | "ROUTED" => break,
            ")" => bail!(
                "DEF line {line}: net {net_name} wire on layer {lname} has an unbalanced `)`"
            ),
            "RECT" => {
                // `pt RECT ( dx1 dy1 dx2 dy2 )` — offset from the preceding
                // point; the delta already encodes the shape, no expansion.
                if !have_point {
                    bail!("DEF line {line}: net {net_name} wire on layer {lname} has `RECT` before any point");
                }
                let mut j = i + 1;
                if rest.get(j) == Some(&"(") {
                    j += 1;
                }
                let dx1: i32 = number(rest.get(j).copied().unwrap_or(""), "NETS RECT dx1", line)?;
                let dy1: i32 = number(rest.get(j + 1).copied().unwrap_or(""), "NETS RECT dy1", line)?;
                let dx2: i32 = number(rest.get(j + 2).copied().unwrap_or(""), "NETS RECT dx2", line)?;
                let dy2: i32 = number(rest.get(j + 3).copied().unwrap_or(""), "NETS RECT dy2", line)?;
                j += 4;
                if rest.get(j) == Some(&")") {
                    j += 1;
                }
                sink.wire_rects.push((
                    lid,
                    RectDbu::new(
                        px + dx1.min(dx2),
                        py + dy1.min(dy2),
                        px + dx1.max(dx2),
                        py + dy1.max(dy2),
                    ),
                ));
                emitted = true;
                i = j;
            }
            "VIRTUAL" => {
                // `VIRTUAL ( x y )` — a virtual connection point; it
                // emits no shape and is not a wire vertex, so the path
                // continues from the previous real point.
                let (x, y, j) = read_wire_point(sink, rest, i + 1, net_name, line)?;
                sink.last_x = x;
                sink.last_y = y;
                i = j;
            }
            // Point modifiers the model does not read: `TAPER` (full-width
            // taper is the default) takes no argument, the rest take one.
            "TAPER" => i += 1,
            "TAPERRULE" | "TAPERWIDTH" | "STYLE" | "MASK" => {
                rest.get(i + 1).with_context(|| {
                    format!("DEF line {line}: net {net_name} `{w}` needs an argument")
                })?;
                i += 2;
            }
            // These change the wire's shape or width; a design that uses
            // them is outside this model, not a silent approximation.
            "EXT" | "DESIGNRULEWIDTH" | "NONDEFAULTRULE" => bail!(
                "DEF line {line}: net {net_name} wire statement has `{w}`, which alters geometry this model does not support"
            ),
            w if w == "(" || is_coord(w) => {
                let (x, y, j) = read_wire_point(sink, rest, i, net_name, line)?;
                i = j;
                if have_point {
                    // Square (extended) caps — the segment runs half its
                    // width past both endpoints along the axis, so a
                    // sub-width jog paints a full-width stub instead of a
                    // narrow tab. Diagonals get the full-bbox expansion.
                    let rect = if py == y {
                        RectDbu::new(px.min(x) - half, y - half, px.max(x) + half, y + half)
                    } else if px == x {
                        RectDbu::new(x - half, py.min(y) - half, x + half, py.max(y) + half)
                    } else {
                        RectDbu::new(
                            px.min(x) - half,
                            py.min(y) - half,
                            px.max(x) + half,
                            py.max(y) + half,
                        )
                    };
                    sink.wire_rects.push((lid, rect));
                    emitted = true;
                }
                px = x;
                py = y;
                have_point = true;
                sink.last_x = x;
                sink.last_y = y;
            }
            via_name => {
                if !have_point {
                    bail!("DEF line {line}: net {net_name} records via {via_name} on layer {lname} with no preceding point");
                }
                i += 1;
                let (lower, upper, cut, cut_count, geom) =
                    resolve_def_via(tech_db, def_vias, via_name, net_name, line)?;
                if lid != lower && lid != upper {
                    bail!(
                        "DEF line {line}: net {net_name} records via {via_name} on layer {lname}, which is neither of its endpoints"
                    );
                }
                sink.vias.push(PdnVia {
                    name: sink.interner.intern(&unescape_def_name_cow(via_name)),
                    lower,
                    upper,
                    cut,
                    cut_count,
                    geom,
                    x_dbu: px,
                    y_dbu: py,
                });
                emitted = true;
                // After a via name the path ends; the next word must open
                // a new statement.
                match rest.get(i) {
                    None | Some(&"+" | &"NEW" | &"ROUTED") => break,
                    Some(&bad) => bail!(
                        "DEF line {line}: unexpected `{bad}` after via {via_name} in net {net_name}"
                    ),
                }
            }
        }
    }
    if !emitted {
        bail!(
            "DEF line {line}: net {net_name} has a one-point wire stub on layer {lname}"
        );
    }
    Ok(i)
}

/// One `NONDEFAULTRULES` record: `- ruleName + LAYER l WIDTH w SPACING s ...`.
/// Only `LAYER` `WIDTH`/`SPACING` sub-attributes are grammar-relevant to our
/// wire geometry; anything else (VIARULE/USEVIA/MINCUTS/...) is rejected
/// loudly rather than silently dropped.
fn add_ndr(
    tech_db: &TechDb,
    ndr_widths: &mut FastMap<String, FastMap<u8, i32>>,
    words: &[&str],
    line: usize,
) -> Result<()> {
    let [name, rest @ ..] = words else {
        bail!("DEF line {line}: NONDEFAULTRULES record without a rule name");
    };
    let mut widths: FastMap<u8, i32> = FastMap::default();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] != "+" {
            bail!("DEF line {line}: NONDEFAULTRULES {name} unexpected `{}`", rest[i]);
        }
        let kw = rest.get(i + 1).copied().with_context(|| {
            format!("DEF line {line}: NONDEFAULTRULES {name} has `+` without a keyword")
        })?;
        i += 2;
        match kw {
            "LAYER" => {
                let lname = rest.get(i).copied().with_context(|| {
                    format!("DEF line {line}: NONDEFAULTRULES {name} `+ LAYER` needs a layer name")
                })?;
                let lid = tech_db
                    .strings
                    .get(lname)
                    .and_then(|s| tech_db.layer_by_name.get(&s))
                    .copied()
                    .with_context(|| {
                        format!("DEF line {line}: NONDEFAULTRULES {name} uses layer {lname}, which the LEF does not define as routing")
                    })?;
                i += 1;
                while i < rest.len() && rest[i] != "+" {
                    match rest[i] {
                        "WIDTH" => {
                            let w: i32 = number(
                                rest.get(i + 1).copied().unwrap_or(""),
                                "NONDEFAULTRULES LAYER WIDTH",
                                line,
                            )?;
                            widths.insert(lid.0, w);
                            i += 2;
                        }
                        // Spacing has no effect on painted wire width.
                        "SPACING" => i += 2,
                        other => bail!(
                            "DEF line {line}: NONDEFAULTRULES {name} `+ LAYER {lname}` has unsupported attribute `{other}`"
                        ),
                    }
                }
            }
            other => bail!(
                "DEF line {line}: NONDEFAULTRULES {name} has unsupported attribute `+ {other}`"
            ),
        }
    }
    ndr_widths.insert(name.to_string(), widths);
    Ok(())
}

/// Lookup tables `add_net` resolves names through: DEF `VIAS` records,
/// `NONDEFAULTRULES` width overrides, and the component/PIN name maps.
struct NetLookups<'a> {
    def_vias: &'a DefViaMap,
    ndr_widths: &'a FastMap<String, FastMap<u8, i32>>,
    io_pins: &'a FastMap<String, PinId>,
    cells: &'a FastMap<String, CellId>,
}

/// A `NETS` record: connectivity (same resolution the old token loop did,
/// moved verbatim) plus routed geometry into `NetGeom`.
fn add_net(
    db: &mut DesignDb,
    tech_db: &TechDb,
    lookups: &NetLookups<'_>,
    net_id: NetId,
    words: &[&str],
    line: usize,
) -> Result<(Vec<PinId>, NetGeom)> {
    let [name, rest @ ..] = words else {
        bail!("DEF line {line}: NETS record without a net name");
    };
    let net_name = unescape_def_name_cow(name).into_owned();

    // Connection groups `( comp pin )` up to the first `+`/`NEW`/`ROUTED`
    // word — NETS records keep parens (wire point groups need them), so
    // groups arrive as exactly four tokens `( comp pin )`.
    let mut pins = Vec::new();
    let mut i = 0;
    while i < rest.len() && !matches!(rest[i], "+" | "NEW" | "ROUTED") {
        if rest.get(i) != Some(&"(") || rest.get(i + 3) != Some(&")") {
            bail!("DEF line {line}: connection of net {net_name} must be `( component pin )`");
        }
        let comp = rest[i + 1];
        let pin = rest[i + 2];
        if matches!(comp, "+" | "NEW" | "ROUTED") || matches!(pin, "+" | "NEW" | "ROUTED")
            || matches!(comp, "(" | ")") || matches!(pin, "(" | ")")
        {
            bail!("DEF line {line}: connection of net {net_name} must be `( component pin )`");
        }
        if comp == "PIN" {
            let pname = unescape_def_name_cow(pin);
            let &pid = lookups.io_pins.get(pname.as_ref()).with_context(|| {
                format!("DEF line {line}: net {net_name} connects to PIN {pname}, which PINS does not define")
            })?;
            pins.push(pid);
            db.pin_nets[pid.0 as usize] = Some(net_id);
        } else {
            let iname = unescape_def_name_cow(comp);
            let &cid = lookups.cells.get(iname.as_ref()).with_context(|| {
                format!("DEF line {line}: net {net_name} connects to component {iname}, which COMPONENTS does not define")
            })?;
            let pid = db
                .cell_pins(cid)
                .iter()
                .copied()
                .find(|&pid| db.strings.resolve(db.pin_names[pid.0 as usize]) == pin)
                .with_context(|| {
                    format!("DEF line {line}: net {net_name} connects to {iname}/{pin}, which the macro does not have")
                })?;
            pins.push(pid);
            db.pin_nets[pid.0 as usize] = Some(net_id);
        }
        i += 4;
    }

    let mut geom = NetGeom::default();
    // `+ NONDEFAULTRULE name` applies to every wire statement after it.
    let mut ndr: Option<&FastMap<u8, i32>> = None;
    let mut sink = WireSink {
        interner: &mut db.strings,
        wire_rects: &mut geom.wire_rects,
        vias: &mut geom.vias,
        last_x: 0,
        last_y: 0,
        net_name: &net_name,
    };

    while i < rest.len() {
        match rest[i] {
            "+" => {
                let kw = rest.get(i + 1).copied().with_context(|| {
                    format!("DEF line {line}: net {net_name} has `+` without an attribute keyword")
                })?;
                i += 2;
                match kw {
                    "USE" => {
                        let arg = rest.get(i).copied().with_context(|| {
                            format!("DEF line {line}: net {net_name} `+ USE` needs a keyword")
                        })?;
                        parse_pdn_use(arg, &net_name, line)?;
                        i += 1;
                    }
                    "ROUTED" | "FIXED" | "COVER" | "PLACED" => {
                        i = parse_net_wire(tech_db, lookups.def_vias, &mut sink, rest, i, line, ndr)?;
                    }
                    "SHIELD" => {
                        // Net-level shielding annotation; the shield metal
                        // itself arrives as its own wire statements.
                        rest.get(i).copied().with_context(|| {
                            format!("DEF line {line}: net {net_name} `+ SHIELD` needs a shield net name")
                        })?;
                        i += 1;
                    }
                    "SUBNET" => bail!(
                        "DEF line {line}: net {net_name} `+ SUBNET` is not supported"
                    ),
                    "NONDEFAULTRULE" => {
                        let arg = rest.get(i).copied().with_context(|| {
                            format!("DEF line {line}: net {net_name} `+ NONDEFAULTRULE` needs a rule name")
                        })?;
                        ndr = Some(lookups.ndr_widths.get(arg).with_context(|| {
                            format!("DEF line {line}: net {net_name} `+ NONDEFAULTRULE {arg}` is not defined in NONDEFAULTRULES")
                        })?);
                        i += 1;
                    }
                    "EXT" | "DESIGNRULEWIDTH" => bail!(
                        "DEF line {line}: net {net_name} `+ {kw}` alters wire geometry, which this model does not support"
                    ),
                    // Attributes with no geometry effect; arguments run to
                    // the next `+`/`NEW`/`ROUTED` word.
                    "TAPER" | "TAPERWIDTH" | "STYLE" | "MASK" | "SYNTHESIZED" | "PATTERN"
                    | "SOURCE" | "ESTCAP" | "WEIGHT" | "ORIGINAL" | "PROPERTY" | "XTALK"
                    | "VOLTAGE" | "FREQUENCY" | "FIXEDBUMP" => {
                        while i < rest.len() && !matches!(rest[i], "+" | "NEW" | "ROUTED") {
                            i += 1;
                        }
                    }
                    other => bail!(
                        "DEF line {line}: unknown NETS attribute `+ {other}` on net {net_name}"
                    ),
                }
            }
            // `NEW` is bare; a bare `ROUTED` is grammar-legal inside a
            // record too, so both reach the wire parser.
            "NEW" => {
                i = parse_net_wire(tech_db, lookups.def_vias, &mut sink, rest, i + 1, line, ndr)?;
            }
            "ROUTED" => {
                i = parse_net_wire(tech_db, lookups.def_vias, &mut sink, rest, i + 1, line, ndr)?;
            }
            other => bail!("DEF line {line}: unexpected `{other}` in net {net_name}"),
        }
    }
    Ok((pins, geom))
}

pub fn parse_def_bytes(bytes: &[u8], tech_db: &TechDb, lib_db: Option<&LibDb>) -> Result<DesignDb> {
    let text = std::str::from_utf8(bytes).context("DEF is not valid UTF-8")?;
    let mut lexer = DefLexer::new(text);
    let mut db = DesignDb::new();
    db.strings = tech_db.strings.clone();
    db.dbu_per_micron = if tech_db.dbu_per_micron > 0 {
        tech_db.dbu_per_micron
    } else {
        1000
    };

    let mut cell_name_to_id: FastMap<String, CellId> = FastMap::default();
    let mut io_pin_name_to_id: FastMap<String, PinId> = FastMap::default();
    // `VIAS` is parsed when reached; SPECIALNETS via records resolve eagerly
    // against it, so a DEF placing SPECIALNETS first errors on the unknown
    // via name — a named assumption (OpenROAD writers emit VIAS first).
    let mut def_vias: DefViaMap = FastMap::default();
    // NONDEFAULTRULE name → `LayerId` → wire width override (DBU).
    let mut ndr_widths: FastMap<String, FastMap<u8, i32>> = FastMap::default();
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
                _ => bail!(
                    "DEF line {}: UNITS must be `UNITS DISTANCE MICRONS <dbu> ;`",
                    lexer.line()
                ),
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
                let [
                    name,
                    site,
                    orig_x,
                    orig_y,
                    orient,
                    "DO",
                    num_x,
                    "BY",
                    num_y,
                    "STEP",
                    step_x,
                    step_y,
                ] = words.as_slice()
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
                        _ => bail!(
                            "DEF line {line}: TRACKS must be `TRACKS X|Y start DO n STEP step [LAYER ...] ;`"
                        ),
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
                    bail!(
                        "DEF line {line}: GCELLGRID must be `GCELLGRID X|Y start DO n STEP step ;`"
                    );
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
                }, false)?;
            }
            DefToken::Word("PINS") => {
                for_each_record(&mut lexer, "PINS", |words, line| {
                    add_pin(&mut db, tech_db, &mut io_pin_name_to_id, words, line)
                }, false)?;
            }
            DefToken::Word("SPECIALNETS") => {
                for_each_record(&mut lexer, "SPECIALNETS", |words, line| {
                    add_special_net(&mut db, tech_db, &def_vias, words, line)
                }, false)?;
            }
            DefToken::Word("VIAS") => {
                for_each_record(&mut lexer, "VIAS", |words, line| {
                    let (name, via) = add_def_via(&mut db, tech_db, words, line)?;
                    def_vias.insert(name, via);
                    Ok(())
                }, false)?;
            }
            // Non-default rules carry per-layer WIDTH overrides that wire
            // expansion needs; other NDR attributes are rejected loudly.
            DefToken::Word("NONDEFAULTRULES") => {
                for_each_record(&mut lexer, "NONDEFAULTRULES", |words, line| {
                    add_ndr(tech_db, &mut ndr_widths, words, line)
                }, false)?;
            }
            DefToken::Word("NETS") => {
                let mut net_pins_list: Vec<Vec<PinId>> = Vec::new();
                let mut net_names_vec: Vec<String> = Vec::new();
                let mut net_spellings_vec: Vec<String> = Vec::new();
                let mut net_geoms_list: Vec<NetGeom> = Vec::new();

                // NETS keeps parens: routed-wire points are `( x y [z] )`
                // groups, and a `( x y 0 )` triple is ambiguous with the
                // next point's `x` once parens are dropped.
                for_each_record(&mut lexer, "NETS", |words, line| {
                    let [name, ..] = words else {
                        bail!("DEF line {line}: NETS record without a net name");
                    };
                    let net_id = NetId(net_names_vec.len() as u32);
                    net_names_vec.push(unescape_def_name_cow(name).into_owned());
                    net_spellings_vec.push(name.to_string());
                    let (pins, geom) = add_net(
                        &mut db,
                        tech_db,
                        &NetLookups {
                            def_vias: &def_vias,
                            ndr_widths: &ndr_widths,
                            io_pins: &io_pin_name_to_id,
                            cells: &cell_name_to_id,
                        },
                        net_id,
                        words,
                        line,
                    )?;
                    net_pins_list.push(pins);
                    net_geoms_list.push(geom);
                    Ok(())
                }, true)?;

                // Build Net SoA & CSR
                let num_nets = net_names_vec.len();
                db.net_geoms = net_geoms_list;
                db.net_names.reserve(num_nets);
                db.net_pin_offsets.reserve(num_nets + 1);

                for name in &net_names_vec {
                    db.net_names.push(db.strings.intern(name));
                }
                for spelling in &net_spellings_vec {
                    db.net_def_spellings.push(db.strings.intern(spelling));
                }

                for mut pins in net_pins_list {
                    let offset = db.net_pin_ids.len() as u32;
                    db.net_pin_offsets.push(offset);

                    // Invariant: driver pin placed first
                    if let Some(drv_pos) = pins.iter().position(|&p| db.pin_is_driver[p.0 as usize])
                    {
                        pins.swap(0, drv_pos);
                    }
                    db.net_pin_ids.extend(pins);
                }
                db.net_pin_offsets.push(db.net_pin_ids.len() as u32);
            }
            DefToken::Word(keyword) => {
                bail!(
                    "DEF line {}: unknown section or statement `{keyword}`",
                    lexer.line()
                )
            }
            other => bail!(
                "DEF line {}: unexpected {other:?} at top level",
                lexer.line()
            ),
        }
    }

    if !saw_end_design {
        bail!("DEF: missing `END DESIGN` (truncated file?)");
    }
    Ok(db)
}

pub fn parse_def(path: &Path, tech_db: &TechDb, lib_db: Option<&LibDb>) -> Result<DesignDb> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open DEF file at {:?}", path))?;
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
        let macro_name = tech_db
            .strings
            .resolve(tech_db.macros[macro_id.0 as usize].name);

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

/// Serializes a complete DEF — header, DIEAREA, ROWs, COMPONENTS, PINS, NETS —
/// with no template. Used by floorplan generation where no DEF exists yet.
/// TRACKS/GCELLGRID/VIAS/SPECIALNETS are omitted: they constrain detailed
/// routing and power, which this database does not model.
pub fn write_def_full(db: &DesignDb, tech_db: &TechDb, out_def_path: &Path) -> Result<()> {
    let name = |sym: crate::interner::SymbolId| escape_def_name(db.strings.resolve(sym));
    let mut out = String::with_capacity(1 << 20);
    out.push_str("VERSION 5.8 ;\nDIVIDERCHAR \"/\" ;\nBUSBITCHARS \"[]\" ;\n");
    out.push_str(&format!("DESIGN {} ;\n", escape_def_name(&db.design_name)));
    out.push_str(&format!("UNITS DISTANCE MICRONS {} ;\n", db.dbu_per_micron));
    out.push_str(&format!(
        "DIEAREA ( {} {} ) ( {} {} ) ;\n",
        db.die_area.xl, db.die_area.yl, db.die_area.xh, db.die_area.yh
    ));
    for (i, r) in db.rows.iter().enumerate() {
        out.push_str(&format!(
            "ROW ROW_{} {} {} {} {} DO {} BY 1 STEP {} 0 ;\n",
            i,
            name(r.site_name),
            r.orig_x_dbu,
            r.orig_y_dbu,
            orient_str(r.orient),
            r.num_x,
            r.step_x_dbu
        ));
    }

    out.push_str(&format!("COMPONENTS {} ;\n", db.num_cells()));
    for c in 0..db.num_cells() {
        let macro_name = tech_db
            .strings
            .resolve(tech_db.macros[db.cell_macros[c].0 as usize].name);
        match db.cell_status[c] {
            PlacementStatus::Unplaced => {
                out.push_str(&format!(
                    "    - {} {} + UNPLACED ;\n",
                    name(db.cell_names[c]),
                    escape_def_name(macro_name)
                ));
            }
            status => {
                out.push_str(&format!(
                    "    - {} {} + {} ( {} {} ) {} ;\n",
                    name(db.cell_names[c]),
                    escape_def_name(macro_name),
                    match status {
                        PlacementStatus::Placed => "PLACED",
                        PlacementStatus::Fixed => "FIXED",
                        PlacementStatus::Cover => "COVER",
                        PlacementStatus::Unplaced => unreachable!(),
                    },
                    db.cell_x_dbu[c],
                    db.cell_y_dbu[c],
                    orient_str(db.cell_orient[c])
                ));
            }
        }
    }
    out.push_str("END COMPONENTS\n");

    out.push_str(&format!("PINS {} ;\n", db.io_pins.len()));
    for &pin in &db.io_pins {
        let p = pin.0 as usize;
        let mut line = format!("    - {}", name(db.pin_names[p]));
        if let Some(net) = db.pin_nets[p] {
            line.push_str(&format!(" + NET {}", name(db.net_names[net.0 as usize])));
        }
        line.push_str(&format!(
            " + DIRECTION {} + USE SIGNAL + PORT + PLACED ( {} {} ) N ;\n",
            match db.pin_dirs[p] {
                PinDir::Input => "INPUT",
                PinDir::Output => "OUTPUT",
                PinDir::Inout | PinDir::Internal => "INOUT",
            },
            db.pin_offset_x_dbu[p],
            db.pin_offset_y_dbu[p]
        ));
        out.push_str(&line);
    }
    out.push_str("END PINS\n");

    out.push_str(&format!("NETS {} ;\n", db.num_nets()));
    for n in 0..db.num_nets() {
        let mut line = format!("    - {}", name(db.net_names[n]));
        let start = db.net_pin_offsets[n] as usize;
        let end = db.net_pin_offsets[n + 1] as usize;
        for &pin in &db.net_pin_ids[start..end] {
            let p = pin.0 as usize;
            match db.pin_cells[p] {
                Some(cell) => line.push_str(&format!(
                    " ( {} {} )",
                    name(db.cell_names[cell.0 as usize]),
                    name(db.pin_names[p])
                )),
                None => line.push_str(&format!(" ( PIN {} )", name(db.pin_names[p]))),
            }
        }
        line.push_str(" + USE SIGNAL ;\n");
        out.push_str(&line);
    }
    out.push_str("END NETS\nEND DESIGN\n");

    let mut file = File::create(out_def_path)
        .with_context(|| format!("Failed to create DEF at {:?}", out_def_path))?;
    file.write_all(out.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::lef::{load_tech_db, parse_lef_bytes};
    use crate::test_support::{design_dir, require, sky130_hd};

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
            .expect("Failed to load TechDb")
            .0;

        let db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["instances"].as_u64().unwrap() as usize
        );
        assert_eq!(db.num_rows(), ref_json["rows"].as_u64().unwrap() as usize);
        assert_eq!(
            db.tracks.len(),
            ref_json["def_tracks_statements"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["bterms_signal"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["def_inst_pins"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.net_pin_ids.len(),
            ref_json["connected_net_pins"].as_u64().unwrap() as usize
        );

        let hpwl = db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            hpwl,
            ref_json["placed_hpwl_dbu"].as_i64().unwrap(),
            "HPWL must equal OpenROAD getTermBBox sum"
        );

        // Verify write_def roundtrip and re-parse
        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_gcd_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path).expect("Failed to write DEF template");
        let db_roundtrip =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
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
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e))
            .0;
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
        assert_eq!(
            checked,
            ours.len(),
            "golden file must cover every signal instance pin"
        );

        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(gcd_dir.join("gcd.reference.json")).unwrap())
                .unwrap();
        assert_eq!(
            db.compute_hpwl_dbu(&tech_db),
            ref_json["orient_hpwl_dbu"].as_i64().unwrap()
        );
    }

    #[test]
    fn test_aes_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("aes").join("aes.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("aes").join("aes.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read aes.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["instances"].as_u64().unwrap() as usize
        );
        assert_eq!(db.num_rows(), ref_json["rows"].as_u64().unwrap() as usize);
        assert_eq!(
            db.tracks.len(),
            ref_json["def_tracks_statements"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["bterms_signal"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["def_inst_pins"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.net_pin_ids.len(),
            ref_json["connected_net_pins"].as_u64().unwrap() as usize
        );

        let hpwl = db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            hpwl,
            ref_json["placed_hpwl_dbu"].as_i64().unwrap(),
            "HPWL must equal OpenROAD getTermBBox sum"
        );

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_aes_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path).expect("Failed to write DEF template");
        let db_roundtrip =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
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
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e))
            .0;
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
        assert_eq!(
            checked,
            ours.len(),
            "golden file must cover every signal instance pin"
        );

        let ref_json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(require(aes_dir.join("aes.reference.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.compute_hpwl_dbu(&tech_db),
            ref_json["orient_hpwl_dbu"].as_i64().unwrap()
        );
    }

    #[test]
    fn test_jpeg_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("jpeg").join("jpeg.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("jpeg").join("jpeg.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read jpeg.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["instances"].as_u64().unwrap() as usize
        );
        assert_eq!(db.num_rows(), ref_json["rows"].as_u64().unwrap() as usize);
        assert_eq!(
            db.tracks.len(),
            ref_json["def_tracks_statements"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["bterms_signal"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["def_inst_pins"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.net_pin_ids.len(),
            ref_json["connected_net_pins"].as_u64().unwrap() as usize
        );

        let hpwl = db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            hpwl,
            ref_json["placed_hpwl_dbu"].as_i64().unwrap(),
            "HPWL must equal OpenROAD getTermBBox sum"
        );

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_jpeg_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path).expect("Failed to write DEF template");
        let db_roundtrip =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
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
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e))
            .0;
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
        assert_eq!(
            checked,
            ours.len(),
            "golden file must cover every signal instance pin"
        );

        let ref_json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(require(jpeg_dir.join("jpeg.reference.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.compute_hpwl_dbu(&tech_db),
            ref_json["orient_hpwl_dbu"].as_i64().unwrap()
        );
    }

    #[test]
    fn test_ibex_placed_def() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("ibex").join("ibex.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = require(design_dir("ibex").join("ibex.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read ibex.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["instances"].as_u64().unwrap() as usize
        );
        assert_eq!(db.num_rows(), ref_json["rows"].as_u64().unwrap() as usize);
        assert_eq!(
            db.tracks.len(),
            ref_json["def_tracks_statements"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["bterms_signal"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["def_inst_pins"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.net_pin_ids.len(),
            ref_json["connected_net_pins"].as_u64().unwrap() as usize
        );

        let hpwl = db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            hpwl,
            ref_json["placed_hpwl_dbu"].as_i64().unwrap(),
            "HPWL must equal OpenROAD getTermBBox sum"
        );

        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_ibex_placed_roundtrip.def");
        write_def(&db, &tech_db, &def_path, &out_def_path).expect("Failed to write DEF template");
        let db_roundtrip =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
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
            .unwrap_or_else(|e| panic!("Failed to load PDK LEF under {:?}: {}", sky130_hd(), e))
            .0;
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
        assert_eq!(
            checked,
            ours.len(),
            "golden file must cover every signal instance pin"
        );

        let ref_json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(require(ibex_dir.join("ibex.reference.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            db.compute_hpwl_dbu(&tech_db),
            ref_json["orient_hpwl_dbu"].as_i64().unwrap()
        );
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
        let db =
            parse_def_bytes(MINI_DEF.as_bytes(), &mini_tech(), None).expect("mini DEF must parse");
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
        let msg = mini_def_error(&MINI_DEF.replace(
            "ROW ROW_0 unithd 0 0 N DO 10 BY 1 STEP 460 2720 ;",
            "ROW ROW_0 unithd 0 0 N ;",
        ));
        assert!(msg.contains("ROW must be"), "{msg}");
    }

    #[test]
    fn test_unknown_section_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("DESIGN top ;", "DESIGN top ;\nFOO 1 ;"));
        assert!(msg.contains("FOO"), "{msg}");
    }

    #[test]
    fn test_unknown_component_attribute_is_an_error() {
        let msg = mini_def_error(
            &MINI_DEF.replace("+ PLACED ( 460 0 ) N ;", "+ PLACED ( 460 0 ) N + BOGUS 1 ;"),
        );
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

    /// A `( component pin )` group that is not exactly two words is rejected
    /// with the same message and line the collected-group destructure produced.
    #[test]
    fn test_connection_group_with_extra_word_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("( u1 A )", "( u1 A extra )"));
        assert!(msg.contains("must be `( component pin )`"), "{msg}");
    }

    #[test]
    fn test_connection_group_with_one_word_is_an_error() {
        let msg = mini_def_error(&MINI_DEF.replace("( u1 A )", "( u1 )"));
        assert!(msg.contains("must be `( component pin )`"), "{msg}");
    }

    #[test]
    fn test_connection_group_unterminated_is_an_error() {
        // `;` closes the statement inside the group, leaving `u1` as a
        // dangling word — same reject as the one-word group.
        let msg = mini_def_error(&MINI_DEF.replace("( u1 A )", "( u1 ;"));
        assert!(msg.contains("must be `( component pin )`"), "{msg}");
    }

    /// `NONDEFAULTRULES` `+ LAYER l WIDTH w` overrides the LEF routing
    /// width for nets under `+ NONDEFAULTRULE`; `SPACING` is consumed but
    /// has no geometry effect.
    #[test]
    fn test_nets_nondefaultrule_width_applies() {
        let def = NETS_DEF
            .replace(
                "COMPONENTS 2 ;",
                "NONDEFAULTRULES 1 ;\n  - wide + LAYER met1 WIDTH 200 SPACING 280 ;\nEND NONDEFAULTRULES\nCOMPONENTS 2 ;",
            )
            .replace("+ USE SIGNAL\n  + ROUTED met1", "+ USE SIGNAL + NONDEFAULTRULE wide\n  + ROUTED met1");
        let db = parse_def_bytes(def.as_bytes(), &pdn_tech(), None).expect("must parse");
        let n1 = db.net_geom(NetId(0)).expect("n1 geom");
        assert_eq!(
            n1.wire_rects[0].1,
            RectDbu::new(-100, -100, 1100, 100),
            "NDR met1 WIDTH 200 (half 100) replaces the LEF 140"
        );
    }

    /// Every name written by `escape_def_name` must come back unchanged
    /// after the lexer splits words and `unescape_def_name` strips escapes.
    /// `[`/`]` stay literal so `name[N]` bus-bit syntax survives, matching
    /// what OpenROAD-flow-scripts emits for synthesized netlists.
    #[test]
    fn test_escaped_names_roundtrip_through_lexer() {
        for raw in [
            "a(b)",
            "a;b",
            "a#b",
            "a b",
            "a\\b",
            "name[0]",
            "req_msg[3]",
            "dpath.a$in0",
            "x+y,z",
        ] {
            let esc = escape_def_name(raw);
            let def = MINI_DEF.replace(
                "- n1 ( PIN clk ) ( u1 A ) + USE SIGNAL ;",
                &format!("- {esc} ( PIN clk ) ( u1 A ) + USE SIGNAL ;"),
            );
            let db = parse_def_bytes(def.as_bytes(), &mini_tech(), None).unwrap_or_else(|e| {
                panic!("DEF with escaped name {esc:?} (raw {raw:?}) failed: {e:#}")
            });
            assert_eq!(
                db.strings.resolve(db.net_names[0]),
                raw,
                "net name {raw:?} did not round-trip"
            );
        }
    }

    /// Every truncation of the mini DEF must return `Ok` or `Err` without panicking, and only the
    /// full document (through `END DESIGN`) may parse, because the trailer is required.
    #[test]
    fn test_every_truncation_of_mini_def_is_rejected_or_parsed_without_panic() {
        let tech = mini_tech();
        let trailer_end =
            MINI_DEF.find("END DESIGN").expect("fixture has END DESIGN") + "END DESIGN".len();
        for n in 0..=MINI_DEF.len() {
            let result = parse_def_bytes(&MINI_DEF.as_bytes()[..n], &tech, None);
            if n < trailer_end {
                assert!(result.is_err(), "prefix of {n} bytes was accepted");
            }
        }
    }

    /// Two routing layers, one cut layer, a LEF `VIA` with a non-descriptive
    /// name, and a cell with a `met1` power pin.
    const PDN_LEF: &str = r#"
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
  WIDTH 0.14 ;
END met1
LAYER via12
  TYPE CUT ;
  RESISTANCE 15.0 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
LAYER met3
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
END met3
VIA stack_a
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END stack_a
MACRO CELL
  CLASS CORE ;
  SIZE 1.0 BY 1.0 ;
  PIN VPWR
    DIRECTION INOUT ;
    USE POWER ;
    PORT
      LAYER met1 ;
        RECT 0.0 -0.05 0.2 0.05 ;
    END
  END VPWR
END CELL
END LIBRARY
"#;

    /// `stack_a` via one met1 rail to one met2 stripe at (1000, 0); the
    /// top-level `VPWR` pad sits on the stripe.
    const PDN_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN pdn ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 2000 2000 ) ;
ROW ROW_0 unithd 0 0 N DO 4 BY 1 STEP 460 2720 ;
TRACKS X 170 DO 100 STEP 340 LAYER met1 ;
COMPONENTS 2 ;
- u1 CELL + PLACED ( 0 0 ) N ;
- u2 CELL + PLACED ( 500 0 ) N ;
END COMPONENTS
PINS 1 ;
- VPWR + NET VPWR + SPECIAL + DIRECTION INOUT + USE POWER
  + PORT
    + LAYER met2 ( -50 -50 ) ( 50 50 )
    + PLACED ( 1000 1800 ) N ;
END PINS
VIAS 1 ;
- via12_inst + VIA stack_a ;
END VIAS
SPECIALNETS 1 ;
- VPWR ( PIN VPWR ) ( * VPWR ) + USE POWER
  + ROUTED met1 100 + SHAPE FOLLOWPIN ( 0 0 ) ( 2000 0 )
  NEW met2 100 + SHAPE STRIPE ( 1000 0 ) ( 1000 2000 )
  NEW met2 0 + SHAPE STRIPE ( 1000 0 ) via12_inst
  ;
END SPECIALNETS
END DESIGN
"#;

    fn pdn_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        parse_lef_bytes(PDN_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("PDN LEF must parse");
        tech_db.strings = interner.clone();
        tech_db.rebuild_maps();
        tech_db
    }

    fn pdn_def_error(def: &str) -> String {
        match parse_def_bytes(def.as_bytes(), &pdn_tech(), None) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("malformed DEF was accepted:\n{def}"),
        }
    }

    /// Routing `LayerId` by name in the design's tech namespace.
    fn layer_id(tech: &TechDb, name: &str) -> crate::interner::LayerId {
        tech.layer_by_name[&tech.strings.get(name).expect(name)]
    }

    #[test]
    fn test_specialnet_parse_wires_and_vias() {
        let tech = pdn_tech();
        let db = parse_def_bytes(PDN_DEF.as_bytes(), &tech, None).expect("PDN DEF must parse");
        assert_eq!(db.pdn_nets.len(), 1);
        let net = &db.pdn_nets[0];
        assert_eq!(db.strings.resolve(net.name), "VPWR");
        assert_eq!(net.use_kind, Some(PdnUse::Power));
        assert_eq!(net.conns.len(), 2);

        // met1 rail (100 wide → ±50 dbu), met2 stripe.
        assert_eq!(net.wire_rects.len(), 2);
        let met1 = layer_id(&tech, "met1");
        let met2 = layer_id(&tech, "met2");
        assert_eq!(
            net.wire_rects[0],
            (met1, RectDbu::new(0, -50, 2000, 50)),
            "met1 rail"
        );
        assert_eq!(
            net.wire_rects[1],
            (met2, RectDbu::new(950, 0, 1050, 2000)),
            "met2 stripe"
        );
        assert_eq!(net.vias.len(), 1);
        let via = &net.vias[0];
        assert_eq!(db.strings.resolve(via.name), "via12_inst");
        assert_eq!(db.strings.resolve(via.cut), "via12");
        assert_eq!(via.cut_count, 1);
        assert_eq!((via.x_dbu, via.y_dbu), (1000, 0));
        // Resolved through the DEF `VIAS` record's `+ VIA stack_a`, whose
        // endpoints are the LEF VIA's routing layers.
        assert_eq!(via.lower, met1);
        assert_eq!(via.upper, met2);
    }

    #[test]
    fn test_specialnet_via_resolves_lef_name_without_def_via() {
        let def = PDN_DEF.replace("via12_inst", "stack_a").replace(
            "VIAS 1 ;\n- via12_inst + VIA stack_a ;\nEND VIAS\n",
            "",
        );
        let db = parse_def_bytes(def.as_bytes(), &pdn_tech(), None).expect("must parse");
        let via = &db.pdn_nets[0].vias[0];
        assert_eq!(db.strings.resolve(via.cut), "via12");
    }

    #[test]
    fn test_specialnet_via_unknown_name_is_an_error() {
        // Rename only the SPECIALNETS reference; the VIAS record still
        // defines `via12_inst` so `no_such_via` is defined nowhere.
        let msg = pdn_def_error(&PDN_DEF.replace(
            "( 1000 0 ) via12_inst",
            "( 1000 0 ) no_such_via",
        ));
        assert!(msg.contains("no_such_via") && msg.contains("VIAS"), "{msg}");
    }

    #[test]
    fn test_specialnet_generated_via_uses_rowcol_cut_count() {
        let def = PDN_DEF.replace(
            "- via12_inst + VIA stack_a ;",
            "- via12_inst + VIARULE RULE + CUTSIZE 150 150 + LAYERS met1 via12 met2 + CUTSPACING 170 170 + ENCLOSURE 85 165 55 85 + ROWCOL 1 3 ;",
        );
        let db = parse_def_bytes(def.as_bytes(), &pdn_tech(), None).expect("must parse");
        let via = &db.pdn_nets[0].vias[0];
        assert_eq!(via.cut_count, 3);
        assert_eq!(db.strings.resolve(via.cut), "via12");
    }

    #[test]
    fn test_specialnet_viarule_without_layers_is_an_error() {
        let msg = pdn_def_error(&PDN_DEF.replace(
            "- via12_inst + VIA stack_a ;",
            "- via12_inst + VIARULE RULE ;",
        ));
        assert!(msg.contains("via12_inst") && msg.contains("LAYERS"), "{msg}");
    }

    #[test]
    fn test_specialnet_inline_via_is_an_error_when_used() {
        let msg = pdn_def_error(&PDN_DEF.replace(
            "- via12_inst + VIA stack_a ;",
            "- via12_inst + RECT met1 ( 0 0 ) ( 10 10 ) ;",
        ));
        assert!(msg.contains("via12_inst") && msg.contains("no resolvable layer stack"), "{msg}");
    }

    #[test]
    fn test_specialnet_via_on_foreign_layer_is_an_error() {
        // `stack_a` spans met1↔met2; recording it on met3 (a routing layer
        // the via does not touch) is inconsistent.
        let msg = pdn_def_error(&PDN_DEF.replace(
            "NEW met2 0 + SHAPE STRIPE ( 1000 0 ) via12_inst",
            "NEW met3 0 + SHAPE STRIPE ( 1000 0 ) via12_inst",
        ));
        assert!(msg.contains("met3") && msg.contains("neither of its endpoints"), "{msg}");
    }

    #[test]
    fn test_specialnet_unknown_use_is_an_error() {
        let msg = pdn_def_error(&PDN_DEF.replace("+ USE POWER", "+ USE CHARGE"));
        assert!(msg.contains("USE CHARGE"), "{msg}");
    }

    #[test]
    fn test_specialnet_subnet_is_an_error() {
        let msg = pdn_def_error(&PDN_DEF.replace("+ USE POWER", "+ SUBNET VPWR_a ( u1 VPWR ) + USE POWER"));
        assert!(msg.contains("SUBNET"), "{msg}");
    }

    #[test]
    fn test_specialnet_stub_wire_is_an_error() {
        // A wire statement whose point list is a single coordinate pair and
        // no via name is a one-point stub.
        let msg = pdn_def_error(&PDN_DEF.replace(
            "NEW met2 0 + SHAPE STRIPE ( 1000 0 ) via12_inst",
            "NEW met2 0 + SHAPE STRIPE ( 1000 0 )",
        ));
        assert!(msg.contains("one-point"), "{msg}");
    }

    /// `- VPWR ( * VPWR )` wildcard conn plus wire statements used by the
    /// recurring-statement and SHIELD tests. met1 only.
    const SPECIAL_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN sp ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 2000 2000 ) ;
ROW ROW_0 unithd 0 0 N DO 1 BY 1 STEP 460 2720 ;
COMPONENTS 1 ;
- u1 INV + PLACED ( 0 0 ) N ;
END COMPONENTS
PINS 1 ;
- VPWR + NET VPWR + SPECIAL + DIRECTION INOUT + USE POWER
  + PORT
    + LAYER met1 ( -50 -50 ) ( 50 50 )
    + PLACED ( 100 1800 ) N ;
END PINS
SPECIALNETS 1 ;
- VPWR ( PIN VPWR ) ( * VPWR ) + USE POWER
  + ROUTED met1 100 + SHAPE FOLLOWPIN ( 0 0 ) ( 2000 0 )
  + FIXED met1 200 + SHAPE STRIPE ( 1000 0 ) ( 1000 2000 )
  + SHIELD sig1 met1 100 + SHAPE STRIPE ( 0 100 ) ( 2000 100 )
  + USE POWER
  ;
END SPECIALNETS
END DESIGN
"#;

    /// A second `+ ROUTED`/`+ FIXED` statement inside one net record, plus a
    /// net-level `+ USE` after the wires, must re-dispatch instead of
    /// erroring as an unknown wire attribute.
    #[test]
    fn test_special_wire_recurrent_statements() {
        let db = parse_def_bytes(SPECIAL_DEF.as_bytes(), &mini_tech(), None)
            .expect("recurring wire statements must parse");
        let net = db.pdn_net("VPWR").expect("VPWR");
        assert_eq!(net.use_kind, Some(PdnUse::Power));
        // ROUTED + FIXED + SHIELD bodies = three wire rects.
        assert_eq!(net.wire_rects.len(), 3);
    }

    /// `+ SHIELD` consumes its shield-net name, then carries the same wire
    /// body — it must not parse the name as a layer.
    #[test]
    fn test_special_wire_shield_parses() {
        let db = parse_def_bytes(SPECIAL_DEF.as_bytes(), &mini_tech(), None)
            .expect("SHIELD wire must parse");
        let net = db.pdn_net("VPWR").expect("VPWR");
        let rects = net
            .wire_rects
            .iter()
            .map(|(_, r)| (r.xl, r.yl, r.xh, r.yh))
            .collect::<Vec<_>>();
        // The shield stripe: met1 width 100 around y=100.
        assert!(rects.contains(&(0, 50, 2000, 150)), "shield rect missing: {rects:?}");
    }

    /// An odd word in the connection list errors loudly.
    #[test]
    fn test_special_net_malformed_conn_fails() {
        let def = SPECIAL_DEF.replace("( PIN VPWR ) ( * VPWR )", "( PIN VPWR ) ODDWORD");
        let err = mini_def_error(&def);
        assert!(err.contains("malformed connection"), "{err}");
    }

    #[test]
    fn test_gcd_specialnets() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("gcd").join("gcd.placed.def"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        assert_eq!(db.pdn_nets.len(), 2);
        let layer = |name: &str| {
            tech_db.layer_by_name[&tech_db.strings.get(name).expect(name)]
        };
        for (name, use_kind, top, wildcard, well) in [
            ("VPWR", PdnUse::Power, "VPWR", "VPWR", "VPB"),
            ("VGND", PdnUse::Ground, "VGND", "VGND", "VNB"),
        ] {
            let net = db.pdn_net(name).expect(name);
            assert_eq!(net.use_kind, Some(use_kind));
            assert_eq!(net.wire_rects.len(), 47, "{name}");
            assert_eq!(net.vias.len(), 46, "{name}");

            // 15 rails per routed layer met1..met3, one met4 stripe, one
            // met5 stripe.
            for (lname, want) in [("met1", 15), ("met2", 15), ("met3", 15), ("met4", 1), ("met5", 1)] {
                let got = net
                    .wire_rects
                    .iter()
                    .filter(|(l, _)| *l == layer(lname))
                    .count();
                assert_eq!(got, want, "{name} {lname}");
            }

            // Conns verbatim: ( PIN <net> ) ( * <well> ) ( * <net> ).
            let names = |s: SymbolId| db.strings.resolve(s);
            assert_eq!(net.conns.len(), 3, "{name}");
            assert!(matches!(&net.conns[0], PdnConn::TopPin(s) if names(*s) == top));
            assert!(matches!(&net.conns[1], PdnConn::AllInstances(s) if names(*s) == well));
            assert!(matches!(&net.conns[2], PdnConn::AllInstances(s) if names(*s) == wildcard));

            // Every via resolves through a VIAS `+ VIARULE` record: stacks
            // metN↔metN+1 with ROWCOL cut counts.
            for via in &net.vias {
                let vn = db.strings.resolve(via.name);
                let (want_cut, want_cc) = if vn.starts_with("via2_3") {
                    ("via", 5)
                } else if vn.starts_with("via3_4") {
                    ("via2", 4)
                } else if vn.starts_with("via4_5") {
                    ("via3", 4)
                } else if vn.starts_with("via5_6") {
                    ("via4", 1)
                } else {
                    panic!("unexpected via {vn}");
                };
                assert_eq!(db.strings.resolve(via.cut), want_cut, "{vn}");
                assert_eq!(via.cut_count, want_cc, "{vn}");
                assert_eq!(via.upper.0, via.lower.0 + 1, "{vn} is not adjacent");
            }
        }
    }

    /// Two `NETS` records exercising every retained construct: `+ ROUTED`
    /// paths with `*` coordinates, a LEF-via link, `+ FIXED`, and a `RECT`
    /// stub. Runs on `PDN_LEF` (met1/2/3 at WIDTH 0.14 → half 70,
    /// `stack_a` spans met1↔met2).
    const NETS_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN nets ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 100000 250000 ) ;
ROW ROW_0 unithd 0 0 N DO 4 BY 1 STEP 460 2720 ;
COMPONENTS 2 ;
- u1 CELL + PLACED ( 0 0 ) N ;
- u2 CELL + PLACED ( 500 0 ) N ;
END COMPONENTS
NETS 2 ;
- n1 ( u1 VPWR ) + USE SIGNAL
  + ROUTED met1 ( 0 0 ) ( 1000 0 )
  NEW met1 ( 500 0 ) stack_a
  NEW met2 ( 500 0 ) ( 500 500 )
  ;
- n2 ( u2 VPWR ) + FIXED met1 ( 0 100 ) ( * 200 ) ( 300 * )
  NEW met3 ( 94530 245140 ) RECT ( -620 -150 0 150 )
  ;
END NETS
END DESIGN
"#;

    #[test]
    fn test_nets_routed_geometry_retained() {
        let db = parse_def_bytes(NETS_DEF.as_bytes(), &pdn_tech(), None).expect("must parse");
        assert_eq!(db.num_nets(), 2);
        assert_eq!(db.net_geoms.len(), 2);

        let (met1, met2, met3) = (
            layer_id(&pdn_tech(), "met1"),
            layer_id(&pdn_tech(), "met2"),
            layer_id(&pdn_tech(), "met3"),
        );
        let n1 = db.net_geom(NetId(0)).expect("n1 geom");
        assert_eq!(
            n1.wire_rects,
            vec![
                // (0,0)-(1000,0) on met1, half-width 70, square caps.
                (met1, RectDbu::new(-70, -70, 1070, 70)),
                // (500,0)-(500,500) on met2.
                (met2, RectDbu::new(430, -70, 570, 570)),
            ]
        );
        assert_eq!(n1.vias.len(), 1);
        let via = &n1.vias[0];
        assert_eq!(db.strings.resolve(via.name), "stack_a");
        assert_eq!(via.lower, met1);
        assert_eq!(via.upper, met2);
        assert_eq!((via.x_dbu, via.y_dbu), (500, 0));
        assert!(matches!(via.geom, ViaGeom::Lef(_)));

        let n2 = db.net_geom(NetId(1)).expect("n2 geom");
        assert_eq!(
            n2.wire_rects,
            vec![
                // (0,100)-(0,200) vertical.
                (met1, RectDbu::new(-70, 30, 70, 270)),
                // (0,200)-(300,200) horizontal.
                (met1, RectDbu::new(-70, 130, 370, 270)),
                // `pt RECT ( dx1 dy1 dx2 dy2 )` offset, un-expanded.
                (met3, RectDbu::new(93910, 244990, 94530, 245290)),
            ]
        );
        assert!(n2.vias.is_empty());

        // Connectivity kept the old behavior: two pins, one per net.
        assert_eq!(db.net_pins(NetId(0)).len(), 1);
        assert_eq!(db.net_pins(NetId(1)).len(), 1);
    }

    /// A reparsed DEF must produce an identical `net_geoms` — emission is
    /// fully input-determined.
    #[test]
    fn test_nets_geometry_is_deterministic() {
        let tech = pdn_tech();
        let a = parse_def_bytes(NETS_DEF.as_bytes(), &tech, None).expect("a");
        let b = parse_def_bytes(NETS_DEF.as_bytes(), &tech, None).expect("b");
        assert_eq!(a.net_geoms, b.net_geoms);
    }

    /// `+ COVER` wires and `VIRTUAL` points parse without emitting extra
    /// shapes; `+ SHIELD` and net attributes are consumed.
    #[test]
    fn test_nets_cover_virtual_and_attributes() {
        let def = NETS_DEF.replace(
            "+ ROUTED met1 ( 0 0 ) ( 1000 0 )",
            "+ COVER met1 ( 0 0 ) VIRTUAL ( 400 0 ) ( 1000 0 ) + WEIGHT 7 + ESTCAP 1.5",
        );
        let db = parse_def_bytes(def.as_bytes(), &pdn_tech(), None).expect("must parse");
        let n1 = db.net_geom(NetId(0)).expect("n1 geom");
        // VIRTUAL leaves no vertex: the met1 wire is one (0,0)-(1000,0)
        // segment; the met2 wire is unchanged.
        assert_eq!(
            n1.wire_rects,
            vec![
                (layer_id(&pdn_tech(), "met1"), RectDbu::new(-70, -70, 1070, 70)),
                (layer_id(&pdn_tech(), "met2"), RectDbu::new(430, -70, 570, 570)),
            ]
        );
    }

    #[test]
    fn test_nets_via_unknown_name_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "( 500 0 ) stack_a",
            "( 500 0 ) no_such_via",
        ));
        assert!(msg.contains("no_such_via") && msg.contains("n1"), "{msg}");
    }

    #[test]
    fn test_nets_via_on_foreign_layer_is_an_error() {
        // `stack_a` spans met1↔met2; recording it on met3 is inconsistent.
        let msg = pdn_def_error(&NETS_DEF.replace(
            "NEW met1 ( 500 0 ) stack_a",
            "NEW met3 ( 500 0 ) stack_a",
        ));
        assert!(msg.contains("met3") && msg.contains("neither of its endpoints"), "{msg}");
    }

    #[test]
    fn test_nets_rect_before_point_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "NEW met3 ( 94530 245140 ) RECT ( -620 -150 0 150 )",
            "NEW met3 RECT ( -620 -150 0 150 ) ( 94530 245140 )",
        ));
        assert!(msg.contains("RECT") && msg.contains("before any point"), "{msg}");
    }

    #[test]
    fn test_nets_one_point_stub_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "NEW met2 ( 500 0 ) ( 500 500 )",
            "NEW met2 ( 500 0 )",
        ));
        assert!(msg.contains("one-point"), "{msg}");
    }

    #[test]
    fn test_nets_ext_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "+ ROUTED met1 ( 0 0 ) ( 1000 0 )",
            "+ ROUTED met1 ( 0 0 ) ( 1000 0 ) + EXT 50",
        ));
        assert!(msg.contains("EXT"), "{msg}");
    }

    /// A net naming a rule `NONDEFAULTRULES` never defined errors loudly.
    #[test]
    fn test_nets_nondefaultrule_unknown_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "+ USE SIGNAL",
            "+ USE SIGNAL + NONDEFAULTRULE wide",
        ));
        assert!(msg.contains("not defined in NONDEFAULTRULES"), "{msg}");
    }

    #[test]
    fn test_nets_subnet_is_an_error() {
        let msg = pdn_def_error(&NETS_DEF.replace(
            "+ USE SIGNAL",
            "+ USE SIGNAL + SUBNET n1_a ( u1 VPWR )",
        ));
        assert!(msg.contains("SUBNET"), "{msg}");
    }

    /// A `+ VIARULE` via cannot be expanded without `+ CUTSIZE` — this is
    /// an unsupported-input error, not a lookup miss.
    #[test]
    fn test_generated_via_without_cutsize_is_an_error() {
        let def = PDN_DEF.replace(
            "- via12_inst + VIA stack_a ;",
            "- via12_inst + VIARULE RULE + LAYERS met1 via12 met2 + ROWCOL 1 3 ;",
        );
        let msg = pdn_def_error(&def);
        assert!(msg.contains("via12_inst") && msg.contains("CUTSIZE"), "{msg}");
    }

    /// Generated-via params are retained on `ViaGeom::Generated`.
    #[test]
    fn test_generated_via_params_retained() {
        let def = PDN_DEF.replace(
            "- via12_inst + VIA stack_a ;",
            "- via12_inst + VIARULE RULE + CUTSIZE 150 150 + LAYERS met1 via12 met2 + CUTSPACING 170 170 + ENCLOSURE 85 165 55 85 + ROWCOL 1 3 ;",
        );
        let db = parse_def_bytes(def.as_bytes(), &pdn_tech(), None).expect("must parse");
        let via = &db.pdn_nets[0].vias[0];
        assert_eq!(via.cut_count, 3);
        let ViaGeom::Generated(g) = &via.geom else {
            panic!("expected Generated geom: {:?}", via.geom);
        };
        assert_eq!((g.cut_w_dbu, g.cut_h_dbu), (150, 150));
        assert_eq!((g.rows, g.cols), (1, 3));
        assert_eq!((g.cut_spacing_x_dbu, g.cut_spacing_y_dbu), (170, 170));
        assert_eq!(
            (
                g.enc_bot_x_dbu,
                g.enc_bot_y_dbu,
                g.enc_top_x_dbu,
                g.enc_top_y_dbu
            ),
            (85, 165, 55, 85)
        );
    }
}
