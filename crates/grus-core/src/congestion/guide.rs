//! Route-guide emission in the `global_route -guide_file` format.
//!
//! Emission semantics are specced in `docs/e4-guides.md`: committed H edges
//! become `met1` strips, V edges `met2` strips, and every pin gets an access
//! stub on its port layer plus a `met1` stack stub.

use crate::congestion::route::{GridEdge, RoutedDesign};
use crate::design_db::DesignDb;
use crate::interner::{LayerId, NetId};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use anyhow::{Context, bail};
use std::collections::{BTreeMap, BTreeSet};

/// One guide box: a DBU rect on a routing layer, as the
/// `xlo ylo xhi yhi layer` lines inside an OpenROAD `global_route
/// -guide_file` block carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuideBox {
    pub layer: LayerId,
    pub rect: RectDbu,
}

/// Guide boxes for one signal net, in deterministic emission order.
#[derive(Debug)]
pub struct NetGuide {
    pub net: NetId,
    pub boxes: Vec<GuideBox>,
}

/// `met1`/`met2` are the documented convention (lowest preferred-direction
/// signal layers); the dir-based fallback keeps the emitter working on techs
/// that name their band differently. Index 0 is the local-interconnect layer,
/// outside the signal band.
fn signal_layer(tech_db: &TechDb, name: &str, dir: LayerDir) -> Option<LayerId> {
    if let Some(l) = tech_db
        .strings
        .get(name)
        .and_then(|s| tech_db.layer_by_name.get(&s))
        .copied()
    {
        return Some(l);
    }
    tech_db
        .routing_layers
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.dir == dir)
        .map(|(i, _)| LayerId(i as u8))
}

