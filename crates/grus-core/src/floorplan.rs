use crate::design_db::{DesignDb, Orient, PlacementRow, PlacementStatus};
use crate::interner::PinDir;
use crate::tech_db::{RectDbu, TechDb};
use anyhow::{Context, Result, bail};

/// Floorplan generation parameters. Exactly one sizing mode applies:
/// `die_area` when set, otherwise `utilization` derives the die from the
/// total standard-cell area plus `margin_dbu` on every side.
#[derive(Clone, Debug)]
pub struct FloorplanSpec {
    pub die_area: Option<RectDbu>,
    pub utilization: f64,
    pub margin_dbu: i32,
    pub site_name: String,
}

impl Default for FloorplanSpec {
    fn default() -> Self {
        Self {
            die_area: None,
            utilization: 0.60,
            margin_dbu: 10_000,
            site_name: String::new(),
        }
    }
}

/// Compares names the way a human reads them: digit runs compare
/// numerically, so `req_msg[2]` sorts before `req_msg[10]`.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ia, mut ib) = (
        a.as_bytes().iter().peekable(),
        b.as_bytes().iter().peekable(),
    );
    loop {
        match (ia.peek().copied(), ib.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(&x), Some(&y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (mut na, mut nb) = (0u64, 0u64);
                while let Some(&d) = ia.peek() {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    na = na.saturating_mul(10).saturating_add((d - b'0') as u64);
                    ia.next();
                }
                while let Some(&d) = ib.peek() {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    nb = nb.saturating_mul(10).saturating_add((d - b'0') as u64);
                    ib.next();
                }
                match na.cmp(&nb) {
                    std::cmp::Ordering::Equal => {}
                    ord => return ord,
                }
            }
            (Some(&x), Some(&y)) => {
                ia.next();
                ib.next();
                match x.cmp(&y) {
                    std::cmp::Ordering::Equal => {}
                    ord => return ord,
                }
            }
        }
    }
}

/// Converts an i64 coordinate to the i32 DBU fields `DesignDb` uses,
/// bailing with context when a floorplan geometry value leaves the
/// representable range instead of wrapping silently.
fn i32_coord(v: i64, what: &str) -> Result<i32> {
    i32::try_from(v)
        .with_context(|| format!("floorplan: {what} coordinate {v} DBU is out of i32 range"))
}

