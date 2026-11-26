use crate::interner::{
    CellId, LayerId, LibCellId, LibPinId, MacroId, MacroPinId, NetId, PinDir, PinId,
    StringInterner, SymbolId,
};
use crate::lib_db::LibDb;
use crate::readers::def::escape_def_name;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementStatus {
    Unplaced,
    Placed,
    Fixed,
    Cover,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orient {
    N,
    S,
    E,
    W,
    FN,
    FS,
    FE,
    FW,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacementRow {
    pub row_name: SymbolId,
    pub site_name: SymbolId,
    pub orig_x_dbu: i32,
    pub orig_y_dbu: i32,
    pub orient: Orient,
    pub num_x: i32,
    pub step_x_dbu: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackGrid {
    pub is_vertical_tracks: bool, // true = X tracks (vertical lines), false = Y tracks (horizontal lines)
    pub start_dbu: i32,
    pub num_tracks: i32,
    pub step_dbu: i32,
    pub layers: Vec<LayerId>,
}

/// One DEF `SPECIALNETS` record: a power-delivery (or other special) net's
/// routed shapes, via stacks, and connection groups verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdnNet {
    pub name: SymbolId,
    pub use_kind: Option<PdnUse>,
    /// `( comp pin )` groups verbatim; expanded by `pdn::build_graph` so the
    /// parse makes no COMPONENTS/PINS ordering assumption.
    pub conns: Vec<PdnConn>,
    /// Two-point wire segments, half-width expanded. `width == 0` segments
    /// never land here — they are the via-stack records in `vias`.
    pub wire_rects: Vec<(LayerId, RectDbu)>,
    pub vias: Vec<PdnVia>,
}

/// DEF `+ USE` keyword on a special net.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdnUse {
    Power,
    Ground,
    Signal,
    Clock,
    Analog,
    Reset,
    Scan,
    TieOff,
    Ring,
}

/// A SPECIALNETS `( comp pin )` connection group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PdnConn {
    /// `( PIN <name> )` — a top-level bterm; the net's voltage source.
    TopPin(SymbolId),
    /// `( * <pin> )` — every component's pin named `<pin>`.
    AllInstances(SymbolId),
    /// `( <inst> <pin> )` — a single instance's pin.
    Instance { inst: SymbolId, pin: SymbolId },
}

/// How a `PdnVia`'s shapes are derived at expansion time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ViaGeom {
    /// LEF `VIA` — index into `TechDb::via_defs`; `rects` verbatim.
    Lef(u32),
    /// DEF `VIAS + VIARULE` generated via — rects from params.
    Generated(GeneratedViaGeom),
}

/// DEF `VIAS` generated-via parameters (`+ CUTSIZE` etc.), retained at
/// parse. `enc_*` are symmetric overhangs per OpenDB semantics:
/// `(botX, botY, topX, topY)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratedViaGeom {
    pub cut_w_dbu: i32,
    pub cut_h_dbu: i32,
    pub rows: u32,
    pub cols: u32,
    /// Edge-to-edge cut spacing; `+ CUTSPACING` absent means 0.
    pub cut_spacing_x_dbu: i32,
    pub cut_spacing_y_dbu: i32,
    pub enc_bot_x_dbu: i32,
    pub enc_bot_y_dbu: i32,
    pub enc_top_x_dbu: i32,
    pub enc_top_y_dbu: i32,
}

/// A DEF via-stack record `pt <viaName>` (or the `NEW <layer> 0 …
/// ( x y ) <viaName>` link in SPECIALNETS), resolved through the DEF
/// `VIAS` section or a LEF `VIA` definition. Shared by `PdnNet` and
/// `NetGeom` — the name predates that use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdnVia {
    /// DEF-spelled via name (kept for diagnostics).
    pub name: SymbolId,
    /// Endpoints of the stack link.
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut layer; resistance via `TechDb::cut_layer_by_name`.
    pub cut: SymbolId,
    /// Parallel cuts (`+ ROWCOL` product or LEF cut-RECT count).
    pub cut_count: u32,
    /// Shape recipe — LEF rects verbatim or generated-from-params.
    pub geom: ViaGeom,
    pub x_dbu: i32,
    pub y_dbu: i32,
}

/// A resolved DEF `VIAS` record: `+ VIARULE` generated vias keep
/// their geometry params; `+ VIA <lef>` aliases resolve to
/// `ViaGeom::Lef`. `+ RECT`/`+ POLYGON` inline records and `+ VIA`
/// names the LEF does not define store nothing — name-reference
/// errors stay lazy on the wire-resolution path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DefViaDef {
    /// DEF-spelled name.
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    pub cut: SymbolId,
    pub cut_count: u32,
    pub geom: ViaGeom,
}

