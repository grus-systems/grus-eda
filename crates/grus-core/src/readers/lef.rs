use crate::interner::{LayerId, MacroId, PinDir, StringInterner};
use crate::tech_db::{
    CutLayer, LayerDir, MacroClass, MacroDef, MacroPin, RectDbu, RoutingLayer, TechDb, ViaDef,
    ViaLayerRect, ViaRectLayer,
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

/// Top-level LEF statements the reader skipped, counted by uppercased keyword.
/// `skipped` is empty when nothing was skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LefSkipReport {
    pub skipped: BTreeMap<String, usize>,
}

impl LefSkipReport {
    /// Adds `other`'s counts into `self` (used to merge .tlef + .lef reports).
    pub fn merge(&mut self, other: &LefSkipReport) {
        for (k, v) in &other.skipped {
            *self.skipped.entry(k.clone()).or_default() += v;
        }
    }

    /// Sum of all per-keyword counts.
    pub fn total(&self) -> usize {
        self.skipped.values().sum()
    }
}

fn count_skip(report: &mut LefSkipReport, keyword: &str) {
    *report.skipped.entry(keyword.to_uppercase()).or_default() += 1;
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Token<'a> {
    Word(&'a str),
    Str(&'a str),
    Semi,
}

pub(crate) struct LefLexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> LefLexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
        }
    }

    /// One-based line of the read position, for error messages. Counted incrementally:
    /// rescanning the prefix on every call made large LEFs quadratic.
    pub fn line(&self) -> usize {
        self.line
    }

    pub fn peek_token(&self) -> Option<Token<'a>> {
        let mut cloned = Self {
            input: self.input,
            pos: self.pos,
            line: self.line,
        };
        cloned.next_token()
    }

    pub(crate) fn next_token(&mut self) -> Option<Token<'a>> {
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
                return Some(Token::Semi);
            }
            if b == b'"' {
                self.pos += 1;
                let start = self.pos;
                while self.pos < len && bytes[self.pos] != b'"' {
                    if bytes[self.pos] == b'\n' {
                        self.line += 1;
                    }
                    self.pos += 1;
                }
                // Quotes and the delimiters below are ASCII, so the slice is on char boundaries.
                let s = &self.input[start..self.pos];
                if self.pos < len && bytes[self.pos] == b'"' {
                    self.pos += 1;
                }
                return Some(Token::Str(s));
            }

            // Word or number up to whitespace, comment, or semicolon
            let start = self.pos;
            while self.pos < len {
                let curr = bytes[self.pos];
                if curr.is_ascii_whitespace() || curr == b';' || curr == b'#' {
                    break;
                }
                self.pos += 1;
            }
            return Some(Token::Word(&self.input[start..self.pos]));
        }
        None
    }

    pub fn skip_to_semi(&mut self) {
        while let Some(tok) = self.next_token() {
            if tok == Token::Semi {
                break;
            }
        }
    }

    pub fn next_word(&mut self) -> Option<&'a str> {
        match self.next_token() {
            Some(Token::Word(w) | Token::Str(w)) => Some(w),
            _ => None,
        }
    }
}

/// The next word, which the construct cannot do without.
fn required_word<'a>(lexer: &mut LefLexer<'a>, construct: &str) -> Result<&'a str> {
    match lexer.next_token() {
        Some(Token::Word(w) | Token::Str(w)) => Ok(w),
        other => bail!(
            "LEF line {}: {construct} is missing its value (found {other:?})",
            lexer.line()
        ),
    }
}

fn required_f32(lexer: &mut LefLexer, construct: &str) -> Result<f32> {
    let w = required_word(lexer, construct)?;
    w.parse::<f32>().with_context(|| {
        format!(
            "LEF line {}: {construct} value `{w}` is not a number",
            lexer.line()
        )
    })
}

fn required_i32(lexer: &mut LefLexer, construct: &str) -> Result<i32> {
    let w = required_word(lexer, construct)?;
    w.parse::<i32>().with_context(|| {
        format!(
            "LEF line {}: {construct} value `{w}` is not an integer",
            lexer.line()
        )
    })
}

/// `w BY h` after a `SIZE` keyword; both numbers are required by the LEF grammar.
fn parse_size(lexer: &mut LefLexer, construct: &str) -> Result<(f32, f32)> {
    let w = required_f32(lexer, &format!("{construct} SIZE width"))?;
    let by = required_word(lexer, &format!("{construct} SIZE"))?;
    if !by.eq_ignore_ascii_case("BY") {
        bail!(
            "LEF line {}: {construct} SIZE expects BY, found `{by}`",
            lexer.line()
        );
    }
    let h = required_f32(lexer, &format!("{construct} SIZE height"))?;
    Ok((w, h))
}

/// Consumes the `END` that was just read. A named block must be closed by its own name; an
/// optional `;` after it is consumed.
fn end_block(lexer: &mut LefLexer, block: &str, name: Option<&str>) -> Result<()> {
    if let Some(exp) = name {
        match lexer.next_token() {
            Some(Token::Word(w)) if w.eq_ignore_ascii_case(exp) => {}
            other => bail!(
                "LEF line {}: expected `END {exp}` closing {block}, found {other:?}",
                lexer.line()
            ),
        }
    }
    if let Some(Token::Semi) = lexer.peek_token() {
        let _ = lexer.next_token();
    }
    Ok(())
}

/// Skips a block whose contents are not modeled, through its `END <block_name>`.
fn skip_block(lexer: &mut LefLexer, block_name: &str) -> Result<()> {
    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok
            && w.eq_ignore_ascii_case("END")
            && let Some(Token::Word(end_name)) = lexer.peek_token()
            && end_name.eq_ignore_ascii_case(block_name)
        {
            let _ = lexer.next_token();
            if let Some(Token::Semi) = lexer.peek_token() {
                let _ = lexer.next_token();
            }
            return Ok(());
        }
    }
    bail!("LEF: {block_name} block is not terminated by END {block_name} (truncated file?)")
}

