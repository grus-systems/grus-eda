//! Per-layer shape assembly for the LEF+DEF DRC subset checker.
//!
//! `assemble` collects every shape Magic sees on the routing and cut
//! layers: signal and special wires, via-instance rects (cut + metals),
//! placed-macro pin/obs rects, and top-level io-port rects. Emission
//! order is fixed — `net_geoms`, `pdn_nets`, via instances, cells,
//! io pins — so two assembles of the same DB produce equal `DrcDesign`s.

use crate::design_db::{DesignDb, PdnVia, PlacementStatus, ViaGeom};
use crate::drc::{DrcDesign, DrcLayer, LayerShapes, ShapeRect, ShapeSrc};
use crate::interner::{CellId, SymbolId};
use crate::tech_db::{RectDbu, TechDb, ViaRectLayer};
use anyhow::{Context, Result};

/// The rects a via instance occupies: LEF `VIA` rects translated to the
/// instance point, or DEF `VIAS + VIARULE` params expanded per OpenDB's
/// `ENCLOSURE (botX, botY, topX, topY)` semantics.
pub fn expand_via(via: &PdnVia, tech: &TechDb) -> Result<Vec<(DrcLayer, RectDbu)>> {
    let (x, y) = (via.x_dbu, via.y_dbu);
    match &via.geom {
        ViaGeom::Lef(i) => {
            let vd = tech
                .via_defs
                .get(*i as usize)
                .with_context(|| format!("via def index {i} out of range"))?;
            Ok(vd
                .rects
                .iter()
                .map(|r| {
                    let layer = match r.layer {
                        ViaRectLayer::Routing(l) => DrcLayer::Routing(l),
                        ViaRectLayer::Cut(c) => DrcLayer::Cut(c),
                    };
                    (
                        layer,
                        RectDbu::new(
                            x + r.rect.xl,
                            y + r.rect.yl,
                            x + r.rect.xh,
                            y + r.rect.yh,
                        ),
                    )
                })
                .collect())
        }
        ViaGeom::Generated(g) => {
            let cut_idx = tech.cut_layer_by_name.get(&via.cut).copied().with_context(|| {
                format!(
                    "via cut layer {} is not in the LEF cut layers",
                    tech.strings.resolve(via.cut)
                )
            })?;
            let aw = g.cols as i32 * g.cut_w_dbu + (g.cols as i32 - 1) * g.cut_spacing_x_dbu;
            let ah = g.rows as i32 * g.cut_h_dbu + (g.rows as i32 - 1) * g.cut_spacing_y_dbu;
            let mut out = Vec::with_capacity((g.rows * g.cols + 2) as usize);
            out.push((
                DrcLayer::Routing(via.lower),
                RectDbu::new(
                    x - aw / 2 - g.enc_bot_x_dbu,
                    y - ah / 2 - g.enc_bot_y_dbu,
                    x + aw / 2 + g.enc_bot_x_dbu,
                    y + ah / 2 + g.enc_bot_y_dbu,
                ),
            ));
            for i in 0..g.rows as i32 {
                for j in 0..g.cols as i32 {
                    let xl = x - aw / 2 + j * (g.cut_w_dbu + g.cut_spacing_x_dbu);
                    let yl = y - ah / 2 + i * (g.cut_h_dbu + g.cut_spacing_y_dbu);
                    out.push((
                        DrcLayer::Cut(cut_idx),
                        RectDbu::new(xl, yl, xl + g.cut_w_dbu, yl + g.cut_h_dbu),
                    ));
                }
            }
            out.push((
                DrcLayer::Routing(via.upper),
                RectDbu::new(
                    x - aw / 2 - g.enc_top_x_dbu,
                    y - ah / 2 - g.enc_top_y_dbu,
                    x + aw / 2 + g.enc_top_x_dbu,
                    y + ah / 2 + g.enc_top_y_dbu,
                ),
            ));
            Ok(out)
        }
    }
}

