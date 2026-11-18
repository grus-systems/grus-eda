use crate::design_db::{DesignDb, Orient, PlacementStatus};
use crate::interner::{CellId, LibPinId, MacroPinId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::tech_db::TechDb;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum VerilogToken {
    Ident(String),
    Int(i64),
    Const(bool), // 1'b0 (false) or 1'b1 (true)
    /// A literal this reader does not model (wider constants, x/z, bad digits). Reported where a
    /// net is expected instead of being read as a net or as zero.
    Unsupported(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Semi,
    Comma,
    Dot,
    AssignOp,
    Eof,
}

pub(crate) struct VerilogLexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> VerilogLexer<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace_and_comments(&mut self) {
        let len = self.input.len();
        while self.pos < len {
            let b = self.input[self.pos];
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'/' && self.pos + 1 < len {
                if self.input[self.pos + 1] == b'/' {
                    self.pos += 2;
                    while self.pos < len && self.input[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                    continue;
                } else if self.input[self.pos + 1] == b'*' {
                    self.pos += 2;
                    while self.pos + 1 < len
                        && !(self.input[self.pos] == b'*' && self.input[self.pos + 1] == b'/')
                    {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(len);
                    continue;
                }
            }
            break;
        }
    }

    /// The part of a based literal after its apostrophe: an optional base letter, then the value.
    /// Only a constant 0 or 1 of one bit is modeled. Anything else is `Unsupported`, because
    /// reading it as a net or as zero would be silently wrong.
    fn based_constant(&mut self, start: usize, one_bit: bool) -> VerilogToken {
        let input = self.input;
        if self.pos < input.len()
            && matches!(
                input[self.pos],
                b'b' | b'B' | b'd' | b'D' | b'h' | b'H' | b'o' | b'O'
            )
        {
            self.pos += 1;
        }
        let value_start = self.pos;
        while self.pos < input.len()
            && (input[self.pos].is_ascii_alphanumeric() || input[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let value = &input[value_start..self.pos];
        if one_bit && (value == b"0" || value == b"1") {
            VerilogToken::Const(value == b"1")
        } else {
            VerilogToken::Unsupported(String::from_utf8_lossy(&input[start..self.pos]).into_owned())
        }
    }

    pub fn next_token(&mut self) -> VerilogToken {
        self.skip_whitespace_and_comments();
        if self.pos >= self.input.len() {
            return VerilogToken::Eof;
        }

        let b = self.input[self.pos];
        match b {
            b'(' => {
                self.pos += 1;
                VerilogToken::LParen
            }
            b')' => {
                self.pos += 1;
                VerilogToken::RParen
            }
            b'[' => {
                self.pos += 1;
                VerilogToken::LBracket
            }
            b']' => {
                self.pos += 1;
                VerilogToken::RBracket
            }
            b'{' => {
                self.pos += 1;
                VerilogToken::LBrace
            }
            b'}' => {
                self.pos += 1;
                VerilogToken::RBrace
            }
            b':' => {
                self.pos += 1;
                VerilogToken::Colon
            }
            b';' => {
                self.pos += 1;
                VerilogToken::Semi
            }
            b',' => {
                self.pos += 1;
                VerilogToken::Comma
            }
            b'.' => {
                self.pos += 1;
                VerilogToken::Dot
            }
            b'=' => {
                self.pos += 1;
                VerilogToken::AssignOp
            }
            b'\\' => {
                // Escaped identifier: continues until the next ASCII whitespace
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.input.len() && !self.input[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                let s = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
                VerilogToken::Ident(s)
            }
            b'\'' => {
                // Unsized constant: '0, '1, 'b0, 'h1, ...
                let start = self.pos;
                self.pos += 1;
                self.based_constant(start, true)
            }
            b'0'..=b'9' => {
                let start = self.pos;
                while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                if self.pos < self.input.len() && self.input[self.pos] == b'\'' {
                    let one_bit = &self.input[start..self.pos] == b"1";
                    self.pos += 1;
                    return self.based_constant(start, one_bit);
                }
                let text = String::from_utf8_lossy(&self.input[start..self.pos]).into_owned();
                match text.parse::<i64>() {
                    Ok(val) => VerilogToken::Int(val),
                    Err(_) => VerilogToken::Unsupported(text),
                }
            }
            _ => {
                let start = self.pos;
                while self.pos < self.input.len() {
                    let c = self.input[self.pos];
                    if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if start == self.pos {
                    self.pos += 1;
                    VerilogToken::Ident(
                        String::from_utf8_lossy(&self.input[start..self.pos]).to_string(),
                    )
                } else {
                    let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap_or("");
                    VerilogToken::Ident(s.to_string())
                }
            }
        }
    }
}

struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        let mut root = i;
        while root != self.parent[root] {
            root = self.parent[root];
        }
        while i != root {
            let next = self.parent[i];
            self.parent[i] = root;
            i = next;
        }
        root
    }

    fn union(&mut self, i: usize, j: usize) {
        let ri = self.find(i);
        let rj = self.find(j);
        if ri != rj {
            self.parent[ri] = rj;
        }
    }
}

pub fn parse_verilog_bytes(
    bytes: &[u8],
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let mut lexer = VerilogLexer::new(bytes);
    let mut tok = lexer.next_token();

    // Find `module`
    let mut module_name = String::new();
    while tok != VerilogToken::Eof {
        if let VerilogToken::Ident(ref s) = tok
            && s == "module"
        {
            tok = lexer.next_token();
            if let VerilogToken::Ident(ref mname) = tok {
                module_name = mname.clone();
                tok = lexer.next_token();
            }
            break;
        }
        tok = lexer.next_token();
    }

    if module_name.is_empty() {
        bail!("No module declaration found in Verilog netlist");
    }

    // Skip port list in module header `( ... );`
    if tok == VerilogToken::LParen {
        while tok != VerilogToken::RParen && tok != VerilogToken::Eof {
            tok = lexer.next_token();
        }
        if tok == VerilogToken::RParen {
            tok = lexer.next_token();
        }
    }
    if tok == VerilogToken::Semi {
        tok = lexer.next_token();
    }

    let mut port_decls: Vec<(String, PinDir)> = Vec::new();
    let mut port_dir_map: HashMap<String, PinDir> = HashMap::new();

    // Struct for cell instantiation before net resolution
    struct RawInstance {
        cell_type: String,
        inst_name: String,
        connections: Vec<(String, Option<String>)>, // (pin_name, optional net_name)
    }

    let mut instances: Vec<RawInstance> = Vec::new();
    let mut alias_pairs: Vec<(String, String)> = Vec::new();

    // Helper to parse bus range `[msb:lsb]`
    let parse_range =
        |tok: &mut VerilogToken, lexer: &mut VerilogLexer| -> Result<Option<(i64, i64)>> {
            if *tok == VerilogToken::LBracket {
                *tok = lexer.next_token();
                let msb = if let VerilogToken::Int(n) = *tok {
                    n
                } else {
                    bail!("Expected int for MSB in range");
                };
                *tok = lexer.next_token();
                if *tok != VerilogToken::Colon {
                    bail!("Expected ':' in range");
                }
                *tok = lexer.next_token();
                let lsb = if let VerilogToken::Int(n) = *tok {
                    n
                } else {
                    bail!("Expected int for LSB in range");
                };
                *tok = lexer.next_token();
                if *tok != VerilogToken::RBracket {
                    bail!("Expected ']' in range");
                }
                *tok = lexer.next_token();
                Ok(Some((msb, lsb)))
            } else {
                Ok(None)
            }
        };

    // Helper to parse net identifier/expression inside connection or assign
    let parse_net_ref =
        |tok: &mut VerilogToken, lexer: &mut VerilogLexer| -> Result<Option<String>> {
            match tok {
                VerilogToken::Const(false) => {
                    *tok = lexer.next_token();
                    Ok(Some("__const_0".to_string()))
                }
                VerilogToken::Const(true) => {
                    *tok = lexer.next_token();
                    Ok(Some("__const_1".to_string()))
                }
                VerilogToken::Unsupported(text) => {
                    bail!("unsupported Verilog constant or literal `{text}`")
                }
                VerilogToken::Ident(name) => {
                    let base = name.clone();
                    *tok = lexer.next_token();
                    if *tok == VerilogToken::LBracket {
                        *tok = lexer.next_token();
                        if let VerilogToken::Int(idx) = *tok {
                            *tok = lexer.next_token();
                            if *tok == VerilogToken::RBracket {
                                *tok = lexer.next_token();
                                return Ok(Some(format!("{}[{}]", base, idx)));
                            }
                        }
                        bail!("Malformed bus bit index on {}", base);
                    }
                    Ok(Some(base))
                }
                _ => Ok(None),
            }
        };

    // Parse module body
    while tok != VerilogToken::Eof {
        match tok {
            VerilogToken::Ident(ref kw) if kw == "endmodule" => {
                break;
            }
            VerilogToken::Ident(ref kw) if kw == "input" || kw == "output" || kw == "inout" => {
                let dir = match kw.as_str() {
                    "input" => PinDir::Input,
                    "output" => PinDir::Output,
                    _ => PinDir::Inout,
                };
                tok = lexer.next_token();
                let range = parse_range(&mut tok, &mut lexer)?;

                while tok != VerilogToken::Semi && tok != VerilogToken::Eof {
                    if let VerilogToken::Ident(ref name) = tok {
                        let base_name = name.clone();
                        tok = lexer.next_token();
                        if let Some((msb, lsb)) = range {
                            let min = msb.min(lsb);
                            let max = msb.max(lsb);
                            for i in min..=max {
                                let scalar_name = format!("{}[{}]", base_name, i);
                                port_dir_map.insert(scalar_name.clone(), dir);
                                port_decls.push((scalar_name, dir));
                            }
                        } else {
                            port_dir_map.insert(base_name.clone(), dir);
                            port_decls.push((base_name, dir));
                        }
                    } else {
                        tok = lexer.next_token();
                    }
                }
                if tok == VerilogToken::Semi {
                    tok = lexer.next_token();
                }
            }
            VerilogToken::Ident(ref kw) if kw == "wire" => {
                tok = lexer.next_token();
                let _range = parse_range(&mut tok, &mut lexer)?;
                while tok != VerilogToken::Semi && tok != VerilogToken::Eof {
                    tok = lexer.next_token();
                }
                if tok == VerilogToken::Semi {
                    tok = lexer.next_token();
                }
            }
            VerilogToken::Ident(ref kw) if kw == "assign" => {
                tok = lexer.next_token();
                let lhs = parse_net_ref(&mut tok, &mut lexer)?
                    .context("Expected LHS in assign statement")?;
                if tok != VerilogToken::AssignOp {
                    bail!("Expected '=' in assign statement");
                }
                tok = lexer.next_token();
                let rhs = parse_net_ref(&mut tok, &mut lexer)?
                    .context("Expected RHS in assign statement")?;
                if tok == VerilogToken::Semi {
                    tok = lexer.next_token();
                }
                alias_pairs.push((lhs, rhs));
            }
            VerilogToken::Ident(cell_type) => {
                // Cell instance: `type inst ( .PIN(net), ... );`
                tok = lexer.next_token();
                let inst_name = if let VerilogToken::Ident(ref iname) = tok {
                    iname.clone()
                } else {
                    bail!("Expected instance name after cell type {}", cell_type);
                };
                tok = lexer.next_token();
                if tok != VerilogToken::LParen {
                    bail!("Expected '(' for instance connections of {}", inst_name);
                }
                tok = lexer.next_token();

                let mut conns: Vec<(String, Option<String>)> = Vec::new();
                while tok != VerilogToken::RParen && tok != VerilogToken::Eof {
                    if tok == VerilogToken::Dot {
                        tok = lexer.next_token();
                        let pin_name = if let VerilogToken::Ident(ref pname) = tok {
                            pname.clone()
                        } else {
                            bail!("Expected pin name after '.' in instance {}", inst_name);
                        };
                        tok = lexer.next_token();
                        if tok != VerilogToken::LParen {
                            bail!("Expected '(' after pin .{}", pin_name);
                        }
                        tok = lexer.next_token();

                        let net_ref = if tok == VerilogToken::RParen {
                            None
                        } else {
                            parse_net_ref(&mut tok, &mut lexer)?
                        };

                        if tok == VerilogToken::RParen {
                            tok = lexer.next_token();
                        }
                        conns.push((pin_name, net_ref));
                    } else {
                        tok = lexer.next_token();
                    }
                }

                if tok == VerilogToken::RParen {
                    tok = lexer.next_token();
                }
                if tok == VerilogToken::Semi {
                    tok = lexer.next_token();
                }

                instances.push(RawInstance {
                    cell_type,
                    inst_name,
                    connections: conns,
                });
            }
            _ => {
                tok = lexer.next_token();
            }
        }
    }

    // Initialize DesignDb
    let mut db = DesignDb::new();
    db.strings = tech_db.strings.clone();
    db.design_name = module_name;
    db.dbu_per_micron = if tech_db.dbu_per_micron > 0 {
        tech_db.dbu_per_micron
    } else {
        1000
    };

    // 1. Gather all named nets
    let mut all_net_names: Vec<String> = Vec::new();
    let mut name_to_id: HashMap<String, usize> = HashMap::new();

    let get_or_create_name_id =
        |name: &str, names: &mut Vec<String>, map: &mut HashMap<String, usize>| -> usize {
            if let Some(&id) = map.get(name) {
                id
            } else {
                let id = names.len();
                map.insert(name.to_string(), id);
                names.push(name.to_string());
                id
            }
        };

    // Primary I/O ports
    for (port_name, _) in &port_decls {
        get_or_create_name_id(port_name, &mut all_net_names, &mut name_to_id);
    }

    // Cell connections
    for inst in &instances {
        for (_, net_opt) in &inst.connections {
            if let Some(net_name) = net_opt {
                get_or_create_name_id(net_name, &mut all_net_names, &mut name_to_id);
            }
        }
    }

    // Aliases
    for (lhs, rhs) in &alias_pairs {
        get_or_create_name_id(lhs, &mut all_net_names, &mut name_to_id);
        get_or_create_name_id(rhs, &mut all_net_names, &mut name_to_id);
    }

    // 2. Disjoint-Set union for aliases
    let mut ds = DisjointSet::new(all_net_names.len());
    for (lhs, rhs) in &alias_pairs {
        let i = name_to_id[lhs];
        let j = name_to_id[rhs];
        ds.union(i, j);
    }

    // Map each root to a contiguous NetId
    let mut root_to_net: HashMap<usize, NetId> = HashMap::new();
    let mut net_canonical_name: Vec<String> = Vec::new();

    // Populate net canonical names, giving priority to port names and clean names
    for (id, name) in all_net_names.iter().enumerate() {
        let root = ds.find(id);
        if let std::collections::hash_map::Entry::Vacant(e) = root_to_net.entry(root) {
            let nid = NetId(net_canonical_name.len() as u32);
            e.insert(nid);
            net_canonical_name.push(name.clone());
        } else {
            // If current name is a port name or cleaner, update canonical name
            let nid = root_to_net[&root];
            let cur_name = &net_canonical_name[nid.0 as usize];
            if port_dir_map.contains_key(name) && !port_dir_map.contains_key(cur_name) {
                net_canonical_name[nid.0 as usize] = name.clone();
            }
        }
    }

    let resolve_net = |name: &str,
                       ds: &mut DisjointSet,
                       map: &HashMap<String, usize>,
                       root_to_net: &HashMap<usize, NetId>|
     -> Option<NetId> {
        let id = map.get(name)?;
        let root = ds.find(*id);
        root_to_net.get(&root).copied()
    };

    // 3. Populate Primary I/O ports in DesignDb
    let mut net_pins_list: Vec<Vec<PinId>> = vec![Vec::new(); net_canonical_name.len()];

    for (port_name, dir) in &port_decls {
        let is_driver = matches!(dir, PinDir::Input | PinDir::Inout);
        let sym = db.strings.intern(port_name);
        let pin_id = PinId(db.pin_names.len() as u32);
        let net_id = resolve_net(port_name, &mut ds, &name_to_id, &root_to_net);

        db.pin_names.push(sym);
        db.pin_dirs.push(*dir);
        db.pin_cells.push(None);
        db.pin_macro_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_nets.push(net_id);
        db.pin_offset_x_dbu.push(0);
        db.pin_offset_y_dbu.push(0);
        db.pin_is_driver.push(is_driver);
        db.io_pins.push(pin_id);

        if let Some(nid) = net_id {
            net_pins_list[nid.0 as usize].push(pin_id);
        }
    }

    // 4. Populate Cells in DesignDb
    db.cell_pin_offsets.push(0);

    for inst in &instances {
        let type_sym = db.strings.intern(&inst.cell_type);
        let macro_id = match tech_db.macro_by_name.get(&type_sym).copied() {
            Some(mid) => mid,
            None => continue, // Skip unmapped / blackbox cells
        };
        let mac = &tech_db.macros[macro_id.0 as usize];

        let lib_cell_id = lib_db.and_then(|lib| {
            lib.strings
                .get(&inst.cell_type)
                .and_then(|s| lib.cell_by_name.get(&s).copied())
        });

        let cell_id = CellId(db.cell_names.len() as u32);
        let cname_sym = db.strings.intern(&inst.inst_name);

        db.cell_names.push(cname_sym);
        db.cell_macros.push(macro_id);
        db.cell_lib_ids.push(lib_cell_id);
        db.cell_status.push(PlacementStatus::Unplaced);
        db.cell_x_dbu.push(0);
        db.cell_y_dbu.push(0);
        db.cell_orient.push(Orient::N);

        let conn_map: HashMap<&str, &Option<String>> = inst
            .connections
            .iter()
            .map(|(p, n)| (p.as_str(), n))
            .collect();

        let mac_pins = tech_db.macro_pins(mac);
        for (pin_idx, mpin) in mac_pins.iter().enumerate() {
            let pin_name = tech_db.strings.resolve(mpin.name);
            let net_ref_opt = conn_map.get(pin_name).and_then(|n| n.as_ref());

            let net_id = net_ref_opt
                .and_then(|nname| resolve_net(nname, &mut ds, &name_to_id, &root_to_net));

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
            db.pin_nets.push(net_id);
            db.pin_offset_x_dbu.push(mpin.offset_x_dbu);
            db.pin_offset_y_dbu.push(mpin.offset_y_dbu);
            db.pin_is_driver.push(is_driver);
            db.cell_pin_ids.push(pin_id);

            if let Some(nid) = net_id {
                net_pins_list[nid.0 as usize].push(pin_id);
            }
        }

        db.cell_pin_offsets.push(db.cell_pin_ids.len() as u32);
    }

    // 5. Build Net SoA & CSR
    let num_nets = net_canonical_name.len();
    db.net_names.reserve(num_nets);
    db.net_pin_offsets.reserve(num_nets + 1);

    for name in &net_canonical_name {
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

    Ok(db)
}

pub fn parse_verilog(path: &Path, tech_db: &TechDb, lib_db: Option<&LibDb>) -> Result<DesignDb> {
    let mut file =
        File::open(path).with_context(|| format!("Failed to open Verilog file at {:?}", path))?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    parse_verilog_bytes(&contents, tech_db, lib_db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::lef::load_tech_db;
    use crate::test_support::{design_dir, require, sky130_hd};

    #[test]
    fn test_gcd_verilog() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let v_path = design_dir("gcd").join("gcd.synth.v");

        if !tlef_path.is_file() {
            panic!("PDK tlef file missing at {:?}", tlef_path);
        }
        if !lef_path.is_file() {
            panic!("PDK lef file missing at {:?}", lef_path);
        }
        if !v_path.is_file() {
            panic!("Design Verilog file missing at {:?}", v_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_verilog(&v_path, &tech_db, None).expect("Failed to parse Verilog netlist");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["verilog_logic_instances"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["verilog_signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["verilog_bterms"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["verilog_inst_pins"].as_u64().unwrap() as usize
        );

        // Verify driver invariant on all nets with at least one driver
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(
                    first_pin, drv,
                    "Driver pin must be first at net_pin_offsets"
                );
            }
        }
        assert!(
            driven_nets >= 260,
            "Expected most nets to be driven, got {}",
            driven_nets
        );
    }

    #[test]
    fn test_aes_verilog() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let v_path = require(design_dir("aes").join("aes.synth.v"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_verilog(&v_path, &tech_db, None).expect("Failed to parse Verilog netlist");

        let ref_path = require(design_dir("aes").join("aes.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read aes.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["verilog_logic_instances"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["verilog_signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["verilog_bterms"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["verilog_inst_pins"].as_u64().unwrap() as usize
        );

        // Driver invariant on every driven net. The count is printed, not asserted: no aes
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(
                    first_pin, drv,
                    "Driver pin must be first at net_pin_offsets"
                );
            }
        }
        println!(
            "aes verilog: {} of {} nets have a driver",
            driven_nets,
            db.num_nets()
        );
    }

    #[test]
    fn test_jpeg_verilog() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let v_path = require(design_dir("jpeg").join("jpeg.synth.v"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_verilog(&v_path, &tech_db, None).expect("Failed to parse Verilog netlist");

        let ref_path = require(design_dir("jpeg").join("jpeg.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read jpeg.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["verilog_logic_instances"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["verilog_signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["verilog_bterms"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["verilog_inst_pins"].as_u64().unwrap() as usize
        );

        // Driver invariant on every driven net. The count is printed, not asserted: no jpeg
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(
                    first_pin, drv,
                    "Driver pin must be first at net_pin_offsets"
                );
            }
        }
        println!(
            "jpeg verilog: {} of {} nets have a driver",
            driven_nets,
            db.num_nets()
        );
    }

    #[test]
    fn test_ibex_verilog() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let v_path = require(design_dir("ibex").join("ibex.synth.v"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let db = parse_verilog(&v_path, &tech_db, None).expect("Failed to parse Verilog netlist");

        let ref_path = require(design_dir("ibex").join("ibex.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read ibex.reference.json"))
                .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(
            db.num_cells(),
            ref_json["verilog_logic_instances"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.num_nets(),
            ref_json["verilog_signal_nets"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.io_pins.len(),
            ref_json["verilog_bterms"].as_u64().unwrap() as usize
        );
        assert_eq!(
            db.cell_pin_ids.len(),
            ref_json["verilog_inst_pins"].as_u64().unwrap() as usize
        );

        // Driver invariant on every driven net. The count is printed, not asserted: no ibex
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(
                    first_pin, drv,
                    "Driver pin must be first at net_pin_offsets"
                );
            }
        }
        println!(
            "ibex verilog: {} of {} nets have a driver",
            driven_nets,
            db.num_nets()
        );
    }

    const INV_LEF: &str = r#"
VERSION 5.8 ;
UNITS
  DATABASE MICRONS 1000 ;
END UNITS
SITE unithd
  CLASS CORE ;
  SIZE 0.46 BY 2.72 ;
END unithd
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

    const TOP_V: &str =
        "module top (a, y);\n  input a;\n  output y;\n  INV u1 (.A(a), .Y(y));\nendmodule\n";

    fn inv_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        crate::readers::lef::parse_lef_bytes(INV_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("INV LEF must parse");
        tech_db.strings = interner.clone();
        tech_db.rebuild_maps();
        tech_db
    }

    fn verilog_error(text: &str) -> String {
        match parse_verilog_bytes(text.as_bytes(), &inv_tech(), None) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("malformed netlist was accepted:\n{text}"),
        }
    }

    #[test]
    fn test_one_bit_constant_connection_is_read() {
        let text = TOP_V.replace(".A(a)", ".A(1'h0)");
        let db = parse_verilog_bytes(text.as_bytes(), &inv_tech(), None)
            .expect("1'h0 is a valid constant");
        assert_eq!(db.num_cells(), 1);
    }

    #[test]
    fn test_wide_constant_connection_is_an_error() {
        let msg = verilog_error(&TOP_V.replace(".A(a)", ".A(4'hF)"));
        assert!(
            msg.contains("unsupported Verilog constant or literal `4'hF`"),
            "{msg}"
        );
    }

    #[test]
    fn test_unknown_base_constant_connection_is_an_error() {
        let msg = verilog_error(&TOP_V.replace(".A(a)", ".A(1'bx)"));
        assert!(msg.contains("1'bx"), "{msg}");
    }

    #[test]
    fn test_every_truncation_of_netlist_is_rejected_or_parsed_without_panic() {
        let tech = inv_tech();
        for n in 0..=TOP_V.len() {
            let _ = parse_verilog_bytes(&TOP_V.as_bytes()[..n], &tech, None);
        }
    }
}
