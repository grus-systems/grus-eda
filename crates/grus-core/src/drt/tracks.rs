//! Per-layer track coordinate lists, merged from the two `TRACKS`
//! provenances: LEF records expanded into `RoutingLayer::tracks_{x,y}`
//! by the LEF reader, and DEF `TRACKS` statements held as
//! `DesignDb::tracks` (`TrackGrid`, already DBU). On the sky130 test
//! designs the LEF side is always empty — the effective grid is
//! entirely DEF-sourced.

use crate::design_db::DesignDb;
use crate::interner::LayerId;
use crate::tech_db::{LayerDir, TechDb};

/// Which coordinate list a track set populates: `X` = vertical lines
/// (x-coordinates), `Y` = horizontal lines (y-coordinates) — the same
/// convention `TrackGrid::is_vertical_tracks` documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// The effective track grid of one routing layer on a design.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerTracks {
    /// Sorted unique DBU coordinates — vertical-line (X) tracks.
    pub tracks_x: Vec<i32>,
    /// Sorted unique DBU coordinates — horizontal-line (Y) tracks.
    pub tracks_y: Vec<i32>,
}

/// A routing layer whose preferred-direction track list is empty.
/// Named, never synthesized from `PITCH`/`OFFSET`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackGap {
    pub layer: LayerId,
    pub axis: Axis,
}

/// Union of the LEF-declared lists on `RoutingLayer` and the expansion
/// of every `DesignDb::tracks` grid. Parallel to `tech.routing_layers`.
pub fn layer_tracks(db: &DesignDb, tech: &TechDb) -> Vec<LayerTracks> {
    let mut out: Vec<LayerTracks> = tech
        .routing_layers
        .iter()
        .map(|l| LayerTracks {
            tracks_x: l.tracks_x.clone(),
            tracks_y: l.tracks_y.clone(),
        })
        .collect();

    for tg in &db.tracks {
        for &lid in &tg.layers {
            let Some(layer) = out.get_mut(lid.0 as usize) else {
                continue;
            };
            let list = if tg.is_vertical_tracks {
                &mut layer.tracks_x
            } else {
                &mut layer.tracks_y
            };
            list.extend((0..tg.num_tracks).map(|k| tg.start_dbu + k * tg.step_dbu));
        }
        // A `TrackGrid` with empty `layers` contributes nothing — the
        // DEF reader drops bare-form layer names silently (pre-existing
        // asymmetry vs the strict LEF side; noted, not changed here).
    }

    for lt in &mut out {
        lt.tracks_x.sort_unstable();
        lt.tracks_x.dedup();
        lt.tracks_y.sort_unstable();
        lt.tracks_y.dedup();
    }
    out
}

/// Preferred-axis coverage: a `Horizontal` layer routes along
/// horizontal lines, so it needs `tracks_y`; a `Vertical` layer needs
/// `tracks_x` (the direction-to-axis flip is easy to get backwards).
pub fn track_gaps(tech: &TechDb, tracks: &[LayerTracks]) -> Vec<TrackGap> {
    tech.routing_layers
        .iter()
        .enumerate()
        .filter_map(|(i, layer)| {
            let empty = match layer.dir {
                LayerDir::Horizontal => tracks.get(i).is_none_or(|t| t.tracks_y.is_empty()),
                LayerDir::Vertical => tracks.get(i).is_none_or(|t| t.tracks_x.is_empty()),
            };
            empty.then_some(TrackGap {
                layer: LayerId(i as u8),
                axis: match layer.dir {
                    LayerDir::Horizontal => Axis::Y,
                    LayerDir::Vertical => Axis::X,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::TrackGrid;
    use crate::interner::StringInterner;
    use crate::tech_db::RoutingLayer;

    fn two_layer_tech() -> TechDb {
        let mut interner = StringInterner::new();
        let mut tech = TechDb::new();
        let layer = |interner: &mut StringInterner, name, dir| RoutingLayer {
            name: interner.intern(name),
            dir,
            pitch_dbu: 340,
            width_dbu: 140,
            offset_dbu: 170,
            tracks_x: Vec::new(),
            tracks_y: Vec::new(),
            res_per_sq: 0.0,
            cap_per_sq_um: 0.0,
            edge_cap_per_um: 0.0,
            spacing_dbu: 0,
            spacing_table: Vec::new(),
            area_dbu2: 0,
            min_enclosed_area_dbu2: 0,
        };
        tech.routing_layers
            .push(layer(&mut interner, "met1", LayerDir::Horizontal));
        tech.routing_layers
            .push(layer(&mut interner, "met2", LayerDir::Vertical));
        tech.routing_layers[0].tracks_x = vec![100, 500];
        tech.strings = interner;
        tech.rebuild_maps();
        tech
    }

    /// LEF coords + DEF `TrackGrid` expansion merge sorted and deduped;
    /// a second axis lands on the matching list only.
    #[test]
    fn test_layer_tracks_merges_def_grids() {
        let tech = two_layer_tech();
        let mut db = DesignDb::new();
        db.tracks.push(TrackGrid {
            is_vertical_tracks: true,
            start_dbu: 300,
            num_tracks: 2,
            step_dbu: 200,
            layers: vec![LayerId(0)],
        });
        db.tracks.push(TrackGrid {
            is_vertical_tracks: false,
            start_dbu: 170,
            num_tracks: 2,
            step_dbu: 340,
            layers: vec![LayerId(0), LayerId(1)],
        });

        let tracks = layer_tracks(&db, &tech);
        // met1 x: LEF [100,500] ∪ DEF [300,500] → dedup at 500.
        assert_eq!(tracks[0].tracks_x, vec![100, 300, 500]);
        assert_eq!(tracks[0].tracks_y, vec![170, 510]);
        assert_eq!(tracks[1].tracks_y, vec![170, 510]);
        assert!(tracks[1].tracks_x.is_empty());
    }

    #[test]
    fn test_track_gaps() {
        let tech = two_layer_tech();
        let tracks = vec![
            LayerTracks {
                tracks_x: vec![100],
                tracks_y: Vec::new(),
            },
            LayerTracks {
                tracks_x: vec![100],
                tracks_y: vec![100],
            },
        ];
        // met1 is Horizontal → needs tracks_y; met2 is Vertical →
        // needs tracks_x. Only met1 is gapped.
        assert_eq!(
            track_gaps(&tech, &tracks),
            vec![TrackGap {
                layer: LayerId(0),
                axis: Axis::Y
            }]
        );

        let ok = vec![
            LayerTracks {
                tracks_x: vec![100],
                tracks_y: vec![100],
            },
            LayerTracks {
                tracks_x: vec![100],
                tracks_y: vec![100],
            },
        ];
        assert!(track_gaps(&tech, &ok).is_empty());
    }
}
