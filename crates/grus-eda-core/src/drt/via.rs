//! Via candidate model: for each adjacent routing-layer pair
//! `(l, l+1)`, the LEF `VIA` defs and DEF `VIAS` records whose
//! (lower, cut, upper) triple matches — with each def's painted rects
//! expanded at the origin so later DRC-aware costs can price
//! enclosure — plus the pair's named default pick and the named
//! findings (`unmatched` defs, `gaps` pairs). See `docs/e6-drt.md` §6.

use crate::design_db::{DesignDb, PdnVia, ViaGeom};
use crate::drc::{DrcLayer, geom::expand_via};
use crate::interner::{LayerId, SymbolId};
use crate::tech_db::{LayerDir, RectDbu, TechDb};
use anyhow::Result;

/// One via candidate for an adjacent routing-layer pair: a LEF `VIA`
/// (`ViaGeom::Lef`) or a DEF `VIAS` record (`ViaGeom::Generated` for
/// `+ VIARULE`, `ViaGeom::Lef` again for a `+ VIA <lef>` alias — the
/// same def under a DEF name). `rects` are the def's shapes expanded
/// at the origin — the data a later DRC-aware cost prices enclosure
/// from.
#[derive(Clone, Debug, PartialEq)]
pub struct ViaCandidate {
    /// LEF `VIA` name or DEF `VIAS` name.
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut-layer name (`cut_layers` lookup by symbol).
    pub cut: SymbolId,
    pub cut_count: u32,
    pub geom: ViaGeom,
    /// `expand_via` output at (0,0) — cut + both metals, `DrcLayer`
    /// keyed; `DrcLayer::Cut` indexes `tech.cut_layers`.
    pub rects: Vec<(DrcLayer, RectDbu)>,
    /// Total painted area (cut + metal rects) — the policy's size key.
    pub area_dbu2: i64,
}

/// A via def whose `(lower, upper)` is not an adjacent pair — e.g. a
/// multi-cut stack. Named, never silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct UnmatchedVia {
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
}

/// An adjacent routing-layer pair with zero candidates — a named
/// tech gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViaGap {
    pub lower: LayerId,
    pub upper: LayerId,
}

/// Candidates and the chosen default for one adjacent pair `(k, k+1)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ViaPair {
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut layer between them (`cut_layers[..].above == k`), `None`
    /// when the LEF declares none — the pair is a `ViaGap` then.
    pub cut: Option<SymbolId>,
    /// Indices into `ViaTable::candidates`, candidate order.
    pub candidates: Vec<u32>,
    /// Index into `candidates` — the policy pick (`docs/e6-drt.md` §6);
    /// `None` iff the pair is a `ViaGap`.
    pub default: Option<u32>,
}

/// The per-design via candidate table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViaTable {
    /// LEF `via_defs` in file order, then `db.def_vias` in record
    /// order — the ranking's declaration-order tie-break.
    pub candidates: Vec<ViaCandidate>,
    /// One per adjacent pair, `pairs[k]` = `(routing_layers[k], [k+1])`.
    pub pairs: Vec<ViaPair>,
    /// Adjacent pairs with zero candidates.
    pub gaps: Vec<ViaGap>,
    /// Defs matching no adjacent pair.
    pub unmatched: Vec<UnmatchedVia>,
}

/// Routing-layer rects whose long axis is *not* along that layer's
/// preferred direction — a `Horizontal` layer's `height > width` or a
/// `Vertical` layer's `width > height`; squares are neutral. This is
/// what the techlef's `PR`/`PR_R`/`PR_M`/`PR_MR` naming encodes
/// (which metals ride the preferred direction).
fn nonpref_metal(tech: &TechDb, rects: &[(DrcLayer, RectDbu)]) -> u32 {
    rects
        .iter()
        .filter(|(layer, r)| match layer {
            DrcLayer::Routing(l) => match tech.routing_layers[l.0 as usize].dir {
                LayerDir::Horizontal => r.height() > r.width(),
                LayerDir::Vertical => r.width() > r.height(),
            },
            DrcLayer::Cut(_) => false,
        })
        .count() as u32
}

