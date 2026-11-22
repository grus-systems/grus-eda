# e8-mesh-rc — API surface

Everything lands in `crates/grus-core/src/pdn/mesh.rs`, a child
module of `pdn` (`pdn.rs` gains `pub mod mesh;` + `pub use
mesh::*;` at the bottom of its public section — `lib.rs`'s existing
`pub use pdn::*` then exports the mesh types flat). All types are
plain data, serde-able (`Serialize`/`Deserialize`/`Clone`/`Debug`),
matching `PdnGraph`/`PdnCensus` style.

## Types

```rust
pub struct PdnMesh {
    /// Net name (indexes `DesignDb::strings`).
    pub net: SymbolId,
    pub nodes: Vec<MeshNode>,
    /// Undirected conductance edges, construction order:
    /// per-rect run order first, then vias in `PdnNet::vias` order.
    pub resistors: Vec<MeshResistor>,
    /// One per distinct attached `PinId` (`graph.attachments` set).
    pub sinks: Vec<MeshSink>,
    /// Node indices under the top-level PIN's io-rect intersections.
    pub sources: Vec<u32>,
    /// Sinks whose pin offered >1 candidate node — recorded at build;
    /// a bare `PdnMesh` cannot re-derive the candidate sets, so
    /// `census()` reads the stored count. (As-implemented addition:
    /// the design left `MeshCensus::multi_node_sinks` storage
    /// unspecified and `census()` must be self-consistent.)
    #[serde(default)]
    pub multi_node_sinks: u32,
}

pub struct MeshNode {
    pub layer: LayerId,
    /// Junction-piece bboxes on this layer (empty for dangling via ends).
    pub rects: Vec<RectDbu>,
    pub is_source: bool,
    pub is_sink: bool,
    /// Via endpoint that landed on no rect (mirrors `PdnNode::via_end`).
    pub dangling: bool,
}

pub struct MeshResistor {
    pub a: u32,
    pub b: u32,
    pub r_ohms: f32,
    pub kind: ResistorKind,
}

pub enum ResistorKind {
    /// Run along `PdnNet::wire_rects[idx]` between two junction nodes.
    Wire { rect: u32 },
    /// Stack link of `PdnNet::vias[idx]`.
    Via { via: u32 },
}

pub struct MeshSink {
    pub pin: PinId,
    pub node: u32,
}

pub struct MeshCensus {
    pub net: SymbolId,
    pub nodes: u32,
    pub source_nodes: u32,
    pub wire_resistors: u32,
    pub via_resistors: u32,
    pub sinks: u32,
    pub multi_node_sinks: u32,
    pub dangling_via_ends: u32,
    pub isolated_nodes: u32,
    pub min_r_ohms: f32,
    pub max_r_ohms: f32,
    pub violations: u32,
}

pub enum MeshViolation {
    NonFiniteResistance { resistor: u32 },
    NonPositiveResistance { resistor: u32, r_ohms: f32 },
    DanglingSink { pin: PinId },
    DanglingSource { node: u32 },
    NoSources,
    UnreachableNode { node: u32 },
}
```

`MeshNode.layer` is non-optional: unlike `PdnNode` (which may merge
across layers), a mesh node is single-layer by construction — even a
dangling via end knows its layer from `via.lower`/`via.upper`.

## Functions

```rust
/// Build the resistor mesh for one `PdnNet`.
/// `Err` on conn-expansion failure (forwarded from `try_build_graph`)
/// or when `TechDb::via_res_ohm` has no resistance for a via's cut
/// layer — an unpriceable via cannot be modelled.
pub fn build_mesh(db: &DesignDb, tech_db: &TechDb, net: &PdnNet) -> Result<PdnMesh>;

/// Build + census; violations are in `census.violations` and
/// `mesh.validate()` — construction itself does not gate on them.
pub fn check_mesh(db: &DesignDb, tech_db: &TechDb, net: &PdnNet)
    -> Result<(PdnMesh, MeshCensus)>;

/// Convenience: every `+ USE POWER`/`GROUND` net's
/// `(PdnMesh, MeshCensus)` — same net filter as `pdn::census`.
pub fn meshes(db: &DesignDb, tech_db: &TechDb) -> Vec<(PdnMesh, MeshCensus)>;

impl PdnMesh {
    /// Fail-proof surface: finite+positive R, sink/source resolution,
    /// source reachability (model.md §validate). Empty on a good mesh.
    pub fn validate(&self) -> Vec<MeshViolation>;

    /// Node/resistor/sink/source census for `docs/results.md` and tests.
    pub fn census(&self) -> MeshCensus; // violations counted inside
}
```

`build_mesh` internally calls `super::try_build_graph` first —
conn expansion, source pins and the attach set are reused verbatim,
then its own feature pass adds the finer geometry. Private parents'
items are reached via `use super::{…}` (`expand_conns`,
`rects_intersect`, `rect_contains_point`, `UnionFind`) — no signature
or visibility changes in `pdn.rs`.

Two `validate()` refinements landed in implementation: `dangling`
via-end nodes are reported `UnreachableNode` even though the via
resistor keeps them BFS-reachable (a stub endpoint on no metal is a
defect — `test_synthetic_dangling_via` asserts it), and the
reachability BFS is skipped when the mesh has no sources (`NoSources`
already carries the verdict; reporting every node unreachable would
only add noise).

## `pdn.rs` diff (mechanical)

```rust
+ /// Resistor network over the extracted PDN geometry (E8 ticket 3).
+ pub mod mesh;
+ pub use mesh::*;
```

plus `RESISTANCE RPERSQ <r> ;` lines added to the `met1`/`met2`
layers of the test-only `PDN_LEF` fixture so mesh tests have a
nonzero `res_per_sq` (existing tests never read that field — the
change is additive to the fixture).

## `design_db.rs` / `tech_db.rs`

No changes. `TechDb` is consumed read-only:
`routing_layers[layer.0 as usize].res_per_sq` and
`tech_db.via_res_ohm(via.cut, via.cut_count)`.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/mesh.rs` (new)
