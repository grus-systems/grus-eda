use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibCellId(pub u16);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibPinId(pub u16);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId(pub u8);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacroId(pub u16);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacroPinId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PinId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinDir {
    Input,
    Output,
    Inout,
    Internal,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct StringInterner {
    strings: Vec<String>,
    #[serde(skip)]
    map: HashMap<String, SymbolId>,
}

impl StringInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> SymbolId {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = SymbolId(self.strings.len() as u32);
        let owned = s.to_owned();
        self.strings.push(owned.clone());
        self.map.insert(owned, id);
        id
    }

    pub fn resolve(&self, id: SymbolId) -> &str {
        &self.strings[id.0 as usize]
    }

    pub fn try_resolve(&self, id: SymbolId) -> Option<&str> {
        self.strings.get(id.0 as usize).map(|s| s.as_str())
    }

    pub fn get(&self, s: &str) -> Option<SymbolId> {
        self.map.get(s).copied()
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    pub fn rebuild_map(&mut self) {
        self.map.clear();
        self.map.reserve(self.strings.len());
        for (i, s) in self.strings.iter().enumerate() {
            self.map.insert(s.clone(), SymbolId(i as u32));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::{DesignDb, Orient, PlacementRow, PlacementStatus};
    use crate::lib_db::{
        LibCell, LibDb, LibPin, LibTimingArc, NldmTable, TableAxisVar, TimingSense, TimingType,
    };
    use crate::tech_db::{LayerDir, MacroClass, MacroDef, MacroPin, RectDbu, RoutingLayer, TechDb};

    #[test]
    fn test_intern_resolve_and_idempotency() {
        let mut interner = StringInterner::new();
        assert!(interner.is_empty());

        let id_clk1 = interner.intern("clk");
        let id_rst = interner.intern("rst");
        let id_clk2 = interner.intern("clk");

        // Idempotency: re-interning "clk" must return the identical SymbolId
        assert_eq!(id_clk1, id_clk2);
        assert_ne!(id_clk1, id_rst);
        assert_eq!(interner.len(), 2);

        // Resolution: resolving SymbolId must return exact str
        assert_eq!(interner.resolve(id_clk1), "clk");
        assert_eq!(interner.resolve(id_rst), "rst");
        assert_eq!(interner.get("clk"), Some(id_clk1));
        assert_eq!(interner.get("nonexistent"), None);
    }

    #[test]
    fn test_bincode_roundtrip_and_rebuild_map() {
        let mut interner = StringInterner::new();
        let s0 = interner.intern("pin_a");
        let s1 = interner.intern("pin_b");
        let s2 = interner.intern("pin_y");

        let config = bincode::config::standard();
        let encoded = bincode::serde::encode_to_vec(&interner, config)
            .expect("bincode encoding should succeed");

        let (mut decoded, _): (StringInterner, usize) =
            bincode::serde::decode_from_slice(&encoded, config)
                .expect("bincode decoding should succeed");

        // Before rebuild_map, strings are preserved but map is skipped by serde
        assert_eq!(decoded.resolve(s0), "pin_a");
        assert_eq!(decoded.resolve(s1), "pin_b");
        assert_eq!(decoded.resolve(s2), "pin_y");
        assert_eq!(decoded.get("pin_a"), None);

        // Rebuild map repopulates lookup map
        decoded.rebuild_map();
        assert_eq!(decoded.get("pin_a"), Some(s0));
        assert_eq!(decoded.get("pin_b"), Some(s1));
        assert_eq!(decoded.get("pin_y"), Some(s2));

        // Interning after rebuild preserves IDs
        let s0_again = decoded.intern("pin_a");
        assert_eq!(s0, s0_again);
        let s3 = decoded.intern("pin_new");
        assert_eq!(decoded.resolve(s3), "pin_new");
    }

    #[test]
    fn test_lib_db_construction_and_roundtrip() {
        let mut lib = LibDb::new();
        lib.library_name = "sky130_fd_sc_hd__tt_025C_1v80".to_string();
        lib.time_unit_ns = 1.0;
        lib.cap_unit_pf = 1.0;

        let cname = lib.strings.intern("sky130_fd_sc_hd__inv_1");
        let p_in = lib.strings.intern("A");
        let p_out = lib.strings.intern("Y");

        lib.pins.push(LibPin {
            name: p_in,
            dir: PinDir::Input,
            cap_rise: 0.003,
            cap_fall: 0.003,
            cap_nom: 0.003,
            is_clock: false,
            max_cap: None,
            max_transition: Some(1.5),
        });
        lib.pins.push(LibPin {
            name: p_out,
            dir: PinDir::Output,
            cap_rise: 0.0,
            cap_fall: 0.0,
            cap_nom: 0.0,
            is_clock: false,
            max_cap: Some(0.2),
            max_transition: Some(1.5),
        });

        let table = NldmTable {
            var1: TableAxisVar::InputNetTransition,
            var2: TableAxisVar::TotalOutputNetCapacitance,
            index_1: vec![0.01, 0.1],
            index_2: vec![0.001, 0.01],
            values: vec![0.02, 0.05, 0.04, 0.09],
        };

        lib.arcs.push(LibTimingArc {
            related_pin: LibPinId(0),
            target_pin: LibPinId(1),
            timing_sense: TimingSense::NegativeUnate,
            timing_type: TimingType::Combinational,
            cell_rise: Some(table.clone()),
            cell_fall: Some(table),
            rise_transition: None,
            fall_transition: None,
            rise_constraint: None,
            fall_constraint: None,
        });

        lib.cells.push(LibCell {
            name: cname,
            area: 1.08,
            is_sequential: false,
            is_latch: false,
            pin_offset: 0,
            pin_count: 2,
            arc_offset: 0,
            arc_count: 1,
        });

        lib.rebuild_cell_by_name();
        assert_eq!(lib.cell_by_name.get(&cname), Some(&LibCellId(0)));

        let cell = &lib.cells[0];
        assert_eq!(lib.cell_pins(cell).len(), 2);
        assert_eq!(lib.cell_arcs(cell).len(), 1);

        // Test bincode roundtrip of entire LibDb
        let config = bincode::config::standard();
        let encoded =
            bincode::serde::encode_to_vec(&lib, config).expect("LibDb bincode encode failed");
        let (mut decoded, _): (LibDb, usize) =
            bincode::serde::decode_from_slice(&encoded, config).expect("LibDb bincode decode failed");

        decoded.strings.rebuild_map();
        decoded.rebuild_cell_by_name();

        assert_eq!(decoded.library_name, lib.library_name);
        assert_eq!(decoded.cells.len(), 1);
        assert_eq!(decoded.pins.len(), 2);
        assert_eq!(decoded.arcs.len(), 1);
        assert_eq!(decoded.cell_by_name.get(&cname), Some(&LibCellId(0)));
        assert_eq!(decoded.strings.resolve(cname), "sky130_fd_sc_hd__inv_1");
    }

    #[test]
    fn test_tech_db_construction() {
        let mut tech = TechDb::new();
        let mut interner = StringInterner::new();
        tech.dbu_per_micron = 1000;
        tech.site_name = "unithd".to_string();
        tech.site_width_dbu = 460;
        tech.site_height_dbu = 2720;

        let met1_name = interner.intern("met1");
        tech.routing_layers.push(RoutingLayer {
            name: met1_name,
            dir: LayerDir::Horizontal,
            pitch_dbu: 340,
            width_dbu: 140,
            offset_dbu: 170,
            res_per_sq: 0.125,
            cap_per_sq_um: 0.00003,
            edge_cap_per_um: 0.00004,
        });

        let macro_name = interner.intern("sky130_fd_sc_hd__buf_1");
        let pin_a_name = interner.intern("A");
        let pin_x_name = interner.intern("X");

        tech.macro_pins.push(MacroPin {
            name: pin_a_name,
            dir: PinDir::Input,
            is_power: false,
            offset_x_dbu: 230,
            offset_y_dbu: 1360,
            offset_x_frac: false,
            offset_y_frac: false,
            layer: Some(LayerId(0)),
        });
        tech.macro_pins.push(MacroPin {
            name: pin_x_name,
            dir: PinDir::Output,
            is_power: false,
            offset_x_dbu: 400,
            offset_y_dbu: 1360,
            offset_x_frac: false,
            offset_y_frac: false,
            layer: Some(LayerId(0)),
        });

        tech.macros.push(MacroDef {
            name: macro_name,
            class: MacroClass::Core,
            width_dbu: 920,
            height_dbu: 2720,
            pin_offset: 0,
            pin_count: 2,
            obs_rects: vec![(LayerId(0), RectDbu::new(0, 0, 920, 2720))],
            pin_obs_rects: Vec::new(),
        });

        tech.rebuild_maps();
        assert_eq!(tech.layer_by_name.get(&met1_name), Some(&LayerId(0)));
        assert_eq!(tech.macro_by_name.get(&macro_name), Some(&MacroId(0)));
        assert_eq!(tech.macro_pins(&tech.macros[0]).len(), 2);

        let rect = RectDbu::new(10, 20, 110, 120);
        assert_eq!(rect.width(), 100);
        assert_eq!(rect.height(), 100);
        assert_eq!(rect.area(), 10_000);
    }

    #[test]
    fn test_design_db_construction() {
        let mut db = DesignDb::new();
        db.design_name = "test_design".to_string();
        db.dbu_per_micron = 1000;
        db.die_area = RectDbu::new(0, 0, 100_000, 100_000);

        let row_name = db.strings.intern("ROW_0");
        let site_name = db.strings.intern("unithd");
        db.rows.push(PlacementRow {
            row_name,
            site_name,
            orig_x_dbu: 0,
            orig_y_dbu: 0,
            orient: Orient::N,
            num_x: 200,
            step_x_dbu: 460,
        });

        // 1 instance with 2 pins: u0 (inv_1)
        // Primary ports: clk_in (driver), out_y (sink)
        let c0_name = db.strings.intern("u0");
        let p0_name = db.strings.intern("A");
        let p1_name = db.strings.intern("Y");
        let port_in = db.strings.intern("in");
        let port_out = db.strings.intern("out");

        // Primary I/O pins: PinId 0 (port in), PinId 1 (port out)
        db.pin_names.push(port_in);
        db.pin_dirs.push(PinDir::Input);
        db.pin_cells.push(None);
        db.pin_macro_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_nets.push(Some(NetId(0)));
        db.pin_offset_x_dbu.push(0);
        db.pin_offset_y_dbu.push(50_000);
        db.pin_is_driver.push(true); // primary input drives net 0
        db.io_pins.push(PinId(0));

        db.pin_names.push(port_out);
        db.pin_dirs.push(PinDir::Output);
        db.pin_cells.push(None);
        db.pin_macro_pins.push(None);
        db.pin_lib_pins.push(None);
        db.pin_nets.push(Some(NetId(1)));
        db.pin_offset_x_dbu.push(100_000);
        db.pin_offset_y_dbu.push(50_000);
        db.pin_is_driver.push(false); // primary output is sink of net 1
        db.io_pins.push(PinId(1));

        // Instance pins: PinId 2 (u0.A), PinId 3 (u0.Y)
        db.pin_names.push(p0_name);
        db.pin_dirs.push(PinDir::Input);
        db.pin_cells.push(Some(CellId(0)));
        db.pin_macro_pins.push(Some(MacroPinId(0)));
        db.pin_lib_pins.push(Some(LibPinId(0)));
        db.pin_nets.push(Some(NetId(0)));
        db.pin_offset_x_dbu.push(230);
        db.pin_offset_y_dbu.push(1360);
        db.pin_is_driver.push(false);

        db.pin_names.push(p1_name);
        db.pin_dirs.push(PinDir::Output);
        db.pin_cells.push(Some(CellId(0)));
        db.pin_macro_pins.push(Some(MacroPinId(1)));
        db.pin_lib_pins.push(Some(LibPinId(1)));
        db.pin_nets.push(Some(NetId(1)));
        db.pin_offset_x_dbu.push(400);
        db.pin_offset_y_dbu.push(1360);
        db.pin_is_driver.push(true); // instance output drives net 1

        // Cell 0 SoA
        db.cell_names.push(c0_name);
        db.cell_macros.push(MacroId(0));
        db.cell_lib_ids.push(Some(LibCellId(0)));
        db.cell_status.push(PlacementStatus::Placed);
        db.cell_x_dbu.push(10_000);
        db.cell_y_dbu.push(20_000);
        db.cell_orient.push(Orient::N);
        db.cell_pin_offsets = vec![0, 2];
        db.cell_pin_ids = vec![PinId(2), PinId(3)];

        // Nets: Net 0 ("net_in") connected to PinId(0) [driver] and PinId(2) [sink]
        // Net 1 ("net_out") connected to PinId(3) [driver] and PinId(1) [sink]
        let net0_name = db.strings.intern("net_in");
        let net1_name = db.strings.intern("net_out");
        db.net_names.push(net0_name);
        db.net_names.push(net1_name);
        db.net_pin_offsets = vec![0, 2, 4];
        db.net_pin_ids = vec![PinId(0), PinId(2), PinId(3), PinId(1)];

        assert_eq!(db.num_cells(), 1);
        assert_eq!(db.num_pins(), 4);
        assert_eq!(db.num_nets(), 2);
        assert_eq!(db.num_rows(), 1);

        assert_eq!(db.cell_pins(CellId(0)), &[PinId(2), PinId(3)]);
        assert_eq!(db.net_pins(NetId(0)), &[PinId(0), PinId(2)]);
        assert_eq!(db.net_pins(NetId(1)), &[PinId(3), PinId(1)]);

        // Check net drivers
        assert_eq!(db.net_driver(NetId(0)), Some(PinId(0)));
        assert_eq!(db.net_driver(NetId(1)), Some(PinId(3)));
    }
}
