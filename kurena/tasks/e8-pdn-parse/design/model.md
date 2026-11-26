# e8-pdn-parse — Data model & connectivity

`DesignDb` gains `pdn_nets: Vec<PdnNet>` plus the parse-level types
below; `crates/grus-core/src/pdn.rs` (new module) owns everything
computed from them — conn expansion, pin attachment, the union-find
graph, census and diagnostics.

## Parse-level types (`design_db.rs`)

```rust
pub struct PdnNet {
    pub name: SymbolId,
    pub use_kind: Option<PdnUse>,
    /// `( comp pin )` groups verbatim; expanded in pdn.rs.
    pub conns: Vec<PdnConn>,
    /// Two-point wire segments, width-expanded; `width == 0` never lands here.
    pub wire_rects: Vec<(LayerId, RectDbu)>,
    pub vias: Vec<PdnVia>,
}

pub enum PdnUse { Power, Ground, Signal, Clock, Analog, Reset, Scan, TieOff, Ring }

pub enum PdnConn {
    /// `( PIN <name> )` — top-level bterm; the net's voltage source.
    TopPin(SymbolId),
    /// `( * <pin> )` — every component's pin named `<pin>`.
    AllInstances(SymbolId),
    /// `( <inst> <pin> )` — a single instance's pin.
    Instance { inst: SymbolId, pin: SymbolId },
}

pub struct PdnVia {
    /// DEF-spelled via name (kept for diagnostics).
    pub name: SymbolId,
    /// Endpoints of the stack link.
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut layer; resistance via `tech_db.cut_layer_by_name`.
    pub cut: SymbolId,
    /// Parallel cuts (`+ ROWCOL` product or LEF cut-RECT count).
    pub cut_count: u32,
    pub x_dbu: i32,
    pub y_dbu: i32,
}
```

All types derive `Serialize`/`Deserialize`/`Debug`/`Clone`; `pdn_nets`
carries `#[serde(default)]`.

`DesignDb::special_wire_rects` (the `Vec` field) is **removed** and
replaced by:

```rust
impl DesignDb {
    pub fn special_wire_rects(&self) -> impl Iterator<Item = (LayerId, RectDbu)> + '_ {
        self.pdn_nets.iter().flat_map(|n| n.wire_rects.iter().copied())
    }
}
```

Identical coverage — every two-point segment any special net declares
lands in `wire_rects`, so `congestion/gcell.rs` repoints unchanged
semantics-wise. The breaking signature change is the reason for the
minor version bump (see `steps.md`).

## Conn-group expansion (`pdn.rs`)

Deferred from parse → `pdn::build_graph` receives `&DesignDb` and
expands `PdnConn`s there, so SPECIALNETS/COMPONENTS/PINS section
ordering cannot matter. `DesignDb` is SoA — expansion walks existing
parallel arrays, no new maps are required:

- `TopPin(name)` → scan `pin_names` for `pin_cells[p] == None &&
  pin_names[p] == name` → `PinId`; `pin_io_rects[p]` (already
  absolute) supplies the source shapes. Note `io_pins` is **not** the
  index — invariant I5 keeps `+ SPECIAL` bterms out of it, so the
  scan is on `pin_cells`. A `( PIN x )` that misses → `bail!`.
- `AllInstances(pin)` → for each `CellId`, walk `cell_pins(c)` and
  keep pins where `tech_db.macro_pins[pin_macro_pins[p]].name ==
  pin`. A pin name no macro carries contributes nothing (wildcard —
  tolerated; the census reports `conn_pins` vs `attachable`).
- `Instance { inst, pin }` → `cell_names` scan → `cell_pins` → same
  macro-pin name match; unknown instance or pin → `bail!` (explicit
  conns that miss are real errors, unlike wildcards).

A `PinId` is **attachable** iff `MacroPin.rects` (routing layers
only) is non-empty. `VNB`/`VPB` are non-attachable by construction.

## Graph construction