/// Routed shapes of one `NETS` net — same shape vocabulary `PdnNet`
/// proved for SPECIALNETS.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NetGeom {
    /// Two-point segments + `RECT` stubs, half-width-expanded at parse
    /// by the layer's LEF `WIDTH` (regular-net wires carry no width
    /// token). Statement order.
    pub wire_rects: Vec<(LayerId, RectDbu)>,
    /// Via instances, path order.
    pub vias: Vec<PdnVia>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GCellGridDef {
    pub is_x: bool, // true = GCELLGRID X, false = GCELLGRID Y
    pub start_dbu: i32,
    /// `GCELLGRID DO` value — the gcell column/row count in the
    /// OpenROAD convention this codebase follows (strict DEF 5.8 reads
    /// `DO` as grid *lines*; the reference tool writes cells).
    pub num_cells: i32,
    pub step_dbu: i32,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct DesignDb {
    pub design_name: String,
    pub strings: StringInterner,
    pub dbu_per_micron: i32,
    pub die_area: RectDbu,
    pub rows: Vec<PlacementRow>,
    pub tracks: Vec<TrackGrid>,
    pub gcell_grids: Vec<GCellGridDef>,
    /// SPECIALNETS records (PDN stripes/rails and via stacks), one `PdnNet`
    /// per DEF net name.
    #[serde(default)]
    pub pdn_nets: Vec<PdnNet>,
    /// DEF `VIAS` defs in record order — the via candidate table's
    /// second source (`drt::via`). Inline `+ RECT`/`+ POLYGON` records
    /// store nothing.
    #[serde(default)]
    pub def_vias: Vec<DefViaDef>,

    // --- Cell SoA (indexed by CellId) ---
    pub cell_names: Vec<SymbolId>,
    pub cell_macros: Vec<MacroId>,
    pub cell_lib_ids: Vec<Option<LibCellId>>,
    pub cell_status: Vec<PlacementStatus>,
    pub cell_x_dbu: Vec<i32>,
    pub cell_y_dbu: Vec<i32>,
    pub cell_orient: Vec<Orient>,
    pub cell_pin_offsets: Vec<u32>, // length num_cells + 1
    pub cell_pin_ids: Vec<PinId>,

    // --- Pin SoA (indexed by PinId; includes instance pins and primary I/O ports) ---
    pub pin_names: Vec<SymbolId>, // port name for primary I/O, or macro pin name for instance pin
    pub pin_dirs: Vec<PinDir>,
    pub pin_cells: Vec<Option<CellId>>, // None for primary I/O port
    pub pin_macro_pins: Vec<Option<MacroPinId>>,
    pub pin_lib_pins: Vec<Option<LibPinId>>,
    pub pin_nets: Vec<Option<NetId>>,
    pub pin_offset_x_dbu: Vec<i32>, // relative to cell (or absolute for I/O port)
    pub pin_offset_y_dbu: Vec<i32>,
    pub pin_io_layer: Vec<Option<LayerId>>, // DEF `+ LAYER` of primary I/O pins; None for instance pins
    /// Absolute `(layer, rect)` shapes of primary I/O ports, in PINS record
    /// order. Guide parasitics reproduce OpenROAD's bterm pin boxes from
    /// these; instance pins stay empty (their shapes live in `MacroPin`).
    pub pin_io_rects: Vec<Vec<(LayerId, RectDbu)>>,
    pub pin_is_driver: Vec<bool>,
    pub io_pins: Vec<PinId>, // indices of primary I/O ports (where pin_cells[p] == None)

    // --- Net SoA & CSR (indexed by NetId) ---
    pub net_names: Vec<SymbolId>,
    /// Raw NETS-section spelling per net (escape characters kept). OpenDB
    /// stores net names verbatim, so route-guide emission must reproduce
    /// this token for `read_guides` to match nets. Empty for non-DEF
    /// sources; then guide emission falls back to escaping `net_names`.
    pub net_def_spellings: Vec<SymbolId>,
    pub net_pin_offsets: Vec<u32>, // length num_nets + 1
    pub net_pin_ids: Vec<PinId>, // driver pin (if any) is always placed first at net_pin_offsets[net]
    /// Per-net routed geometry from `NETS`, parallel to `net_names`.
    /// Empty for non-DEF sources (verilog/yosys) that never populate it.
    #[serde(default)]
    pub net_geoms: Vec<NetGeom>,
}

impl DesignDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn num_cells(&self) -> usize {
        self.cell_names.len()
    }

    pub fn num_pins(&self) -> usize {
        self.pin_names.len()
    }

    pub fn num_nets(&self) -> usize {
        self.net_names.len()
    }

    pub fn num_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn cell_pins(&self, cell: CellId) -> &[PinId] {
        let idx = cell.0 as usize;
        let start = self.cell_pin_offsets[idx] as usize;
        let end = self.cell_pin_offsets[idx + 1] as usize;
        &self.cell_pin_ids[start..end]
    }

    pub fn net_pins(&self, net: NetId) -> &[PinId] {
        let idx = net.0 as usize;
        let start = self.net_pin_offsets[idx] as usize;
        let end = self.net_pin_offsets[idx + 1] as usize;
        &self.net_pin_ids[start..end]
    }

    /// Flat view over every special net's wire rects — identical coverage to
    /// the removed `special_wire_rects` field: every two-point segment any
    /// special net declares lands in `PdnNet::wire_rects`.
    pub fn special_wire_rects(&self) -> impl Iterator<Item = (LayerId, RectDbu)> + '_ {
        self.pdn_nets.iter().flat_map(|n| n.wire_rects.iter().copied())
    }

    /// Flat view over every signal net's wire rects — every routed
    /// segment a `NETS` record declares lands in `NetGeom::wire_rects`.
    pub fn signal_wire_rects(&self) -> impl Iterator<Item = (LayerId, RectDbu)> + '_ {
        self.net_geoms.iter().flat_map(|n| n.wire_rects.iter().copied())
    }

    /// Routed geometry of `net`; `None` only on non-DEF designs where
    /// `net_geoms` is not populated.
    pub fn net_geom(&self, net: NetId) -> Option<&NetGeom> {
        self.net_geoms.get(net.0 as usize)
    }

    /// Net lookup by DEF name, for `pdn` and tests.
    pub fn pdn_net(&self, name: &str) -> Option<&PdnNet> {
        let sym = self.strings.get(name)?;
        self.pdn_nets.iter().find(|n| n.name == sym)
    }

    /// Macro-coords rect → oriented macro-coords rect (`w`/`h` = macro size).
    /// Shared by `congestion::gcell` (blockages) and `pdn` (pin attach); the
    /// caller translates by the cell's placement point.
    pub fn orient_rect(r: RectDbu, w: i32, h: i32, orient: Orient) -> RectDbu {
        match orient {
            Orient::N => r,
            Orient::S => RectDbu::new(w - r.xh, h - r.yh, w - r.xl, h - r.yl),
            Orient::W => RectDbu::new(h - r.yh, r.xl, h - r.yl, r.xh),
            Orient::E => RectDbu::new(r.yl, w - r.xh, r.yh, w - r.xl),
            Orient::FN => RectDbu::new(w - r.xh, r.yl, w - r.xl, r.yh),
            Orient::FS => RectDbu::new(r.xl, h - r.yh, r.xh, h - r.yl),
            Orient::FW => RectDbu::new(r.yl, r.xl, r.yh, r.xh),
            Orient::FE => RectDbu::new(h - r.yh, w - r.xh, h - r.yl, w - r.xl),
        }
    }

    pub fn net_driver(&self, net: NetId) -> Option<PinId> {
        let pins = self.net_pins(net);
        pins.first()
            .copied()
            .filter(|&first_pin| self.pin_is_driver[first_pin.0 as usize])
    }

    /// Finds the design pin of `cell` whose Liberty pin name is `sym` —
    /// Liberty names pins by `function`/`related_pin`/`when` text, not by
    /// the netlist's pin names.
    pub(crate) fn pin_by_lib_name(
        &self,
        lib: &LibDb,
        cell: CellId,
        sym: SymbolId,
    ) -> Option<PinId> {
        self.cell_pins(cell).iter().copied().find(|&p| {
            self.pin_lib_pins[p.0 as usize]
                .map(|lp| lib.pins[lp.0 as usize].name == sym)
                .unwrap_or(false)
        })
    }

    /// Computes the absolute coordinates (x, y) in DBU for a given pin under its cell's placement and orientation.
    pub fn pin_pos_dbu(&self, pin: PinId, tech_db: &TechDb) -> (i32, i32) {
        let p = pin.0 as usize;
        match self.pin_cells[p] {
            None => {
                // Primary I/O pin: absolute coordinates
                (self.pin_offset_x_dbu[p], self.pin_offset_y_dbu[p])
            }
            Some(cell_id) => {
                let c = cell_id.0 as usize;
                let cx = self.cell_x_dbu[c];
                let cy = self.cell_y_dbu[c];
                let orient = self.cell_orient[c];
                let macro_id = self.cell_macros[c];
                let mac = &tech_db.macros[macro_id.0 as usize];
                let w = mac.width_dbu;
                let h = mac.height_dbu;
                let ox = self.pin_offset_x_dbu[p];
                let oy = self.pin_offset_y_dbu[p];
                let (fx, fy) = match self.pin_macro_pins[p] {
                    Some(mp) => {
                        let mpin = &tech_db.macro_pins[mp.0 as usize];
                        (mpin.offset_x_frac, mpin.offset_y_frac)
                    }
                    None => (false, false),
                };
                let x = |base, off, frac| truncated_axis(base, off, frac, false);
                let mx = |base, off, frac| truncated_axis(base, off, frac, true);

                match orient {
                    Orient::N => (x(cx, ox, fx), x(cy, oy, fy)),
                    Orient::S => (mx(cx + w, ox, fx), mx(cy + h, oy, fy)),
                    Orient::W => (mx(cx + h, oy, fy), x(cy, ox, fx)),
                    Orient::E => (x(cx, oy, fy), mx(cy + w, ox, fx)),
                    Orient::FN => (mx(cx + w, ox, fx), x(cy, oy, fy)),
                    Orient::FS => (x(cx, ox, fx), mx(cy + h, oy, fy)),
                    Orient::FW => (x(cx, oy, fy), x(cy, ox, fx)),
                    Orient::FE => (mx(cx + h, oy, fy), mx(cy + w, ox, fx)),
                }
            }
        }
    }

    /// Computes the total Half-Perimeter Wirelength (HPWL) in DBU across all signal nets.
    pub fn compute_hpwl_dbu(&self, tech_db: &TechDb) -> i64 {
        let mut total_hpwl: i64 = 0;
        for net_idx in 0..self.num_nets() {
            let nid = NetId(net_idx as u32);
            let pins = self.net_pins(nid);
            if pins.len() <= 1 {
                continue;
            }
            let mut min_x = i32::MAX;
            let mut max_x = i32::MIN;
            let mut min_y = i32::MAX;
            let mut max_y = i32::MIN;

            for &pin in pins {
                let (px, py) = self.pin_pos_dbu(pin, tech_db);
                min_x = min_x.min(px);
                max_x = max_x.max(px);
                min_y = min_y.min(py);
                max_y = max_y.max(py);
            }

            if max_x >= min_x && max_y >= min_y {
                let dx = (max_x - min_x) as i64;
                let dy = (max_y - min_y) as i64;
                total_hpwl += dx + dy;
            }
        }
        total_hpwl
    }
}

