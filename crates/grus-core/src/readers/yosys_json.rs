use crate::design_db::{DesignDb, Orient, PlacementStatus};
use crate::interner::{CellId, LibPinId, MacroPinId, NetId, PinDir, PinId};
use crate::lib_db::LibDb;
use crate::tech_db::TechDb;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Deserialize)]
struct YosysRoot {
    #[serde(default)]
    modules: BTreeMap<String, YosysModule>,
}

#[derive(Deserialize)]
struct YosysModule {
    #[serde(default)]
    attributes: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    ports: BTreeMap<String, YosysPort>,
    #[serde(default)]
    cells: BTreeMap<String, YosysCell>,
    #[serde(default)]
    netnames: BTreeMap<String, YosysNetname>,
}

#[derive(Deserialize)]
struct YosysPort {
    direction: String,
    bits: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct YosysCell {
    #[serde(default)]
    #[allow(dead_code)]
    hide_name: i32,
    #[serde(rename = "type")]
    cell_type: String,
    #[serde(default)]
    connections: BTreeMap<String, Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct YosysNetname {
    #[serde(default)]
    hide_name: i32,
    bits: Vec<serde_json::Value>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum YosysBit {
    Int(i64),
    Const0,
    Const1,
    Other,
}

impl YosysBit {
    fn from_value(val: &serde_json::Value) -> Self {
        match val {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    YosysBit::Int(i)
                } else {
                    YosysBit::Other
                }
            }
            serde_json::Value::String(s) => match s.as_str() {
                "0" => YosysBit::Const0,
                "1" => YosysBit::Const1,
                _ => YosysBit::Other,
            },
            _ => YosysBit::Other,
        }
    }
}

pub fn parse_yosys_json_bytes(
    bytes: &[u8],
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let root: YosysRoot = serde_json::from_slice(bytes)
        .context("Failed to parse Yosys JSON netlist")?;

    // Find top module (non-blackbox with cells)
    let (mod_name, top_module) = root
        .modules
        .iter()
        .find(|(_, m)| {
            let is_blackbox = m
                .attributes
                .get("blackbox")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                == 1;
            !is_blackbox && !m.cells.is_empty()
        })
        .or_else(|| root.modules.iter().next())
        .context("No valid top module found in Yosys JSON")?;

    let mut db = DesignDb::new();
    db.strings = tech_db.strings.clone();
    db.design_name = mod_name.clone();
    db.dbu_per_micron = if tech_db.dbu_per_micron > 0 {
        tech_db.dbu_per_micron
    } else {
        1000
    };

    // 1. Identify all unique bits used in ports and cells
    let mut bit_to_net: HashMap<YosysBit, NetId> = HashMap::new();
    let mut net_bits: Vec<YosysBit> = Vec::new();

    // Map canonical names from netnames
    let mut bit_canonical_name: HashMap<YosysBit, String> = HashMap::new();
    for (net_name, net_data) in &top_module.netnames {
        let is_hidden = net_data.hide_name == 1;
        for (idx, b_val) in net_data.bits.iter().enumerate() {
            let bit = YosysBit::from_value(b_val);
            if matches!(bit, YosysBit::Other) {
                continue;
            }
            let name = if net_data.bits.len() == 1 {
                net_name.clone()
            } else {
                format!("{}[{}]", net_name, idx)
            };
            if !is_hidden || !bit_canonical_name.contains_key(&bit) {
                bit_canonical_name.insert(bit, name);
            }
        }
    }

    let get_or_create_net = |bit: YosysBit, net_bits: &mut Vec<YosysBit>, bit_to_net: &mut HashMap<YosysBit, NetId>| -> Option<NetId> {
        if matches!(bit, YosysBit::Other) {
            return None;
        }
        if let Some(&nid) = bit_to_net.get(&bit) {
            Some(nid)
        } else {
            let nid = NetId(net_bits.len() as u32);
            bit_to_net.insert(bit, nid);
            net_bits.push(bit);
            Some(nid)
        }
    };

    // 2. Primary I/O ports
    let mut net_pins_list: Vec<Vec<PinId>> = Vec::new();

    for (port_name, port_data) in &top_module.ports {
        let dir = match port_data.direction.as_str() {
            "input" => PinDir::Input,
            "output" => PinDir::Output,
            _ => PinDir::Inout,
        };
        let is_driver = matches!(dir, PinDir::Input | PinDir::Inout);

        for (idx, b_val) in port_data.bits.iter().enumerate() {
            let bit = YosysBit::from_value(b_val);
            let scalar_name = if port_data.bits.len() == 1 {
                port_name.clone()
            } else {
                format!("{}[{}]", port_name, idx)
            };
            let sym = db.strings.intern(&scalar_name);
            let pin_id = PinId(db.pin_names.len() as u32);

            let net_id = get_or_create_net(bit, &mut net_bits, &mut bit_to_net);

            db.pin_names.push(sym);
            db.pin_dirs.push(dir);
            db.pin_cells.push(None);
            db.pin_macro_pins.push(None);
            db.pin_lib_pins.push(None);
            db.pin_nets.push(net_id);
            db.pin_offset_x_dbu.push(0);
            db.pin_offset_y_dbu.push(0);
            db.pin_is_driver.push(is_driver);
            db.io_pins.push(pin_id);

            if let Some(nid) = net_id {
                let u = nid.0 as usize;
                if u >= net_pins_list.len() {
                    net_pins_list.resize(u + 1, Vec::new());
                }
                net_pins_list[u].push(pin_id);
            }
        }
    }

    // 3. Cells
    db.cell_pin_offsets.push(0);

    for (cell_name, cell_data) in &top_module.cells {
        let type_sym = db.strings.intern(&cell_data.cell_type);
        let macro_id = match tech_db.macro_by_name.get(&type_sym).copied() {
            Some(mid) => mid,
            None => continue, // Skip unmapped / helper cells
        };
        let mac = &tech_db.macros[macro_id.0 as usize];

        let lib_cell_id = lib_db.and_then(|lib| {
            lib.strings.get(&cell_data.cell_type)
                .and_then(|s| lib.cell_by_name.get(&s).copied())
        });

        let cell_id = CellId(db.cell_names.len() as u32);
        let cname_sym = db.strings.intern(cell_name);

        db.cell_names.push(cname_sym);
        db.cell_macros.push(macro_id);
        db.cell_lib_ids.push(lib_cell_id);
        db.cell_status.push(PlacementStatus::Unplaced);
        db.cell_x_dbu.push(0);
        db.cell_y_dbu.push(0);
        db.cell_orient.push(Orient::N);

        let mac_pins = tech_db.macro_pins(mac);
        for (pin_idx, mpin) in mac_pins.iter().enumerate() {
            let pin_name = tech_db.strings.resolve(mpin.name);
            let conn_bits = cell_data.connections.get(pin_name);

            let bit_val = conn_bits.and_then(|b| b.first());
            let net_id = bit_val
                .map(YosysBit::from_value)
                .and_then(|b| get_or_create_net(b, &mut net_bits, &mut bit_to_net));

            let pin_id = PinId(db.pin_names.len() as u32);
            let is_driver = matches!(mpin.dir, PinDir::Output);

            let lib_pin_id = lib_db.and_then(|lib| {
                if let Some(lcid) = lib_cell_id {
                    let lcell = &lib.cells[lcid.0 as usize];
                    let lpins = lib.cell_pins(lcell);
                    lpins.iter().position(|lp| {
                        lib.strings.resolve(lp.name) == pin_name
                    }).map(|idx| LibPinId((lcell.pin_offset as usize + idx) as u16))
                } else {
                    None
                }
            });

            db.pin_names.push(mpin.name);
            db.pin_dirs.push(mpin.dir);
            db.pin_cells.push(Some(cell_id));
            db.pin_macro_pins.push(Some(MacroPinId(mac.pin_offset + pin_idx as u32)));
            db.pin_lib_pins.push(lib_pin_id);
            db.pin_nets.push(net_id);
            db.pin_offset_x_dbu.push(mpin.offset_x_dbu);
            db.pin_offset_y_dbu.push(mpin.offset_y_dbu);
            db.pin_is_driver.push(is_driver);
            db.cell_pin_ids.push(pin_id);

            if let Some(nid) = net_id {
                let u = nid.0 as usize;
                if u >= net_pins_list.len() {
                    net_pins_list.resize(u + 1, Vec::new());
                }
                net_pins_list[u].push(pin_id);
            }
        }

        db.cell_pin_offsets.push(db.cell_pin_ids.len() as u32);
    }

    // 4. Net SoA and CSR arrays
    let num_nets = net_bits.len();
    db.net_names.reserve(num_nets);
    db.net_pin_offsets.reserve(num_nets + 1);

    for (nid_idx, &bit) in net_bits.iter().enumerate() {
        let name = bit_canonical_name.get(&bit).cloned().unwrap_or_else(|| match bit {
            YosysBit::Int(i) => format!("_net_{}", i),
            YosysBit::Const0 => "__const_0".to_string(),
            YosysBit::Const1 => "__const_1".to_string(),
            YosysBit::Other => format!("_net_other_{}", nid_idx),
        });
        db.net_names.push(db.strings.intern(&name));
    }

    for n in 0..num_nets {
        let offset = db.net_pin_ids.len() as u32;
        db.net_pin_offsets.push(offset);

        if n < net_pins_list.len() {
            let mut pins = net_pins_list[n].clone();
            // Invariant: driver pin placed first
            if let Some(drv_pos) = pins.iter().position(|&p| db.pin_is_driver[p.0 as usize]) {
                pins.swap(0, drv_pos);
            }
            db.net_pin_ids.extend(pins);
        }
    }
    db.net_pin_offsets.push(db.net_pin_ids.len() as u32);

    Ok(db)
}

pub fn parse_yosys_json(
    path: &Path,
    tech_db: &TechDb,
    lib_db: Option<&LibDb>,
) -> Result<DesignDb> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open Yosys JSON file at {:?}", path))?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    parse_yosys_json_bytes(&contents, tech_db, lib_db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, require, sky130_hd};
    use crate::readers::lef::load_tech_db;
    use crate::interner::StringInterner;

