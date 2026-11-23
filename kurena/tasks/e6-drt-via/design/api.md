# e6-drt-via — API surface

New module `crates/grus-core/src/drt/via.rs`, re-exported from
`drt.rs` (`pub mod via;`). Imports: `crate::design_db::{DesignDb,
PdnVia, ViaGeom}`, `crate::drc::{DrcLayer, geom::expand_via}`,
`crate::interner::{LayerId, SymbolId}`, `crate::tech_db::{RectDbu,
TechDb}`.

## `drt/via.rs`

```rust
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViaPair {
    pub lower: LayerId,          // k
    pub upper: LayerId,          // k+1
    /// Cut layer between them (`cut_layers[..].above == k`), `None`
    /// when the LEF declares none — the pair is a `ViaGap` then.
    pub cut: Option<SymbolId>,
    /// Indices into `ViaTable::candidates`, candidate order.
    pub candidates: Vec<u32>,
    /// Index into `candidates` — the policy pick (`via-model.md` §3);
    /// `None` iff the pair is a `ViaGap`.
    pub default: Option<u32>,
}

/// The per-design via candidate table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViaTable {
    /// LEF `via_defs` in file order, then `db.def_vias` in record
    /// order — the ranking's declaration-order tie-break.
    pub candidates: Vec<ViaCandidate>,
    /// One per adjacent pair, `pairs[k]` = (routing_layers[k], [k+1]).
    pub pairs: Vec<ViaPair>,
    /// Adjacent pairs with zero candidates.
    pub gaps: Vec<ViaGap>,
    /// Defs matching no adjacent pair.
    pub unmatched: Vec<UnmatchedVia>,
}

/// Builds the table: `tech.via_defs` plus `db.def_vias`, each matched
/// to its pair by `(lower, upper) == (k, k+1)`, rects expanded via
/// `drc::geom::expand_via` on an origin `PdnVia`, defaults ranked per
/// `via-model.md` §3. `Err` only on a corrupted store (a `ViaGeom`
/// index/`cut` the parse already validated missing anyway) — same
/// `expand_via` error path.
pub fn via_table(tech: &TechDb, db: &DesignDb) -> anyhow::Result<ViaTable>;
```

## `drt/graph.rs` delta

```rust
/// A layer-transition edge between nodes `lower` (layer l) and
/// `upper` (l+1) at the same (x,y) — a track crossing on both layers
/// and guide-covered on both. `lower < upper` always (nodes are
/// emitted layer-ascending).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViaEdge {
    pub lower: u32,
    pub upper: u32,
}
```

- `NetGraph` gains `pub via_edges: Vec<ViaEdge>` — separate from the
  in-plane `edges` (the router prices transitions differently).
  Emission order fixed: adjacent pair ascending → lower-layer node
  order.
- `RouteGraphs` gains `pub total_via_edges: usize` and
  `pub via_edges_per_pair: Vec<usize>` — parallel to
  `tech.routing_layers.len() - 1` (index = lower layer), the census
  evidence.
- `build_route_graphs(db, tech, tracks, join)` keeps its signature.
  Emission is purely geometric — shared crossing + coverage on both
  layers, per `via-edges.md` §2 — so `graph.rs` never imports
  `via.rs` and no `ViaTable` is passed. A pair with zero candidates
  is a `ViaGap` (the named finding); edges the geometry still yields
  are the data explaining the gap, and the maze router joins
  table + graph for pricing, so no phantom routability.

## `design_db.rs` delta

```rust
/// A resolved DEF `VIAS` record: `+ VIARULE` generated vias keep
/// their geometry params; `+ VIA <lef>` aliases resolve to
/// `ViaGeom::Lef`. `+ RECT`/`+ POLYGON` inline records and `+ VIA`
/// names the LEF does not define store nothing — name-reference
/// errors stay lazy on the wire-resolution path, unchanged.
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

// DesignDb:
/// DEF `VIAS` defs in record order — the via candidate table's
/// second source (`drt::via`).
#[serde(default)]
pub def_vias: Vec<DefViaDef>,
```

`PdnVia` is *not* reused: it carries `x_dbu`/`y_dbu` (an instance,
not a def) and `geom` already matches — a dedicated struct is ~10
lines and keeps the def/instance distinction explicit.

## `readers/def.rs` delta

At the `VIAS` dispatch (~line 1719): after `add_def_via` returns
`(name, via)`, match on `&via` —

- `DefVia::Generated { lower, upper, cut, cut_count, geom }` → push
  `DefViaDef { name: db.strings.intern(&name), .., geom:
  ViaGeom::Generated(geom.clone()) }`.
- `DefVia::Fixed(sym)` → `tech_db.via_by_name.get(sym)` hits → push
  `DefViaDef` with the resolved `ViaDef` triple and `ViaGeom::Lef(i)`;
  a miss stores nothing (the `resolve_def_via` lazy error is
  unchanged).
- `DefVia::Inline` → stores nothing.

`db.strings` clones `tech_db.strings` at parse start, so the interned
name resolves identically. No other parse behavior changes;
`DefViaMap` still resolves net-referenced vias as today.

## `drt.rs` delta

`pub mod via;` plus one module-doc line: `via` enumerates the
adjacent-pair candidate defs and names each pair's default pick.

## Files

- `crates/grus-core/src/drt/via.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/readers/def.rs`
