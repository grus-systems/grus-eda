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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerShapes {
    pub rects: Vec<ShapeRect>,
    /// `rects` indices sorted by `rect.xl` (stable — `sort_by_key`).
    /// Private: a hand-rolled value with an empty index would make
    /// `iter_overlapping` silently return nothing — construction must
    /// go through `new`/`from_rects`/`push`/`finish`.
    by_x: Vec<u32>,
    /// Prefix max of `rect.xh` over `by_x` order. Non-decreasing by
    /// construction — this is what makes the overlap binary search
    /// sound (an `xh` predicate over `by_x` itself is not monotone:
    /// a wide early rect sits before narrow ones).
    max_xh: Vec<i32>,
}

impl LayerShapes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Finished shapes — the index is built here, so the result can
    /// never carry a dead index.
    pub fn from_rects(rects: Vec<ShapeRect>) -> Self {
        let mut s = LayerShapes::new();
        s.rects = rects;
        s.finish()
    }

    fn push(&mut self, s: ShapeRect) {
        self.rects.push(s);
    }

    fn finish(mut self) -> Self {
        self.by_x = (0..self.rects.len() as u32).collect();
        self.by_x.sort_by_key(|&i| self.rects[i as usize].rect.xl);
        let mut reach = i32::MIN;
        self.max_xh = self
            .by_x
            .iter()
            .map(|&i| {
                reach = reach.max(self.rects[i as usize].rect.xh);
                reach
            })
            .collect();
        self
    }

    /// Rects strictly overlapping `probe` — touching edges are not
    /// overlapping, the `pdn.rs`/gcell convention. The query the future
    /// spacing family drives; `width` only needs `rects.iter()`.
    pub fn iter_overlapping(&self, probe: &RectDbu) -> impl Iterator<Item = &ShapeRect> {
        // Skip only positions whose prefix max ends left of the probe —
        // those rects all end left of it too. Positions kept must still
        // pass an `xh` re-check: a wide predecessor carries the prefix
        // max forward, so a rect entirely left of `probe` can sit after
        // `start`.
        let start = self.max_xh.partition_point(|&mx| mx <= probe.xl);
        self.by_x[start.min(self.by_x.len())..]
            .iter()
            .map(move |&i| &self.rects[i as usize])
            .take_while(move |s| s.rect.xl < probe.xh)
            .filter(move |s| {
                s.rect.xh > probe.xl && s.rect.yl < probe.yh && s.rect.yh > probe.yl
            })
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
