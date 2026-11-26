//! Sparse DC solve of the PDN mesh (E8 ticket 4).
//!
//! `PdnMesh` supplies the resistor network, the sink set, and the source
//! nodes. Source nodes are Dirichlet clamps (assigned, never solved);
//! every other node is an unknown of the reduced conductance system
//! `G_ff · V_F = G_fs · V_s + I_signed`. Sink currents are the N7
//! per-instance power divided by the rail-to-rail scale — `POWER`
//! subtracts them (drop below the clamp), `GROUND` adds them (bounce
//! above it). The reduced matrix is a grounded graph Laplacian, SPD when
//! every free node reaches a source, so a direct `nalgebra-sparse`
//! `CscCholesky` solves it without tolerance tuning. Voltages, drops and
//! residuals are `f64`: drops are ~1e-4 V on a 1.8 V clamp, too close to
//! `f32` roundoff for the six-significant-digit target.

use super::{PdnMesh, PdnNet};
use crate::design_db::{DesignDb, PdnUse};
use crate::interner::PinId;
use crate::tech_db::TechDb;
use anyhow::{Context, Result, ensure};
use nalgebra_sparse::factorization::CscCholesky;
use nalgebra_sparse::na::DMatrixView;
use nalgebra_sparse::{CooMatrix, CscMatrix};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;

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

/// Convert per-cell watts to per-sink amperes (finite; a negative
/// Liberty `internal_power` total passes through as a signed current).
/// Splits a cell's total evenly across its sinks on this mesh.
pub fn sink_currents_a(
    db: &DesignDb,
    mesh: &PdnMesh,
    cell_power_w: &[f64],
    current_scale_v: f64,
) -> Result<Vec<f64>> {
    ensure!(
        cell_power_w.len() == db.num_cells(),
        "cell_power_w has {} entries for {} design cells",
        cell_power_w.len(),
        db.num_cells()
    );
    // Finite-only, not non-negative: sky130 Liberty `internal_power`
    // tables contain negative energies, so a legitimate per-instance
    // total can go slightly negative (measured on picorv32 `o211a_2`).
    // The signed current is injected as-is — the same feed an OpenSTA
    // `report_power -instances` dump would produce.
    ensure!(
        cell_power_w.iter().all(|p| p.is_finite()),
        "instance powers must be finite"
    );
    ensure!(
        current_scale_v.is_finite() && current_scale_v > 0.0,
        "current_scale_v {} must be finite and positive",
        current_scale_v
    );
    let mut counts = vec![0u64; db.num_cells()];
    for sink in &mesh.sinks {
        let cell = db.pin_cells[sink.pin.0 as usize].with_context(|| {
            format!("PDN sink pin {} has no owning cell", sink.pin.0)
        })?;
        counts[cell.0 as usize] += 1;
    }
    Ok(mesh
        .sinks
        .iter()
        .map(|sink| {
            let cell = db.pin_cells[sink.pin.0 as usize].unwrap().0 as usize;
            cell_power_w[cell] / (current_scale_v * counts[cell] as f64)
        })
        .collect())
}

/// Solve with explicit per-sink current magnitudes. `sink_currents[i]`
/// applies to `mesh.sinks[i]`; this is the unit-test boundary used by the
/// zero-power and analytic tests.
pub fn solve_with_sink_currents(
    mesh: &PdnMesh,
    use_kind: PdnUse,
    options: &SolveOptions,
    sink_currents: &[f64],
    net_label: &str,
) -> Result<PdnSolve> {
    solve_inner(
        mesh,
        use_kind,
        options,
        sink_currents,
        net_label.to_string(),
    )
}

/// Estimate currents from `cell_power_w[CellId.0]`, then solve.
/// `use_kind` comes from the owning `PdnNet`; callers validate
/// `mesh.net == net.name` before calling or use `solve_pdn_net`.
pub fn solve_mesh(
    db: &DesignDb,
    mesh: &PdnMesh,
    use_kind: PdnUse,
    options: &SolveOptions,
    cell_power_w: &[f64],
) -> Result<PdnSolve> {
    let currents = sink_currents_a(db, mesh, cell_power_w, options.current_scale_v)?;
    let net_name = db.strings.resolve(mesh.net).to_string();
    let mut solve = solve_inner(mesh, use_kind, options, &currents, net_name)?;
    let total_power: f64 = cell_power_w.iter().sum();
    solve.summary.uninjected_power_w = total_power - solve.summary.injected_power_w;
    Ok(solve)
}

