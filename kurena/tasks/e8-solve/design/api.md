# e8-solve — API surface

New code lives in `crates/grus-core/src/pdn/solve.rs`, a child module of
`pdn.rs`. `pdn.rs` gains `pub mod solve;` and `pub use solve::*;`, so
`crate::pdn`/`crate::*` expose the API in the same flat style as mesh.
All report rows and summaries are `Clone + Debug + Serialize`; internal
assembly stays private.

## Per-instance power extension

`power.rs` gains one public record and one `PowerReport` field:

```rust
/// Total power attribution for one design cell.
#[derive(Clone, Debug, Default, Serialize)]
pub struct InstancePower {
    /// DEF component name, resolved at estimate time.
    pub instance: String,
    pub internal_w: f64,
    pub switching_w: f64,
    pub leakage_w: f64,
    pub total_w: f64,
}

pub struct PowerReport {
    // existing fields ...
    /// Parallel to `DesignDb` cell order: `instances[cell.0]` is the
    /// power used for `CellId`'s sinks. Includes zero-valued entries for
    /// cells without a Liberty cell so indexing never shifts.
    pub instances: Vec<InstancePower>,
}
```

Implementation keeps the existing group arithmetic unchanged:

- cell loop accumulates leakage/internal into both `groups[g]` and
  `instances[cell]`;
- switching loop adds each driven net's switching power to the driver
  cell's `instances[cell]`;
- `total_w` fills after the three component values;
- aggregate `PowerReport` fields remain the group sums. A test asserts
  the per-instance sum equals the aggregate total within `1e-15 W`.

No second estimator, annotation path, or `set_pdnsim_inst_power` model is
introduced.

## Solve types

```rust
#[derive(Clone, Copy, Debug)]
pub struct SolveOptions {
    /// Exact voltage assigned to every mesh source node.
    pub source_voltage_v: f64,
    /// Positive rail-to-rail scale used by `I = P / V`.
    pub current_scale_v: f64,
}

#[derive(Clone, Debug)]
pub struct PdnSolve {
    /// Resolved net name for diagnostics (`db.strings.resolve`).
    pub net: String,
    pub use_kind: PdnUse,
    /// Dense, in `mesh.nodes` order; source entries are exact clamps.
    pub node_voltages_v: Vec<f64>,
    /// Parallel to `mesh.sinks`.
    pub sink_voltages_v: Vec<f64>,
    pub summary: SolveSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct SolveSummary {
    pub net: String,
    pub use_kind: PdnUse,
    pub nodes: u32,
    pub source_nodes: u32,
    pub resistors: u32,
    pub sinks: u32,
    pub source_voltage_v: f64,
    pub current_scale_v: f64,
    pub injected_power_w: f64,
    /// `solve_mesh` only; explicit-current solves report 0 because no
    /// cell-power input exists to compare against.
    pub uninjected_power_w: f64,
    pub total_current_a: f64,
    pub average_voltage_v: f64,
    pub worstcase_voltage_v: f64,
    pub average_drop_v: f64,
    pub worst_drop_v: f64,
    pub drop_percent: f64,
    /// "drop" for POWER, "bounce" for GROUND — golden wording is still
    /// `*_drop_v`, this field carries the physics.
    pub drop_semantics: String,
    /// "nalgebra-sparse CscCholesky" for the selected backend.
    pub solver_method: String,
    /// 0 for a direct solve; kept for a possible iterative backend.
    pub iterations: u32,
    /// ||A*x - b||∞ on the reduced free-node system, in amperes.
    pub residual_inf: f64,
    /// residual_inf / (||A||∞||x||∞ + ||b||∞).
    pub residual_rel_inf: f64,
    /// max |node_voltage[source] - source_voltage_v|, expected 0.
    pub max_source_error_v: f64,
}
```

`drop_percent` uses `current_scale_v`, not `source_voltage_v`, so VGND's
0 V clamp cannot divide by zero.

## Functions

