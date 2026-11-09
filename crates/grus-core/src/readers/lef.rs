use crate::interner::{LayerId, MacroId, PinDir, StringInterner};
use crate::tech_db::{LayerDir, MacroClass, MacroDef, MacroPin, RectDbu, RoutingLayer, TechDb};
use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Token<'a> {
    Word(&'a str),
    Str(&'a str),
    Semi,
}

pub(crate) struct LefLexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> LefLexer<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    pub fn peek_token(&self) -> Option<Token<'a>> {
        let mut cloned = Self {
            input: self.input,
            pos: self.pos,
        };
        cloned.next_token()
    }

    pub(crate) fn next_token(&mut self) -> Option<Token<'a>> {
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
                return Some(Token::Semi);
            }
            if b == b'"' {
                self.pos += 1;
                let start = self.pos;
                while self.pos < len && self.input[self.pos] != b'"' {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
                if self.pos < len && self.input[self.pos] == b'"' {
                    self.pos += 1;
                }
                return Some(Token::Str(s));
            }

            // Word or number up to whitespace, comment, or semicolon
            let start = self.pos;
            while self.pos < len {
                let curr = self.input[self.pos];
                if curr.is_ascii_whitespace() || curr == b';' || curr == b'#' {
                    break;
                }
                self.pos += 1;
            }
            let word = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
            return Some(Token::Word(word));
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

    pub fn next_f32(&mut self) -> Option<f32> {
        self.next_word().and_then(|w| w.parse::<f32>().ok())
    }

    pub fn next_i32(&mut self) -> Option<i32> {
        self.next_word().and_then(|w| w.parse::<i32>().ok())
    }
}

fn consume_end(lexer: &mut LefLexer, expected_name: Option<&str>) {
    if let Some(exp) = expected_name
        && let Some(Token::Word(w)) = lexer.peek_token()
            && w.eq_ignore_ascii_case(exp) {
                let _ = lexer.next_token();
            }
    if let Some(Token::Semi) = lexer.peek_token() {
        let _ = lexer.next_token();
    }
}

fn skip_block(lexer: &mut LefLexer, block_name: &str) {
    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok
            && w.eq_ignore_ascii_case("END")
                && let Some(Token::Word(end_name)) = lexer.peek_token()
                    && end_name.eq_ignore_ascii_case(block_name) {
                        let _ = lexer.next_token();
                        if let Some(Token::Semi) = lexer.peek_token() {
                            let _ = lexer.next_token();
                        }
                        break;
                    }
    }
}

/// Parses a Technology LEF (.tlef) or Macro LEF (.lef) file bytes into `TechDb`.
pub fn parse_lef_bytes(
    bytes: &[u8],
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    if tech_db.dbu_per_micron == 0 {
        tech_db.dbu_per_micron = 1000;
    }

    let mut lexer = LefLexer::new(bytes);

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
            skip_block(&mut lexer, "PROPERTYDEFINITIONS");
        } else if word.eq_ignore_ascii_case("VIA")
            || word.eq_ignore_ascii_case("VIARULE")
            || word.eq_ignore_ascii_case("NONDEFAULTRULE")
        {
            let name = lexer.next_word().unwrap_or("").to_string();
            skip_block(&mut lexer, &name);
        } else if word.eq_ignore_ascii_case("SPACING") {
            skip_block(&mut lexer, "SPACING");
        } else if word.eq_ignore_ascii_case("END") {
            let _ = lexer.next_word();
            if let Some(Token::Semi) = lexer.peek_token() {
                let _ = lexer.next_token();
            }
        } else if tok != Token::Semi {
            lexer.skip_to_semi();
        }
    }

    Ok(())
}

fn parse_units(lexer: &mut LefLexer, tech_db: &mut TechDb) -> Result<()> {
    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, Some("UNITS"));
                break;
            } else if w.eq_ignore_ascii_case("DATABASE") {
                if let Some(sub) = lexer.next_word()
                    && sub.eq_ignore_ascii_case("MICRONS")
                        && let Some(val) = lexer.next_i32() {
                            tech_db.dbu_per_micron = val;
                        }
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }
    Ok(())
}