/// Assembles the per-layer shape store. See the module doc for the fixed
/// source order; `Unplaced` cells contribute nothing.
pub fn assemble(db: &DesignDb, tech: &TechDb) -> Result<DrcDesign> {
    let mut routing: Vec<LayerShapes> = (0..tech.routing_layers.len())
        .map(|_| LayerShapes::new())
        .collect();
    let mut cut: Vec<LayerShapes> = (0..tech.cut_layers.len())
        .map(|_| LayerShapes::new())
        .collect();

    let push = |routing: &mut Vec<LayerShapes>,
                    cut: &mut Vec<LayerShapes>,
                    layer: DrcLayer,
                    s: ShapeRect| {
        match layer {
            DrcLayer::Routing(l) => routing[l.0 as usize].push(s),
            DrcLayer::Cut(c) => cut[c as usize].push(s),
        }
    };

    // 1. Signal-net wires.
    for (n, geom) in db.net_geoms.iter().enumerate() {
        let net = db.net_names.get(n).copied();
        for &(lid, rect) in &geom.wire_rects {
            push(
                &mut routing,
                &mut cut,
                DrcLayer::Routing(lid),
                ShapeRect {
                    rect,
                    src: ShapeSrc::SignalWire,
                    net,
                },
            );
        }
    }
    // 2. Special-net wires.
    for pdn in &db.pdn_nets {
        for &(lid, rect) in &pdn.wire_rects {
            push(
                &mut routing,
                &mut cut,
                DrcLayer::Routing(lid),
                ShapeRect {
                    rect,
                    src: ShapeSrc::SpecialWire,
                    net: Some(pdn.name),
                },
            );
        }
    }
    // 3. Via instances, signal then special, expanded at their points.
    let vias = db
        .net_geoms
        .iter()
        .enumerate()
        .flat_map(|(n, g)| g.vias.iter().map(move |v| (db.net_names.get(n).copied(), v)))
        .chain(db.pdn_nets.iter().flat_map(|p| {
            p.vias.iter().map(move |v| (Some(p.name) as Option<SymbolId>, v))
        }));
    for (net, via) in vias {
        for (layer, rect) in expand_via(via, tech)? {
            push(&mut routing, &mut cut, layer, ShapeRect {
                rect,
                src: ShapeSrc::Via,
                net,
            });
        }
    }
    // 4. Placed cells: macro pin shapes, OBS, and pin OBS, transformed by
    //    the same orient+translate `pdn.rs`/`gcell` use.
    for c in 0..db.num_cells() {
        if !matches!(
            db.cell_status[c],
            PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
        ) {
            continue;
        }
        let cell = CellId(c as u32);
        let mac = &tech.macros[db.cell_macros[c].0 as usize];
        let (cx, cy, orient) = (db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_orient[c]);
        let place = |r: RectDbu| {
            let r = DesignDb::orient_rect(r, mac.width_dbu, mac.height_dbu, orient);
            RectDbu::new(cx + r.xl, cy + r.yl, cx + r.xh, cy + r.yh)
        };
        for mpin in tech.macro_pins(mac) {
            let net = db
                .cell_pins(cell)
                .iter()
                .find(|&&p| {
                    db.pin_macro_pins[p.0 as usize]
                        .map(|mp| tech.macro_pins[mp.0 as usize].name == mpin.name)
                        .unwrap_or(false)
                })
                .and_then(|&p| db.pin_nets[p.0 as usize])
                .map(|n| db.net_names[n.0 as usize]);
            for &(lid, r) in &mpin.rects {
                push(&mut routing, &mut cut, DrcLayer::Routing(lid), ShapeRect {
                    rect: place(r),
                    src: ShapeSrc::MacroPin,
                    net,
                });
            }
        }
        for &(lid, r) in mac.obs_rects.iter().chain(mac.pin_obs_rects.iter()) {
            push(&mut routing, &mut cut, DrcLayer::Routing(lid), ShapeRect {
                rect: place(r),
                src: ShapeSrc::MacroObs,
                net: None,
            });
        }
    }
    // 5. Top-level io-port rects (`PINS` `+ LAYER`, already absolute).
    for p in 0..db.num_pins() {
        if db.pin_cells[p].is_some() {
            continue;
        }
        for &(lid, r) in &db.pin_io_rects[p] {
            push(&mut routing, &mut cut, DrcLayer::Routing(lid), ShapeRect {
                rect: r,
                src: ShapeSrc::IoPort,
                net: db.pin_nets[p].map(|n| db.net_names[n.0 as usize]),
            });
        }
    }

    Ok(DrcDesign {
        routing: routing.into_iter().map(LayerShapes::finish).collect(),
        cut: cut.into_iter().map(LayerShapes::finish).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::GeneratedViaGeom;
    use crate::readers::def::parse_def_bytes;
    use crate::readers::lef::parse_lef_bytes;
    use crate::interner::{LayerId, StringInterner};

    /// met1(via12)met2 stack with one LEF `VIA`, plus a 1000×1000 `CELL`
    /// whose pin rect and OBS rect are deliberately asymmetric.
    const DRC_LEF: &str = r#"
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
  WIDTH 0.15 ;
  RESISTANCE 15.0 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
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
  PIN A
    DIRECTION INPUT ;
    USE SIGNAL ;
    PORT
      LAYER met1 ;
        RECT 0.0 0.4 0.2 0.5 ;
    END
  END A
  OBS
    LAYER met1 ;
      RECT 0.0 0.0 0.1 0.05 ;
  END
END CELL
END LIBRARY
"#;

    /// Three placements of `CELL` at the orientations the real designs do
    /// not exercise (`W`, `FE`) plus `N` as the control; one signal net
    /// with a wire + LEF via, one special wire, one io port.
    const DRC_DEF: &str = r#"VERSION 5.8 ;
DIVIDERCHAR "/" ;
BUSBITCHARS "[]" ;
DESIGN d ;
UNITS DISTANCE MICRONS 1000 ;
DIEAREA ( 0 0 ) ( 12000 12000 ) ;
ROW ROW_0 unithd 0 0 N DO 4 BY 1 STEP 460 2720 ;
COMPONENTS 3 ;
- uN CELL + PLACED ( 0 0 ) N ;
- uW CELL + PLACED ( 5000 0 ) W ;
- uFE CELL + PLACED ( 10000 0 ) FE ;
END COMPONENTS
PINS 1 ;
- io + NET n1 + DIRECTION INPUT + USE SIGNAL + LAYER met1 ( -70 -70 ) ( 70 70 ) + PLACED ( 3000 5000 ) N ;
END PINS
SPECIALNETS 1 ;
- VPWR ( * VPWR ) + USE POWER
  + ROUTED met1 200 + SHAPE STRIPE ( 0 0 ) ( 12000 0 ) ;
END SPECIALNETS
NETS 1 ;
- n1 ( PIN io ) ( uN A ) + USE SIGNAL
  + ROUTED met1 ( 0 0 ) ( 1000 0 )
  NEW met1 ( 500 0 ) stack_a
  ;
END NETS
END DESIGN
"#;

    fn drc_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech_db = TechDb::new();
        parse_lef_bytes(DRC_LEF.as_bytes(), &mut tech_db, &mut interner)
            .expect("DRC LEF must parse");
        tech_db.strings = interner.clone();
        tech_db.rebuild_maps();
        tech_db
    }

    fn drc_design() -> (DesignDb, TechDb) {
        let tech = drc_tech();
        let db = parse_def_bytes(DRC_DEF.as_bytes(), &tech, None).expect("DRC DEF must parse");
        (db, tech)
    }

    fn met1(tech: &TechDb) -> LayerId {
        tech.layer_by_name[&tech.strings.get("met1").unwrap()]
    }

    /// Generated-via arithmetic, checked against the committed
    /// `via2_3_1600_480_1_5_320_320` record: CUTSIZE 150, ROWCOL 1 5,
    /// CUTSPACING 170, ENCLOSURE 85 165 55 85 → cut array 1430×150,
    /// lower metal 1430+2·85 × 150+2·165 = 1600×480 (the via's name),
    /// upper 1540×320.
    #[test]
    fn test_expand_generated_via_matches_committted_record() {
        let mut tech = drc_tech();
        let name = tech.strings.intern("via2_3_1600_480_1_5_320_320");
        let cut = tech.strings.get("via12").unwrap();
        let via = PdnVia {
            name,
            lower: LayerId(0),
            upper: LayerId(1),
            cut,
            cut_count: 5,
            geom: ViaGeom::Generated(GeneratedViaGeom {
                cut_w_dbu: 150,
                cut_h_dbu: 150,
                rows: 1,
                cols: 5,
                cut_spacing_x_dbu: 170,
                cut_spacing_y_dbu: 170,
                enc_bot_x_dbu: 85,
                enc_bot_y_dbu: 165,
                enc_top_x_dbu: 55,
                enc_top_y_dbu: 85,
            }),
            x_dbu: 0,
            y_dbu: 0,
        };
        let out = expand_via(&via, &tech).expect("expands");
        let by_layer = |l: DrcLayer| -> Vec<RectDbu> {
            out.iter().filter(|(dl, _)| *dl == l).map(|(_, r)| *r).collect()
        };
        assert_eq!(by_layer(DrcLayer::Routing(LayerId(0))), vec![RectDbu::new(-800, -240, 800, 240)]);
        assert_eq!(by_layer(DrcLayer::Routing(LayerId(1))), vec![RectDbu::new(-770, -160, 770, 160)]);
        assert_eq!(
            by_layer(DrcLayer::Cut(0)),
            vec![
                RectDbu::new(-715, -75, -565, 75),
                RectDbu::new(-395, -75, -245, 75),
                RectDbu::new(-75, -75, 75, 75),
                RectDbu::new(245, -75, 395, 75),
                RectDbu::new(565, -75, 715, 75),
            ]
        );
    }

    /// LEF-via expansion places `ViaDef.rects` at the instance point.
    #[test]
    fn test_expand_lef_via_translates_rects() {
        let (db, tech) = drc_design();
        let via = &db.net_geoms[0].vias[0];
        let out = expand_via(via, &tech).expect("expands");
        assert_eq!(
            out,
            vec![
                (DrcLayer::Cut(0), RectDbu::new(425, -75, 575, 75)),
                (DrcLayer::Routing(LayerId(0)), RectDbu::new(340, -130, 660, 130)),
                (DrcLayer::Routing(LayerId(1)), RectDbu::new(370, -160, 630, 160)),
            ]
        );
    }

    /// `iter_overlapping` is a strict-overlap query: edge-touching does
    /// not count.
    #[test]
    fn test_iter_overlapping_strict() {
        let mut l = LayerShapes::new();
        for r in [
            RectDbu::new(0, 0, 100, 100),
            RectDbu::new(100, 0, 150, 100), // touches A on the x=100 edge only
            RectDbu::new(150, 50, 300, 80),
        ] {
            l.push(ShapeRect { rect: r, src: ShapeSrc::SignalWire, net: None });
        }
        let l = l.finish();
        let hits = |q: RectDbu| -> Vec<RectDbu> {
            l.iter_overlapping(&q).map(|s| s.rect).collect()
        };
        // xh == B.xl: touches only, no hit on B.
        assert_eq!(
            hits(RectDbu::new(40, 40, 100, 100)),
            vec![RectDbu::new(0, 0, 100, 100)]
        );
        // One DBU past the edge → strict overlap with B as well.
        assert_eq!(
            hits(RectDbu::new(40, 40, 101, 60)),
            vec![RectDbu::new(0, 0, 100, 100), RectDbu::new(100, 0, 150, 100)]
        );
    }

    /// Regression for the index monotone predicate: a wide rect sitting
    /// before narrower ones in `xl` order must still be found (the
    /// prefix-max-xh partition carries it), and a rect ending left of
    /// the probe is not a hit even though the scan starts before it.
    #[test]
    fn test_iter_overlapping_wide_and_left_rects() {
        let mut l = LayerShapes::new();
        for r in [
            RectDbu::new(5, 0, 100, 100), // wide — reaches past the probe
            RectDbu::new(6, 0, 7, 100),   // fully left of the probe
            RectDbu::new(10, 0, 20, 100), // fully left of the probe
        ] {
            l.push(ShapeRect {
                rect: r,
                src: ShapeSrc::SignalWire,
                net: None,
            });
        }
        let l = l.finish();
        let hits: Vec<RectDbu> = l
            .iter_overlapping(&RectDbu::new(50, 20, 60, 40))
            .map(|s| s.rect)
            .collect();
        assert_eq!(hits, vec![RectDbu::new(5, 0, 100, 100)]);
    }

    /// Every shape source lands on the right layer with the right tag:
    /// signal wire, special wire, via (cut + both metals), macro pin +
    /// OBS under `N`/`W`/`FE`, io port.
    #[test]
    fn test_assemble_sources_and_orientations() {
        let (db, tech) = drc_design();
        let d = assemble(&db, &tech).expect("assemble");
        let m1 = met1(&tech);
        let m2 = tech.layer_by_name[&tech.strings.get("met2").unwrap()];
        let shapes = |l: DrcLayer, src: ShapeSrc| -> Vec<RectDbu> {
            let ls = match l {
                DrcLayer::Routing(l) => &d.routing[l.0 as usize],
                DrcLayer::Cut(c) => &d.cut[c as usize],
            };
            ls.rects
                .iter()
                .filter(|s| s.src == src)
                .map(|s| s.rect)
                .collect()
        };

        // Signal wire: (0,0)-(1000,0) on met1, half-width 70, square caps.
        assert_eq!(
            shapes(DrcLayer::Routing(m1), ShapeSrc::SignalWire),
            vec![RectDbu::new(-70, -70, 1070, 70)]
        );
        // Special wire: met1 200-wide rail.
        assert_eq!(
            shapes(DrcLayer::Routing(m1), ShapeSrc::SpecialWire),
            vec![RectDbu::new(0, -100, 12000, 100)]
        );
        // Via at (500,0): cut on cut layer, metals on met1/met2.
        assert_eq!(shapes(DrcLayer::Cut(0), ShapeSrc::Via), vec![RectDbu::new(425, -75, 575, 75)]);
        assert_eq!(shapes(DrcLayer::Routing(m1), ShapeSrc::Via), vec![RectDbu::new(340, -130, 660, 130)]);
        assert_eq!(shapes(DrcLayer::Routing(m2), ShapeSrc::Via), vec![RectDbu::new(370, -160, 630, 160)]);

        // OBS (0,0)-(100,50) of a 1000×1000 macro under N/W/FE, plus the
        // pin PORT rects that `pin_obs_rects` carries (emitted per cell:
        // obs_rects first, then pin_obs_rects).
        assert_eq!(
            shapes(DrcLayer::Routing(m1), ShapeSrc::MacroObs),
            vec![
                RectDbu::new(0, 0, 100, 50),           // uN obs at (0,0) N
                RectDbu::new(0, 400, 200, 500),        // uN pin obs
                RectDbu::new(5950, 0, 6000, 100),      // uW obs: (h-50, 0, h, 100)
                RectDbu::new(5500, 0, 5600, 200),      // uW pin obs
                RectDbu::new(10950, 900, 11000, 1000), // uFE obs: (h-50, w-100, h, w)
                RectDbu::new(10500, 800, 10600, 1000), // uFE pin obs
            ]
        );
        // Pin A rect (0,400)-(200,500) under the same three orientations.
        assert_eq!(
            shapes(DrcLayer::Routing(m1), ShapeSrc::MacroPin),
            vec![
                RectDbu::new(0, 400, 200, 500),       // uN
                RectDbu::new(5500, 0, 5600, 200),     // uW: (h-500, 0, h-400, 200)
                RectDbu::new(10500, 800, 10600, 1000),// uFE: (h-500, w-200, h-400, w)
            ]
        );
        // Io port rect, already absolute at (3000,5000) ± 70.
        assert_eq!(
            shapes(DrcLayer::Routing(m1), ShapeSrc::IoPort),
            vec![RectDbu::new(2930, 4930, 3070, 5070)]
        );
    }

    /// Two assembles of the same DB produce equal `DrcDesign`s — the
    /// deterministic-emission contract.
    #[test]
    fn test_assemble_is_deterministic() {
        let (db, tech) = drc_design();
        assert_eq!(
            assemble(&db, &tech).unwrap(),
            assemble(&db, &tech).unwrap()
        );
    }
}
