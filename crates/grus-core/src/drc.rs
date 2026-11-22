//! LEF+DEF DRC subset checker: `geom` assembles every shape the subset
//! covers into a per-layer index, `width` implements the first rule
//! family verified against the committed Magic goldens.

pub mod geom;
pub mod width;

use crate::interner::{LayerId, SymbolId};
use crate::tech_db::RectDbu;
use serde::{Deserialize, Serialize};

/// A checkable layer: routing stack or cut stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrcLayer {
    Routing(LayerId),
    /// Index into `TechDb::cut_layers`.
    Cut(u32),
}

/// Which DB source produced the shape — diagnostics and fixture tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeSrc {
    /// `NETS` wire segment / `RECT` stub.
    SignalWire,
    /// `SPECIALNETS` wire segment.
    SpecialWire,
    /// Via-instance-derived (cut + endpoint metals).
    Via,
    /// Placed instance's macro `PIN` port rect.
    MacroPin,
    /// Placed instance's `OBS` / pin `OBS` rect.
    MacroObs,
    /// Top-level `PINS` `+ LAYER` rect.
    IoPort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeRect {
    pub rect: RectDbu,
    pub src: ShapeSrc,
    /// Interned net name for wire/via shapes (diagnostics).
    pub net: Option<SymbolId>,
}

/// Shapes on one layer, insertion order plus a sorted index into them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerShapes {
    pub rects: Vec<ShapeRect>,
    /// `rects` indices sorted by `rect.xl` (stable — `sort_by_key`).
    pub by_x: Vec<u32>,
}

impl LayerShapes {
    fn push(&mut self, s: ShapeRect) {
        self.rects.push(s);
    }

    fn finish(mut self) -> Self {
        self.by_x = (0..self.rects.len() as u32).collect();
        self.by_x.sort_by_key(|&i| self.rects[i as usize].rect.xl);
        self
    }

    /// Rects strictly overlapping `probe` — touching edges are not
    /// overlapping, the `pdn.rs`/gcell convention. The query the future
    /// spacing family drives; `width` only needs `rects.iter()`.
    pub fn iter_overlapping(&self, probe: &RectDbu) -> impl Iterator<Item = &ShapeRect> {
        // First index whose rect could reach `probe.xl`.
        let start = self
            .by_x
            .partition_point(|&i| self.rects[i as usize].rect.xh <= probe.xl);
        self.by_x[start..]
            .iter()
            .map(move |&i| &self.rects[i as usize])
            .take_while(move |s| s.rect.xl < probe.xh)
            .filter(move |s| s.rect.yl < probe.yh && s.rect.yh > probe.yl)
    }
}

/// All shapes the LEF+DEF subset checker sees, addressed by layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrcDesign {
    /// Indexed by `LayerId` (same order as `TechDb::routing_layers`).
    pub routing: Vec<LayerShapes>,
    /// Indexed by `cut_layers` position.
    pub cut: Vec<LayerShapes>,
}