fn parse_site(lexer: &mut LefLexer, tech_db: &mut TechDb) -> Result<()> {
    let site_id = lexer.next_word().unwrap_or("").to_string();
    let mut is_core = false;
    let mut w_um = 0.0f32;
    let mut h_um = 0.0f32;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, Some(&site_id));
                break;
            } else if w.eq_ignore_ascii_case("CLASS") {
                if let Some(c) = lexer.next_word()
                    && c.eq_ignore_ascii_case("CORE") {
                        is_core = true;
                    }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("SIZE") {
                w_um = lexer.next_f32().unwrap_or(0.0);
                let _ = lexer.next_word(); // BY
                h_um = lexer.next_f32().unwrap_or(0.0);
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }

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
    let layer_name = lexer.next_word().unwrap_or("").to_string();
    let mut is_routing = false;
    let mut dir = LayerDir::Horizontal;
    let mut pitch_x_um = None;
    let mut pitch_y_um = None;
    let mut offset_x_um = None;
    let mut offset_y_um = None;
    let mut width_um = 0.0f32;
    let mut res_per_sq = 0.0f32;
    let mut cap_per_sq_um = 0.0f32;
    let mut edge_cap_per_um = 0.0f32;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, Some(&layer_name));
                break;
            } else if w.eq_ignore_ascii_case("TYPE") {
                if let Some(t) = lexer.next_word()
                    && t.eq_ignore_ascii_case("ROUTING") {
                        is_routing = true;
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
                if let Some(p1) = lexer.next_f32() {
                    pitch_x_um = Some(p1);
                    if let Some(Token::Word(p2_str)) = lexer.peek_token()
                        && let Ok(p2) = p2_str.parse::<f32>() {
                            let _ = lexer.next_token();
                            pitch_y_um = Some(p2);
                        }
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("OFFSET") {
                if let Some(o1) = lexer.next_f32() {
                    offset_x_um = Some(o1);
                    if let Some(Token::Word(o2_str)) = lexer.peek_token()
                        && let Ok(o2) = o2_str.parse::<f32>() {
                            let _ = lexer.next_token();
                            offset_y_um = Some(o2);
                        }
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("WIDTH") {
                width_um = lexer.next_f32().unwrap_or(0.0);
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("RESISTANCE") {
                if let Some(r_kw) = lexer.next_word()
                    && r_kw.eq_ignore_ascii_case("RPERSQ") {
                        res_per_sq = lexer.next_f32().unwrap_or(0.0);
                    }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("CAPACITANCE") {
                if let Some(c_kw) = lexer.next_word()
                    && c_kw.eq_ignore_ascii_case("CPERSQDIST") {
                        cap_per_sq_um = lexer.next_f32().unwrap_or(0.0);
                    }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("EDGECAPACITANCE") {
                edge_cap_per_um = lexer.next_f32().unwrap_or(0.0);
                lexer.skip_to_semi();
            } else {
                lexer.skip_to_semi();
            }
        }
    }

    if is_routing {
        let dbu = tech_db.dbu_per_micron as f32;
        let pitch_um = match dir {
            LayerDir::Vertical => pitch_x_um.or(pitch_y_um).unwrap_or(0.0),
            LayerDir::Horizontal => pitch_y_um.or(pitch_x_um).unwrap_or(0.0),
        };
        let offset_um = match dir {
            LayerDir::Vertical => offset_x_um.or(offset_y_um).unwrap_or(0.0),
            LayerDir::Horizontal => offset_y_um.or(offset_x_um).unwrap_or(0.0),
        };

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

fn parse_macro(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let macro_name = lexer.next_word().unwrap_or("").to_string();
    let mac_sym = interner.intern(&macro_name);
    let pin_offset = tech_db.macro_pins.len() as u32;

    let mut class = MacroClass::Core;
    let mut width_dbu = 0;
    let mut height_dbu = 0;
    let mut obs_rects = Vec::new();
    let dbu = tech_db.dbu_per_micron as f32;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, Some(&macro_name));
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
                        _ => {}
                    }
                }
                class = parse_macro_class(&c1, &c2);
            } else if w.eq_ignore_ascii_case("SIZE") {
                let w_um = lexer.next_f32().unwrap_or(0.0);
                let _ = lexer.next_word(); // BY
                let h_um = lexer.next_f32().unwrap_or(0.0);
                width_dbu = (w_um * dbu).round() as i32;
                height_dbu = (h_um * dbu).round() as i32;
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("PIN") {
                parse_pin(lexer, tech_db, interner)?;
            } else if w.eq_ignore_ascii_case("OBS") {
                parse_obs(lexer, tech_db, interner, &mut obs_rects)?;
            } else {
                lexer.skip_to_semi();
            }
        }
    }

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
    });

    Ok(())
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

fn parse_pin(
    lexer: &mut LefLexer,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let pin_name = lexer.next_word().unwrap_or("").to_string();
    let pin_sym = interner.intern(&pin_name);
    let mut dir = PinDir::Input;
    let mut is_power = false;
    let mut pin_layer = None;

    let mut sum_center_x: i64 = 0;
    let mut sum_center_y: i64 = 0;
    let mut rect_count: i64 = 0;
    let dbu = tech_db.dbu_per_micron as f32;

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, Some(&pin_name));
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
                    && (u.eq_ignore_ascii_case("POWER") || u.eq_ignore_ascii_case("GROUND")) {
                        is_power = true;
                    }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("PORT") {
                while let Some(port_tok) = lexer.next_token() {
                    if let Token::Word(pw) = port_tok {
                        if pw.eq_ignore_ascii_case("END") {
                            consume_end(lexer, None);
                            break;
                        } else if pw.eq_ignore_ascii_case("LAYER") {
                            if let Some(lname) = lexer.next_word() {
                                let lsym = interner.intern(lname);
                                let l_id = tech_db.layer_by_name.get(&lsym).copied();
                                if pin_layer.is_none() && l_id.is_some() {
                                    pin_layer = l_id;
                                }
                            }
                            lexer.skip_to_semi();
                        } else if pw.eq_ignore_ascii_case("RECT") {
                            let x1 = lexer.next_f32().unwrap_or(0.0);
                            let y1 = lexer.next_f32().unwrap_or(0.0);
                            let x2 = lexer.next_f32().unwrap_or(0.0);
                            let y2 = lexer.next_f32().unwrap_or(0.0);

                            let x1_dbu = (x1 * dbu).round() as i32;
                            let y1_dbu = (y1 * dbu).round() as i32;
                            let x2_dbu = (x2 * dbu).round() as i32;
                            let y2_dbu = (y2 * dbu).round() as i32;

                            let cx = (x1_dbu + x2_dbu) / 2;
                            let cy = (y1_dbu + y2_dbu) / 2;
                            sum_center_x += cx as i64;
                            sum_center_y += cy as i64;
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

    let offset_x_dbu = if rect_count > 0 {
        (sum_center_x / rect_count) as i32
    } else {
        0
    };
    let offset_y_dbu = if rect_count > 0 {
        (sum_center_y / rect_count) as i32
    } else {
        0
    };

    tech_db.macro_pins.push(MacroPin {
        name: pin_sym,
        dir,
        is_power,
        offset_x_dbu,
        offset_y_dbu,
        layer: pin_layer,
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

    while let Some(tok) = lexer.next_token() {
        if let Token::Word(w) = tok {
            if w.eq_ignore_ascii_case("END") {
                consume_end(lexer, None);
                break;
            } else if w.eq_ignore_ascii_case("LAYER") {
                if let Some(lname) = lexer.next_word() {
                    let lsym = interner.intern(lname);
                    current_obs_layer = tech_db.layer_by_name.get(&lsym).copied();
                }
                lexer.skip_to_semi();
            } else if w.eq_ignore_ascii_case("RECT") {
                let x1 = lexer.next_f32().unwrap_or(0.0);
                let y1 = lexer.next_f32().unwrap_or(0.0);
                let x2 = lexer.next_f32().unwrap_or(0.0);
                let y2 = lexer.next_f32().unwrap_or(0.0);

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
    Ok(())
}

/// Parses a single LEF file into an existing `TechDb`.
pub fn parse_lef(
    path: &Path,
    tech_db: &mut TechDb,
    interner: &mut StringInterner,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Failed to open LEF file at {:?}", path))?;
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    parse_lef_bytes(&mmap, tech_db, interner)?;
    tech_db.strings = interner.clone();
    tech_db.rebuild_maps();
    Ok(())
}

/// Convenience function to parse both Technology LEF (.tlef) and Macro LEF (.lef) into a new `TechDb`.
pub fn load_tech_db(
    tlef_path: &Path,
    lef_path: &Path,
    interner: &mut StringInterner,
) -> Result<TechDb> {
    let mut tech_db = TechDb::new();
    parse_lef(tlef_path, &mut tech_db, interner)?;
    parse_lef(lef_path, &mut tech_db, interner)?;
    tech_db.strings = interner.clone();
    tech_db.rebuild_maps();
    Ok(tech_db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
        let home = dirs_home();
        let pdk_root = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".ciel"));

        let tlef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef");

        if !tlef_path.is_file() || !lef_path.is_file() {
            eprintln!("Skipping sky130 LEF integration test: files not found");
            return;
        }

        let mut interner = StringInterner::new();
        let t0 = std::time::Instant::now();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load sky130 TechDb");
        let elapsed = t0.elapsed();

        println!("sky130 LEF load elapsed: {:?}", elapsed);

        assert_eq!(tech_db.dbu_per_micron, 1000);
        assert_eq!(tech_db.site_name, "unithd");
        assert_eq!(tech_db.site_width_dbu, 460);
        assert_eq!(tech_db.site_height_dbu, 2720);
        assert_eq!(tech_db.routing_layers.len(), 6);
        assert_eq!(tech_db.macros.len(), 437);
        assert_eq!(tech_db.macro_pins.len(), 3526);

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
        let inv_sym = interner.get("sky130_fd_sc_hd__inv_1").expect("inv_1 symbol");
        let inv_id = tech_db.macro_by_name.get(&inv_sym).expect("inv_1 macro id");
        let inv = &tech_db.macros[inv_id.0 as usize];
        assert_eq!(inv.width_dbu, 1380);
        assert_eq!(inv.height_dbu, 2720);
        assert_eq!(inv.pin_count, 6);
        assert_eq!(inv.obs_rects.len(), 4);

        let inv_pins = tech_db.macro_pins(inv);
        let a_pin = inv_pins.iter().find(|p| interner.resolve(p.name) == "A").expect("A pin");
        assert_eq!(a_pin.dir, PinDir::Input);
        assert!(!a_pin.is_power);
        assert_eq!(a_pin.offset_x_dbu, 485);
        assert_eq!(a_pin.offset_y_dbu, 1195);

        let y_pin = inv_pins.iter().find(|p| interner.resolve(p.name) == "Y").expect("Y pin");
        assert_eq!(y_pin.dir, PinDir::Output);
        assert!(!y_pin.is_power);
        assert_eq!(y_pin.offset_x_dbu, 901);
        assert_eq!(y_pin.offset_y_dbu, 1243);
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
    }
}