/// Builds per-net guides from a routed design. Semantics: `docs/e4-guides.md`.
pub fn build_guides(
    design_db: &DesignDb,
    tech_db: &TechDb,
    routed: &RoutedDesign,
) -> Vec<NetGuide> {
    let grid = &routed.grid;
    let nx = grid.nx;
    let edge_cols = nx - 1;
    let die_xh = design_db.die_area.xh;
    let die_yh = design_db.die_area.yh;

    let met1 = signal_layer(tech_db, "met1", LayerDir::Horizontal);
    let met2 = signal_layer(tech_db, "met2", LayerDir::Vertical);

    let gcell_rect = |x: usize, y: usize| {
        let (xl, xh) = grid.x_bounds(x, die_xh);
        let (yl, yh) = grid.y_bounds(y, die_yh);
        RectDbu::new(xl, yl, xh, yh)
    };

    let mut guides = Vec::new();
    for net_idx in 0..design_db.num_nets() {
        let net = NetId(net_idx as u32);
        // OpenROAD emits no block for nets with < 2 pins — a lone port pin
        // has nothing to connect (measured: the 67 irq/pcpi_* port-only
        // nets of picorv32 carry no block in the reference).
        if design_db.net_pins(net).len() < 2 {
            continue;
        }

        // Collect the committed edges per direction: row -> edge x set (H),
        // column -> edge y set (V). BTree iteration is the sorted order.
        let mut h_rows: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut v_cols: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let seg_start = routed.net_seg_offsets[net_idx] as usize;
        let seg_end = routed.net_seg_offsets[net_idx + 1] as usize;
        for path in &routed.segment_paths[seg_start..seg_end] {
            for &edge in path {
                match edge {
                    GridEdge::H(idx) => {
                        h_rows.entry(idx / edge_cols).or_default().insert(idx % edge_cols);
                    }
                    GridEdge::V(idx) => {
                        v_cols.entry(idx % nx).or_default().insert(idx / nx);
                    }
                }
            }
        }

        // Box order is semantic: `updateVias` pairs only consecutive
        // same-point segments on adjacent layers, so every pin's
        // [access, met1] stub pair is emitted next to the strip box whose
        // wire segment touches the pin's cell — strip runs split at pin
        // GCells so a covered pin cell is always a strip start/end.
        let runs_of = |indices: &BTreeSet<usize>| -> Vec<(usize, usize)> {
            let mut runs = Vec::new();
            let mut run: Option<(usize, usize)> = None;
            for &i in indices {
                match run {
                    Some((a, b)) if i == b + 1 => run = Some((a, i)),
                    _ => {
                        if let Some(r) = run {
                            runs.push(r);
                        }
                        run = Some((i, i));
                    }
                }
            }
            if let Some(r) = run {
                runs.push(r);
            }
            runs
        };
        let h_runs: Vec<(usize, Vec<(usize, usize)>)> =
            h_rows.iter().map(|(&y, xs)| (y, runs_of(xs))).collect();
        let v_runs: Vec<(usize, Vec<(usize, usize)>)> =
            v_cols.iter().map(|(&x, ys)| (x, runs_of(ys))).collect();

        // Pin cells in `net_pins` order (deduped), each with the distinct
        // access layers of the pins it carries: the pin's GCell rect on
        // its access layer (LEF port layer for instance pins, the DEF
        // `+ LAYER` for I/O pins) plus a `met1` stack stub into the
        // routing band.
        let mut pin_cells: Vec<(usize, usize)> = Vec::new();
        let mut pin_accesses: Vec<BTreeSet<u8>> = Vec::new();
        for &pin in design_db.net_pins(net) {
            let p = pin.0 as usize;
            let (px, py) = design_db.pin_pos_dbu(pin, tech_db);
            let gp = grid.gcell_xy(px, py);
            let access = match design_db.pin_cells[p] {
                Some(_) => design_db.pin_macro_pins[p]
                    .and_then(|mp| tech_db.macro_pins[mp.0 as usize].layer)
                    .or(met1),
                None => design_db.pin_io_layer[p].or(met1),
            };
            let cell = (gp.x as usize, gp.y as usize);
            let set = match pin_cells.iter().position(|&c| c == cell) {
                Some(i) => &mut pin_accesses[i],
                None => {
                    pin_cells.push(cell);
                    pin_accesses.push(BTreeSet::new());
                    pin_accesses.last_mut().expect("just pushed")
                }
            };
            if let Some(l) = access {
                set.insert(l.0);
            }
        }

        // A pin cell binds to the highest strip covering it — met2 when
        // a V run covers it, else met1. Its stub chain climbs the access
        // layers strictly below the strip's layer, so every stub pairs
        // into the wire (OpenROAD convention: a met1-covered pin emits
        // only its access stub — the strip itself is the met1 rung).
        let covered = |runs: &[(usize, Vec<(usize, usize)>)], key: usize, coord: usize| {
            runs.iter()
                .find(|(k, _)| *k == key)
                .and_then(|(_, rs)| rs.iter().find(|&&(a, b)| a <= coord && coord <= b + 1))
                .copied()
        };
        let bound_to_v: Vec<bool> = pin_cells
            .iter()
            .map(|&(x, y)| met2.is_some() && covered(&v_runs, x, y).is_some())
            .collect();
        let bound_to_h: Vec<bool> = pin_cells
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| {
                !bound_to_v[i] && met1.is_some() && covered(&h_runs, y, x).is_some()
            })
            .collect();
        // Stub chain per pin cell: accesses below the covering strip's
        // layer plus the met1 rung when it is also below it; accesses at
        // or above the strip layer keep their own stub (dead island).
        let cover_layer = |i: usize| {
            if bound_to_v[i] {
                met2.map(|l| l.0)
            } else if bound_to_h[i] {
                met1.map(|l| l.0)
            } else {
                None
            }
            .unwrap_or(u8::MAX)
        };
        let met1_id = met1.map(|l| l.0);
        let stub_chain: Vec<BTreeSet<u8>> = (0..pin_cells.len())
            .map(|i| {
                let cov = cover_layer(i);
                let mut chain: BTreeSet<u8> = pin_accesses[i]
                    .iter()
                    .copied()
                    .filter(|&a| a < cov)
                    .collect();
                if let Some(m) = met1_id
                    && m < cov
                {
                    chain.insert(m);
                }
                chain
            })
            .collect();
        let stub_extra: Vec<BTreeSet<u8>> = (0..pin_cells.len())
            .map(|i| {
                let cov = cover_layer(i);
                pin_accesses[i]
                    .iter()
                    .copied()
                    .filter(|&a| a >= cov)
                    .collect()
            })
            .collect();

        let ctx = StubCtx {
            cells: &pin_cells,
            chain: &stub_chain,
            extra: &stub_extra,
            cell_rect: &gcell_rect,
        };
        let mut boxes: Vec<GuideBox> = Vec::new();
        // Strips emit met1 lines then met2 lines, each run split at pin
        // cells with the bound stubs interleaved; unbound pin cells
        // append their stub group last, in pin order.
        let strip_h = |y: usize, a: usize, b: usize| {
            let (xl, _) = grid.x_bounds(a, die_xh);
            let (_, xh) = grid.x_bounds(b + 1, die_xh);
            let (yl, yh) = grid.y_bounds(y, die_yh);
            RectDbu::new(xl, yl, xh, yh)
        };
        let strip_v = |x: usize, a: usize, b: usize| {
            let (yl, _) = grid.y_bounds(a, die_yh);
            let (_, yh) = grid.y_bounds(b + 1, die_yh);
            let (xl, xh) = grid.x_bounds(x, die_xh);
            RectDbu::new(xl, yl, xh, yh)
        };
        if let Some(layer) = met1 {
            emit_strips(
                &h_runs,
                false,
                &ctx,
                &bound_to_h,
                layer,
                &strip_h,
                &mut boxes,
            );
        }
        if let Some(layer) = met2 {
            emit_strips(
                &v_runs,
                true,
                &ctx,
                &bound_to_v,
                layer,
                &strip_v,
                &mut boxes,
            );
        }
        for i in 0..pin_cells.len() {
            if !bound_to_v[i] && !bound_to_h[i] {
                ctx.group_before(i, &mut boxes);
            }
        }
        guides.push(NetGuide { net, boxes });
    }
    guides
}