/// Parses a Technology LEF (.tlef) or Macro LEF (.lef) file bytes into `TechDb`,
/// reporting the top-level statements it skipped.
pub fn parse_lef_bytes(
    bytes: &[u8],
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<LefSkipReport> {
    if tech_db.dbu_per_micron == 0 {
        tech_db.dbu_per_micron = 1000;
    }

    let text = std::str::from_utf8(bytes).context("LEF is not valid UTF-8")?;
    let mut lexer = LefLexer::new(text);

    // Top-level statements we do not model (VERSION, BUSBITCHARS, DIVIDERCHAR, MANUFACTURINGGRID,
    // NAMESCASESENSITIVE, ...) fall through to the `skip_to_semi` at the end of the chain. The
    // top-level keyword set is open-ended in LEF, so unknown ones are skipped, not rejected.
    let mut report = LefSkipReport::default();
    while let Some(tok) = lexer.next_token() {
        let word = match tok {
            Token::Word(w) => w,
            _ => continue,
        };

        if word.eq_ignore_ascii_case("UNITS") {
            parse_units(&mut lexer, tech_db)?;
        } else if word.eq_ignore_ascii_case("SITE") {
            parse_site(&mut lexer, tech_db)?;
        } else if word.eq_ignore_ascii_case("LAYER") {
            parse_layer(&mut lexer, tech_db, interner)?;
        } else if word.eq_ignore_ascii_case("MACRO") {
            parse_macro(&mut lexer, tech_db, interner)?;
        } else if word.eq_ignore_ascii_case("PROPERTYDEFINITIONS") {
            skip_block(&mut lexer, "PROPERTYDEFINITIONS")?;
            count_skip(&mut report, word);
        } else if word.eq_ignore_ascii_case("VIA") {
            parse_via(&mut lexer, tech_db, interner)?;
        } else if word.eq_ignore_ascii_case("VIARULE")
            || word.eq_ignore_ascii_case("NONDEFAULTRULE")
        {
            count_skip(&mut report, word);
            let name = required_word(&mut lexer, word)?.to_string();
            skip_block(&mut lexer, &name)?;
        } else if word.eq_ignore_ascii_case("SPACING") {
            skip_block(&mut lexer, "SPACING")?;
            count_skip(&mut report, word);
        } else if word.eq_ignore_ascii_case("END") {
            let _ = lexer.next_word();
            if let Some(Token::Semi) = lexer.peek_token() {
                let _ = lexer.next_token();
            }
        } else {
            count_skip(&mut report, word);
            lexer.skip_to_semi();
        }
    }

    Ok(report)
}

fn parse_units(lexer: &mut LefLexer, tech_db: &mut TechDb) -> Result<()> {
    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "UNITS", Some("UNITS"))?;
                return Ok(());
            } else if w.eq_ignore_ascii_case("DATABASE") {
                if required_word(lexer, "UNITS DATABASE")?.eq_ignore_ascii_case("MICRONS") {
                    tech_db.dbu_per_micron = required_i32(lexer, "UNITS DATABASE MICRONS")?;
                }
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    bail!("LEF: UNITS is not terminated by END UNITS (truncated file?)")
}