    #[test]
    fn test_gcd_yosys_json() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let json_path = design_dir("gcd").join("gcd.synth.json");

        if !tlef_path.is_file() {
            panic!("PDK tlef file missing at {:?}", tlef_path);
        }
        if !lef_path.is_file() {
            panic!("PDK lef file missing at {:?}", lef_path);
        }
        if !json_path.is_file() {
            panic!("Design JSON file missing at {:?}", json_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_yosys_json(&json_path, &tech_db, None)
            .expect("Failed to parse Yosys JSON");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(db.num_cells(), ref_json["verilog_logic_instances"].as_u64().unwrap() as usize);
        assert_eq!(db.num_nets(), ref_json["verilog_signal_nets"].as_u64().unwrap() as usize);
        assert_eq!(db.io_pins.len(), ref_json["verilog_bterms"].as_u64().unwrap() as usize);
        assert_eq!(db.cell_pin_ids.len(), ref_json["verilog_inst_pins"].as_u64().unwrap() as usize);

        // Verify driver invariant on all nets with at least one driver
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(first_pin, drv, "Driver pin must be first at net_pin_offsets");
            }
        }
        assert!(driven_nets >= 260, "Expected most nets to be driven, got {}", driven_nets);
    }

    #[test]
    fn test_aes_yosys_json() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let json_path = require(design_dir("aes").join("aes.synth.json"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_yosys_json(&json_path, &tech_db, None)
            .expect("Failed to parse Yosys JSON");

        let ref_path = require(design_dir("aes").join("aes.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read aes.reference.json"))
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(db.num_cells(), ref_json["verilog_logic_instances"].as_u64().unwrap() as usize);
        assert_eq!(db.num_nets(), ref_json["verilog_signal_nets"].as_u64().unwrap() as usize);
        assert_eq!(db.io_pins.len(), ref_json["verilog_bterms"].as_u64().unwrap() as usize);
        assert_eq!(db.cell_pin_ids.len(), ref_json["verilog_inst_pins"].as_u64().unwrap() as usize);

        // Driver invariant on every driven net. The count is printed, not asserted: no aes
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(first_pin, drv, "Driver pin must be first at net_pin_offsets");
            }
        }
        println!("aes yosys: {} of {} nets have a driver", driven_nets, db.num_nets());
    }

    #[test]
    fn test_jpeg_yosys_json() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let json_path = require(design_dir("jpeg").join("jpeg.synth.json"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_yosys_json(&json_path, &tech_db, None)
            .expect("Failed to parse Yosys JSON");

        let ref_path = require(design_dir("jpeg").join("jpeg.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read jpeg.reference.json"))
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(db.num_cells(), ref_json["verilog_logic_instances"].as_u64().unwrap() as usize);
        assert_eq!(db.num_nets(), ref_json["verilog_signal_nets"].as_u64().unwrap() as usize);
        assert_eq!(db.io_pins.len(), ref_json["verilog_bterms"].as_u64().unwrap() as usize);
        assert_eq!(db.cell_pin_ids.len(), ref_json["verilog_inst_pins"].as_u64().unwrap() as usize);

        // Driver invariant on every driven net. The count is printed, not asserted: no jpeg
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(first_pin, drv, "Driver pin must be first at net_pin_offsets");
            }
        }
        println!("jpeg yosys: {} of {} nets have a driver", driven_nets, db.num_nets());
    }

    #[test]
    fn test_ibex_yosys_json() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let json_path = require(design_dir("ibex").join("ibex.synth.json"));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let db = parse_yosys_json(&json_path, &tech_db, None)
            .expect("Failed to parse Yosys JSON");

        let ref_path = require(design_dir("ibex").join("ibex.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(&std::fs::read(&ref_path).expect("read ibex.reference.json"))
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        assert_eq!(db.design_name, ref_json["design"].as_str().unwrap());
        assert_eq!(db.num_cells(), ref_json["verilog_logic_instances"].as_u64().unwrap() as usize);
        assert_eq!(db.num_nets(), ref_json["verilog_signal_nets"].as_u64().unwrap() as usize);
        assert_eq!(db.io_pins.len(), ref_json["verilog_bterms"].as_u64().unwrap() as usize);
        assert_eq!(db.cell_pin_ids.len(), ref_json["verilog_inst_pins"].as_u64().unwrap() as usize);

        // Driver invariant on every driven net. The count is printed, not asserted: no ibex
        // golden value exists for it.
        let mut driven_nets = 0;
        for net_idx in 0..db.num_nets() {
            let nid = NetId(net_idx as u32);
            if let Some(drv) = db.net_driver(nid) {
                driven_nets += 1;
                let first_pin = db.net_pins(nid)[0];
                assert_eq!(first_pin, drv, "Driver pin must be first at net_pin_offsets");
            }
        }
        println!("ibex yosys: {} of {} nets have a driver", driven_nets, db.num_nets());
    }
}