/// Pin-cell stub emission context: each cell's stub chain (layers below
/// the covering strip — these pair into the wire) and its extra stubs
/// (access layers at/above the strip — dead islands).
struct StubCtx<'a> {
    cells: &'a [(usize, usize)],
    chain: &'a [BTreeSet<u8>],
    extra: &'a [BTreeSet<u8>],
    cell_rect: &'a dyn Fn(usize, usize) -> RectDbu,
}

impl StubCtx<'_> {
    fn push(&self, i: usize, layer: u8, boxes: &mut Vec<GuideBox>) {
        let cell = self.cells[i];
        boxes.push(GuideBox {
            layer: LayerId(layer),
            rect: (self.cell_rect)(cell.0, cell.1),
        });
    }

    /// Stub group before a strip wire (start cell, interior split, or
    /// unbound): the extra stubs (access at/above the strip — unpairable,
    /// each keeps its own self-loop as in OpenROAD) go first so the
    /// chain's top stub lands adjacent to the wire and can pair.
    fn group_before(&self, i: usize, boxes: &mut Vec<GuideBox>) {
        for &l in &self.extra[i] {
            self.push(i, l, boxes);
        }
        for &l in &self.chain[i] {
            self.push(i, l, boxes);
        }
    }

    /// Stub group after a strip's last wire (end cell): the chain top
    /// first so it pairs with the wire — OpenROAD's own tail convention
    /// (`_000_`: `[met2-wire, met1, li1]`), where the stubs below it
    /// then keep their self-loops.
    fn group_after(&self, i: usize, boxes: &mut Vec<GuideBox>) {
        for &l in self.chain[i].iter().rev() {
            self.push(i, l, boxes);
        }
        for &l in &self.extra[i] {
            self.push(i, l, boxes);
        }
    }
}

/// Emits one direction's strips: every run (edges `a..=b`, covering
/// cells `a..=b+1` on its line) splits at each pin GCell bound to this
/// direction — an unbound or opposite-bound cell gains nothing from a
/// split — and interleaves the bound cells' stub group: extras then the
/// chain before the wire at a start/interior cell, chain descending
/// then extras after it at an end cell — so `updateVias` sees the
/// chain's top stub consecutive with a same-point `L+1` wire endpoint.
fn emit_strips(
    runs: &[(usize, Vec<(usize, usize)>)],
    vertical: bool,
    ctx: &StubCtx<'_>,
    bound: &[bool],
    layer: LayerId,
    strip_rect: &dyn Fn(usize, usize, usize) -> RectDbu,
    boxes: &mut Vec<GuideBox>,
) {
    // Bound pin cells on each line keyed by line index -> sorted (line
    // coord, pin_cells index): for H strips the key is the row y and the
    // coord is x; for V strips the column x and the coord y.
    let mut on_line: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, &(x, y)) in ctx.cells.iter().enumerate() {
        if !bound[i] {
            continue;
        }
        let (key, coord) = if vertical { (x, y) } else { (y, x) };
        on_line.entry(key).or_default().push((coord, i));
    }
    for v in on_line.values_mut() {
        v.sort_unstable();
    }
    for (key, line_runs) in runs {
        let cells = on_line.get(key).map(Vec::as_slice).unwrap_or(&[]);
        for &(a, b) in line_runs {
            let mut cur = a;
            for &(c, i) in cells {
                if c == a {
                    ctx.group_before(i, boxes);
                } else if c < b + 1 && c > a {
                    if cur < c {
                        boxes.push(GuideBox {
                            layer,
                            rect: strip_rect(*key, cur, c - 1),
                        });
                        cur = c;
                    }
                    ctx.group_before(i, boxes);
                }
            }
            if cur <= b {
                boxes.push(GuideBox {
                    layer,
                    rect: strip_rect(*key, cur, b),
                });
            }
            for &(c, i) in cells {
                if c == b + 1 {
                    ctx.group_after(i, boxes);
                }
            }
        }
    }
}

/// Net-name escaping for the guide file. OpenROAD's guide writer also
/// escapes the `[`/`]` bus delimiters the DEF-word escaper leaves literal
/// (measured on `gcd.guide`: `ctrl.state.out\[1\]`), so `read_guides`
/// sees the same spelling the DEF carried.
pub fn escape_guide_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\\' || c.is_ascii_whitespace() || matches!(c, ';' | '(' | ')' | '#' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The name a net block carries in the file: the raw DEF spelling when
/// the design came from DEF (OpenDB keeps net names verbatim, so
/// `read_guides` matches on exactly this token), otherwise the escaped
/// `net_names` entry.
pub fn guide_net_name(design_db: &DesignDb, net: NetId) -> String {
    if design_db.net_def_spellings.len() == design_db.num_nets() {
        design_db
            .strings
            .resolve(design_db.net_def_spellings[net.0 as usize])
            .to_string()
    } else {
        escape_guide_name(
            design_db
                .strings
                .resolve(design_db.net_names[net.0 as usize]),
        )
    }
}