/// Convenience wrapper tying the mesh to its net and checking the name.
pub fn solve_pdn_net(
    db: &DesignDb,
    net: &PdnNet,
    mesh: &PdnMesh,
    options: &SolveOptions,
    cell_power_w: &[f64],
) -> Result<PdnSolve> {
    ensure!(
        mesh.net == net.name,
        "mesh net {:?} does not match PdnNet {:?}",
        mesh.net,
        net.name
    );
    let use_kind = net
        .use_kind
        .with_context(|| "PDN net carries no + USE kind")?;
    solve_mesh(db, mesh, use_kind, options, cell_power_w)
}

/// One row per `mesh.sinks`, in exactly that order. `x_um/y_um` are
/// `DesignDb::pin_pos_dbu / dbu_per_micron`.
pub fn voltage_rows(
    db: &DesignDb,
    tech_db: &TechDb,
    mesh: &PdnMesh,
    solve: &PdnSolve,
) -> Result<Vec<VoltageRow>> {
    let first = tech_db
        .routing_layers
        .first()
        .context("tech DB has no routing layers")?;
    let layer = tech_db.strings.resolve(first.name).to_string();
    let dbu = f64::from(tech_db.dbu_per_micron.max(1));
    let mut rows = Vec::with_capacity(mesh.sinks.len());
    for sink in &mesh.sinks {
        let pin = sink.pin;
        let cell = db.pin_cells[pin.0 as usize].with_context(|| {
            format!("PDN sink pin {} has no owning cell", pin.0)
        })?;
        let (x, y) = db.pin_pos_dbu(pin, tech_db);
        rows.push(VoltageRow {
            instance: db.strings.resolve(db.cell_names[cell.0 as usize]).to_string(),
            terminal: db.strings.resolve(db.pin_names[pin.0 as usize]).to_string(),
            layer: layer.clone(),
            x_um: f64::from(x) / dbu,
            y_um: f64::from(y) / dbu,
            voltage_v: solve.node_voltages_v[sink.node as usize],
            pin,
            node: sink.node,
        });
    }
    Ok(rows)
}

/// Reference-format CSV: header + six fields per row.
pub fn write_voltage_csv<W: Write>(rows: &[VoltageRow], out: &mut W) -> Result<()> {
    writeln!(out, "Instance,Terminal,Layer,X location,Y location,Voltage")?;
    for r in rows {
        writeln!(
            out,
            "{},{},{},{:.4},{:.4},{:.6}",
            csv_field(&r.instance),
            csv_field(&r.terminal),
            csv_field(&r.layer),
            unzero(r.x_um),
            unzero(r.y_um),
            unzero(r.voltage_v),
        )?;
    }
    Ok(())
}

/// `-0.000000` would print for a negative zero; collapse it.
fn unzero(v: f64) -> f64 {
    if v == 0.0 { 0.0 } else { v }
}