/// Expands one def at the origin and places it in the table: into
/// `pairs[lower]` when `(lower, upper)` is an adjacent pair, else
/// into `unmatched`. `probe` is the via as an origin `PdnVia` —
/// `x_dbu`/`y_dbu` are ignored beyond the expansion.
fn add_candidate(table: &mut ViaTable, tech: &TechDb, probe: PdnVia) -> Result<()> {
    let rects = expand_via(&probe, tech)?;
    let area_dbu2 = rects.iter().map(|(_, r)| r.area()).sum();
    let idx = table.candidates.len() as u32;
    let (name, lower, upper) = (probe.name, probe.lower, probe.upper);
    table.candidates.push(ViaCandidate {
        name,
        lower,
        upper,
        cut: probe.cut,
        cut_count: probe.cut_count,
        geom: probe.geom,
        rects,
        area_dbu2,
    });
    if upper.0 == lower.0 + 1 && (lower.0 as usize) < table.pairs.len() {
        table.pairs[lower.0 as usize].candidates.push(idx);
    } else {
        table.unmatched.push(UnmatchedVia { name, lower, upper });
    }
    Ok(())
}

/// Builds the table: `tech.via_defs` plus `db.def_vias`, each matched
/// to its pair by `(lower, upper) == (k, k+1)`, rects expanded via
/// `drc::geom::expand_via` on an origin `PdnVia`, defaults ranked by
/// `(nonpref_metal, area_dbu2, declaration order)`. `Err` only on a
/// corrupted store (a `ViaGeom` index/`cut` the parse already
/// validated missing anyway) — same `expand_via` error path.
pub fn via_table(tech: &TechDb, db: &DesignDb) -> Result<ViaTable> {
    let n_pairs = tech.routing_layers.len().saturating_sub(1);
    let mut table = ViaTable {
        pairs: (0..n_pairs)
            .map(|k| ViaPair {
                lower: LayerId(k as u8),
                upper: LayerId(k as u8 + 1),
                cut: tech
                    .cut_layers
                    .iter()
                    .find(|c| c.above.0 as usize == k)
                    .map(|c| c.name),
                candidates: Vec::new(),
                default: None,
            })
            .collect(),
        ..Default::default()
    };

    for (i, vd) in tech.via_defs.iter().enumerate() {
        add_candidate(
            &mut table,
            tech,
            PdnVia {
                name: vd.name,
                lower: vd.lower,
                upper: vd.upper,
                cut: vd.cut,
                cut_count: vd.cut_count,
                geom: ViaGeom::Lef(i as u32),
                x_dbu: 0,
                y_dbu: 0,
            },
        )?;
    }
    for dv in &db.def_vias {
        add_candidate(
            &mut table,
            tech,
            PdnVia {
                name: dv.name,
                lower: dv.lower,
                upper: dv.upper,
                cut: dv.cut,
                cut_count: dv.cut_count,
                geom: dv.geom.clone(),
                x_dbu: 0,
                y_dbu: 0,
            },
        )?;
    }

    for k in 0..n_pairs {
        if table.pairs[k].candidates.is_empty() {
            table.gaps.push(ViaGap {
                lower: table.pairs[k].lower,
                upper: table.pairs[k].upper,
            });
            continue;
        }
        table.pairs[k].default = table.pairs[k]
            .candidates
            .iter()
            .enumerate()
            .min_by_key(|&(_, &ci)| {
                let c = &table.candidates[ci as usize];
                (nonpref_metal(tech, &c.rects), c.area_dbu2, ci)
            })
            .map(|(pos, _)| pos as u32);
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::DefViaDef;
    use crate::readers::lef::parse_lef_bytes;
    use crate::interner::StringInterner;

    /// met1/via12/met2/via23/met3 stack. `mv_pr` aligns both metals to
    /// preferred direction; `mv_pr_dup` is rect-identical (file-order
    /// tie-break); `mv_r` rotates both off it; `mv_stack` spans
    /// met1–met3 and matches no adjacent pair.
    const VIA_LEF: &str = r#"
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
  WIDTH 0.15 ;
END via12
LAYER met2
  TYPE ROUTING ;
  DIRECTION VERTICAL ;
  PITCH 0.46 ;
  WIDTH 0.14 ;
END met2
LAYER via23
  TYPE CUT ;
  WIDTH 0.2 ;
END via23
LAYER met3
  TYPE ROUTING ;
  DIRECTION HORIZONTAL ;
  PITCH 0.34 ;
  WIDTH 0.14 ;
END met3
VIA mv_pr
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END mv_pr
VIA mv_pr_dup
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met2 ;
    RECT -0.13 -0.16 0.13 0.16 ;
END mv_pr_dup
VIA mv_r
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER met1 ;
    RECT -0.13 -0.16 0.13 0.16 ;
  LAYER met2 ;
    RECT -0.16 -0.13 0.16 0.13 ;
END mv_r
VIA mv_stack
  LAYER via12 ;
    RECT -0.075 -0.075 0.075 0.075 ;
  LAYER via23 ;
    RECT -0.1 -0.1 0.1 0.1 ;
  LAYER met1 ;
    RECT -0.16 -0.13 0.16 0.13 ;
  LAYER met3 ;
    RECT -0.16 -0.13 0.16 0.13 ;
END mv_stack
"#;

    fn via_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech = TechDb::new();
        parse_lef_bytes(VIA_LEF.as_bytes(), &mut tech, &mut interner)
            .expect("fixture LEF parses");
        tech.strings = interner.clone();
        tech.rebuild_maps();
        tech
    }

    /// Adjacent triples land on their pair; `mv_stack`'s met1–met3
    /// span is a named unmatched def, and the empty pairs (met2–met3)
    /// become `ViaGap`s.
    #[test]
    fn test_triple_match_and_unmatched() {
        let tech = via_tech();
        let db = DesignDb::new();
        let table = via_table(&tech, &db).unwrap();

        let p0 = &table.pairs[0];
        assert_eq!((p0.lower.0, p0.upper.0), (0, 1));
        assert_eq!(
            tech.strings.resolve(p0.cut.unwrap()),
            "via12"
        );
        assert_eq!(p0.candidates.len(), 3);

        assert_eq!(
            table.unmatched,
            vec![UnmatchedVia {
                name: tech.strings.get("mv_stack").unwrap(),
                lower: LayerId(0),
                upper: LayerId(2),
            }]
        );
        // met2–met3 has no def → named gap, no default.
        assert_eq!(
            table.gaps,
            vec![ViaGap {
                lower: LayerId(1),
                upper: LayerId(2)
            }]
        );
        assert!(table.pairs[1].default.is_none());
    }

    /// The `(nonpref_metal, area_dbu2, order)` ranking picks the
    /// preferred-aligned variant and breaks a shape-tie by file order.
    #[test]
    fn test_default_ranking() {
        let tech = via_tech();
        let db = DesignDb::new();
        let table = via_table(&tech, &db).unwrap();
        let p0 = &table.pairs[0];
        let pick = &table.candidates[p0.candidates[p0.default.unwrap() as usize] as usize];
        assert_eq!(tech.strings.resolve(pick.name), "mv_pr");

        let by_name = |n: &str| {
            table
                .candidates
                .iter()
                .find(|c| tech.strings.resolve(c.name) == n)
                .unwrap()
        };
        assert_eq!(nonpref_metal(&tech, &by_name("mv_pr").rects), 0);
        assert_eq!(nonpref_metal(&tech, &by_name("mv_r").rects), 2);
        // `mv_pr_dup` ties `mv_pr` on both keys; file order keeps
        // `mv_pr` the default.
        assert_eq!(by_name("mv_pr").rects, by_name("mv_pr_dup").rects);
    }

    /// A `+ VIARULE` generated record becomes a candidate whose rects
    /// are the `expand_via` cut-array expansion (the committed
    /// `via2_3_1600_480_1_5_320_320` shape from `drc::geom`'s gate).
    #[test]
    fn test_generated_record_candidate() {
        let tech = via_tech();
        let mut db = DesignDb::new();
        let name = db.strings.intern("gen12");
        db.def_vias.push(DefViaDef {
            name,
            lower: LayerId(0),
            upper: LayerId(1),
            cut: tech.strings.get("via12").unwrap(),
            cut_count: 5,
            geom: ViaGeom::Generated(crate::design_db::GeneratedViaGeom {
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
        });
        let table = via_table(&tech, &db).unwrap();
        // 3 LEF defs + the generated record, in declaration order.
        assert_eq!(table.pairs[0].candidates.len(), 4);
        let g = table
            .candidates
            .iter()
            .find(|c| c.name == name)
            .unwrap();
        assert!(matches!(g.geom, ViaGeom::Generated(_)));
        let by_layer = |l: DrcLayer| -> Vec<RectDbu> {
            g.rects
                .iter()
                .filter(|(dl, _)| *dl == l)
                .map(|(_, r)| *r)
                .collect()
        };
        assert_eq!(
            by_layer(DrcLayer::Routing(LayerId(0))),
            vec![RectDbu::new(-800, -240, 800, 240)]
        );
        assert_eq!(by_layer(DrcLayer::Cut(0)).len(), 5);
        // met2 is Vertical and the 1540×320 metal is wider than tall
        // — one nonpreferred metal.
        assert_eq!(nonpref_metal(&tech, &g.rects), 1);
    }
}