fn parse_site(lexer: &mut LefLexer, tech_db: &mut TechDb) -> Result<()> {
    let site_id = required_word(lexer, "SITE name")?.to_string();
    let mut is_core = false;
    let mut size_um = None;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "SITE", Some(&site_id))?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("CLASS") {
                if let Some(c) = lexer.next_word()
                    && c.eq_ignore_ascii_case("CORE")
                {
                    is_core = true;
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("SIZE") {
                size_um = Some(parse_size(lexer, &format!("SITE {site_id}"))?);
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: SITE {site_id} is not terminated by END {site_id} (truncated file?)");
    }
    let Some((w_um, h_um)) = size_um else {
        bail!("LEF: SITE {site_id} has no SIZE");
    };

    let dbu = tech_db.dbu_per_micron as f32;
    if is_core && (tech_db.site_name.is_empty() || site_id.eq_ignore_ascii_case("unithd")) {
        tech_db.site_name = site_id;
        tech_db.site_width_dbu = (w_um * dbu).round() as i32;
        tech_db.site_height_dbu = (h_um * dbu).round() as i32;
    }

    Ok(())
}

fn parse_layer(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let layer_name = required_word(lexer, "LAYER name")?.to_string();
    let mut is_routing = false;
    let mut is_cut = false;
    let mut dir = LayerDir::Horizontal;
    let mut pitch_x_um = None;
    let mut pitch_y_um = None;
    let mut offset_x_um = None;
    let mut offset_y_um = None;
    let mut width_um = None;
    let mut res_per_sq = 0.0f32;
    let mut cut_res_ohm = None;
    let mut cap_per_sq_um = 0.0f32;
    let mut edge_cap_per_um = 0.0f32;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "LAYER", Some(&layer_name))?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("TYPE") {
                if let Some(t) = lexer.next_word() {
                    if t.eq_ignore_ascii_case("ROUTING") {
                        is_routing = true;
                    } else if t.eq_ignore_ascii_case("CUT") {
                        is_cut = true;
                    }
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("DIRECTION") {
                if let Some(d) = lexer.next_word() {
                    if d.eq_ignore_ascii_case("VERTICAL") {
                        dir = LayerDir::Vertical;
                    } else if d.eq_ignore_ascii_case("HORIZONTAL") {
                        dir = LayerDir::Horizontal;
                    }
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("PITCH") {
                pitch_x_um = Some(required_f32(lexer, "LAYER PITCH")?);
                // The second value (y pitch) is optional.
                if let Some(Token::Word(p2_str)) = lexer.peek_token()
                    && let Ok(p2) = p2_str.parse::<f32>()
                {
                    let _ = lexer.next_token();
                    pitch_y_um = Some(p2);
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("OFFSET") {
                offset_x_um = Some(required_f32(lexer, "LAYER OFFSET")?);
                // The second value (y offset) is optional.
                if let Some(Token::Word(o2_str)) = lexer.peek_token()
                    && let Ok(o2) = o2_str.parse::<f32>()
                {
                    let _ = lexer.next_token();
                    offset_y_um = Some(o2);
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("WIDTH") {
                width_um = Some(required_f32(lexer, "LAYER WIDTH")?);
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("RESISTANCE") {
                if let Some(r_kw) = lexer.next_word() {
                    if r_kw.eq_ignore_ascii_case("RPERSQ") {
                        res_per_sq = required_f32(lexer, "LAYER RESISTANCE RPERSQ")?;
                    } else if let Ok(r) = r_kw.parse::<f32>() {
                        // A bare `RESISTANCE <ohms> ;` is the per-cut value on
                        // CUT layers — the via resistance guide parasitics read.
                        cut_res_ohm = Some(r);
                    }
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("CAPACITANCE") {
                if let Some(c_kw) = lexer.next_word()
                    && c_kw.eq_ignore_ascii_case("CPERSQDIST")
                {
                    cap_per_sq_um = required_f32(lexer, "LAYER CAPACITANCE CPERSQDIST")?;
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("EDGECAPACITANCE") {
                edge_cap_per_um = required_f32(lexer, "LAYER EDGECAPACITANCE")?;
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: LAYER {layer_name} is not terminated by END {layer_name} (truncated file?)");
    }

    if is_cut && !tech_db.routing_layers.is_empty() {
        // The cut layer sits above the routing layer parsed just before it;
        // `load_tech_db` resolves the resistance into `via_res_ohm_above`.
        // A cut layer before the first routing layer (`licon`) has no `above`
        // and is not recorded.
        let below = LayerId(tech_db.routing_layers.len() as u8 - 1);
        let cut_sym = interner.intern(&layer_name);
        let cut_idx = tech_db.cut_layers.len() as u32;
        let dbu = tech_db.dbu_per_micron as f32;
        tech_db.cut_layers.push(CutLayer {
            name: cut_sym,
            above: below,
            width_dbu: width_um.map(|w| (w * dbu).round() as i32).unwrap_or(0),
        });
        tech_db.cut_layer_by_name.insert(cut_sym, cut_idx);
        if let Some(res) = cut_res_ohm {
            tech_db.cut_res_above.push((below.0, res));
        }
    }

    if is_routing {
        let dbu = tech_db.dbu_per_micron as f32;
        let width_um =
            width_um.with_context(|| format!("LEF: routing layer {layer_name} has no WIDTH"))?;
        let pitch_um = match dir {
            LayerDir::Vertical => pitch_x_um.or(pitch_y_um),
            LayerDir::Horizontal => pitch_y_um.or(pitch_x_um),
        }
        .with_context(|| format!("LEF: routing layer {layer_name} has no PITCH"))?;
        // OFFSET is optional; an absent OFFSET is the LEF default of 0.
        let offset_um = match dir {
            LayerDir::Vertical => offset_x_um.or(offset_y_um),
            LayerDir::Horizontal => offset_y_um.or(offset_x_um),
        }
        .unwrap_or(0.0);

        let pitch_dbu = (pitch_um * dbu).round() as i32;
        let offset_dbu = (offset_um * dbu).round() as i32;
        let width_dbu = (width_um * dbu).round() as i32;

        let layer_sym = interner.intern(&layer_name);
        let layer_id = LayerId(tech_db.routing_layers.len() as u8);
        tech_db.layer_by_name.insert(layer_sym, layer_id);

        tech_db.routing_layers.push(RoutingLayer {
            name: layer_sym,
            dir,
            pitch_dbu,
            width_dbu,
            offset_dbu,
            res_per_sq,
            cap_per_sq_um,
            edge_cap_per_um,
        });
    }

    Ok(())
}

/// A `VIA` block: `LAYER`/`RECT` groups plus attributes the model does not
/// keep. Every `RECT` is retained in group order — the PDN model reads the
/// cut count and layer stack; the DRC shape model places all of them.
fn parse_via(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let via_name = required_word(lexer, "VIA name")?.to_string();
    // (name sym, routing layer, cut-layer index, rects) per LAYER group.
    struct ViaGroup {
        name: crate::interner::SymbolId,
        routing: Option<LayerId>,
        cut: Option<u32>,
        rects: Vec<RectDbu>,
    }
    let mut groups: Vec<ViaGroup> = Vec::new();
    let dbu = tech_db.dbu_per_micron as f32;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "VIA", Some(&via_name))?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("LAYER") {
                let lname = required_word(lexer, "VIA LAYER")?.to_string();
                let lsym = interner.intern(&lname);
                let routing = tech_db.layer_by_name.get(&lsym).copied();
                let cut = tech_db.cut_layer_by_name.get(&lsym).copied();
                if routing.is_none() && cut.is_none() {
                    bail!(
                        "LEF line {}: VIA {via_name} LAYER {lname} is neither a routing nor a cut layer",
                        lexer.line()
                    );
                }
                groups.push(ViaGroup {
                    name: lsym,
                    routing,
                    cut,
                    rects: Vec::new(),
                });
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("RECT") {
                let Some(group) = groups.last_mut() else {
                    bail!(
                        "LEF line {}: VIA {via_name} RECT before any LAYER",
                        lexer.line()
                    );
                };
                let x1 = required_f32(lexer, "VIA RECT x1")?;
                let y1 = required_f32(lexer, "VIA RECT y1")?;
                let x2 = required_f32(lexer, "VIA RECT x2")?;
                let y2 = required_f32(lexer, "VIA RECT y2")?;
                let x1_dbu = (x1 * dbu).round() as i32;
                let y1_dbu = (y1 * dbu).round() as i32;
                let x2_dbu = (x2 * dbu).round() as i32;
                let y2_dbu = (y2 * dbu).round() as i32;
                group.rects.push(RectDbu::new(
                    x1_dbu.min(x2_dbu),
                    y1_dbu.min(y2_dbu),
                    x1_dbu.max(x2_dbu),
                    y1_dbu.max(y2_dbu),
                ));
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("DEFAULT") || w.eq_ignore_ascii_case("TOPOFSTACKONLY")
            {
                // Bare markers with no `;`-terminated statement.
            } else {
                // FOREIGN, PATTERN, RESISTANCE, PROPERTY, MASK, …: `;`-terminated
                // attributes the model does not read.
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: VIA {via_name} is not terminated by END {via_name} (truncated file?)");
    }

    if groups.is_empty() {
        // An empty `VIA name END name` block records nothing.
        return Ok(());
    }
    let routing: Vec<LayerId> = groups.iter().filter_map(|g| g.routing).collect();
    let cuts: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.cut.is_some())
        .map(|(i, _)| i)
        .collect();
    if routing.len() < 2 || cuts.is_empty() {
        bail!("LEF: VIA {via_name} needs at least two routing layers and one cut layer");
    }

    let via_sym = interner.intern(&via_name);
    if tech_db.via_by_name.contains_key(&via_sym) {
        bail!("LEF: VIA {via_name} is defined more than once");
    }
    let rects: Vec<ViaLayerRect> = groups
        .iter()
        .flat_map(|g| {
            let layer = g
                .routing
                .map_or_else(|| ViaRectLayer::Cut(g.cut.unwrap()), ViaRectLayer::Routing);
            g.rects.iter().map(move |&rect| ViaLayerRect { layer, rect })
        })
        .collect();
    let idx = tech_db.via_defs.len() as u32;
    tech_db.via_defs.push(ViaDef {
        name: via_sym,
        lower: *routing.iter().min_by_key(|l| l.0).unwrap(),
        upper: *routing.iter().max_by_key(|l| l.0).unwrap(),
        cut: groups[cuts[0]].name,
        cut_count: cuts.iter().map(|&i| groups[i].rects.len() as u32).sum(),
        rects,
    });
    tech_db.via_by_name.insert(via_sym, idx);
    Ok(())
}

fn parse_macro(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let macro_name = required_word(lexer, "MACRO name")?.to_string();
    let mac_sym = interner.intern(&macro_name);
    let pin_offset = tech_db.macro_pins.len() as u32;

    let mut class = MacroClass::Core;
    let mut size_dbu = None;
    let mut obs_rects = Vec::new();
    let mut pin_obs_rects = Vec::new();
    let dbu = tech_db.dbu_per_micron as f32;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "MACRO", Some(&macro_name))?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("CLASS") {
                let mut c1 = String::new();
                let mut c2 = String::new();
                while let Some(arg_tok) = lexer.next_token() {
                    match arg_tok {
                        Token::Semi => break,
                        Token::Word(arg) => {
                            if c1.is_empty() {
                                c1 = arg.to_string();
                            } else if c2.is_empty() {
                                c2 = arg.to_string();
                            }
                        }
                        // Quoted or otherwise unexpected tokens in CLASS are ignored: the class
                        // name is only the first one or two words.
                        Token::Str(_) => {}
                    }
                }
                class = parse_macro_class(&c1, &c2);
            } else if w.eq_ignore_ascii_case("SIZE") {
                let (w_um, h_um) = parse_size(lexer, &format!("MACRO {macro_name}"))?;
                lexer.skip_to_semi();
                size_dbu = Some(((w_um * dbu).round() as i32, (h_um * dbu).round() as i32));
            } else if w.eq_ignore_ascii_case("PIN") {
                parse_pin(lexer, tech_db, interner, &mut pin_obs_rects)?;
            } else if w.eq_ignore_ascii_case("OBS") {
                parse_obs(lexer, tech_db, interner, &mut obs_rects)?;
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: MACRO {macro_name} is not terminated by END {macro_name} (truncated file?)");
    }
    let Some((width_dbu, height_dbu)) = size_dbu else {
        bail!("LEF: MACRO {macro_name} has no SIZE");
    };

    merge_connected_obs(&mut obs_rects);

    let pin_count = (tech_db.macro_pins.len() as u32 - pin_offset) as u16;
    let mac_id = MacroId(tech_db.macros.len() as u16);
    tech_db.macro_by_name.insert(mac_sym, mac_id);

    tech_db.macros.push(MacroDef {
        name: mac_sym,
        class,
        width_dbu,
        height_dbu,
        pin_offset,
        pin_count,
        obs_rects,
        pin_obs_rects,
    });

    Ok(())
}

/// Merges touching or overlapping obstruction rectangles on the same layer until fixed point.
fn merge_connected_obs(obs: &mut Vec<(LayerId, RectDbu)>) {
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < obs.len() {
            let mut j = i + 1;
            while j < obs.len() {
                if obs[i].0 == obs[j].0 {
                    let a = obs[i].1;
                    let b = obs[j].1;
                    if a.xl <= b.xh && b.xl <= a.xh && a.yl <= b.yh && b.yl <= a.yh {
                        obs[i].1 = RectDbu::new(
                            a.xl.min(b.xl),
                            a.yl.min(b.yl),
                            a.xh.max(b.xh),
                            a.yh.max(b.yh),
                        );
                        obs.remove(j);
                        changed = true;
                        continue;
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }
}

fn parse_macro_class(c1: &str, c2: &str) -> MacroClass {
    if c1.eq_ignore_ascii_case("CORE") {
        if c2.eq_ignore_ascii_case("SPACER") {
            MacroClass::CoreSpacer
        } else if c2.eq_ignore_ascii_case("WELLTAP") {
            MacroClass::CoreWellTap
        } else if c2.eq_ignore_ascii_case("TIEHIGH") || c2.eq_ignore_ascii_case("TIELOW") {
            MacroClass::CoreTie
        } else {
            MacroClass::Core
        }
    } else if c1.eq_ignore_ascii_case("ENDCAP") {
        MacroClass::EndCap
    } else if c1.eq_ignore_ascii_case("BLOCK") {
        MacroClass::Block
    } else if c1.eq_ignore_ascii_case("PAD") {
        MacroClass::Pad
    } else {
        MacroClass::Other
    }
}

/// Floor of `sum_twice / (2 * count)` and whether a remainder was dropped.
fn mean_center(sum_twice: i64, count: i64) -> (i32, bool) {
    if count == 0 {
        return (0, false);
    }
    let den = 2 * count;
    (
        sum_twice.div_euclid(den) as i32,
        sum_twice.rem_euclid(den) != 0,
    )
}

fn parse_pin(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
    pin_obs_rects: &mut Vec<(LayerId, RectDbu)>,
) -> Result<()> {
    let pin_name = required_word(lexer, "PIN name")?.to_string();
    let pin_sym = interner.intern(&pin_name);
    let mut dir = PinDir::Input;
    let mut is_power = false;
    let mut pin_layer = None;
    let mut local_pin_rects: Vec<(LayerId, RectDbu)> = Vec::new();

    let mut sum_twice_center_x: i64 = 0;
    let mut sum_twice_center_y: i64 = 0;
    let mut rect_count: i64 = 0;
    let dbu = tech_db.dbu_per_micron as f32;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "PIN", Some(&pin_name))?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("DIRECTION") {
                if let Some(d) = lexer.next_word() {
                    dir = if d.eq_ignore_ascii_case("OUTPUT")
                        || d.eq_ignore_ascii_case("OUTPUT TRISTATE")
                    {
                        PinDir::Output
                    } else if d.eq_ignore_ascii_case("INOUT") {
                        PinDir::Inout
                    } else if d.eq_ignore_ascii_case("INTERNAL") {
                        PinDir::Internal
                    } else {
                        PinDir::Input
                    };
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("USE") {
                if let Some(u) = lexer.next_word()
                    && (u.eq_ignore_ascii_case("POWER") || u.eq_ignore_ascii_case("GROUND"))
                {
                    is_power = true;
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("PORT") {
                let mut current_port_layer = None;
                while let Some(port_tok) = lexer.next_token() {
                    if let Token::Word(pw) = port_tok {
                        if pw.eq_ignore_ascii_case("END") {
                            end_block(lexer, "PORT", None)?;
                            break;
                        } else if pw.eq_ignore_ascii_case("LAYER") {
                            let lname = required_word(lexer, "PIN PORT LAYER")?;
                            let lsym = interner.intern(lname);
                            let l_id = tech_db.layer_by_name.get(&lsym).copied();
                            current_port_layer = l_id;
                            if pin_layer.is_none() && l_id.is_some() {
                                pin_layer = l_id;
                            }
                            lexer.skip_to_semi();
                        } else if pw.eq_ignore_ascii_case("RECT") {
                            let x1 = required_f32(lexer, "PIN PORT RECT x1")?;
                            let y1 = required_f32(lexer, "PIN PORT RECT y1")?;
                            let x2 = required_f32(lexer, "PIN PORT RECT x2")?;
                            let y2 = required_f32(lexer, "PIN PORT RECT y2")?;

                            let x1_dbu = (x1 * dbu).round() as i32;
                            let y1_dbu = (y1 * dbu).round() as i32;
                            let x2_dbu = (x2 * dbu).round() as i32;
                            let y2_dbu = (y2 * dbu).round() as i32;

                            if let Some(layer_id) = current_port_layer {
                                local_pin_rects.push((
                                    layer_id,
                                    RectDbu::new(
                                        x1_dbu.min(x2_dbu),
                                        y1_dbu.min(y2_dbu),
                                        x1_dbu.max(x2_dbu),
                                        y1_dbu.max(y2_dbu),
                                    ),
                                ));
                            }

                            sum_twice_center_x += (x1_dbu + x2_dbu) as i64;
                            sum_twice_center_y += (y1_dbu + y2_dbu) as i64;
                            rect_count += 1;
                            lexer.skip_to_semi();
                        } else {
                            lexer.skip_to_semi();
                        }
                    }
                }
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: PIN {pin_name} is not terminated by END {pin_name} (truncated file?)");
    }

    let pin_rects = local_pin_rects.clone();
    if !local_pin_rects.is_empty() {
        merge_connected_obs(&mut local_pin_rects);
        pin_obs_rects.extend(local_pin_rects);
    }

    let (offset_x_dbu, offset_x_frac) = mean_center(sum_twice_center_x, rect_count);
    let (offset_y_dbu, offset_y_frac) = mean_center(sum_twice_center_y, rect_count);

    tech_db.macro_pins.push(MacroPin {
        name: pin_sym,
        dir,
        is_power,
        offset_x_dbu,
        offset_y_dbu,
        offset_x_frac,
        offset_y_frac,
        layer: pin_layer,
        rects: pin_rects,
    });

    Ok(())
}

fn parse_obs(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
    obs_rects: &mut Vec<(LayerId, RectDbu)>,
) -> Result<()> {
    let mut current_obs_layer = None;
    let dbu = tech_db.dbu_per_micron as f32;
    let mut closed = false;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                end_block(lexer, "OBS", None)?;
                closed = true;
                break;
            } else if w.eq_ignore_ascii_case("LAYER") {
                let lname = required_word(lexer, "OBS LAYER")?;
                let lsym = interner.intern(lname);
                current_obs_layer = tech_db.layer_by_name.get(&lsym).copied();
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("RECT") {
                let x1 = required_f32(lexer, "OBS RECT x1")?;
                let y1 = required_f32(lexer, "OBS RECT y1")?;
                let x2 = required_f32(lexer, "OBS RECT x2")?;
                let y2 = required_f32(lexer, "OBS RECT y2")?;

                if let Some(layer_id) = current_obs_layer {
                    let x1_dbu = (x1 * dbu).round() as i32;
                    let y1_dbu = (y1 * dbu).round() as i32;
                    let x2_dbu = (x2 * dbu).round() as i32;
                    let y2_dbu = (y2 * dbu).round() as i32;

                    obs_rects.push((
                        layer_id,
                        RectDbu::new(
                            x1_dbu.min(x2_dbu),
                            y1_dbu.min(y2_dbu),
                            x1_dbu.max(x2_dbu),
                            y1_dbu.max(y2_dbu),
                        ),
                    ));
                }
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    if !closed {
        bail!("LEF: OBS is not terminated by END (truncated file?)");
    }
    Ok(())
}

/// Parses a single LEF file into an existing `TechDb`, reporting the top-level
/// statements it skipped.
pub fn parse_lef(
    path: &Path,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<LefSkipReport> {
    let file =
        File::open(path).with_context(|| format!("Failed to open LEF file at {:?}", path))?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    let report = parse_lef_bytes(&mmap, tech_db, interner)
        .with_context(|| format!("Failed to parse LEF file at {:?}", path))?;
    tech_db.strings = interner.clone();
    tech_db.rebuild_maps();
    Ok(report)
}

/// Convenience function to parse both Technology LEF (.tlef) and Macro LEF (.lef) into a
/// new `TechDb`, with the merged skip report of both files.
pub fn load_tech_db(
    tlef_path: &Path,
    lef_path: &Path,
    interner: &mut StringInterner,
) -> Result<(TechDb, LefSkipReport)> {
    let mut tech_db = TechDb::new();
    let mut report = parse_lef(tlef_path, &mut tech_db, interner)?;
    report.merge(&parse_lef(lef_path, &mut tech_db, interner)?);
    // The first cut layer above a routing layer is the via between it and
    // the next routing layer; later cuts between the same pair stack higher.
    tech_db.via_res_ohm_above = vec![0.0; tech_db.routing_layers.len()];
    for (below, res) in std::mem::take(&mut tech_db.cut_res_above) {
        let slot = &mut tech_db.via_res_ohm_above[below as usize];
        if *slot == 0.0 {
            *slot = res;
        }
    }
    tech_db.strings = interner.clone();
    tech_db.rebuild_maps();
    Ok((tech_db, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, sky130_hd};

    const SAMPLE_TLEF: &str = r#"
VERSION 5.7 ;
BUSBITCHARS "[]" ;
DIVIDERCHAR "/" ;

UNITS
  DATABASE MICRONS 1000 ;
END UNITS

SITE unithd
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
END unithd

LAYER li1
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 0.34 ;
  OFFSET 0.23 0.17 ;
  WIDTH 0.17 ;
  RESISTANCE RPERSQ 12.8 ;
  CAPACITANCE CPERSQDIST 36.9866E-6 ;
  EDGECAPACITANCE 40.697E-6 ;
END li1

LAYER met1
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  OFFSET 0.17 ;
  WIDTH 0.14 ;
  RESISTANCE RPERSQ 0.125 ;
  CAPACITANCE CPERSQDIST 25.7784E-6 ;
  EDGECAPACITANCE 40.567E-6 ;
END met1
"#;

    const SAMPLE_LEF: &str = r#"
VERSION 5.7 ;
MACRO inv_sample
  CLASS CORE ;
  SIZE 1.380 BY 2.720 ;
  SITE unithd ;
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER li1 ;
        RECT 0.320 1.075 0.650 1.315 ;
    END
  END A
  PIN Y
    DIRECTION OUTPUT ;
    USE SIGNAL ;
    PORT
      LAYER li1 ;
        RECT 0.720 1.485 1.050 2.465 ;
        RECT 0.720 0.255 1.050 0.885 ;
    END
  END Y
  PIN VGND
    DIRECTION INOUT ;
    USE GROUND ;
    PORT
      LAYER met1 ;
        RECT 0.000 -0.240 1.380 0.240 ;
    END
  END VGND
  OBS
    LAYER li1 ;
      RECT 0.000 2.635 1.380 2.805 ;
  END
END inv_sample
"#;

    #[test]
    fn test_parse_sample_lef() {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        parse_lef_bytes(SAMPLE_TLEF.as_bytes(), &mut db, &mut interner).unwrap();
        db.rebuild_maps();

        assert_eq!(db.dbu_per_micron, 1000);
        assert_eq!(db.site_name, "unithd");
        assert_eq!(db.site_width_dbu, 460);
        assert_eq!(db.site_height_dbu, 2720);
        assert_eq!(db.routing_layers.len(), 2);

        let li1 = &db.routing_layers[0];
        assert_eq!(interner.resolve(li1.name), "li1");
        assert_eq!(li1.dir, LayerDir::Vertical);
        assert_eq!(li1.pitch_dbu, 460);
        assert_eq!(li1.offset_dbu, 230);
        assert_eq!(li1.width_dbu, 170);
        assert!((li1.res_per_sq - 12.8).abs() < 1e-4);

        let met1 = &db.routing_layers[1];
        assert_eq!(interner.resolve(met1.name), "met1");
        assert_eq!(met1.dir, LayerDir::Horizontal);
        assert_eq!(met1.pitch_dbu, 340);
        assert_eq!(met1.offset_dbu, 170);
        assert_eq!(met1.width_dbu, 140);
        assert!((met1.res_per_sq - 0.125).abs() < 1e-4);

        // Now parse macro LEF
        parse_lef_bytes(SAMPLE_LEF.as_bytes(), &mut db, &mut interner).unwrap();
        db.rebuild_maps();

        assert_eq!(db.macros.len(), 1);
        let mac = &db.macros[0];
        assert_eq!(interner.resolve(mac.name), "inv_sample");
        assert_eq!(mac.class, MacroClass::Core);
        assert_eq!(mac.width_dbu, 1380);
        assert_eq!(mac.height_dbu, 2720);
        assert_eq!(mac.pin_count, 3);
        assert_eq!(mac.obs_rects.len(), 1);

        let pins = db.macro_pins(mac);
        assert_eq!(pins.len(), 3);
        assert_eq!(interner.resolve(pins[0].name), "A");
        assert_eq!(pins[0].dir, PinDir::Input);
        assert!(!pins[0].is_power);
        // bbox: (320, 1075) to (650, 1315) -> center (485, 1195)
        assert_eq!(pins[0].offset_x_dbu, 485);
        assert_eq!(pins[0].offset_y_dbu, 1195);
        assert_eq!(pins[0].layer, Some(LayerId(0)));

        assert_eq!(interner.resolve(pins[1].name), "Y");
        assert_eq!(pins[1].dir, PinDir::Output);
        assert!(!pins[1].is_power);
        // rects average center: 885, 1272
        assert_eq!(pins[1].offset_x_dbu, 885);
        assert_eq!(pins[1].offset_y_dbu, 1272);
        assert_eq!(pins[1].layer, Some(LayerId(0)));

        assert_eq!(interner.resolve(pins[2].name), "VGND");
        assert_eq!(pins[2].dir, PinDir::Inout);
        assert!(pins[2].is_power);
        assert_eq!(pins[2].layer, Some(LayerId(1)));
    }

    #[test]
    fn test_sky130_lef_integration() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");

        if !tlef_path.is_file() {
            panic!("PDK tlef file missing at {:?}", tlef_path);
        }
        if !lef_path.is_file() {
            panic!("PDK lef file missing at {:?}", lef_path);
        }

        let mut interner = StringInterner::new();
        let t0 = std::time::Instant::now();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load sky130 TechDb")
            .0;
        let elapsed = t0.elapsed();

        println!("sky130 LEF load elapsed: {:?}", elapsed);

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(
            tech_db.dbu_per_micron,
            ref_json["tech_dbu_per_micron"].as_i64().unwrap() as i32
        );
        assert_eq!(tech_db.site_name, "unithd");
        assert_eq!(
            tech_db.site_width_dbu,
            ref_json["site_width_dbu"].as_i64().unwrap() as i32
        );
        assert_eq!(
            tech_db.site_height_dbu,
            ref_json["site_height_dbu"].as_i64().unwrap() as i32
        );
        assert_eq!(
            tech_db.routing_layers.len(),
            ref_json["tech_routing_layers"].as_u64().unwrap() as usize
        );
        assert_eq!(
            tech_db.macros.len(),
            ref_json["tech_macros"].as_u64().unwrap() as usize
        );
        assert_eq!(
            tech_db.macro_pins.len(),
            ref_json["tech_macro_pins"].as_u64().unwrap() as usize
        );

        // Spot-check met1
        let met1_sym = interner.get("met1").expect("met1 not interned");
        let met1_id = tech_db.layer_by_name.get(&met1_sym).expect("met1 layer id");
        let met1 = &tech_db.routing_layers[met1_id.0 as usize];
        assert_eq!(met1.dir, LayerDir::Horizontal);
        assert_eq!(met1.pitch_dbu, 340);
        assert_eq!(met1.width_dbu, 140);
        assert_eq!(met1.offset_dbu, 170);
        assert!((met1.res_per_sq - 0.125).abs() < 1e-4);
        assert!((met1.cap_per_sq_um - 25.7784e-6).abs() < 1e-9);
        assert!((met1.edge_cap_per_um - 40.567e-6).abs() < 1e-9);

        // Spot-check met2
        let met2_sym = interner.get("met2").expect("met2 not interned");
        let met2_id = tech_db.layer_by_name.get(&met2_sym).expect("met2 layer id");
        let met2 = &tech_db.routing_layers[met2_id.0 as usize];
        assert_eq!(met2.dir, LayerDir::Vertical);
        assert_eq!(met2.pitch_dbu, 460);
        assert_eq!(met2.width_dbu, 140);
        assert_eq!(met2.offset_dbu, 230);
        assert!((met2.res_per_sq - 0.125).abs() < 1e-4);

        // Spot-check inv_1 macro
        let inv_sym = interner
            .get("sky130_fd_sc_hd__inv_1")
            .expect("inv_1 symbol");
        let inv_id = tech_db.macro_by_name.get(&inv_sym).expect("inv_1 macro id");
        let inv = &tech_db.macros[inv_id.0 as usize];
        assert_eq!(inv.width_dbu, 1380);
        assert_eq!(inv.height_dbu, 2720);
        assert_eq!(inv.pin_count, 6);
        // 4 raw li1 rects merge into 2 connected jumpers
        assert_eq!(inv.obs_rects.len(), 2);

        let inv_pins = tech_db.macro_pins(inv);
        let a_pin = inv_pins
            .iter()
            .find(|p| interner.resolve(p.name) == "A")
            .expect("A pin");
        assert_eq!(a_pin.dir, PinDir::Input);
        assert!(!a_pin.is_power);
        assert_eq!(a_pin.offset_x_dbu, 485);
        assert_eq!(a_pin.offset_y_dbu, 1195);

        let y_pin = inv_pins
            .iter()
            .find(|p| interner.resolve(p.name) == "Y")
            .expect("Y pin");
        assert_eq!(y_pin.dir, PinDir::Output);
        assert!(!y_pin.is_power);
        assert_eq!(y_pin.offset_x_dbu, 901);
        assert_eq!(y_pin.offset_y_dbu, 1243);

        // Spot-check multi-rect internal jumper merging on met1
        let met1_id = tech_db
            .layer_by_name
            .get(&met1_sym)
            .copied()
            .expect("met1 layer id");
        let dfxtp2_sym = interner
            .get("sky130_fd_sc_hd__dfxtp_2")
            .expect("dfxtp_2 symbol");
        let dfxtp2 = &tech_db.macros[tech_db.macro_by_name[&dfxtp2_sym].0 as usize];
        let dfxtp2_met1_obs = dfxtp2
            .obs_rects
            .iter()
            .filter(|(l, _)| *l == met1_id)
            .count();
        assert_eq!(
            dfxtp2_met1_obs, 2,
            "dfxtp_2 met1 OBS: 14 rects merged into 2 jumpers"
        );

        let xnor2_sym = interner
            .get("sky130_fd_sc_hd__xnor2_2")
            .expect("xnor2_2 symbol");
        let xnor2 = &tech_db.macros[tech_db.macro_by_name[&xnor2_sym].0 as usize];
        let xnor2_met1_obs = xnor2
            .obs_rects
            .iter()
            .filter(|(l, _)| *l == met1_id)
            .count();
        assert_eq!(
            xnor2_met1_obs, 1,
            "xnor2_2 met1 OBS: 5 rects merged into 1 jumper"
        );

        let xor2_sym = interner
            .get("sky130_fd_sc_hd__xor2_2")
            .expect("xor2_2 symbol");
        let xor2 = &tech_db.macros[tech_db.macro_by_name[&xor2_sym].0 as usize];
        let xor2_met1_obs = xor2.obs_rects.iter().filter(|(l, _)| *l == met1_id).count();
        assert_eq!(
            xor2_met1_obs, 1,
            "xor2_2 met1 OBS: 5 rects merged into 1 jumper"
        );
    }

    fn lef_error(text: &str) -> String {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        match parse_lef_bytes(text.as_bytes(), &mut db, &mut interner) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("malformed LEF was accepted:\n{text}"),
        }
    }

    /// Asserts the fixture still contains `needle`, so a test cannot pass by editing nothing.
    fn edit_fixture(base: &str, needle: &str, replacement: &str) -> String {
        assert!(
            base.contains(needle),
            "fixture no longer contains `{needle}`"
        );
        base.replacen(needle, replacement, 1)
    }

    #[test]
    fn test_bad_site_size_is_an_error() {
        let text = edit_fixture(SAMPLE_TLEF, "SIZE 0.46 BY 2.72 ;", "SIZE 0.46 BY 2.7x ;");
        let msg = lef_error(&text);
        assert!(msg.contains("SITE unithd") && msg.contains("2.7x"), "{msg}");
    }

    #[test]
    fn test_layer_width_without_value_is_an_error() {
        let text = edit_fixture(SAMPLE_TLEF, "WIDTH 0.14 ;", "WIDTH ;");
        let msg = lef_error(&text);
        assert!(
            msg.contains("LAYER WIDTH") && msg.contains("missing"),
            "{msg}"
        );
    }

    #[test]
    fn test_routing_layer_without_width_is_an_error() {
        let text = edit_fixture(SAMPLE_TLEF, "  WIDTH 0.14 ;\n", "");
        let msg = lef_error(&text);
        assert!(msg.contains("met1") && msg.contains("no WIDTH"), "{msg}");
    }

    #[test]
    fn test_truncated_rect_is_an_error() {
        let text = edit_fixture(
            SAMPLE_LEF,
            "RECT 0.320 1.075 0.650 1.315 ;",
            "RECT 0.320 1.075 0.650 ;",
        );
        let msg = lef_error(&text);
        assert!(msg.contains("PIN PORT RECT y2"), "{msg}");
    }

    #[test]
    fn test_macro_without_size_is_an_error() {
        let text = edit_fixture(SAMPLE_LEF, "  SIZE 1.380 BY 2.720 ;\n", "");
        let msg = lef_error(&text);
        assert!(
            msg.contains("inv_sample") && msg.contains("no SIZE"),
            "{msg}"
        );
    }

    #[test]
    fn test_unterminated_macro_is_an_error() {
        let text = edit_fixture(SAMPLE_LEF, "END inv_sample", "");
        let msg = lef_error(&text);
        assert!(
            msg.contains("MACRO inv_sample") && msg.contains("not terminated"),
            "{msg}"
        );
    }

    #[test]
    fn test_block_closed_by_another_name_is_an_error() {
        let text = edit_fixture(SAMPLE_TLEF, "END met1", "END met");
        let msg = lef_error(&text);
        assert!(msg.contains("expected `END met1`"), "{msg}");
    }

    /// Every truncation of the sample technology LEF must return `Ok` or `Err` without panicking,
    /// and a truncation inside the `met1` layer block must be an error.
    #[test]
    fn test_every_truncation_of_sample_tlef_is_rejected_or_parsed_without_panic() {
        let layer_start =
            SAMPLE_TLEF.find("LAYER met1").expect("fixture has met1") + "LAYER met1".len();
        let layer_end =
            SAMPLE_TLEF.find("END met1").expect("fixture has END met1") + "END met1".len();
        for n in 0..=SAMPLE_TLEF.len() {
            let mut interner = StringInterner::new();
            let mut db = TechDb::new();
            let result = parse_lef_bytes(&SAMPLE_TLEF.as_bytes()[..n], &mut db, &mut interner);
            if n >= layer_start && n < layer_end {
                assert!(
                    result.is_err(),
                    "truncation at {n} bytes inside met1 was accepted"
                );
            }
        }
    }

    #[test]
    fn test_every_truncation_of_sample_lef_is_rejected_or_parsed_without_panic() {
        for n in 0..=SAMPLE_LEF.len() {
            let mut interner = StringInterner::new();
            let mut db = TechDb::new();
            let _ = parse_lef_bytes(&SAMPLE_LEF.as_bytes()[..n], &mut db, &mut interner);
        }
    }

    const SKIP_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS

FOO 42 ;
VERSION 5.7 ;

VIA via_a
END via_a

via via_b
END via_b

VIARULE rule1
END rule1

NONDEFAULTRULE ndr1
END ndr1

PROPERTYDEFINITIONS
END PROPERTYDEFINITIONS

SPACING
END SPACING

MACRO m1
  CLASS CORE ;
  SIZE 1.0 BY 1.0 ;
END m1
END
"#;

    #[test]
    fn test_skip_report_counts_skips() {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        let report = parse_lef_bytes(SKIP_LEF.as_bytes(), &mut db, &mut interner).unwrap();

        assert_eq!(db.macros.len(), 1);
        assert_eq!(report.skipped["FOO"], 1);
        assert_eq!(report.skipped["VERSION"], 1);
        // `VIA` blocks are parsed now — both the empty uppercase record and
        // the lowercase one record nothing but count as consumed, not skipped.
        assert!(!report.skipped.contains_key("VIA"));
        assert_eq!(report.skipped["VIARULE"], 1);
        assert_eq!(report.skipped["NONDEFAULTRULE"], 1);
        assert_eq!(report.skipped["PROPERTYDEFINITIONS"], 1);
        assert_eq!(report.skipped["SPACING"], 1);
        assert_eq!(report.total(), 6);
    }

    const KNOWN_LEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS

SITE unithd
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
END unithd

LAYER li1
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.17 ;
END li1

MACRO m1
  CLASS CORE ;
  SIZE 1.0 BY 1.0 ;
END m1
END
"#;

    #[test]
    fn test_skip_report_empty_when_nothing_skipped() {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        let report = parse_lef_bytes(KNOWN_LEF.as_bytes(), &mut db, &mut interner).unwrap();

        assert!(report.skipped.is_empty());
        assert_eq!(report.total(), 0);
    }

    /// A `VIA` whose name spells nothing about layers — resolution must come
    /// from its `LAYER` groups, not the name.
    const VIA_TLEF: &str = r#"
UNITS
  DATABASE MICRONS 1000 ;
END UNITS

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

VIA xyzzy DEFAULT
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END xyzzy
"#;

    fn parse_via_tlef() -> (StringInterner, TechDb) {
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        parse_lef_bytes(VIA_TLEF.as_bytes(), &mut db, &mut interner).unwrap();
        db.strings = interner.clone();
        // `load_tech_db` folds `cut_res_above` into `via_res_ohm_above`;
        // `parse_lef_bytes` alone does not.
        db.via_res_ohm_above = vec![0.0; db.routing_layers.len()];
        for (below, res) in std::mem::take(&mut db.cut_res_above) {
            db.via_res_ohm_above[below as usize] = res;
        }
        db.rebuild_maps();
        (interner, db)
    }

    #[test]
    fn test_via_renamed_still_resolves_by_layer_triple() {
        let (interner, db) = parse_via_tlef();
        let layer = |name: &str| db.layer_by_name[&interner.get(name).unwrap()];

        assert_eq!(db.via_defs.len(), 1);
        let vd = &db.via_defs[0];
        assert_eq!(interner.resolve(vd.name), "xyzzy");
        assert_eq!(vd.lower, layer("met1"));
        assert_eq!(vd.upper, layer("met2"));
        assert_eq!(interner.resolve(vd.cut), "via12");
        assert_eq!(vd.cut_count, 2);

        // The cut layer sits above met1; two parallel cuts halve the 15 Ω.
        assert_eq!(db.via_res_ohm(vd.cut, 2), Some(7.5));
        assert_eq!(db.via_res_ohm(vd.cut, 1), Some(15.0));
        // A name unknown to the CUT-layer map has no resistance.
        assert_eq!(db.via_res_ohm(interner.get("met1").unwrap(), 1), None);
    }

    #[test]
    fn test_via_without_a_cut_layer_is_an_error() {
        let text = edit_fixture(
            VIA_TLEF,
            "  LAYER via12 ;\n    RECT -0.075 -0.075 0.075 0.075 ;\n    RECT -0.075 -0.075 0.075 0.075 ;\n",
            "",
        );
        let msg = lef_error(&text);
        assert!(msg.contains("xyzzy") && msg.contains("cut layer"), "{msg}");
    }

    #[test]
    fn test_via_on_an_unknown_layer_is_an_error() {
        // The indented `LAYER <name> ;` inside the VIA block is the group
        // header; `LAYER via12` alone would hit the layer's own definition.
        let text = edit_fixture(VIA_TLEF, "  LAYER via12 ;", "  LAYER via99 ;");
        let msg = lef_error(&text);
        assert!(msg.contains("via99") && msg.contains("neither a routing nor a cut"), "{msg}");
    }

    #[test]
    fn test_unterminated_via_is_an_error() {
        let text = edit_fixture(VIA_TLEF, "END xyzzy", "");
        let msg = lef_error(&text);
        assert!(msg.contains("VIA xyzzy") && msg.contains("not terminated"), "{msg}");
    }

    #[test]
    fn test_duplicate_via_name_is_an_error() {
        // A second full definition of `xyzzy` after the first.
        let dup = VIA_TLEF.replace(
            "END xyzzy",
            "END xyzzy\nVIA xyzzy\n  LAYER via12 ;\n    RECT -0.075 -0.075 0.075 0.075 ;\n  LAYER met1 ;\n    RECT -0.16 -0.13 0.16 0.13 ;\n  LAYER met2 ;\n    RECT -0.13 -0.16 0.13 0.16 ;\nEND xyzzy",
        );
        let msg = lef_error(&dup);
        assert!(msg.contains("xyzzy") && msg.contains("more than once"), "{msg}");
    }

    #[test]
    fn test_sky130_tlef_via_defs() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        if !tlef_path.is_file() || !lef_path.is_file() {
            panic!("PDK LEF files missing under {:?}", sky130_hd());
        }
        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load sky130 TechDb")
            .0;

        let layer = |name: &str| {
            tech_db.layer_by_name[&tech_db.strings.get(name).expect(name)]
        };
        let via = |name: &str| {
            let sym = tech_db.strings.get(name).expect(name);
            &tech_db.via_defs[tech_db.via_by_name[&sym] as usize]
        };

        // `licon` precedes the first routing layer and is not a PDN cut;
        // `mcon`..`via4` are.
        assert!(tech_db
            .strings
            .get("licon")
            .is_none_or(|s| !tech_db.cut_layer_by_name.contains_key(&s)));
        for name in ["mcon", "via", "via2", "via3", "via4"] {
            let s = tech_db.strings.get(name).unwrap_or_else(|| panic!("{name}"));
            assert!(tech_db.cut_layer_by_name.contains_key(&s), "{name}");
        }

        // The DEF via names in the test designs resolve through these
        // fixed-via definitions.
        let vd = via("L1M1_PR");
        assert_eq!(vd.lower, layer("li1"));
        assert_eq!(vd.upper, layer("met1"));
        assert_eq!(tech_db.strings.resolve(vd.cut), "mcon");
        // nom corner: mcon RESISTANCE 9.30, one cut RECT.
        assert_eq!(vd.cut_count, 1);
        assert_eq!(tech_db.via_res_ohm(vd.cut, vd.cut_count), Some(9.30));

        let vd = via("M1M2_PR");
        assert_eq!(vd.lower, layer("met1"));
        assert_eq!(vd.upper, layer("met2"));
        assert_eq!(tech_db.strings.resolve(vd.cut), "via");
        assert_eq!(vd.cut_count, 1);
        // nom corner: via RESISTANCE 4.50 over 5 parallel cuts (ROWCOL 1 5).
        assert_eq!(tech_db.via_res_ohm(vd.cut, 5), Some(0.90));

        let vd = via("M4M5_PR");
        assert_eq!(vd.lower, layer("met4"));
        assert_eq!(vd.upper, layer("met5"));
        assert_eq!(tech_db.strings.resolve(vd.cut), "via4");
    }

    /// `VIA` rects are retained in group order with the right
    /// `ViaRectLayer` discriminants; a multi-`RECT` cut group keeps every
    /// parallel cut.
    #[test]
    fn test_via_rects_retained_in_group_order() {
        let (interner, db) = parse_via_tlef();
        let vd = &db.via_defs[0];
        let met1 = db.layer_by_name[&interner.get("met1").unwrap()];
        let met2 = db.layer_by_name[&interner.get("met2").unwrap()];
        let cut = db.cut_layer_by_name[&interner.get("via12").unwrap()];

        assert_eq!(vd.cut_count, 2);
        assert_eq!(
            vd.rects,
            vec![
                ViaLayerRect {
                    layer: ViaRectLayer::Cut(cut),
                    rect: RectDbu::new(-75, -75, 75, 75),
                },
                ViaLayerRect {
                    layer: ViaRectLayer::Cut(cut),
                    rect: RectDbu::new(-75, -75, 75, 75),
                },
                ViaLayerRect {
                    layer: ViaRectLayer::Routing(met1),
                    rect: RectDbu::new(-160, -130, 160, 130),
                },
                ViaLayerRect {
                    layer: ViaRectLayer::Routing(met2),
                    rect: RectDbu::new(-130, -160, 130, 160),
                },
            ]
        );
    }

    /// A CUT layer's `WIDTH` is recorded in DBU; an omitted `WIDTH` is 0.
    #[test]
    fn test_cut_layer_width_recorded() {
        let (_, db) = parse_via_tlef();
        assert_eq!(db.cut_layers.len(), 1);
        assert_eq!(db.cut_layers[0].width_dbu, 0);

        let with_width = VIA_TLEF.replace(
            "  RESISTANCE 15.0 ;",
            "  RESISTANCE 15.0 ;\n  WIDTH 0.15 ;",
        );
        assert!(with_width.contains("WIDTH 0.15"));
        let mut interner = StringInterner::new();
        let mut db = TechDb::new();
        parse_lef_bytes(with_width.as_bytes(), &mut db, &mut interner).unwrap();
        assert_eq!(db.cut_layers[0].width_dbu, 150);
    }
}