/// RFC-style double-quote escaping, applied only when needed.
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn solve_inner(
    mesh: &PdnMesh,
    use_kind: PdnUse,
    options: &SolveOptions,
    sink_currents: &[f64],
    net_name: String,
) -> Result<PdnSolve> {
    ensure!(
        matches!(use_kind, PdnUse::Power | PdnUse::Ground),
        "PDN solve supports + USE POWER/GROUND nets only, got {use_kind:?}"
    );
    ensure!(
        options.source_voltage_v.is_finite(),
        "source_voltage_v {} is not finite",
        options.source_voltage_v
    );
    ensure!(
        options.current_scale_v.is_finite() && options.current_scale_v > 0.0,
        "current_scale_v {} must be finite and positive",
        options.current_scale_v
    );
    ensure!(
        sink_currents.len() == mesh.sinks.len(),
        "sink_currents has {} entries for {} sinks",
        sink_currents.len(),
        mesh.sinks.len()
    );
    ensure!(
        sink_currents.iter().all(|i| i.is_finite()),
        "sink currents must be finite"
    );
    let violations = mesh.validate();
    ensure!(
        violations.is_empty(),
        "PDN mesh validation failed: {violations:?}"
    );

    let n = mesh.nodes.len();
    let vs = options.source_voltage_v;
    let scale = options.current_scale_v;

    // Free-node indexing: source nodes are eliminated Dirichlet clamps.
    let mut free_of = vec![u32::MAX; n];
    let mut nf = 0usize;
    for i in 0..n {
        if !mesh.nodes[i].is_source {
            free_of[i] = nf as u32;
            nf += 1;
        }
    }

    // Conductance stamps; parallel resistors accumulate naturally.
    let mut accum: BTreeMap<(u32, u32), f64> = BTreeMap::new();
    let mut rhs = vec![0.0f64; nf];
    for r in &mesh.resistors {
        if r.a == r.b {
            continue;
        }
        let g = 1.0 / f64::from(r.r_ohms);
        let (fa, fb) = (free_of[r.a as usize], free_of[r.b as usize]);
        match (fa != u32::MAX, fb != u32::MAX) {
            (true, true) => {
                *accum.entry((fa, fa)).or_default() += g;
                *accum.entry((fb, fb)).or_default() += g;
                *accum.entry((fa, fb)).or_default() -= g;
                *accum.entry((fb, fa)).or_default() -= g;
            }
            (true, false) => {
                *accum.entry((fa, fa)).or_default() += g;
                rhs[fa as usize] += g * vs;
            }
            (false, true) => {
                *accum.entry((fb, fb)).or_default() += g;
                rhs[fb as usize] += g * vs;
            }
            (false, false) => {}
        }
    }

    // Signed sink terms: POWER pulls current out of the rail (drop),
    // GROUND returns it (bounce). A sink on a clamped node loads the
    // source directly — counted in the total, nothing stamped.
    let sign = match use_kind {
        PdnUse::Power => -1.0,
        _ => 1.0,
    };
    let mut total_current_a = 0.0;
    for (sink, &i) in mesh.sinks.iter().zip(sink_currents) {
        total_current_a += i;
        let f = free_of[sink.node as usize];
        if f != u32::MAX {
            rhs[f as usize] += sign * i;
        }
    }

    let mut x = vec![0.0f64; nf];
    if nf > 0 {
        let mut coo = CooMatrix::new(nf, nf);
        for (&(row, col), &v) in &accum {
            coo.push(row as usize, col as usize, v);
        }
        let csc = CscMatrix::from(&coo);
        let chol = CscCholesky::factor(&csc)
            .map_err(|e| anyhow::anyhow!("PDN Cholesky factorization failed: {e:?}"))?;
        let solved = chol.solve(DMatrixView::from_slice(&rhs, nf, 1));
        x.copy_from_slice(solved.as_slice());
    }

    // Residual on the reduced system: ||Ax - b||∞ and its normalized
    // form ||A||∞||x||∞ + ||b||∞ in the denominator.
    let mut ax = vec![0.0f64; nf];
    let mut a_rowsum = vec![0.0f64; nf];
    for (&(row, col), &v) in &accum {
        ax[row as usize] += v * x[col as usize];
        a_rowsum[row as usize] += v.abs();
    }
    let mut residual_inf = 0.0f64;
    let mut b_norm = 0.0f64;
    let mut x_norm = 0.0f64;
    for i in 0..nf {
        residual_inf = residual_inf.max((ax[i] - rhs[i]).abs());
        b_norm = b_norm.max(rhs[i].abs());
        x_norm = x_norm.max(x[i].abs());
    }
    let a_norm = a_rowsum.iter().copied().fold(0.0f64, f64::max);
    let denom = a_norm * x_norm + b_norm;
    let residual_rel_inf = if denom > 0.0 {
        residual_inf / denom
    } else {
        0.0
    };

    let mut node_voltages_v = vec![vs; n];
    for (i, &f) in free_of.iter().enumerate() {
        if f != u32::MAX {
            node_voltages_v[i] = x[f as usize];
        }
    }
    let sink_voltages_v: Vec<f64> = mesh
        .sinks
        .iter()
        .map(|s| node_voltages_v[s.node as usize])
        .collect();

    let drop_semantics = match use_kind {
        PdnUse::Power => "drop",
        _ => "bounce",
    };
    let (average_voltage_v, worstcase_voltage_v, average_drop_v, worst_drop_v) =
        if sink_voltages_v.is_empty() {
            (vs, vs, 0.0, 0.0)
        } else {
            let mut dev_sum = 0.0;
            let mut dev_max = f64::NEG_INFINITY;
            let mut v_sum = 0.0;
            for &v in &sink_voltages_v {
                let dev = match use_kind {
                    PdnUse::Power => vs - v,
                    _ => v - vs,
                };
                dev_sum += dev;
                dev_max = dev_max.max(dev);
                v_sum += v;
            }
            let worst_v = match use_kind {
                PdnUse::Power => sink_voltages_v.iter().copied().fold(f64::INFINITY, f64::min),
                _ => sink_voltages_v
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max),
            };
            let count = sink_voltages_v.len() as f64;
            (
                v_sum / count,
                worst_v,
                dev_sum / count,
                dev_max.max(0.0),
            )
        };
    let max_source_error_v = mesh
        .sources
        .iter()
        .map(|&s| (node_voltages_v[s as usize] - vs).abs())
        .fold(0.0f64, f64::max);

    let summary = SolveSummary {
        net: net_name.clone(),
        use_kind,
        nodes: n as u32,
        source_nodes: mesh.sources.len() as u32,
        resistors: mesh.resistors.len() as u32,
        sinks: mesh.sinks.len() as u32,
        source_voltage_v: vs,
        current_scale_v: scale,
        injected_power_w: total_current_a * scale,
        uninjected_power_w: 0.0,
        total_current_a,
        average_voltage_v,
        worstcase_voltage_v,
        average_drop_v,
        worst_drop_v,
        drop_percent: 100.0 * worst_drop_v / scale,
        drop_semantics: drop_semantics.to_string(),
        solver_method: "nalgebra-sparse CscCholesky".to_string(),
        iterations: 0,
        residual_inf,
        residual_rel_inf,
        max_source_error_v,
    };
    Ok(PdnSolve {
        net: net_name,
        use_kind,
        node_voltages_v,
        sink_voltages_v,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::pdn_design;
    use super::super::{MeshNode, MeshResistor, MeshSink, ResistorKind, build_mesh};
    use super::*;
    use crate::interner::{LayerId, SymbolId};
    use crate::test_support::{design_dir, require, sky130_hd};
    use std::path::Path;

    /// source node 0 —R1→ node 1 —R2→ node 2, one sink at node 2.
    fn analytic_mesh() -> PdnMesh {
        PdnMesh {
            net: SymbolId(0),
            nodes: vec![
                MeshNode {
                    layer: LayerId(0),
                    rects: vec![],
                    is_source: true,
                    is_sink: false,
                    dangling: false,
                },
                MeshNode {
                    layer: LayerId(0),
                    rects: vec![],
                    is_source: false,
                    is_sink: false,
                    dangling: false,
                },
                MeshNode {
                    layer: LayerId(0),
                    rects: vec![],
                    is_source: false,
                    is_sink: true,
                    dangling: false,
                },
            ],
            resistors: vec![
                MeshResistor {
                    a: 0,
                    b: 1,
                    r_ohms: 2.0,
                    kind: ResistorKind::Wire { rect: 0 },
                },
                MeshResistor {
                    a: 1,
                    b: 2,
                    r_ohms: 3.0,
                    kind: ResistorKind::Wire { rect: 0 },
                },
            ],
            sinks: vec![MeshSink {
                pin: PinId(0),
                node: 2,
            }],
            sources: vec![0],
            multi_node_sinks: 0,
            self_loop_resistors: 0,
        }
    }

    #[test]
    fn test_analytic_vpwr_drop() {
        let mesh = analytic_mesh();
        let options = SolveOptions {
            source_voltage_v: 1.8,
            current_scale_v: 1.8,
        };
        let solve =
            solve_with_sink_currents(&mesh, PdnUse::Power, &options, &[0.01], "net").unwrap();
        // V_leaf = 1.8 - I*(R1+R2) = 1.8 - 0.05
        assert!((solve.node_voltages_v[2] - 1.75).abs() <= 1e-12);
        assert!((solve.sink_voltages_v[0] - 1.75).abs() <= 1e-12);
        let s = &solve.summary;
        assert!((s.worst_drop_v - 0.05).abs() <= 1e-12);
        assert!((s.average_drop_v - 0.05).abs() <= 1e-12);
        assert_eq!(s.drop_semantics, "drop");
        assert!((s.total_current_a - 0.01).abs() <= 1e-18);
    }

    #[test]
    fn test_analytic_vgnd_bounce() {
        let mesh = analytic_mesh();
        let options = SolveOptions {
            source_voltage_v: 0.0,
            current_scale_v: 1.8,
        };
        let solve =
            solve_with_sink_currents(&mesh, PdnUse::Ground, &options, &[0.01], "net").unwrap();
        // V_leaf = I*(R1+R2) = 0.05 above the 0 V clamp.
        assert!((solve.node_voltages_v[2] - 0.05).abs() <= 1e-12);
        let s = &solve.summary;
        assert!((s.worst_drop_v - 0.05).abs() <= 1e-12);
        assert_eq!(s.drop_semantics, "bounce");
        assert!((s.worstcase_voltage_v - 0.05).abs() <= 1e-12);
    }

    #[test]
    fn test_source_clamp_exact_on_fixture_mesh() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        let options = SolveOptions {
            source_voltage_v: 1.8,
            current_scale_v: 1.8,
        };
        let currents = vec![0.001; mesh.sinks.len()];
        let solve =
            solve_with_sink_currents(&mesh, PdnUse::Power, &options, &currents, "net").unwrap();
        for &s in &mesh.sources {
            assert_eq!(solve.node_voltages_v[s as usize], 1.8);
        }
        assert_eq!(solve.summary.max_source_error_v, 0.0);
    }

    /// Acceptance sanity: all-zero currents leave the map uniformly at
    /// the clamp, both rails.
    #[test]
    fn test_zero_power_uniform_map() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        for (use_kind, vs) in [(PdnUse::Power, 1.8), (PdnUse::Ground, 0.0)] {
            let options = SolveOptions {
                source_voltage_v: vs,
                current_scale_v: 1.8,
            };
            let currents = vec![0.0; mesh.sinks.len()];
            let solve =
                solve_with_sink_currents(&mesh, use_kind, &options, &currents, "net")
                    .unwrap();
            // The exact answer is `vs` on every free node; the Cholesky
            // factor/solve returns it plus rounding noise (measured
            // ≤ 7.1e-15 V). The bound is far below the six-significant-
            // digit target on a 1.8 V clamp.
            let max_dev = solve
                .node_voltages_v
                .iter()
                .map(|&v| (v - vs).abs())
                .fold(0.0f64, f64::max);
            assert!(
                max_dev <= 1e-12,
                "{use_kind:?} node voltages not uniform (max dev {max_dev:e})"
            );
            assert!(
                solve
                    .sink_voltages_v
                    .iter()
                    .all(|&v| (v - vs).abs() <= 1e-12)
            );
            assert!(solve.summary.average_drop_v.abs() <= 1e-12);
            assert!(solve.summary.worst_drop_v <= 1e-12);
            assert_eq!(solve.summary.total_current_a, 0.0);
            assert!(solve.summary.residual_inf <= 1e-12);
        }
    }

    /// Two parallel R's between the same nodes must equal one R/2.
    #[test]
    fn test_parallel_resistor_stamping() {
        let mut mesh = analytic_mesh();
        mesh.resistors.pop();
        mesh.sinks[0].node = 1;
        mesh.nodes[1].is_sink = true;
        mesh.nodes[2].is_sink = false;
        mesh.nodes.pop();
        let single = solve_with_sink_currents(
            &mesh,
            PdnUse::Power,
            &SolveOptions {
                source_voltage_v: 1.8,
                current_scale_v: 1.8,
            },
            &[0.01],
            "net",
        )
        .unwrap();
        // Add a second, parallel 2 Ω resistor: equivalent 1 Ω.
        mesh.resistors.push(MeshResistor {
            a: 0,
            b: 1,
            r_ohms: 2.0,
            kind: ResistorKind::Wire { rect: 0 },
        });
        let parallel = solve_with_sink_currents(
            &mesh,
            PdnUse::Power,
            &SolveOptions {
                source_voltage_v: 1.8,
                current_scale_v: 1.8,
            },
            &[0.01],
            "net",
        )
        .unwrap();
        // Single R=2: V = 1.8 - 0.02; parallel pair R=1: V = 1.8 - 0.01.
        assert!((single.node_voltages_v[1] - 1.78).abs() <= 1e-12);
        assert!((parallel.node_voltages_v[1] - 1.79).abs() <= 1e-12);
    }

    #[test]
    fn test_input_validation_errors() {
        let mesh = analytic_mesh();
        let options = SolveOptions {
            source_voltage_v: 1.8,
            current_scale_v: 1.8,
        };
        assert!(solve_with_sink_currents(&mesh, PdnUse::Power, &options, &[0.1, 0.2], "net").is_err());
        // Negative currents are legal (negative Liberty internal-power
        // energies feed through as signed current sources).
        assert!(solve_with_sink_currents(&mesh, PdnUse::Power, &options, &[-0.1], "net").is_ok());
        assert!(solve_with_sink_currents(&mesh, PdnUse::Power, &options, &[f64::NAN], "net").is_err());
        assert!(solve_with_sink_currents(&mesh, PdnUse::Power, &options, &[f64::INFINITY], "net").is_err());
        assert!(
            solve_with_sink_currents(
                &mesh,
                PdnUse::Power,
                &SolveOptions {
                    source_voltage_v: f64::NAN,
                    current_scale_v: 1.8
                },
                &[0.1],
                "net",
            )
            .is_err()
        );
        assert!(
            solve_with_sink_currents(
                &mesh,
                PdnUse::Power,
                &SolveOptions {
                    source_voltage_v: 1.8,
                    current_scale_v: 0.0
                },
                &[0.1],
                "net",
            )
            .is_err()
        );
        // Non POWER/GROUND use kinds are rejected.
        assert!(
            solve_with_sink_currents(&mesh, PdnUse::Signal, &options, &[0.1], "net").is_err()
        );
        // No sources at all: validate() reports NoSources.
        let mut no_src = analytic_mesh();
        no_src.sources.clear();
        no_src.nodes[0].is_source = false;
        assert!(solve_with_sink_currents(&no_src, PdnUse::Power, &options, &[0.1], "net").is_err());
        // A dangling free node is caught by validate() before factoring.
        let mut dangling = analytic_mesh();
        dangling.nodes.push(MeshNode {
            layer: LayerId(0),
            rects: vec![],
            is_source: false,
            is_sink: false,
            dangling: false,
        });
        assert!(
            solve_with_sink_currents(&dangling, PdnUse::Power, &options, &[0.1], "net").is_err()
        );
    }

    #[test]
    fn test_sink_currents_input_validation() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        assert!(sink_currents_a(&db, &mesh, &[1.0], 1.8).is_err());
        assert!(sink_currents_a(&db, &mesh, &[f64::NAN, 0.0], 1.8).is_err());
        assert!(sink_currents_a(&db, &mesh, &[1.0, 0.0], 0.0).is_err());
        let currents = sink_currents_a(&db, &mesh, &[1.8, 0.9], 1.8).unwrap();
        assert_eq!(currents.len(), mesh.sinks.len());
        // u1 and u2 each own one VPWR sink: I = P/1.8 per sink.
        let total: f64 = currents.iter().sum();
        assert!((total - 1.5).abs() <= 1e-12);
    }

    /// Synthetic mesh solve end to end: `solve_mesh` on `pdn_design`,
    /// distinct powers for u1/u2, both rails.
    #[test]
    fn test_synthetic_solve_mesh_both_rails() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        assert_eq!(mesh.sinks.len(), 2);
        for (use_kind, vs) in [(PdnUse::Power, 1.8), (PdnUse::Ground, 0.0)] {
            let options = SolveOptions {
                source_voltage_v: vs,
                current_scale_v: 1.8,
            };
            let solve = solve_mesh(&db, &mesh, use_kind, &options, &[1.8, 0.9]).unwrap();
            let s = &solve.summary;
            for &v in &solve.node_voltages_v {
                match use_kind {
                    PdnUse::Power => assert!(v <= vs + 1e-9),
                    _ => assert!(v >= vs - 1e-9),
                }
            }
            assert!(s.worst_drop_v >= s.average_drop_v);
            assert!(s.average_drop_v > 0.0);
            // u1: 1.8/1.8 = 1.0 A; u2: 0.9/1.8 = 0.5 A.
            assert!((s.total_current_a - 1.5).abs() <= 1e-12);
            assert!(s.residual_rel_inf <= 1e-10, "rel residual {}", s.residual_rel_inf);
            // Both fixture cells attach: nothing uninjected.
            assert_eq!(s.uninjected_power_w, 0.0);
        }
    }

    #[test]
    fn test_deterministic_rows_and_csv() {
        let (tech, db) = pdn_design();
        let net = db.pdn_net("VPWR").unwrap();
        let mesh = build_mesh(&db, &tech, net).unwrap();
        let options = SolveOptions {
            source_voltage_v: 1.8,
            current_scale_v: 1.8,
        };
        let solve = solve_mesh(&db, &mesh, PdnUse::Power, &options, &[1.8, 0.9]).unwrap();
        let r1 = voltage_rows(&db, &tech, &mesh, &solve).unwrap();
        let r2 = voltage_rows(&db, &tech, &mesh, &solve).unwrap();
        assert_eq!(r1.len(), r2.len());
        for (a, b) in r1.iter().zip(&r2) {
            assert_eq!(
                (&a.instance, &a.terminal, &a.layer, a.x_um, a.y_um, a.node),
                (&b.instance, &b.terminal, &b.layer, b.x_um, b.y_um, b.node)
            );
        }
        let mut c1 = Vec::new();
        let mut c2 = Vec::new();
        write_voltage_csv(&r1, &mut c1).unwrap();
        write_voltage_csv(&r1, &mut c2).unwrap();
        assert_eq!(c1, c2);
        let text = String::from_utf8(c1).unwrap();
        assert_eq!(
            text.lines().next().unwrap(),
            "Instance,Terminal,Layer,X location,Y location,Voltage"
        );
        assert_eq!(text.lines().count(), mesh.sinks.len() + 1);
    }

    /// Narrow escaping claim: a comma in a field name gets RFC quoting.
    /// Committed fixtures never exercise it, so only this narrow case is
    /// asserted.
    #[test]
    fn test_csv_escaping() {
        let rows = vec![VoltageRow {
            instance: "u,1".to_string(),
            terminal: "VPWR".to_string(),
            layer: "li1".to_string(),
            x_um: 1.0,
            y_um: 2.0,
            voltage_v: 1.8,
            pin: PinId(0),
            node: 0,
        }];
        let mut out = Vec::new();
        write_voltage_csv(&rows, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text.lines().nth(1).unwrap(),
            "\"u,1\",VPWR,li1,1.0000,2.0000,1.800000"
        );
    }

    /// Compare our rows' first five fields against a golden CSV, per
    /// element with the first mismatch's index reported.
    fn compare_golden_first_five(csv_path: &Path, rows: &[VoltageRow]) {
        let text = std::fs::read_to_string(csv_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", csv_path.display()));
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "Instance,Terminal,Layer,X location,Y location,Voltage"
        );
        let golden: Vec<&str> = lines.collect();
        assert_eq!(
            golden.len(),
            rows.len(),
            "row count vs {}",
            csv_path.display()
        );
        for (i, (gl, r)) in golden.iter().zip(rows.iter()).enumerate() {
            let f: Vec<&str> = gl.split(',').collect();
            assert_eq!(f.len(), 6, "golden row {i}");
            let ours = [
                r.instance.as_str(),
                r.terminal.as_str(),
                r.layer.as_str(),
            ];
            for k in 0..3 {
                assert_eq!(f[k], ours[k], "row {i} col {k}");
            }
            let xy = [
                format!("{:.4}", unzero(r.x_um)),
                format!("{:.4}", unzero(r.y_um)),
            ];
            for k in 0..2 {
                assert_eq!(f[3 + k], xy[k], "row {i} col {}", 3 + k);
            }
        }
    }

    struct LoadedDesign {
        tech: TechDb,
        db: DesignDb,
        report: crate::power::PowerReport,
        ref_json: serde_json::Value,
        dir: std::path::PathBuf,
    }

    /// Liberty + LEF + placed DEF + SDC + N7 estimate (default activity —
    /// the same input regime as `generate_irdrop_ref.sh`) + reference JSON.
    fn load_powered_design(design: &str, def_file: &str, sdc_file: &str) -> LoadedDesign {
        let hd = sky130_hd();
        let lib_path = require(hd.join("lib/sky130_fd_sc_hd__tt_025C_1v80.lib"));
        let tlef = require(hd.join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef = require(hd.join("lef/sky130_fd_sc_hd.lef"));
        let dir = design_dir(design);
        let def = require(dir.join(def_file));
        let sdc_path = require(dir.join(sdc_file));
        let ref_path = require(dir.join(format!("{design}.irdrop.reference.json")));

        let lib = crate::readers::load_liberty(&lib_path).expect("liberty");
        let mut interner = crate::interner::StringInterner::new();
        let (tech, _) = crate::readers::load_tech_db(&tlef, &lef, &mut interner).expect("lef");
        let db = crate::readers::parse_def(&def, &tech, Some(&lib)).expect("def");
        let sdc = crate::sta::load_sdc(&sdc_path).expect("sdc");
        let slews = crate::sta::sta_pin_slews(&db, &lib, &sdc, None).expect("slews");
        let report = crate::power::estimate_power(
            &db,
            &lib,
            &sdc,
            None,
            &crate::readers::VcdActivity::default(),
            &slews,
            &crate::activity::ActivityOverrides::default(),
        );
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("reference json"))
                .expect("reference json parse");
        LoadedDesign {
            tech,
            db,
            report,
            ref_json,
            dir,
        }
    }

    fn embedded<'a>(d: &'a LoadedDesign, net: &str) -> &'a serde_json::Value {
        &d.ref_json["nets"][net]["conventions"]["embedded"]
    }

    /// Shared contract for the committed designs: solve both rails with
    /// the N7 per-instance feed, assert the mesh/sink/clamp/residual
    /// contract, and compare the emitted rows' first five columns against
    /// the golden CSV verbatim. Returns nothing; prints per-net data for
    /// `docs/results.md`.
    fn solve_row_contract(d: &LoadedDesign) {
        let cell_power: Vec<f64> = d.report.instances.iter().map(|i| i.total_w).collect();
        let supply = |name: &str| {
            embedded(d, name)["supply_voltage_v"]
                .as_f64()
                .expect("supply_voltage_v")
        };
        let scale = (supply("VPWR") - supply("VGND")).abs();
        assert!(scale.is_finite() && scale > 0.0);
        for name in ["VPWR", "VGND"] {
            let net = d.db.pdn_net(name).expect(name);
            let mesh = build_mesh(&d.db, &d.tech, net).expect("mesh");
            let options = SolveOptions {
                source_voltage_v: supply(name),
                current_scale_v: scale,
            };
            let t0 = std::time::Instant::now();
            let solve =
                solve_pdn_net(&d.db, net, &mesh, &options, &cell_power).expect("solve");
            let elapsed = t0.elapsed();
            let s = &solve.summary;
            let emb = embedded(d, name);
            assert_eq!(
                s.sinks as u64,
                emb["csv_rows"].as_u64().unwrap(),
                "{name} sinks vs embedded csv_rows"
            );
            for &src in &mesh.sources {
                assert_eq!(solve.node_voltages_v[src as usize], supply(name));
            }
            assert_eq!(s.max_source_error_v, 0.0);
            assert!(
                s.residual_rel_inf <= 1e-10,
                "{name} residual_rel_inf {}",
                s.residual_rel_inf
            );
            assert!(s.total_current_a.is_finite());
            assert!(
                s.uninjected_power_w.abs() <= 1e-15,
                "{name} uninjected_power_w {}",
                s.uninjected_power_w
            );
            let rows = voltage_rows(&d.db, &d.tech, &mesh, &solve).expect("rows");
            let csv_name = emb["csv_file"].as_str().unwrap();
            compare_golden_first_five(&d.dir.join(csv_name), &rows);
            let ref_power = emb["total_power_w"].as_f64().unwrap();
            let ref_current = ref_power / scale;
            println!(
                "{name}: sinks={} current={:.6e} A ref_current={:.6e} A delta={:.3e} A \
                 avg_{}={:.6e} V worst={:.6e} V residual={:.3e} A rel={:.3e} \
                 est_power={:.6e} W ref_power={:.6e} W delta={:.3e} W elapsed={:.2?}",
                s.sinks,
                s.total_current_a,
                ref_current,
                s.total_current_a - ref_current,
                s.drop_semantics,
                s.average_drop_v,
                s.worst_drop_v,
                s.residual_inf,
                s.residual_rel_inf,
                s.injected_power_w,
                ref_power,
                s.injected_power_w - ref_power,
                elapsed,
            );
        }
    }

    #[test]
    fn test_gcd_solve_embedded_row_contract() {
        let d = load_powered_design("gcd", "gcd.placed.def", "gcd.sdc");
        solve_row_contract(&d);
    }

    #[test]
    fn test_picorv32_solve_embedded_row_contract() {
        let d = load_powered_design("picorv32", "picorv32.placed.def", "picorv32.sdc");
        // Negative Liberty `internal_power` energies make 7 `o211a_2`
        // instance totals slightly negative (~-2.4e-11 W); the feed is
        // signed, not clamped — recorded in docs/results.md.
        let negative = d
            .report
            .instances
            .iter()
            .filter(|ip| ip.total_w < 0.0)
            .count();
        println!("picorv32 negative-power instances: {negative}");
        solve_row_contract(&d);
    }
}