Nodes = union components over same-layer wire rects, computed
**per layer** with a sorted sweep — not O(n²): sort rect ids by `xl`
(after normalizing the rect so `xl<xh`, `yl<yh`), walk in order
unioning every rect whose `xl < current.xh` (open interval) against an
active set. For gcd/picorv32 rails this is ~linear; worst case stays
quadratic but inputs don't provoke it. Reuses the same
`RectDbu::intersects` semantics as gcell (strict `<` overlap —
boundary-touching rects are **not** connected; chosen to match the
existing codebase's geometric convention, recorded here).

Via edges: each `PdnVia` connects the `lower`-layer component
containing `(x_dbu, y_dbu)` to the `upper`-layer component containing
the same point. A point landing outside every rect on a side creates
a **degenerate singleton node** for that end — it must not silently
attach to nothing: a dangling via IS an unconnected shape, so it
stays in the component count and surfaces in `all_shapes_connected`.

After unions, `PdnGraph` holds:

```rust
pub struct PdnGraph {
    /// Connected components; `rect_indices` indexes `PdnNet::wire_rects`.
    pub nodes: Vec<PdnNode>,
    /// (PinId, node) — the pin-attach table.
    pub attachments: Vec<(PinId, u32)>,
    /// Node indices reachable from the top-level PIN's io rects.
    pub source_nodes: Vec<u32>,
    pub conn_pins: Vec<PinId>,
    /// Attachable conn pins on no node or a non-source node.
    pub unreachable_pins: Vec<PinId>,
    /// Attachable conn pins touching no wire at all (⊆ unreachable).
    pub unattached_pins: Vec<PinId>,
}

pub struct PdnNode {
    pub layer: Option<LayerId>,      // `None` for degenerate via ends
    pub rect_indices: Vec<u32>,
    /// True when the node exists only as a via endpoint.
    pub via_end: bool,
}
```

## Attachment

Per attachable conn pin on a `Placed`/`Fixed` cell (`cell_status[c]`):
for each `MacroPin.rects` entry (macro coords) compute
`orient_rect(r, mac.w, mac.h, cell_orient[c])` then translate by
`(cell_x_dbu[c], cell_y_dbu[c])` → absolute rect; intersect with
`PdnNet.wire_rects` on the same layer → attach `(pin, node)` per hit.

Transform: `design_db.rs` already has the *point* form inside
`pin_pos_dbu` (scalar + frac flags for OpenROAD truncation — a
different job); the *rect* form lives in `gcell.rs` lines ~202–215
and is lifted verbatim into `DesignDb::orient_rect` so pdn.rs and
gcell.rs share one implementation. **Orientation coverage claim is
bounded to what inputs exercise** — gcd uses N/S/FN/FS; the
`gcd.orient.def` fixture exists for W/E/FW/FE and the pin-attach test
runs both (`tests.md`). Cells still `Unplaced` contribute as
non-attachable pins (surfaced in the census, never a crash).

Source marking: each `source_pin`'s `pin_io_rects` (already absolute,
already stored) intersect same-layer wire rects → those components
become `source_nodes`. A PIN with no io rects or no intersection → no
source nodes → every attachable pin is unreachable (loud failure, not
silence).

`unreachable_pins` = attachable conn pins whose node set is empty or
entirely outside `source_nodes`' components. `all_shapes_connected`
= `nodes.len() == 1` — mirrors OpenDB's `check_power_grid` shape-level
verdict. The acceptance's pin-level check is the stronger condition
and what tests assert; `all_shapes_connected` is the diagnostic that
maps 1:1 onto the reference's `connectivity_result`.

## Census

```rust
pub struct PdnCensus {
    pub net: SymbolId,
    pub use_kind: Option<PdnUse>,
    pub wire_rects: u32,
    pub vias: u32,
    pub nodes: u32,
    pub source_nodes: u32,
    pub conn_pins: u32,
    /// Conn pins with ≥1 routing-layer rect.
    pub attachable_pins: u32,
    /// Distinct attachable pins with ≥1 wire intersection.
    pub attached_terminals: u32,
    pub unattached_pins: u32,
    pub unreachable_pins: u32,
    pub all_shapes_connected: bool,
}
```

`attached_terminals` counts **distinct pins**, matching one CSV row
per `(instance, terminal)`. Predicted values (implementation
verifies, `docs/results.md` records actuals): gcd per net
`wire_rects=47, vias=46, nodes=1, attached=384`; picorv32 per net
`wire_rects=615, vias=792, nodes=1, attached=10205`.

## Files

- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/pdn.rs` (new)
- `crates/grus-core/src/congestion/gcell.rs`
- `crates/grus-core/src/lib.rs`
