# e8-pdn-parse — API surface

Concrete signatures. Everything lands in `crates/grus-core`; types are
plain data (no lifetimes beyond `SymbolId`/`LayerId`), all serde-able.

## `design_db.rs`

```rust
pub struct PdnNet {
    pub name: SymbolId,
    pub use_kind: Option<PdnUse>,
    pub conns: Vec<PdnConn>,
    pub wire_rects: Vec<(LayerId, RectDbu)>,
    pub vias: Vec<PdnVia>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdnUse { Power, Ground, Signal, Clock, Analog, Reset, Scan, TieOff, Ring }

pub enum PdnConn {
    TopPin(SymbolId),
    AllInstances(SymbolId),
    Instance { inst: SymbolId, pin: SymbolId },
}

pub struct PdnVia {
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    pub cut: SymbolId,
    pub cut_count: u32,
    pub x_dbu: i32,
    pub y_dbu: i32,
}

impl DesignDb {
    /// Flat view over every special net's wire rects — replaces the
    /// removed `special_wire_rects` field; congestion consumer unchanged.
    pub fn special_wire_rects(&self) -> impl Iterator<Item = (LayerId, RectDbu)> + '_;

    /// Net lookup for pdn.rs and tests.
    pub fn pdn_net(&self, name: &str) -> Option<&PdnNet>;

    /// Macro-coords rect → oriented macro-coords rect (`w`/`h` = macro
    /// size). Lifted verbatim out of gcell.rs lines ~202–215 (the
    /// 8-arm `match orient`) so pin-attach and blockage share one
    /// transform; callers add `cell_x_dbu`/`cell_y_dbu`.
    pub(crate) fn orient_rect(r: RectDbu, w: i32, h: i32, orient: Orient) -> RectDbu;
}
```

`DesignDb` field diff:

```rust
- pub special_wire_rects: Vec<(LayerId, RectDbu)>,
+ #[serde(default)]
+ pub pdn_nets: Vec<PdnNet>,
```

## `tech_db.rs`

```rust
pub struct CutLayer {
    pub name: SymbolId,
    pub above: LayerId,
}

pub struct ViaDef {
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    pub cut: SymbolId,
    pub cut_count: u32,
}

pub struct TechDb {
    // existing …
    #[serde(default)]
    pub cut_layers: Vec<CutLayer>,
    #[serde(skip)]
    pub cut_layer_by_name: HashMap<SymbolId, u32>,
    #[serde(default)]
    pub via_defs: Vec<ViaDef>,
    #[serde(skip)]
    pub via_by_name: HashMap<SymbolId, u32>,
}

impl TechDb {
    /// Per-instance via resistance for a resolved `PdnVia`:
    /// `via_res_ohm_above[cut.above] / cut_count`. Returns `None`
    /// when the cut layer carries no RESISTANCE.
    pub fn via_res_ohm(&self, cut: SymbolId, cut_count: u32) -> Option<f32>;
}
```

`rebuild_maps` gains rebuilds for both `#[serde(skip)]` maps.

## `pdn.rs` (new module, `pub mod pdn` + `pub use pdn::*` in lib.rs)

```rust
pub struct PdnGraph { /* model.md */ }
pub struct PdnNode {
    pub layer: Option<LayerId>,
    pub rect_indices: Vec<u32>,
    pub via_end: bool,
}
pub struct PdnCensus { /* model.md */ }

/// Build the per-net graph: conn expansion, same-layer union, via
/// links, pin attach, source marking. Panics on conn-expansion errors;
/// `try_build_graph` is the `Result`-returning twin.
pub fn build_graph(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> PdnGraph;
pub fn try_build_graph(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> Result<PdnGraph>;

/// Connectivity verdict + census for one net.
pub fn check_net(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> (PdnGraph, PdnCensus);

/// Convenience: every `+ USE POWER`/`GROUND` net's census.
pub fn census(db: &DesignDb, tech_db: &TechDb) -> Vec<PdnCensus>;
```

`tech_db` is a parameter — attachability needs `MacroPin::rects`. Errors
surface through `anyhow::Result` on `try_build_graph` only where
user-input lookups can fail (`TopPin`/`Instance` conn names);
attachable-pin math is infallible.

## `def.rs` / `lef.rs` (internal)

```rust
// def.rs — parse-local
enum DefVia { Generated { lower, upper, cut, cut_count }, Fixed(SymbolId), Inline }
struct DefViaMap(FastMap<String, DefVia>);

fn add_def_via(db: &mut DesignDb, tech_db: &TechDb, words: &[&str],
               line: usize) -> Result<(String, DefVia)>;
fn add_special_net(db: &mut DesignDb, tech_db: &TechDb, vias: &DefViaMap,
                   words: &[&str], line: usize) -> Result<()>;

// lef.rs — internal, same shape as parse_layer
fn parse_via(lexer: &mut LefLexer, tech_db: &mut TechDb, interner: &mut StringInterner) -> Result<()>;
```

`parse_def` signature unchanged (`&mut DesignDb, &TechDb, &[u8]`) —
the `VIAS` map is parse-internal state.

## `gcell.rs`

```rust
- for &(wire_layer, r) in &design_db.special_wire_rects {
+ for (wire_layer, r) in design_db.special_wire_rects() {
```

and the inline orient match-arm block is replaced by
`DesignDb::orient_rect`.

## Files

- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/pdn.rs` (new)
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/congestion/gcell.rs`
- `crates/grus-core/src/lib.rs`