/// One axis of an instance pin position, as OpenROAD's `dbITerm::getAvgXY` gives it:
/// the exact value is `base + c` (or `base - c` when `mirrored`), where the local mean
/// center `c` lies in `[off, off + 1)` and is fractional iff `frac`. OpenROAD truncates
/// toward zero, so the result is whichever of `floor` and `floor + 1` is nearer zero.
fn truncated_axis(base: i32, off: i32, frac: bool, mirrored: bool) -> i32 {
    if !frac {
        return if mirrored { base - off } else { base + off };
    }
    let floor = if mirrored { base - off - 1 } else { base + off };
    if floor < 0 { floor + 1 } else { floor }
}

/// Netlist mutation for CTS-style edits. Every method preserves the
/// invariants the readers establish at parse time:
///
/// - **I1 — SoA lengths.** All seven `cell_*` columns have length
///   `num_cells()`; all eleven `pin_*` columns (including
///   `pin_io_rects`) have length `num_pins()`. `cell_pin_offsets` and
///   `net_pin_offsets` have length `num_cells()+1`/`num_nets()+1`, are
///   nondecreasing, start at 0, and end at `cell_pin_ids.len()`/
///   `net_pin_ids.len()`.
/// - **I2 — `cell_pin_ids` partition.** `pin_cells[p] == Some(c)` iff `p`
///   lies in cell `c`'s range.
/// - **I3 — `net_pin_ids` partition + `pin_nets` back-pointer.**
///   `pin_nets[p] == Some(n)` iff `p` lies in net `n`'s segment;
///   `pin_nets[p] == None` iff `p` is in no segment.
/// - **I4 — driver-first.** If any pin in a net's segment has
///   `pin_is_driver`, the pin at segment index 0 is a driver
///   (`net_driver` reads `seg[0]`).
/// - **I5 — `io_pins`.** The non-special primary I/O ports in `PinId`
///   order — a subset of the `pin_cells == None` pins (`+ SPECIAL`
///   DEF ports such as VGND/VPWR also have `pin_cells == None` but are
///   not listed). The mutators never touch it; inserted cells produce
///   only instance pins.
/// - **I6 — `net_def_spellings`.** `len() ∈ {0, num_nets()}`; the guide
///   emitter uses per-net spellings only when the lengths match, and both
///   states stay valid.
/// - **I7 — `net_geoms`.** `len() ∈ {0, num_nets()}`; `0` only for
///   non-DEF sources (verilog/yosys) that never populate routed
///   geometry.
///
/// Cost policy (recorded choice): `connect_pin`/`disconnect_pin` edit
/// `net_pin_ids` in place — one `Vec::remove`/`insert` plus an
/// O(`num_nets`) offset shift — preserving intra-net order exactly, so
/// untouched nets re-emit byte-identically. Each call is
/// O(#`net_pin_ids` + #nets); at E5 scale (a picorv32-size CTS is ~10³
/// calls × ~10⁵ entries) that is trivial. A batch move rebuilding the CSR
/// once is the upgrade path if CTS profiling shows the per-call cost in a
/// hot loop.
impl DesignDb {
    /// Appends an instance of `macro_id` named `name`, `Unplaced` at
    /// `(0, 0)` `N` — the caller seeds `cell_x_dbu`/`cell_y_dbu` before
    /// legalization. One pin is created per `tech_db.macro_pins(macro)`:
    /// direction, offsets and `is_driver` come from the `MacroPin`, and
    /// `cell_lib_ids`/`pin_lib_pins` are resolved by name against `lib_db`
    /// exactly as the DEF reader's `add_component` does (`None` when
    /// `lib_db` is `None` or the cell is not in the Liberty — inserted
    /// cells then carry no Liberty ids and are invisible to STA, which is
    /// the caller's choice to make).
    ///
    /// `mpin.name` `SymbolId`s go into `pin_names` as-is: every reader
    /// sets `db.strings = tech_db.strings.clone()`, so symbol ids from
    /// `tech_db.strings` resolve identically in `db.strings`.
    ///
    /// Preserves I1 and I2. Errors on a duplicate `cell_names` entry (a
    /// linear scan through `db.strings` — a `cell_by_name` map is
    /// deliberately deferred) or `macro_id` out of range.
    pub fn insert_cell(
        &mut self,
        name: &str,
        macro_id: MacroId,
        tech_db: &TechDb,
        lib_db: Option<&LibDb>,
    ) -> Result<CellId> {
        if self
            .cell_names
            .iter()
            .any(|&s| self.strings.resolve(s) == name)
        {
            bail!("insert_cell: cell name `{name}` already exists");
        }
        let mac = tech_db
            .macros
            .get(macro_id.0 as usize)
            .with_context(|| format!("insert_cell: macro id {} out of range", macro_id.0))?;
        let macro_name = tech_db.strings.resolve(mac.name);
        let lib_cell_id = lib_db.and_then(|lib| {
            lib.strings
                .get(macro_name)
                .and_then(|s| lib.cell_by_name.get(&s).copied())
        });

        let cell_id = CellId(self.cell_names.len() as u32);
        self.cell_names.push(self.strings.intern(name));
        self.cell_macros.push(macro_id);
        self.cell_lib_ids.push(lib_cell_id);
        self.cell_status.push(PlacementStatus::Unplaced);
        self.cell_x_dbu.push(0);
        self.cell_y_dbu.push(0);
        self.cell_orient.push(Orient::N);

        for (pin_idx, mpin) in tech_db.macro_pins(mac).iter().enumerate() {
            let pin_name = tech_db.strings.resolve(mpin.name);
            let pin_id = PinId(self.pin_names.len() as u32);
            let lib_pin_id = lib_db.and_then(|lib| {
                lib_cell_id.and_then(|lcid| {
                    let lcell = &lib.cells[lcid.0 as usize];
                    lib.cell_pins(lcell)
                        .iter()
                        .position(|lp| lib.strings.resolve(lp.name) == pin_name)
                        .map(|idx| LibPinId((lcell.pin_offset as usize + idx) as u16))
                })
            });
            self.pin_names.push(mpin.name);
            self.pin_dirs.push(mpin.dir);
            self.pin_cells.push(Some(cell_id));
            self.pin_macro_pins
                .push(Some(MacroPinId(mac.pin_offset + pin_idx as u32)));
            self.pin_lib_pins.push(lib_pin_id);
            self.pin_nets.push(None);
            self.pin_offset_x_dbu.push(mpin.offset_x_dbu);
            self.pin_offset_y_dbu.push(mpin.offset_y_dbu);
            self.pin_io_layer.push(None);
            self.pin_io_rects.push(Vec::new());
            self.pin_is_driver.push(matches!(mpin.dir, PinDir::Output));
            self.cell_pin_ids.push(pin_id);
        }
        self.cell_pin_offsets.push(self.cell_pin_ids.len() as u32);
        Ok(cell_id)
    }