/// Emits the P-GR text format: per net `name` / `(` / `xlo ylo xhi yhi
/// layer` / `)`. Deterministic: nets in `net_names` order, boxes in
/// `GuideBox` sort order ((layer, yl, xl, xh, yh)).
pub fn write_guides(
    out: &mut impl std::io::Write,
    design_db: &DesignDb,
    tech_db: &TechDb,
    guides: &[NetGuide],
) -> std::io::Result<()> {
    let layer_name = |l: LayerId| {
        tech_db
            .strings
            .resolve(tech_db.routing_layers[l.0 as usize].name)
    };
    for g in guides {
        let name = guide_net_name(design_db, g.net);
        writeln!(out, "{name}")?;
        writeln!(out, "(")?;
        // Keep `NetGuide.boxes` order — it is semantic, since
        // `updateVias` pairs only consecutive same-point segments — and
        // drop exact duplicates on first occurrence.
        let mut seen = BTreeSet::new();
        for b in &g.boxes {
            if !seen.insert((b.layer.0, b.rect.yl, b.rect.xl, b.rect.xh, b.rect.yh)) {
                continue;
            }
            writeln!(
                out,
                "{} {} {} {} {}",
                b.rect.xl,
                b.rect.yl,
                b.rect.xh,
                b.rect.yh,
                layer_name(b.layer)
            )?;
        }
        writeln!(out, ")")?;
    }
    Ok(())
}

/// Resolves parsed guide net names to `NetId`s. Names match on the same
/// token `write_guides` emits (`guide_net_name`: raw DEF spelling when
/// `net_def_spellings` is populated, else `escape_guide_name`). A guide
/// net with no match in the design is an error, never a skip; nets absent
/// from the guide simply contribute no entry. `parsed` order is preserved.
pub fn resolve_guide_nets(
    design_db: &DesignDb,
    parsed: &[(String, Vec<GuideBox>)],
) -> anyhow::Result<Vec<(NetId, Vec<GuideBox>)>> {
    let mut by_name: std::collections::HashMap<String, NetId> =
        std::collections::HashMap::with_capacity(design_db.num_nets());
    for n in 0..design_db.num_nets() {
        let net = NetId(n as u32);
        if by_name.insert(guide_net_name(design_db, net), net).is_some() {
            bail!("guide: two design nets emit the same guide name");
        }
    }
    let mut out = Vec::with_capacity(parsed.len());
    for (name, boxes) in parsed {
        match by_name.get(name.as_str()) {
            Some(&net) => out.push((net, boxes.clone())),
            None => bail!("guide net `{name}` not found in design"),
        }
    }
    Ok(out)
}