/// Fills `die_area`, `rows`, and I/O pin positions of a netlist-derived
/// `DesignDb` so it can be emitted as a floorplan DEF. Cells stay
/// `Unplaced`; rows alternate N/FS like OpenROAD floorplans.
pub fn generate_floorplan(db: &mut DesignDb, tech_db: &TechDb, spec: &FloorplanSpec) -> Result<()> {
    if !(spec.utilization > 0.0 && spec.utilization <= 1.0) {
        bail!(
            "floorplan utilization must be in (0.0, 1.0], got {}",
            spec.utilization
        );
    }
    if spec.margin_dbu < 0 {
        bail!("floorplan margin must be >= 0 DBU, got {}", spec.margin_dbu);
    }
    // Row geometry always comes from the single site recorded in TechDb, so
    // accepting an arbitrary name would emit a ROW for a site the tech LEF
    // never defined. An empty spec falls back to the loaded site.
    let site_name = if spec.site_name.is_empty() {
        &tech_db.site_name
    } else {
        &spec.site_name
    };
    if !site_name.eq_ignore_ascii_case(&tech_db.site_name) {
        bail!(
            "floorplan: site '{}' is not the selected site in the tech LEF (selected: '{}')",
            site_name,
            tech_db.site_name
        );
    }
    let site_w = tech_db.site_width_dbu;
    let site_h = tech_db.site_height_dbu;
    if site_w <= 0 || site_h <= 0 {
        bail!(
            "tech LEF has no usable SITE size ({}x{} DBU)",
            site_w,
            site_h
        );
    }

    // Area of every cell the placer can move. All netlist-derived cells are
    // Unplaced; fixed-class macros would need their own handling anyway.
    let mut total_area_dbu2 = 0i64;
    for c in 0..db.num_cells() {
        let mac = &tech_db.macros[db.cell_macros[c].0 as usize];
        total_area_dbu2 = total_area_dbu2
            .checked_add(mac.width_dbu as i64 * mac.height_dbu as i64)
            .context("floorplan: summed cell area overflowed i64")?;
    }
    if total_area_dbu2 == 0 {
        bail!(
            "floorplan: design {} has no standard cells to size a die for",
            db.design_name
        );
    }

    let die = match spec.die_area {
        Some(d) => {
            if d.width() <= 0 || d.height() <= 0 {
                bail!("floorplan: --die-area must have positive width and height");
            }
            d
        }
        None => {
            let side = ((total_area_dbu2 as f64 / spec.utilization).sqrt()
                + 2.0 * spec.margin_dbu as f64)
                .round();
            if !side.is_finite() || side < 0.0 || side > i32::MAX as f64 {
                bail!("floorplan: computed die side {} DBU is out of range", side);
            }
            let side = side as i32;
            RectDbu::new(0, 0, side, side)
        }
    };

    // Geometry stays in i64 until it is stored: die coordinates are i32 but
    // margin arithmetic can leave that range, and the core check below is
    // what guarantees the surviving values fit back into i32.
    let core_xl = die.xl as i64 + spec.margin_dbu as i64;
    let core_yl = die.yl as i64 + spec.margin_dbu as i64;
    let core_xh = die.xh as i64 - spec.margin_dbu as i64;
    let core_yh = die.yh as i64 - spec.margin_dbu as i64;
    if core_xh - core_xl < site_w as i64 || core_yh - core_yl < site_h as i64 {
        bail!(
            "floorplan: core area {}x{} DBU cannot hold one {} site ({}x{} DBU); enlarge --die-area or shrink --margin",
            core_xh - core_xl,
            core_yh - core_yl,
            site_name,
            site_w,
            site_h
        );
    }

    db.die_area = die;
    db.rows.clear();
    let site_sym = db.strings.intern(site_name);
    let core_xl32 = i32_coord(core_xl, "core left")?;
    let num_x = i32_coord((core_xh - core_xl) / site_w as i64, "row site count")?;
    let mut y = core_yl;
    let mut row_idx = 0usize;
    while y + site_h as i64 <= core_yh {
        let orient = if row_idx.is_multiple_of(2) {
            Orient::N
        } else {
            Orient::FS
        };
        db.rows.push(PlacementRow {
            row_name: db.strings.intern(&format!("ROW_{}", row_idx)),
            site_name: site_sym,
            orig_x_dbu: core_xl32,
            orig_y_dbu: i32_coord(y, "row origin")?,
            orient,
            num_x,
            step_x_dbu: site_w,
        });
        y += site_h as i64;
        row_idx += 1;
    }
    if db.rows.is_empty() {
        bail!("floorplan: no rows fit in the core area");
    }

    // Edge pin assignment: inputs on the left edge, outputs on the right,
    // inouts appended to the right. Names sort naturally so buses read in
    // order. Positions are absolute DBU (I/O pin offsets are absolute).
    let mut inputs: Vec<_> = db
        .io_pins
        .iter()
        .copied()
        .filter(|&p| db.pin_dirs[p.0 as usize] == PinDir::Input)
        .collect();
    let mut outputs: Vec<_> = db
        .io_pins
        .iter()
        .copied()
        .filter(|&p| matches!(db.pin_dirs[p.0 as usize], PinDir::Output | PinDir::Inout))
        .collect();
    let name_of =
        |p: crate::interner::PinId| db.strings.resolve(db.pin_names[p.0 as usize]).to_string();
    inputs.sort_by(|&a, &b| natural_cmp(&name_of(a), &name_of(b)));
    outputs.sort_by(|&a, &b| natural_cmp(&name_of(a), &name_of(b)));

    // Keep one site of headroom at both ends when the core is tall enough;
    // a core shorter than two sites collapses to its midline so every pin
    // still lands inside the die.
    let (span_lo, span_hi) = {
        let lo = core_yl + site_h as i64;
        let hi = core_yh - site_h as i64;
        if hi >= lo {
            (lo, hi)
        } else {
            let mid = (core_yl + core_yh) / 2;
            (mid, mid)
        }
    };
    let place_edge = |pins: &[crate::interner::PinId], x: i64, db: &mut DesignDb| -> Result<()> {
        let n = pins.len() as i128;
        for (i, &p) in pins.iter().enumerate() {
            let y = if n <= 1 {
                span_lo + (span_hi - span_lo) / 2
            } else {
                // i128 keeps the product exact for any pin count; the
                // result always lies inside [span_lo, span_hi].
                span_lo + ((span_hi - span_lo) as i128 * (i as i128 + 1) / (n + 1)) as i64
            };
            let pi = p.0 as usize;
            db.pin_offset_x_dbu[pi] = i32_coord(x, "pin x")?;
            db.pin_offset_y_dbu[pi] = i32_coord(y, "pin y")?;
        }
        Ok(())
    };
    place_edge(&inputs, die.xl as i64 + site_w as i64, db)?;
    place_edge(&outputs, die.xh as i64 - site_w as i64, db)?;

    // Cells must look unplaced to the placer; netlist readers already set this,
    // but a DEF-fed db could carry placements — floorplan regenerates them.
    for s in &mut db.cell_status {
        if *s == PlacementStatus::Placed {
            *s = PlacementStatus::Unplaced;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::def::{parse_def, write_def_full};
    use crate::readers::lef::load_tech_db;
    use crate::readers::parse_yosys_json;
    use crate::test_support::{design_dir, sky130_hd};
    use std::collections::BTreeMap;

    fn gcd_tech() -> TechDb {
        let lib_dir = sky130_hd();
        let mut interner = StringInterner::new();
        load_tech_db(
            &lib_dir.join("techlef/sky130_fd_sc_hd__nom.tlef"),
            &lib_dir.join("lef/sky130_fd_sc_hd.lef"),
            &mut interner,
        )
        .expect("LEF")
        .0
    }

    fn gcd_netlist_db(tech: &TechDb) -> DesignDb {
        parse_yosys_json(&design_dir("gcd").join("gcd.synth.json"), tech, None).expect("Yosys JSON")
    }

    /// net name -> sorted "PIN port" / "cell/pin" connection strings, so two
    /// DesignDbs can be compared regardless of PinId ordering.
    fn connectivity(db: &DesignDb) -> BTreeMap<String, Vec<String>> {
        let mut map = BTreeMap::new();
        for n in 0..db.num_nets() {
            let start = db.net_pin_offsets[n] as usize;
            let end = db.net_pin_offsets[n + 1] as usize;
            let mut conns: Vec<String> = db.net_pin_ids[start..end]
                .iter()
                .map(|&pin| {
                    let p = pin.0 as usize;
                    match db.pin_cells[p] {
                        Some(cell) => format!(
                            "{}/{}",
                            db.strings.resolve(db.cell_names[cell.0 as usize]),
                            db.strings.resolve(db.pin_names[p])
                        ),
                        None => format!("PIN {}", db.strings.resolve(db.pin_names[p])),
                    }
                })
                .collect();
            conns.sort();
            map.insert(db.strings.resolve(db.net_names[n]).to_string(), conns);
        }
        map
    }

    #[test]
    fn test_floorplan_gcd_roundtrip() {
        let tech = gcd_tech();
        let mut db = gcd_netlist_db(&tech);
        let (cells, nets, ios) = (db.num_cells(), db.num_nets(), db.io_pins.len());
        generate_floorplan(&mut db, &tech, &FloorplanSpec::default()).expect("floorplan");

        assert!(db.die_area.width() > 0 && db.die_area.height() > 0);
        assert!(!db.rows.is_empty());
        for (i, r) in db.rows.iter().enumerate() {
            assert_eq!(r.orig_x_dbu, db.die_area.xl + 10_000);
            assert_eq!(r.step_x_dbu, tech.site_width_dbu);
            assert_eq!(r.orient, if i % 2 == 0 { Orient::N } else { Orient::FS });
        }
        for &pin in &db.io_pins {
            let p = pin.0 as usize;
            let (x, y) = (db.pin_offset_x_dbu[p], db.pin_offset_y_dbu[p]);
            assert!(
                x >= db.die_area.xl && x <= db.die_area.xh,
                "pin {x} outside die"
            );
            assert!(
                y >= db.die_area.yl && y <= db.die_area.yh,
                "pin {y} outside die"
            );
        }

        let out = std::env::temp_dir().join("grus_floorplan_gcd.def");
        write_def_full(&db, &tech, &out).expect("write_def_full");
        let db2 = parse_def(&out, &tech, None).expect("re-parse floorplan DEF");
        let _ = std::fs::remove_file(&out);

        assert_eq!(db2.num_cells(), cells);
        assert_eq!(db2.num_nets(), nets);
        assert_eq!(db2.io_pins.len(), ios);
        assert_eq!(db2.num_rows(), db.num_rows());
        assert_eq!(
            connectivity(&db),
            connectivity(&db2),
            "NETS connectivity changed"
        );
        // Every cell must still look movable to the placer.
        assert!(
            db2.cell_status
                .iter()
                .all(|&s| s == PlacementStatus::Unplaced)
        );
    }

    #[test]
    fn test_floorplan_explicit_die_area_and_edges() {
        let tech = gcd_tech();
        let mut db = gcd_netlist_db(&tech);
        let die = RectDbu::new(0, 0, 120_000, 80_000);
        let spec = FloorplanSpec {
            die_area: Some(die),
            ..Default::default()
        };
        generate_floorplan(&mut db, &tech, &spec).expect("floorplan");
        assert_eq!(db.die_area, die);

        // Inputs sit on the left edge, outputs on the right, inside the core.
        let left_x = die.xl + tech.site_width_dbu;
        let right_x = die.xh - tech.site_width_dbu;
        let mut saw_input = false;
        let mut saw_output = false;
        for &pin in &db.io_pins {
            let p = pin.0 as usize;
            match db.pin_dirs[p] {
                PinDir::Input => {
                    saw_input = true;
                    assert_eq!(db.pin_offset_x_dbu[p], left_x, "input pin not on left edge");
                }
                PinDir::Output => {
                    saw_output = true;
                    assert_eq!(
                        db.pin_offset_x_dbu[p], right_x,
                        "output pin not on right edge"
                    );
                }
                _ => {}
            }
        }
        assert!(saw_input && saw_output);
    }

    #[test]
    fn test_floorplan_rejects_bad_spec() {
        let tech = gcd_tech();
        for spec in [
            FloorplanSpec {
                utilization: 0.0,
                ..Default::default()
            },
            FloorplanSpec {
                utilization: 1.5,
                ..Default::default()
            },
            FloorplanSpec {
                margin_dbu: -1,
                ..Default::default()
            },
            FloorplanSpec {
                die_area: Some(RectDbu::new(0, 0, 1_000, 1_000)),
                ..Default::default()
            },
            FloorplanSpec {
                site_name: "nonexistent_site".to_string(),
                ..Default::default()
            },
        ] {
            let mut db = gcd_netlist_db(&tech);
            assert!(
                generate_floorplan(&mut db, &tech, &spec).is_err(),
                "spec {spec:?} accepted"
            );
        }
    }
}