```rust
/// Convert per-cell watts to positive per-sink amperes. Splits a cell's
/// total evenly across its sinks on this mesh.
pub fn sink_currents_a(
    db: &DesignDb,
    mesh: &PdnMesh,
    cell_power_w: &[f64],
    current_scale_v: f64,
) -> Result<Vec<f64>>;

/// Solve with explicit per-sink current magnitudes. `sink_currents_a[i]`
/// applies to `mesh.sinks[i]`; this is the unit-test boundary used by the
/// zero-power and analytic tests.
pub fn solve_with_sink_currents(
    mesh: &PdnMesh,
    use_kind: PdnUse,
    options: &SolveOptions,
    sink_currents: &[f64],
) -> Result<PdnSolve>;

/// Estimate currents from `cell_power_w[CellId.0]`, then solve.
/// `use_kind` comes from the owning `PdnNet`; callers validate
/// `mesh.net == net.name` before calling or use `solve_pdn_net`.
pub fn solve_mesh(
    db: &DesignDb,
    mesh: &PdnMesh,
    use_kind: PdnUse,
    options: &SolveOptions,
    cell_power_w: &[f64],
) -> Result<PdnSolve>;

/// Convenience wrapper tying the mesh to its net and checking the name.
pub fn solve_pdn_net(
    db: &DesignDb,
    net: &PdnNet,
    mesh: &PdnMesh,
    options: &SolveOptions,
    cell_power_w: &[f64],
) -> Result<PdnSolve>;
```

`SolveOptions` is deliberately small. There is no tolerance or max-iter
knob while the backend is direct; adding one now would imply a solver
contract the chosen method does not use.

## CSV rows

```rust
#[derive(Clone, Debug, Serialize)]
pub struct VoltageRow {
    pub instance: String,
    pub terminal: String,
    /// PDNSim map convention: first routing-layer name (sky130 `li1`),
    /// not necessarily `mesh.nodes[sink.node].layer`.
    pub layer: String,
    pub x_um: f64,
    pub y_um: f64,
    pub voltage_v: f64,
    /// Kept for tests/e2e joins without repeating name lookups.
    pub pin: PinId,
    pub node: u32,
}

/// One row per `mesh.sinks`, in exactly that order. `x_um/y_um` are
/// `DesignDb::pin_pos_dbu / dbu_per_micron`.
pub fn voltage_rows(
    db: &DesignDb,
    tech_db: &TechDb,
    mesh: &PdnMesh,
    solve: &PdnSolve,
) -> Result<Vec<VoltageRow>>;

/// Reference-format CSV: header + six fields per row.
pub fn write_voltage_csv<W: std::io::Write>(
    rows: &[VoltageRow],
    out: &mut W,
) -> Result<()>;
```

The first five fields are what the golden comparison checks. `pin` and
`node` are implementation metadata and are never serialized into the CSV
line.

## Persistent report types

```rust
#[derive(Clone, Debug, Serialize)]
pub struct IrDropReport {
    pub design: String,
    pub current_scale_v: f64,
    pub estimator_total_power_w: f64,
    pub nets: Vec<IrDropNetReport>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrDropNetReport {
    pub csv_file: String,
    pub csv_rows: u32,
    pub elapsed_ms: f64,
    pub summary: SolveSummary,
    pub reference: Option<IrDropReference>,
    pub deltas: Option<IrDropDeltas>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrDropReference {
    pub total_power_w: f64,
    pub supply_voltage_v: f64,
    pub average_voltage_v: f64,
    pub worstcase_voltage_v: f64,
    pub average_drop_v: f64,
    pub worst_drop_v: f64,
    pub total_current_a: f64,
    pub csv_rows: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct IrDropDeltas {
    /// ours - reference; nonzero is expected because N7 != OpenSTA.
    pub total_power_w: f64,
    pub total_current_a: f64,
    pub average_voltage_v: f64,
    pub worstcase_voltage_v: f64,
    pub average_drop_v: f64,
    pub worst_drop_v: f64,
}
```

These report types live in `grus-core` so the e2e ticket and tests share
one schema. The CLI fills reference/delta fields; the solver itself does
not parse golden JSON.

## `pdn.rs` mechanical diff

```rust
pub mod solve;
pub use solve::*;
```

No changes to `PdnMesh`, `PdnGraph`, `mesh.rs`, `design_db.rs`, or
`tech_db.rs` are planned. The mesh's existing `sinks`, `sources`,
`resistors`, `validate()`, and construction order are the solver
contract.

## Errors

Functions return `anyhow::Result`, matching `build_mesh`/readers. Error
messages name the net plus the offending node/pin/cell where available;
mesh validation errors include the full `MeshViolation` list in `Debug`.
The module does not define a second error enum unless clippy or call-site
matching makes it materially clearer.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/solve.rs`
- `crates/grus-core/src/power.rs`
- `crates/grus-eda/src/main.rs`