    /// Appends an empty net named `name` and returns its `NetId`.
    /// Preserves I1, I3 (the new segment is empty) and I6: when
    /// `net_def_spellings` currently matches `num_nets()` (a DEF-sourced
    /// design) the escaped spelling is pushed so the emitted guide token
    /// equals what `write_def_full` writes; a non-DEF design's empty
    /// `net_def_spellings` stays empty. Errors on a duplicate `net_names`
    /// entry.
    pub fn create_net(&mut self, name: &str) -> Result<NetId> {
        if self
            .net_names
            .iter()
            .any(|&s| self.strings.resolve(s) == name)
        {
            bail!("create_net: net name `{name}` already exists");
        }
        if self.net_def_spellings.len() == self.net_names.len() {
            let spelling = self.strings.intern(&escape_def_name(name));
            self.net_def_spellings.push(spelling);
        }
        if self.net_geoms.len() == self.net_names.len() {
            self.net_geoms.push(NetGeom::default());
        }
        let net_id = NetId(self.net_names.len() as u32);
        self.net_names.push(self.strings.intern(name));
        self.net_pin_offsets.push(self.net_pin_ids.len() as u32);
        Ok(net_id)
    }

    /// Moves `pin` onto `net`; a no-op when it is already there. A driver
    /// pin lands at segment index 0 — on a multi-driver net the previous
    /// index-0 driver slides to index 1 and `net_driver` then answers the
    /// most recently connected driver (single-driver nets are all this
    /// API is built for; STA counts drivers elsewhere). A sink appends at
    /// the segment end. Works on I/O pins (`pin_cells == None`)
    /// unchanged. Preserves I1, I3, I4; see the impl-block cost policy.
    ///
    /// Panics on an out-of-range `PinId`/`NetId`, same as `net_pins`.
    pub fn connect_pin(&mut self, pin: PinId, net: NetId) {
        let p = pin.0 as usize;
        let n = net.0 as usize;
        if self.pin_nets[p] == Some(net) {
            return;
        }
        self.disconnect_pin(pin);
        // `net_pin_offsets` is already post-removal here: `disconnect_pin`
        // shifted every later segment start, so the insert index is right
        // even when the old net sat before `net`.
        let idx = if self.pin_is_driver[p] {
            self.net_pin_offsets[n] as usize
        } else {
            self.net_pin_offsets[n + 1] as usize
        };
        self.net_pin_ids.insert(idx, pin);
        self.pin_nets[p] = Some(net);
        // Net n's segment grew by one: every later segment start shifts.
        for off in &mut self.net_pin_offsets[n + 1..] {
            *off += 1;
        }
    }

    /// Removes `pin` from its net; a no-op when `pin_nets[p]` is `None`.
    /// The shrunk segment keeps its order and is re-normalized to I4 the
    /// way the readers normalize a parsed segment: if the first remaining
    /// pin is not a driver but a later member is, it moves to index 0.
    /// Preserves I1, I3, I4.
    ///
    /// Panics on an out-of-range `PinId`.
    pub fn disconnect_pin(&mut self, pin: PinId) {
        let p = pin.0 as usize;
        let Some(net) = self.pin_nets[p] else { return };
        let n = net.0 as usize;
        let start = self.net_pin_offsets[n] as usize;
        let end = self.net_pin_offsets[n + 1] as usize;
        let pos = self.net_pin_ids[start..end]
            .iter()
            .position(|&q| q == pin)
            .expect("pin_nets disagrees with net_pin_ids membership");
        self.net_pin_ids.remove(start + pos);
        self.pin_nets[p] = None;
        // Net n's segment shrank by one: every later segment start shifts.
        for off in &mut self.net_pin_offsets[n + 1..] {
            *off -= 1;
        }
        let start = self.net_pin_offsets[n] as usize;
        let end = self.net_pin_offsets[n + 1] as usize;
        if start < end
            && !self.pin_is_driver[self.net_pin_ids[start].0 as usize]
            && let Some(dpos) = self.net_pin_ids[start..end]
                .iter()
                .position(|&q| self.pin_is_driver[q.0 as usize])
        {
            self.net_pin_ids.swap(start, start + dpos);
        }
    }