/// Parses the same format back (comparison tests + round-trip checks).
/// Returns `(net_name, boxes)` in file order; unknown layer names and
/// malformed lines are errors, never skips.
pub fn parse_guides(
    text: &str,
    tech_db: &TechDb,
) -> anyhow::Result<Vec<(String, Vec<GuideBox>)>> {
    let layer_id = |name: &str, lineno: usize| -> anyhow::Result<LayerId> {
        tech_db
            .strings
            .get(name)
            .and_then(|s| tech_db.layer_by_name.get(&s))
            .copied()
            .with_context(|| format!("guide line {lineno}: unknown layer `{name}`"))
    };

    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut boxes: Option<Vec<GuideBox>> = None;
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let lineno = ln + 1;
        if line.is_empty() {
            continue;
        }
        match (&name, &mut boxes) {
            (None, None) => {
                if line == "(" {
                    bail!("guide line {lineno}: box group without a net name");
                }
                // A DEF-escaped name may embed `\ `; the name is the whole line.
                name = Some(line.to_string());
            }
            (Some(_), None) => {
                if line != "(" {
                    bail!("guide line {lineno}: expected `(` after a net name, got `{line}`");
                }
                boxes = Some(Vec::new());
            }
            (Some(_), Some(bs)) => {
                if line == ")" {
                    out.push((name.take().unwrap(), std::mem::take(bs)));
                    boxes = None;
                    continue;
                }
                let fields: Vec<&str> = line.split_whitespace().collect();
                let [xlo, ylo, xhi, yhi, lname] = fields.as_slice() else {
                    bail!("guide line {lineno}: box line must be `xlo ylo xhi yhi layer`");
                };
                let rect = RectDbu::new(
                    xlo.parse()
                        .with_context(|| format!("guide line {lineno}: bad xlo `{xlo}`"))?,
                    ylo.parse()
                        .with_context(|| format!("guide line {lineno}: bad ylo `{ylo}`"))?,
                    xhi.parse()
                        .with_context(|| format!("guide line {lineno}: bad xhi `{xhi}`"))?,
                    yhi.parse()
                        .with_context(|| format!("guide line {lineno}: bad yhi `{yhi}`"))?,
                );
                bs.push(GuideBox {
                    layer: layer_id(lname, lineno)?,
                    rect,
                });
            }
            (None, Some(_)) => unreachable!("boxes imply a pending name"),
        }
    }
    if name.is_some() || boxes.is_some() {
        bail!("guide: truncated file (a net block is missing `)`)");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::congestion::gcell::{CongestionReport, GCellGrid, GPoint, SteinerSegment};
    use crate::design_db::{DesignDb, Orient, PlacementStatus};
    use crate::interner::{CellId, LibPinId, MacroId, MacroPinId, PinDir, PinId};
    use crate::tech_db::{MacroDef, MacroPin, RoutingLayer};

    fn mini_tech() -> TechDb {
        let mut t = TechDb::default();
        for (name, dir) in [
            ("li1", LayerDir::Horizontal),
            ("met1", LayerDir::Horizontal),
            ("met2", LayerDir::Vertical),
        ] {
            let sym = t.strings.intern(name);
            t.routing_layers.push(RoutingLayer {
                name: sym,
                dir,
                pitch_dbu: 100,
                width_dbu: 50,
                offset_dbu: 0,
                res_per_sq: 0.0,
                cap_per_sq_um: 0.0,
                edge_cap_per_um: 0.0,
            });
        }
        // One macro, two li1 pins; offsets land the design pins in the
        // GCells the fixture comment assigns them.
        let mac_sym = t.strings.intern("cell");
        t.macros.push(MacroDef {
            name: mac_sym,
            class: crate::tech_db::MacroClass::Core,
            width_dbu: 60,
            height_dbu: 40,
            pin_offset: 0,
            pin_count: 2,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        });
        for pname in ["A", "B"] {
            let sym = t.strings.intern(pname);
            t.macro_pins.push(MacroPin {
                name: sym,
                dir: PinDir::Input,
                is_power: false,
                offset_x_dbu: 10,
                offset_y_dbu: 10,
                offset_x_frac: false,
                offset_y_frac: false,
                layer: Some(LayerId(0)), // li1
                rects: Vec::new(),
            });
        }
        t.rebuild_maps();
        t
    }

    /// Net `n0`: instance pin A at GCell (0,0), instance pin B at (2,1),
    /// I/O pin C at (3,2) on `met2`. Net `n1`: pins A and D, both at (0,0)
    /// — a two-pin net in one GCell still emits its stub block.
    fn mini_design() -> DesignDb {
        let mut d = DesignDb {
            die_area: RectDbu::new(0, 0, 400, 300),
            ..Default::default()
        };
        let [n0, n1] = ["n0", "n1"].map(|s| d.strings.intern(s));
        d.net_names = vec![n0, n1];
        d.net_pin_offsets = vec![0, 3, 5];
        d.net_pin_ids = vec![PinId(0), PinId(1), PinId(2), PinId(0), PinId(3)];

        for cname in ["c0", "c1"] {
            let sym = d.strings.intern(cname);
            d.cell_names.push(sym);
            d.cell_macros.push(MacroId(0));
            d.cell_lib_ids.push(None);
            d.cell_status.push(PlacementStatus::Placed);
            d.cell_orient.push(Orient::N);
        }
        d.cell_x_dbu = vec![0, 200];
        d.cell_y_dbu = vec![0, 100];
        d.cell_pin_offsets = vec![0, 2, 3];
        d.cell_pin_ids = vec![PinId(0), PinId(3), PinId(1)];

        for (pname, net) in [("A", 0), ("B", 0), ("io", 0), ("D", 1)] {
            let sym = d.strings.intern(pname);
            d.pin_names.push(sym);
            d.pin_dirs.push(PinDir::Input);
            d.pin_lib_pins.push(None::<LibPinId>);
            d.pin_nets.push(Some(NetId(net)));
        }
        d.pin_cells = vec![Some(CellId(0)), Some(CellId(1)), None, Some(CellId(0))];
        d.pin_macro_pins = vec![
            Some(MacroPinId(0)),
            Some(MacroPinId(1)),
            None,
            Some(MacroPinId(0)),
        ];
        // Cell0 A/D at (10,10) -> gcell (0,0); cell1 B at (210,110) -> (2,1);
        // I/O C at (350,250) -> (3,2).
        d.pin_offset_x_dbu = vec![10, 10, 350, 10];
        d.pin_offset_y_dbu = vec![10, 10, 250, 10];
        d.pin_io_layer = vec![None, None, Some(LayerId(2)), None]; // met2
        d.pin_is_driver = vec![true, false, false, true];
        d.io_pins = vec![PinId(2)];
        d
    }

    fn mini_grid() -> GCellGrid {
        GCellGrid {
            origin_x_dbu: 0,
            origin_y_dbu: 0,
            gcell_w_dbu: 100,
            gcell_h_dbu: 100,
            nx: 4,
            ny: 3,
            h_cap: vec![0.0; 9],
            h_raw_cap: vec![0.0; 9],
            h_demand: vec![0.0; 9],
            v_cap: vec![0.0; 8],
            v_raw_cap: vec![0.0; 8],
            v_demand: vec![0.0; 8],
            layer_raw_edge_tracks: Vec::new(),
            layer_orig_edge_tracks: Vec::new(),
            layer_derated_edge_tracks: Vec::new(),
        }
    }

    fn routed(grid: GCellGrid, paths: Vec<Vec<GridEdge>>) -> RoutedDesign {
        let n = paths.len() as u32;
        RoutedDesign {
            grid,
            report: CongestionReport {
                nx: 4,
                ny: 3,
                total_overflow: 0.0,
                max_edge_overflow: 0.0,
                top_05pct_overflow: 0.0,
                overflow_gcells: 0,
                gcell_usage_ratio: Vec::new(),
                h_usage_ratio: Vec::new(),
                v_usage_ratio: Vec::new(),
            },
            segments: paths
                .iter()
                .map(|_| SteinerSegment {
                    p1: GPoint { x: 0, y: 0 },
                    p2: GPoint { x: 0, y: 0 },
                })
                .collect(),
            // net0 owns all paths; net1 has none.
            net_seg_offsets: vec![0, n, n],
            segment_paths: paths,
        }
    }

    fn has(g: &NetGuide, layer: u8, xl: i32, yl: i32, xh: i32, yh: i32) -> bool {
        g.boxes
            .iter()
            .any(|b| b.layer == LayerId(layer) && b.rect == RectDbu::new(xl, yl, xh, yh))
    }

    #[test]
    fn parse_fixture_and_errors() {
        let tech = mini_tech();
        let text = "net_a\n(\n0 100 300 200 met1\n50 60 70 80 li1\n)\nnet_b\n(\n0 0 100 100 met2\n)\n";
        let parsed = parse_guides(text, &tech).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "net_a");
        assert_eq!(parsed[0].1.len(), 2);
        assert_eq!(
            parsed[0].1[0],
            GuideBox {
                layer: LayerId(1),
                rect: RectDbu::new(0, 100, 300, 200)
            }
        );
        assert_eq!(
            parsed[1].1[0],
            GuideBox {
                layer: LayerId(2),
                rect: RectDbu::new(0, 0, 100, 100)
            }
        );

        assert!(parse_guides("n\n(\n0 0 1 1 bogus\n)\n", &tech).is_err());
        assert!(parse_guides("n\n(\n0 0 1 met1\n)\n", &tech).is_err());
        assert!(parse_guides("n\n(\n0 0 1 1 met1\n", &tech).is_err());
    }

    #[test]
    fn write_parse_roundtrip() {
        let tech = mini_tech();
        let design = mini_design();
        let guides = vec![NetGuide {
            net: NetId(0),
            boxes: vec![
                GuideBox {
                    layer: LayerId(1),
                    rect: RectDbu::new(0, 0, 300, 100),
                },
                GuideBox {
                    layer: LayerId(0),
                    rect: RectDbu::new(0, 0, 100, 100),
                },
            ],
        }];
        let mut buf = Vec::new();
        write_guides(&mut buf, &design, &tech, &guides).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Builder order is preserved verbatim — it carries the
        // updateVias pairing semantics.
        assert_eq!(text, "n0\n(\n0 0 300 100 met1\n0 0 100 100 li1\n)\n");
        let parsed = parse_guides(&text, &tech).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "n0");
        assert_eq!(
            parsed[0].1,
            vec![
                GuideBox {
                    layer: LayerId(1),
                    rect: RectDbu::new(0, 0, 300, 100)
                },
                GuideBox {
                    layer: LayerId(0),
                    rect: RectDbu::new(0, 0, 100, 100)
                },
            ]
        );
    }

    fn load_design(design: &str) -> (TechDb, DesignDb) {
        use crate::interner::StringInterner;
        use crate::readers::def::parse_def;
        use crate::readers::lef::load_tech_db;
        use crate::test_support::{design_dir, require, sky130_hd};

        let tlef = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def = require(design_dir(design).join(format!("{design}.placed.def")));
        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef, &lef, &mut interner)
            .expect("failed to load TechDb")
            .0;
        let design_db = parse_def(&def, &tech_db, None).expect("failed to parse placed DEF");
        (tech_db, design_db)
    }

    fn intersects(a: &RectDbu, b: &RectDbu) -> bool {
        a.xl < b.xh && b.xl < a.xh && a.yl < b.yh && b.yl < a.yh
    }

    /// Per-net overlap share + box-count deltas vs the committed OpenROAD
    /// `global_route -guide_file` reference. The metrics and gates are
    /// `kurena/tasks/e4-guides-emit/design/metrics.md`; results are recorded
    /// in `docs/results.md`.
    fn guide_metrics(design: &str) {
        use crate::test_support::{design_dir, require};
        use std::collections::HashMap;

        let (tech_db, design_db) = load_design(design);
        let routed = crate::congestion::route_design(&design_db, &tech_db, None, None);
        let guides = build_guides(&design_db, &tech_db, &routed);
        let ours: HashMap<String, &NetGuide> = guides
            .iter()
            .map(|g| (guide_net_name(&design_db, g.net), g))
            .collect();

        let ref_path = design_dir(design).join(format!("{design}.guide"));
        let ref_path = require(&ref_path);
        let ref_text = std::fs::read_to_string(&ref_path)
            .unwrap_or_else(|e| panic!("failed to read {ref_path:?}: {e}"));
        let reference = parse_guides(&ref_text, &tech_db)
            .unwrap_or_else(|e| panic!("failed to parse {ref_path:?}: {e}"));

        let mut ref_total = 0usize;
        let mut ours_total = 0usize;
        let mut covered_total = 0usize;
        let mut covered_layer_total = 0usize;
        let mut shares: Vec<(f64, String)> = Vec::new();
        let mut rel_deltas: Vec<(f64, i64, String)> = Vec::new(); // (|rel-1|, delta, name)
        let mut missing_nets = 0usize;
        let ref_names: std::collections::HashSet<&str> =
            reference.iter().map(|(n, _)| n.as_str()).collect();

        for (name, ref_boxes) in &reference {
            ref_total += ref_boxes.len();
            let Some(g) = ours.get(name.as_str()) else {
                missing_nets += 1;
                shares.push((0.0, name.clone()));
                rel_deltas.push((1.0, -(ref_boxes.len() as i64), name.clone()));
                continue;
            };
            ours_total += g.boxes.len();
            let mut covered = 0usize;
            let mut covered_layer = 0usize;
            for rb in ref_boxes {
                if g.boxes.iter().any(|b| intersects(&b.rect, &rb.rect)) {
                    covered += 1;
                }
                if g.boxes
                    .iter()
                    .any(|b| b.layer == rb.layer && intersects(&b.rect, &rb.rect))
                {
                    covered_layer += 1;
                }
            }
            covered_total += covered;
            covered_layer_total += covered_layer;
            shares.push((covered as f64 / ref_boxes.len() as f64, name.clone()));
            rel_deltas.push((
                (g.boxes.len() as f64 / ref_boxes.len() as f64 - 1.0).abs(),
                g.boxes.len() as i64 - ref_boxes.len() as i64,
                name.clone(),
            ));
        }
        let ours_extra: Vec<&String> = ours
            .keys()
            .filter(|n| !ref_names.contains(n.as_str()))
            .collect();
        for name in &ours_extra {
            let n = ours[name.as_str()].boxes.len();
            ours_total += n;
            rel_deltas.push((f64::INFINITY, n as i64, (*name).clone()));
        }

        let mut share_vals: Vec<f64> = shares.iter().map(|(s, _)| *s).collect();
        share_vals.sort_by(|a, b| a.total_cmp(b));
        let median_share = share_vals[share_vals.len() / 2];
        let weighted = covered_total as f64 / ref_total as f64;
        let below = shares.iter().filter(|(s, _)| *s < 0.70).count();
        shares.sort_by(|a, b| a.0.total_cmp(&b.0));
        rel_deltas.sort_by(|a, b| b.0.total_cmp(&a.0));

        let over = rel_deltas.iter().filter(|(_, d, _)| *d > 0).count();
        let under = rel_deltas.iter().filter(|(_, d, _)| *d < 0).count();
        eprintln!(
            "{design}: ref={} ours={} delta={:+.1}% | weighted share={:.3} (layer-aware {:.3}) \
             median share={:.3} min={:.3} nets<0.70={}/{} missing={} ours_only={} over={} under={}",
            ref_total,
            ours_total,
            100.0 * (ours_total as f64 - ref_total as f64) / ref_total as f64,
            weighted,
            covered_layer_total as f64 / ref_total as f64,
            median_share,
            share_vals[0],
            below,
            shares.len(),
            missing_nets,
            ours_extra.len(),
            over,
            under,
        );
        eprintln!("  worst-5 share: {:?}", &shares[..5.min(shares.len())]);
        eprintln!("  top-5 box delta: {:?}", &rel_deltas[..5.min(rel_deltas.len())]);

        // Provisional gates (done-when.md E4 rows 1-2).
        assert!(
            weighted >= 0.70,
            "{design}: box-weighted overlap share {weighted:.3} < 0.70"
        );
        let count_delta =
            (ours_total as f64 - ref_total as f64).abs() / ref_total as f64;
        assert!(
            count_delta <= 0.15,
            "{design}: box count delta {:.1}% > 15%",
            100.0 * count_delta
        );
    }

    #[test]
    fn test_gcd_guide_matches_openroad() {
        guide_metrics("gcd");
    }

    #[test]
    fn test_picorv32_guide_matches_openroad() {
        guide_metrics("picorv32");
    }

    /// Diagnostic census of the emitted box stream — per-layer box count
    /// plus a strip/stub split (square, single-GCell boxes are stubs).
    /// Print-only diagnostic for `--nocapture`; the strip/stub classes
    /// behind the residual self-loop count are documented in
    /// docs/e4-guides.md §10.
    fn stub_binding_census(design: &str) {
        use std::collections::BTreeMap;

        let (tech_db, design_db) = load_design(design);
        let routed = crate::congestion::route_design(&design_db, &tech_db, None, None);
        let guides = build_guides(&design_db, &tech_db, &routed);
        // Recount boxes by shape: gcell-sized stubs vs strips, per layer.
        let mut strips = 0usize;
        let mut stubs = 0usize;
        for g in &guides {
            for b in &g.boxes {
                // A stub box covers exactly one GCell — emit code produces
                // it via `gcell_rect`; strips are strictly larger.
                if b.rect.xh - b.rect.xl == b.rect.yh - b.rect.yl {
                    stubs += 1;
                } else {
                    strips += 1;
                }
            }
        }
        let mut per_layer: BTreeMap<u8, usize> = BTreeMap::new();
        for g in &guides {
            for b in &g.boxes {
                *per_layer.entry(b.layer.0).or_default() += 1;
            }
        }
        eprintln!(
            "{design}: nets={} boxes={} strips={} stubs={} per_layer={per_layer:?}",
            guides.len(),
            strips + stubs,
            strips,
            stubs
        );
    }

    #[test]
    fn stub_binding_census_gcd() {
        stub_binding_census("gcd");
    }

    #[test]
    fn stub_binding_census_picorv32() {
        stub_binding_census("picorv32");
    }

    #[test]
    fn test_guide_emit_deterministic() {
        let (tech_db, design_db) = load_design("gcd");
        let emit = || {
            let routed = crate::congestion::route_design(&design_db, &tech_db, None, None);
            let guides = build_guides(&design_db, &tech_db, &routed);
            let mut buf = Vec::new();
            write_guides(&mut buf, &design_db, &tech_db, &guides).unwrap();
            buf
        };
        assert_eq!(emit(), emit(), "guide emission is not deterministic");
    }

    #[test]
    fn strips_merge_dedup_and_cover_corners() {
        let tech = mini_tech();
        let design = mini_design();
        // Edge_cols = 3, nx = 4. Segment 0: H run x=0..=1 on row 1 -> one
        // box over gcells (0,1)..(2,1). V(2): column 2, y=0 -> (2,0)-(2,1).
        // Segment 1 re-traverses H(3) and adds V(6): column 2, y=1 ->
        // (2,1)-(2,2), merging into the same column run as V(2).
        let r = routed(
            mini_grid(),
            vec![
                vec![GridEdge::H(3), GridEdge::H(4), GridEdge::V(2)],
                vec![GridEdge::H(3), GridEdge::V(6)],
            ],
        );
        let guides = build_guides(&design, &tech, &r);
        let g = &guides[0];
        // H run x=0..=1 row 1: [0,100]-[300,200] met1. Pin B sits at its
        // end cell x=2 but binds to the met2 strip, so no stub interleaves.
        assert!(has(g, 1, 0, 100, 300, 200), "met1 strip: {g:?}");
        // V run y=0..=1 col 2 covers pin B's cell (2,1) interior: the
        // strip splits so B's stub pair sits between the two pieces.
        assert!(has(g, 2, 200, 0, 300, 200), "met2 prefix: {g:?}");
        assert!(has(g, 2, 200, 100, 300, 300), "met2 suffix: {g:?}");
        assert_eq!(g.boxes.len(), 9, "{g:?}");
        // B's bound stubs emit access-first, adjacent to a strip wire.
        let b_stub = g
            .boxes
            .iter()
            .position(|b| b.rect == RectDbu::new(200, 100, 300, 200) && b.layer == LayerId(0))
            .expect("B access stub");
        assert_eq!(g.boxes[b_stub + 1].layer, LayerId(1), "met1 after access");
        // Stubs: A unbound [li1, met1] at (0,0); C unbound [met1, met2] at (3,2).
        assert!(has(g, 0, 0, 0, 100, 100));
        assert!(has(g, 1, 0, 0, 100, 100));
        assert!(has(g, 1, 200, 100, 300, 200));
        assert!(has(g, 1, 300, 200, 400, 300));
        assert!(has(g, 2, 300, 200, 400, 300));
        // Net n1 (pin A only, no segments) emits just its stubs.
        assert_eq!(guides[1].boxes.len(), 2);
        assert!(has(&guides[1], 0, 0, 0, 100, 100));
        assert!(has(&guides[1], 1, 0, 0, 100, 100));
    }
}
