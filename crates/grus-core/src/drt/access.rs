//! Candidate access points: the grid locations where a layer's track
//! lines cross a routed pin's shapes. One point per track line per pin
//! rect — a superset candidate list a later maze router prunes, not
//! TritonRoute's access-point algorithm (which scores preferred-track
//! × secondary-track intersections) and not a DRC-clean or
//! track-assigned set. See `docs/e6-drt.md` for the claim boundary.

use crate::design_db::{DesignDb, PlacementStatus};
use crate::drt::tracks::LayerTracks;
use crate::interner::{LayerId, NetId, PinId};
use crate::tech_db::{RectDbu, TechDb};
use std::collections::{BTreeMap, BTreeSet};

/// A grid location on `layer` where a track line crosses `pin`'s shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessPoint {
    pub pin: PinId,
    pub layer: LayerId,
    pub x: i32,
    pub y: i32,
}

/// A member pin whose shapes intersected no track on any candidate
/// layer — a finding, never silently counted.
#[derive(Clone, Debug, PartialEq)]
pub struct OffTrackPin {
    pub pin: PinId,
    /// `db.pin_nets[pin]` at census time.
    pub net: Option<NetId>,
    /// The absolute candidate rects that were checked.
    pub rects: Vec<(LayerId, RectDbu)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessReport {
    /// In emission order (see `access_points`); deduplicated per pin.
    pub points: Vec<AccessPoint>,
    /// `points` count per routing layer — parallel to
    /// `tech.routing_layers`. The census evidence.
    pub per_layer: Vec<usize>,
    /// Pins with candidate shapes but zero access points.
    pub off_track: Vec<OffTrackPin>,
    /// Pins with no candidate shapes at all (unplaced cell, missing
    /// `pin_macro_pins` mapping, io pin without `+ LAYER` rects).
    pub no_shapes: Vec<PinId>,
}

/// `(x, y)` points one rect contributes on `lt`: each `tracks_x` line
/// inside `[xl, xh]` gives `(t, mid_y)`, each `tracks_y` line inside
/// `[yl, yh]` gives `(mid_x, t)`. Boundaries are inclusive — a track on
/// the rect edge touches the shape. `mid` is i32 division (truncates
/// toward zero; die-internal coords are non-negative, so this is floor).
fn rect_points(rect: &RectDbu, lt: &LayerTracks) -> Vec<(i32, i32)> {
    let mid_x = (rect.xl + rect.xh) / 2;
    let mid_y = (rect.yl + rect.yh) / 2;
    let mut out = Vec::new();
    out.extend(
        lt.tracks_x
            .iter()
            .filter(|&&t| t >= rect.xl && t <= rect.xh)
            .map(|&t| (t, mid_y)),
    );
    out.extend(
        lt.tracks_y
            .iter()
            .filter(|&&t| t >= rect.yl && t <= rect.yh)
            .map(|&t| (mid_x, t)),
    );
    out
}

/// A pin's absolute candidate rects — instance pins via
/// `pin_macro_pins` + `orient_rect` + the cell translate (gated on
/// `Placed | Fixed | Cover`, the `drc::geom` source-4 convention); io
/// `BTERM`s via `pin_io_rects` (already absolute, source 5). `+ SPECIAL`
/// ports share `pin_cells == None` but are never members of
/// `net_names` nets, so they are never visited.
fn pin_shapes(db: &DesignDb, tech: &TechDb, p: PinId) -> Vec<(LayerId, RectDbu)> {
    let idx = p.0 as usize;
    match db.pin_cells[idx] {
        None => db.pin_io_rects[idx].clone(),
        Some(c) => {
            if !matches!(
                db.cell_status[c.0 as usize],
                PlacementStatus::Placed | PlacementStatus::Fixed | PlacementStatus::Cover
            ) {
                return Vec::new();
            }
            let Some(mp) = db.pin_macro_pins[idx] else {
                return Vec::new();
            };
            let mac = &tech.macros[db.cell_macros[c.0 as usize].0 as usize];
            let (cx, cy) = (db.cell_x_dbu[c.0 as usize], db.cell_y_dbu[c.0 as usize]);
            let orient = db.cell_orient[c.0 as usize];
            tech.macro_pins[mp.0 as usize]
                .rects
                .iter()
                .map(|&(lid, r)| {
                    let r = DesignDb::orient_rect(r, mac.width_dbu, mac.height_dbu, orient);
                    (lid, RectDbu::new(cx + r.xl, cy + r.yl, cx + r.xh, cy + r.yh))
                })
                .collect()
        }
    }
}

/// Enumerates every signal-net member pin's candidate access points.
/// Emission order is fixed — nets `0..num_nets()` ascending → member
/// pins in `net_pins` order → candidate layers ascending → per rect,
/// `tracks_x` hits then `tracks_y` hits (both ascending) — so two runs
/// produce identical `points`. `(layer, x, y)` duplicates within one
/// pin collapse; a pin with shapes but no points lands in `off_track`,
/// a pin with no candidate rects in `no_shapes`.
pub fn access_points(db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks]) -> AccessReport {
    let mut report = AccessReport {
        points: Vec::new(),
        per_layer: vec![0; tech.routing_layers.len()],
        off_track: Vec::new(),
        no_shapes: Vec::new(),
    };

    for net_idx in 0..db.num_nets() {
        for &pin in db.net_pins(NetId(net_idx as u32)) {
            let shapes = pin_shapes(db, tech, pin);
            if shapes.is_empty() {
                report.no_shapes.push(pin);
                continue;
            }
            let mut by_layer: BTreeMap<LayerId, Vec<RectDbu>> = BTreeMap::new();
            for (lid, r) in &shapes {
                by_layer.entry(*lid).or_default().push(*r);
            }
            let mut seen: BTreeSet<(u8, i32, i32)> = BTreeSet::new();
            let mut emitted = 0;
            for (lid, rects) in &by_layer {
                let Some(lt) = tracks.get(lid.0 as usize) else {
                    continue;
                };
                for rect in rects {
                    for (x, y) in rect_points(rect, lt) {
                        if seen.insert((lid.0, x, y)) {
                            report.points.push(AccessPoint {
                                pin,
                                layer: *lid,
                                x,
                                y,
                            });
                            report.per_layer[lid.0 as usize] += 1;
                            emitted += 1;
                        }
                    }
                }
            }
            if emitted == 0 {
                report.off_track.push(OffTrackPin {
                    pin,
                    net: db.pin_nets[pin.0 as usize],
                    rects: shapes,
                });
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Edge-inclusive hits on both axes: `[100,300]×[50,250]` with
    /// `tracks_x=[100,170,510]` and `tracks_y=[230]` gives `(100,150)`
    /// (edge track `t == xl`), `(170,150)`, and `(200,230)` — `mid_x`
    /// is the truncated `(100+300)/2 = 200`. Out-of-span tracks
    /// contribute nothing.
    #[test]
    fn test_rect_points() {
        let lt = LayerTracks {
            tracks_x: vec![100, 170, 510],
            tracks_y: vec![230],
        };
        let pts = rect_points(&RectDbu::new(100, 50, 300, 250), &lt);
        assert_eq!(pts, vec![(100, 150), (170, 150), (200, 230)]);
    }
}