    /// CTS-shaped convenience: inserts a `buffer_macro` instance `name`
    /// whose input joins `net` and whose output drives a new net
    /// `new_net_name` carrying every former sink of `net`, in order.
    /// `net` keeps its driver: the result is `net = {driver, buf_in}` and
    /// `new_net = {buf_out, old sinks…}`. A net whose only member is a
    /// driver produces `new_net = {buf_out}` — legal output, no special
    /// case. Returns the inserted cell and the new net.
    ///
    /// Errors when `net` has no driver (nothing to anchor the split),
    /// when it has more than one driver pin (which driver keeps the
    /// original net is ambiguous — connect the extra drivers explicitly
    /// first), or when `buffer_macro` does not have exactly one
    /// non-power `Input` pin and one non-power `Output` pin — the
    /// `is_power` test is load-bearing: `clkbuf_*` power pins are
    /// `DIRECTION INOUT` in LEF and would pass a direction-only filter.
    /// Both names are validated before any mutation, so a failure
    /// leaves the design untouched.
    pub fn insert_buffer_on_net(
        &mut self,
        net: NetId,
        buffer_macro: MacroId,
        name: &str,
        new_net_name: &str,
        tech_db: &TechDb,
        lib_db: Option<&LibDb>,
    ) -> Result<(CellId, NetId)> {
        let drivers: Vec<PinId> = self
            .net_pins(net)
            .iter()
            .copied()
            .filter(|&p| self.pin_is_driver[p.0 as usize])
            .collect();
        match drivers.as_slice() {
            [] => bail!(
                "insert_buffer_on_net: net {} `{}` has no driver",
                net.0,
                self.strings.resolve(self.net_names[net.0 as usize])
            ),
            [_] => {}
            _ => bail!(
                "insert_buffer_on_net: net {} `{}` has {} driver pins; only single-driver nets are supported",
                net.0,
                self.strings.resolve(self.net_names[net.0 as usize]),
                drivers.len()
            ),
        }
        let driver = drivers[0];
        if self
            .cell_names
            .iter()
            .any(|&s| self.strings.resolve(s) == name)
        {
            bail!("insert_buffer_on_net: cell name `{name}` already exists");
        }
        if self
            .net_names
            .iter()
            .any(|&s| self.strings.resolve(s) == new_net_name)
        {
            bail!("insert_buffer_on_net: net name `{new_net_name}` already exists");
        }
        let mac = tech_db
            .macros
            .get(buffer_macro.0 as usize)
            .with_context(|| {
                format!(
                    "insert_buffer_on_net: macro id {} out of range",
                    buffer_macro.0
                )
            })?;
        let inputs: Vec<SymbolId> = tech_db
            .macro_pins(mac)
            .iter()
            .filter(|m| !m.is_power && m.dir == PinDir::Input)
            .map(|m| m.name)
            .collect();
        let outputs: Vec<SymbolId> = tech_db
            .macro_pins(mac)
            .iter()
            .filter(|m| !m.is_power && m.dir == PinDir::Output)
            .map(|m| m.name)
            .collect();
        if inputs.len() != 1 || outputs.len() != 1 {
            bail!(
                "insert_buffer_on_net: macro `{}` has {} signal input and {} signal output pins; need exactly one of each",
                tech_db.strings.resolve(mac.name),
                inputs.len(),
                outputs.len()
            );
        }

        let cell = self.insert_cell(name, buffer_macro, tech_db, lib_db)?;
        let new_net = self.create_net(new_net_name)?;
        let buf_pin = |sym: SymbolId| {
            self.cell_pins(cell)
                .iter()
                .copied()
                .find(|&p| self.pin_names[p.0 as usize] == sym)
                .expect("insert_cell creates one pin per macro pin")
        };
        let buf_in = buf_pin(inputs[0]);
        let buf_out = buf_pin(outputs[0]);

        self.connect_pin(buf_out, new_net); // driver -> index 0
        self.connect_pin(buf_in, net); // sink -> segment end
        // Snapshot first: `net_pins` borrows &self, and moving a member
        // mutates the segment being iterated.
        let sinks: Vec<PinId> = self
            .net_pins(net)
            .iter()
            .copied()
            .filter(|&p| p != driver && p != buf_in)
            .collect();
        for p in sinks {
            self.connect_pin(p, new_net);
        }
        Ok((cell, new_net))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::placement::{legalize_placement, verify_legality};
    use crate::readers::def::{parse_def, write_def_full};
    use crate::readers::lef::load_tech_db;
    use crate::readers::liberty::load_liberty;
    use crate::test_support::{design_dir, require, sky130_hd};
    use std::collections::{BTreeMap, BTreeSet};

    /// Asserts invariants I1–I6 (documented on the mutation impl block),
    /// including both directions of the `pin_cells`/`pin_nets`
    /// back-pointers. Called after every mutation step in every test.
    fn assert_db_invariants(db: &DesignDb) {
        let nc = db.num_cells();
        let np = db.num_pins();
        let nn = db.num_nets();

        // I1 — SoA lengths and CSR shape.
        for (col, len) in [
            ("cell_names", db.cell_names.len()),
            ("cell_macros", db.cell_macros.len()),
            ("cell_lib_ids", db.cell_lib_ids.len()),
            ("cell_status", db.cell_status.len()),
            ("cell_x_dbu", db.cell_x_dbu.len()),
            ("cell_y_dbu", db.cell_y_dbu.len()),
            ("cell_orient", db.cell_orient.len()),
        ] {
            assert_eq!(len, nc, "I1 {col} length");
        }
        for (col, len) in [
            ("pin_names", db.pin_names.len()),
            ("pin_dirs", db.pin_dirs.len()),
            ("pin_cells", db.pin_cells.len()),
            ("pin_macro_pins", db.pin_macro_pins.len()),
            ("pin_lib_pins", db.pin_lib_pins.len()),
            ("pin_nets", db.pin_nets.len()),
            ("pin_offset_x_dbu", db.pin_offset_x_dbu.len()),
            ("pin_offset_y_dbu", db.pin_offset_y_dbu.len()),
            ("pin_io_layer", db.pin_io_layer.len()),
            ("pin_io_rects", db.pin_io_rects.len()),
            ("pin_is_driver", db.pin_is_driver.len()),
        ] {
            assert_eq!(len, np, "I1 {col} length");
        }
        for (what, offsets, n, ids) in [
            ("cell", &db.cell_pin_offsets, nc, db.cell_pin_ids.len()),
            ("net", &db.net_pin_offsets, nn, db.net_pin_ids.len()),
        ] {
            assert_eq!(offsets.len(), n + 1, "I1 {what}_pin_offsets length");
            assert_eq!(offsets[0], 0, "I1 {what}_pin_offsets[0]");
            assert_eq!(
                *offsets.last().unwrap() as usize, ids,
                "I1 {what}_pin_offsets last"
            );
            assert!(
                offsets.windows(2).all(|w| w[0] <= w[1]),
                "I1 {what}_pin_offsets nondecreasing"
            );
        }

        // I2 — cell_pin_ids partition, both directions.
        let mut in_cell = vec![false; np];
        for c in 0..nc {
            for &p in db.cell_pins(CellId(c as u32)) {
                let pi = p.0 as usize;
                assert!(pi < np, "I2 pin {} out of range", pi);
                assert!(!in_cell[pi], "I2 pin {pi} appears in two cell ranges");
                in_cell[pi] = true;
                assert_eq!(
                    db.pin_cells[pi],
                    Some(CellId(c as u32)),
                    "I2 pin_cells[{pi}] back-pointer"
                );
            }
        }
        for p in 0..np {
            assert_eq!(
                db.pin_cells[p].is_some(),
                in_cell[p],
                "I2 pin_cells[{p}] iff in a cell range"
            );
        }

        // I3 — net_pin_ids partition + pin_nets, both directions; I4 per net.
        let mut in_net = vec![false; np];
        for n in 0..nn {
            let seg = db.net_pins(NetId(n as u32));
            for &p in seg {
                let pi = p.0 as usize;
                assert!(pi < np, "I3 pin {} out of range", pi);
                assert!(!in_net[pi], "I3 pin {pi} appears in two net segments");
                in_net[pi] = true;
                assert_eq!(
                    db.pin_nets[pi],
                    Some(NetId(n as u32)),
                    "I3 pin_nets[{pi}] back-pointer"
                );
            }
            if seg.iter().any(|&p| db.pin_is_driver[p.0 as usize]) {
                assert!(
                    db.pin_is_driver[seg[0].0 as usize],
                    "I4 net {n}: a driver exists but seg[0] is not a driver"
                );
            }
        }
        for p in 0..np {
            assert_eq!(
                db.pin_nets[p].is_some(),
                in_net[p],
                "I3 pin_nets[{p}] iff in a net segment"
            );
        }

        // I5 — io_pins ⊆ {pin_cells == None}, in PinId order. `+ SPECIAL`
        // ports share pin_cells == None but are correctly absent.
        assert!(
            db.io_pins.windows(2).all(|w| w[0].0 < w[1].0),
            "I5 io_pins sorted"
        );
        for &p in &db.io_pins {
            assert_eq!(db.pin_cells[p.0 as usize], None, "I5 io pin is a port");
        }

        // I6 — net_def_spellings is empty or matches num_nets.
        assert!(
            db.net_def_spellings.is_empty() || db.net_def_spellings.len() == nn,
            "I6 net_def_spellings length {} vs num_nets {nn}",
            db.net_def_spellings.len()
        );

        // I7 — net_geoms is empty or matches num_nets.
        assert!(
            db.net_geoms.is_empty() || db.net_geoms.len() == nn,
            "I7 net_geoms length {} vs num_nets {nn}",
            db.net_geoms.len()
        );
    }

    /// Hand-built two-net design on the `interner.rs` construction recipe:
    /// net0 = {port `in` (driver), u0/A (sink)},
    /// net1 = {u0/Y (driver), port `out` (sink)}.
    fn two_net_db() -> DesignDb {
        let mut db = DesignDb::new();
        db.design_name = "two_net".to_string();
        db.dbu_per_micron = 1000;
        db.die_area = RectDbu::new(0, 0, 100_000, 100_000);

        let c0_name = db.strings.intern("u0");
        let p_a = db.strings.intern("A");
        let p_y = db.strings.intern("Y");
        let port_in = db.strings.intern("in");
        let port_out = db.strings.intern("out");
        let net0_name = db.strings.intern("net_in");
        let net1_name = db.strings.intern("net_out");

        fn push_pin(
            db: &mut DesignDb,
            name: SymbolId,
            dir: PinDir,
            cell: Option<CellId>,
            driver: bool,
            net: Option<NetId>,
        ) -> PinId {
            let pin = PinId(db.pin_names.len() as u32);
            db.pin_names.push(name);
            db.pin_dirs.push(dir);
            db.pin_cells.push(cell);
            db.pin_macro_pins.push(None);
            db.pin_lib_pins.push(None);
            db.pin_nets.push(net);
            db.pin_offset_x_dbu.push(0);
            db.pin_offset_y_dbu.push(0);
            db.pin_io_layer.push(None);
            db.pin_io_rects.push(Vec::new());
            db.pin_is_driver.push(driver);
            pin
        }

        let p0 = push_pin(&mut db, port_in, PinDir::Input, None, true, Some(NetId(0)));
        let p1 = push_pin(&mut db, port_out, PinDir::Output, None, false, Some(NetId(1)));
        let p2 = push_pin(&mut db, p_a, PinDir::Input, Some(CellId(0)), false, Some(NetId(0)));
        let p3 = push_pin(&mut db, p_y, PinDir::Output, Some(CellId(0)), true, Some(NetId(1)));
        db.io_pins = vec![p0, p1];

        db.cell_names.push(c0_name);
        db.cell_macros.push(MacroId(0));
        db.cell_lib_ids.push(None);
        db.cell_status.push(PlacementStatus::Placed);
        db.cell_x_dbu.push(0);
        db.cell_y_dbu.push(0);
        db.cell_orient.push(Orient::N);
        db.cell_pin_offsets = vec![0, 2];
        db.cell_pin_ids = vec![p2, p3];

        db.net_names.push(net0_name);
        db.net_names.push(net1_name);
        db.net_pin_offsets = vec![0, 2, 4];
        db.net_pin_ids = vec![p0, p2, p3, p1];
        db
    }

    #[test]
    fn test_connect_disconnect_unit() {
        let mut db = two_net_db();
        assert_db_invariants(&db);

        // create_net appends an empty segment; duplicate names are refused.
        let n2 = db.create_net("n_extra").expect("create_net");
        assert_eq!(n2, NetId(2));
        assert!(db.net_pins(n2).is_empty());
        assert!(db.create_net("net_in").is_err(), "duplicate net name");
        assert_db_invariants(&db);

        // A sink moves onto the new net at its segment end.
        db.connect_pin(PinId(2), n2);
        assert_eq!(db.net_pins(NetId(0)), &[PinId(0)]);
        assert_eq!(db.net_pins(n2), &[PinId(2)]);
        assert_eq!(db.pin_nets[2], Some(n2));
        assert_db_invariants(&db);

        // A driver lands at segment index 0 on insertion.
        db.connect_pin(PinId(3), n2);
        assert_eq!(db.net_pins(n2), &[PinId(3), PinId(2)]);
        assert_eq!(db.net_driver(n2), Some(PinId(3)));
        assert_eq!(db.net_pins(NetId(1)), &[PinId(1)]);
        assert_db_invariants(&db);

        // A second driver also lands at index 0; removing it leaves the
        // remaining driver at index 0.
        db.connect_pin(PinId(0), n2);
        assert_eq!(db.net_pins(n2), &[PinId(0), PinId(3), PinId(2)]);
        db.disconnect_pin(PinId(0));
        assert_eq!(db.net_pins(n2), &[PinId(3), PinId(2)]);
        assert_eq!(db.net_driver(n2), Some(PinId(3)));
        assert_eq!(db.pin_nets[0], None);
        assert_db_invariants(&db);

        // Moving a pin between nets removes it from the old segment.
        db.connect_pin(PinId(2), NetId(0));
        assert_eq!(db.net_pins(n2), &[PinId(3)]);
        assert_eq!(db.net_pins(NetId(0)), &[PinId(2)]);
        assert_db_invariants(&db);

        // No-ops: connect to the current net, disconnect an unconnected pin.
        db.connect_pin(PinId(2), NetId(0));
        assert_eq!(db.net_pins(NetId(0)), &[PinId(2)]);
        db.disconnect_pin(PinId(0));
        assert_eq!(db.pin_nets[0], None);
        assert_db_invariants(&db);
    }

    fn net_by_name(db: &DesignDb, name: &str) -> NetId {
        db.net_names
            .iter()
            .position(|&s| db.strings.resolve(s) == name)
            .map(|i| NetId(i as u32))
            .unwrap_or_else(|| panic!("net `{name}` not in design"))
    }

    fn clkbuf16(tech: &TechDb) -> MacroId {
        // The cell TritonCTS actually picked on gcd (docs/results.md 2026-11-22).
        let sym = tech
            .strings
            .get("sky130_fd_sc_hd__clkbuf_16")
            .expect("clkbuf_16 interned in tech strings");
        *tech
            .macro_by_name
            .get(&sym)
            .expect("clkbuf_16 in macro_by_name")
    }

    /// The two-buffer clock-tree edit shared by T1–T3: `clkbuf_0_clk`
    /// splits `clk`, `clkbuf_1_clk` splits the new `clknet_0_clk`.
    /// Returns each inserted cell with the net it was inserted on.
    fn insert_two_clk_buffers(
        db: &mut DesignDb,
        tech: &TechDb,
        lib: &LibDb,
    ) -> Vec<(CellId, NetId)> {
        let clk = net_by_name(db, "clk");
        let mac = clkbuf16(tech);
        let (b0, n0) = db
            .insert_buffer_on_net(clk, mac, "clkbuf_0_clk", "clknet_0_clk", tech, Some(lib))
            .expect("insert clkbuf_0_clk");
        let (b1, _) = db
            .insert_buffer_on_net(n0, mac, "clkbuf_1_clk", "clknet_1_clk", tech, Some(lib))
            .expect("insert clkbuf_1_clk");
        vec![(b0, clk), (b1, n0)]
    }

    struct Gcd {
        tech: TechDb,
        lib: LibDb,
        db: DesignDb,
    }

    fn gcd() -> Gcd {
        let lib_dir = sky130_hd();
        let tlef = require(lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(lib_dir.join("lef/sky130_fd_sc_hd.lef"));
        let lib_path = require(lib_dir.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let def_path = require(design_dir("gcd").join("gcd.placed.def"));

        let mut interner = StringInterner::new();
        let tech = load_tech_db(&tlef, &lef, &mut interner).expect("LEF").0;
        let lib = load_liberty(&lib_path).expect("Liberty");
        let db = parse_def(&def_path, &tech, Some(&lib)).expect("DEF");
        Gcd { tech, lib, db }
    }

    /// Net member identity for cross-DB comparison: `inst/pin` for
    /// instance pins, `PIN/port` for primary I/O.
    fn member_name(db: &DesignDb, p: PinId) -> String {
        match db.pin_cells[p.0 as usize] {
            Some(c) => format!(
                "{}/{}",
                db.strings.resolve(db.cell_names[c.0 as usize]),
                db.strings.resolve(db.pin_names[p.0 as usize])
            ),
            None => format!("PIN/{}", db.strings.resolve(db.pin_names[p.0 as usize])),
        }
    }

    #[test]
    fn test_insert_buffers_on_clk_net() {
        let Gcd { tech, lib, mut db } = gcd();
        assert_db_invariants(&db);

        let clk = net_by_name(&db, "clk");
        let clk_seg_before = db.net_pins(clk).to_vec();
        let driver = db.net_driver(clk).expect("clk has a driver");
        assert!(
            db.pin_cells[driver.0 as usize].is_none(),
            "clk's driver is the clk I/O port"
        );
        let io_before = db.io_pins.clone();
        let (cells0, nets0, pins0) = (db.num_cells(), db.num_nets(), db.num_pins());
        let mac = clkbuf16(&tech);
        let mac_pin_count = tech.macro_pins(&tech.macros[mac.0 as usize]).len();

        let (buf0, net0) = db
            .insert_buffer_on_net(clk, mac, "clkbuf_0_clk", "clknet_0_clk", &tech, Some(&lib))
            .expect("insert clkbuf_0_clk");
        assert_eq!(db.num_cells(), cells0 + 1);
        assert_eq!(db.num_nets(), nets0 + 1);
        assert_eq!(db.num_pins(), pins0 + mac_pin_count);
        assert!(db.insert_cell("clkbuf_0_clk", mac, &tech, Some(&lib)).is_err());
        assert!(db.create_net("clknet_0_clk").is_err());

        // clk keeps the port driver; the buffer input is the only other member.
        let clk_seg = db.net_pins(clk).to_vec();
        assert_eq!(clk_seg.len(), 2);
        assert_eq!(clk_seg[0], driver, "clk driver stays first");
        assert_eq!(db.net_driver(clk), Some(driver));
        assert_eq!(db.pin_cells[clk_seg[1].0 as usize], Some(buf0));
        assert_eq!(db.strings.resolve(db.pin_names[clk_seg[1].0 as usize]), "A");

        // clknet_0_clk: buf0/X drives the 35 former sinks in order.
        let new_seg = db.net_pins(net0).to_vec();
        assert_eq!(new_seg.len(), clk_seg_before.len());
        assert_eq!(db.pin_cells[new_seg[0].0 as usize], Some(buf0));
        assert_eq!(db.strings.resolve(db.pin_names[new_seg[0].0 as usize]), "X");
        assert!(db.pin_is_driver[new_seg[0].0 as usize]);
        assert_eq!(db.net_driver(net0), Some(new_seg[0]));
        assert_eq!(&new_seg[1..], &clk_seg_before[1..], "former sinks keep order");
        assert_eq!(db.io_pins, io_before, "I5: io_pins untouched");

        // Power pins stay unconnected; signal pins are on the two nets.
        for &p in db.cell_pins(buf0) {
            let mp = db.pin_macro_pins[p.0 as usize].expect("inserted pin has a MacroPin");
            if tech.macro_pins[mp.0 as usize].is_power {
                assert_eq!(db.pin_nets[p.0 as usize], None, "power pin unconnected");
            } else {
                assert!(db.pin_nets[p.0 as usize].is_some());
            }
        }
        // Liberty ids resolved through Some(&lib) — the CTS STA pass needs them.
        assert!(db.cell_lib_ids[buf0.0 as usize].is_some());
        for &p in db.cell_pins(buf0) {
            let mp = db.pin_macro_pins[p.0 as usize].unwrap();
            if !tech.macro_pins[mp.0 as usize].is_power {
                assert!(
                    db.pin_lib_pins[p.0 as usize].is_some(),
                    "signal pin must resolve a Liberty pin"
                );
            }
        }
        assert_db_invariants(&db);

        // Chain a second buffer: buf0/A — a pin this API created — moves
        // onto clknet_1_clk, which buf1/X now drives.
        let (buf1, net1) = db
            .insert_buffer_on_net(net0, mac, "clkbuf_1_clk", "clknet_1_clk", &tech, Some(&lib))
            .expect("insert clkbuf_1_clk");
        let seg0 = db.net_pins(net0).to_vec();
        assert_eq!(seg0.len(), 2);
        assert_eq!(seg0[0], new_seg[0], "buf0/X still drives clknet_0_clk");
        assert_eq!(db.pin_cells[seg0[1].0 as usize], Some(buf1));
        assert_eq!(db.strings.resolve(db.pin_names[seg0[1].0 as usize]), "A");
        let seg1 = db.net_pins(net1).to_vec();
        assert_eq!(db.pin_cells[seg1[0].0 as usize], Some(buf1));
        assert_eq!(db.strings.resolve(db.pin_names[seg1[0].0 as usize]), "X");
        assert_eq!(db.net_driver(net1), Some(seg1[0]));
        assert_eq!(&seg1[1..], &clk_seg_before[1..]);
        assert_eq!(db.io_pins, io_before);
        assert_db_invariants(&db);
    }

    /// Per-net HPWL computed inline from member-pin positions — the
    /// inputs guide parasitics are a pure function of.
    fn net_hpwl_dbu(db: &DesignDb, tech: &TechDb, net: NetId) -> i64 {
        let pins = db.net_pins(net);
        if pins.len() <= 1 {
            return 0;
        }
        let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
        let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
        for &p in pins {
            let (px, py) = db.pin_pos_dbu(p, tech);
            min_x = min_x.min(px);
            max_x = max_x.max(px);
            min_y = min_y.min(py);
            max_y = max_y.max(py);
        }
        (max_x - min_x) as i64 + (max_y - min_y) as i64
    }

    /// Emitted DEF sections indexed for line-level comparison: record
    /// lines keyed by their first token after `- `, plus the whole PINS
    /// block. Section header counts are not lines and are not indexed.
    struct EmittedIndex {
        pins_block: String,
        comps: BTreeMap<String, String>,
        nets: BTreeMap<String, String>,
    }

    fn emit_def_string(db: &DesignDb, tech: &TechDb, tag: &str) -> String {
        let tmp =
            std::env::temp_dir().join(format!("grus_e5_{tag}_{}.def", std::process::id()));
        write_def_full(db, tech, &tmp).expect("write_def_full");
        let text = std::fs::read_to_string(&tmp).expect("read emitted DEF");
        let _ = std::fs::remove_file(&tmp);
        text
    }

    fn index_emitted(text: &str) -> EmittedIndex {
        let mut idx = EmittedIndex {
            pins_block: String::new(),
            comps: BTreeMap::new(),
            nets: BTreeMap::new(),
        };
        let mut section = 0u8; // b'c' components, b'p' pins, b'n' nets
        for line in text.lines() {
            if line.starts_with("COMPONENTS ") {
                section = b'c';
                continue;
            }
            if line.starts_with("PINS ") {
                section = b'p';
            } else if line.starts_with("NETS ") {
                section = b'n';
                continue;
            } else if line.starts_with("END ") {
                if section == b'p' {
                    idx.pins_block.push_str(line);
                    idx.pins_block.push('\n');
                }
                section = 0;
                continue;
            }
            if section == b'p' {
                idx.pins_block.push_str(line);
                idx.pins_block.push('\n');
            }
            if let Some(rest) = line.strip_prefix("    - ") {
                let name = rest.split_whitespace().next().unwrap();
                match section {
                    b'c' => {
                        idx.comps.insert(name.to_string(), line.to_string());
                    }
                    b'n' => {
                        idx.nets.insert(name.to_string(), line.to_string());
                    }
                    _ => {}
                }
            }
        }
        idx
    }

    #[test]
    fn test_mutation_def_roundtrip() {
        let Gcd { tech, lib, mut db } = gcd();
        let anchored = insert_two_clk_buffers(&mut db, &tech, &lib);

        // Seed each inserted cell at its net driver's pin position —
        // Abacus needs a finite start point.
        for &(cell, anchor_net) in &anchored {
            let drv = db.net_driver(anchor_net).expect("anchor net driver");
            let (x, y) = db.pin_pos_dbu(drv, &tech);
            db.cell_x_dbu[cell.0 as usize] = x;
            db.cell_y_dbu[cell.0 as usize] = y;
        }
        let dbu = db.dbu_per_micron as f32;
        let gp_x: Vec<f32> = db.cell_x_dbu.iter().map(|&x| x as f32 / dbu).collect();
        let gp_y: Vec<f32> = db.cell_y_dbu.iter().map(|&y| y as f32 / dbu).collect();
        legalize_placement(&mut db, &tech, &gp_x, &gp_y).expect("legalize_placement");
        verify_legality(&db, &tech).expect("verify_legality");
        for &(cell, _) in &anchored {
            assert_eq!(
                db.cell_status[cell.0 as usize],
                PlacementStatus::Placed,
                "inserted cell must end Placed"
            );
        }
        assert_db_invariants(&db);

        let tmp =
            std::env::temp_dir().join(format!("grus_e5_roundtrip_{}.def", std::process::id()));
        write_def_full(&db, &tech, &tmp).expect("write_def_full");
        let db2 = parse_def(&tmp, &tech, Some(&lib)).expect("re-parse emitted DEF");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(db2.num_cells(), db.num_cells());
        // write_def_full emits only io_pins; the `+ SPECIAL` ports
        // (VGND/VPWR, pin_cells == None but not in io_pins) are dropped on
        // re-read — a pre-existing emission limitation, not part of this
        // API (recorded in docs/results.md).
        let special_ports = (0..db.num_pins())
            .filter(|&p| db.pin_cells[p].is_none() && !db.io_pins.contains(&PinId(p as u32)))
            .count();
        assert_eq!(special_ports, 2, "gcd has VGND/VPWR special ports");
        assert_eq!(db2.num_pins(), db.num_pins() - special_ports);
        assert_eq!(db2.num_nets(), db.num_nets());
        assert_db_invariants(&db2);

        let net_members = |d: &DesignDb| -> BTreeMap<String, BTreeSet<String>> {
            (0..d.num_nets())
                .map(|n| {
                    let net = NetId(n as u32);
                    let members = d
                        .net_pins(net)
                        .iter()
                        .map(|&p| member_name(d, p))
                        .collect();
                    (d.strings.resolve(d.net_names[n]).to_string(), members)
                })
                .collect()
        };
        assert_eq!(net_members(&db), net_members(&db2), "net membership");

        let cell_pins_map = |d: &DesignDb| -> BTreeMap<String, BTreeSet<String>> {
            (0..d.num_cells())
                .map(|c| {
                    let pins = d
                        .cell_pins(CellId(c as u32))
                        .iter()
                        .map(|&p| d.strings.resolve(d.pin_names[p.0 as usize]).to_string())
                        .collect();
                    (d.strings.resolve(d.cell_names[c]).to_string(), pins)
                })
                .collect()
        };
        assert_eq!(cell_pins_map(&db), cell_pins_map(&db2), "cell pin sets");

        for net_name in ["clk", "clknet_0_clk", "clknet_1_clk"] {
            let d1 = db
                .net_driver(net_by_name(&db, net_name))
                .map(|p| member_name(&db, p));
            let d2 = db2
                .net_driver(net_by_name(&db2, net_name))
                .map(|p| member_name(&db2, p));
            assert_eq!(d1, d2, "driver of {net_name}");
        }
        assert_eq!(
            db2.net_driver(net_by_name(&db2, "clk"))
                .map(|p| member_name(&db2, p)),
            Some("PIN/clk".to_string()),
            "re-read clk driver is the clk port"
        );
    }

    #[test]
    fn test_untouched_net_emission_identical() {
        let Gcd { tech, lib, mut db } = gcd();
        let before = emit_def_string(&db, &tech, "untouched_a");

        // Control net: a 3-pin signal net untouched by the clk edits.
        let ctrl = net_by_name(&db, "ctrl.state.out[1]");
        let ctrl_hpwl_before = net_hpwl_dbu(&db, &tech, ctrl);
        let ctrl_pos_before: Vec<(i32, i32)> = db
            .net_pins(ctrl)
            .iter()
            .map(|&p| db.pin_pos_dbu(p, &tech))
            .collect();

        insert_two_clk_buffers(&mut db, &tech, &lib);
        assert_db_invariants(&db);
        let after = emit_def_string(&db, &tech, "untouched_b");

        let a = index_emitted(&before);
        let b = index_emitted(&after);
        assert_eq!(a.pins_block, b.pins_block, "PINS section byte-identical");
        for (name, line) in &a.comps {
            assert_eq!(b.comps.get(name), Some(line), "component line: {name}");
        }
        for (name, line) in &a.nets {
            if name == "clk" {
                continue;
            }
            assert_eq!(b.nets.get(name), Some(line), "net line: {name}");
        }

        // Control net: same member pins at the same positions -> same HPWL;
        // guide parasitics are a pure function of member-pin shapes and
        // positions, so they are unchanged too (docs/results.md).
        let ctrl_pos_after: Vec<(i32, i32)> = db
            .net_pins(ctrl)
            .iter()
            .map(|&p| db.pin_pos_dbu(p, &tech))
            .collect();
        assert_eq!(ctrl_pos_before, ctrl_pos_after);
        assert_eq!(ctrl_hpwl_before, net_hpwl_dbu(&db, &tech, ctrl));
    }
}
