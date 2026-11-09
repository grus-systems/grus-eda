use crate::design_db::DesignDb;
use crate::placement::density::DensitySolver;
use crate::placement::wirelength::{
    compute_hpwl_um, compute_wa_wirelength_and_grad, WaWirelengthPrecomp,
};
use crate::tech_db::TechDb;

#[inline]
pub fn l1_norm_f64(xs: &[f32], ys: &[f32]) -> f64 {
    xs.iter().chain(ys.iter()).map(|&v| (v as f64).abs()).sum()
}

#[inline]
pub fn l2_dist_f64(ax: &[f32], ay: &[f32], bx: &[f32], by: &[f32]) -> f32 {
    let sum_sq: f64 = ax
        .iter()
        .zip(bx.iter())
        .chain(ay.iter().zip(by.iter()))
        .map(|(&a, &b)| {
            let d = (a as f64) - (b as f64);
            d * d
        })
        .sum();
    sum_sq.sqrt() as f32
}

#[derive(Clone, Debug)]
pub struct NesterovConfig {
    pub target_density: f32,
    pub stop_overflow: f32,
    pub max_iters: usize,
    pub grid_m_override: Option<usize>,
}

impl Default for NesterovConfig {
    fn default() -> Self {
        Self {
            target_density: 0.60,
            stop_overflow: 0.10,
            max_iters: 300,
            grid_m_override: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NesterovResult {
    pub cell_x_um: Vec<f32>,
    pub cell_y_um: Vec<f32>,
    pub iterations: usize,
    pub initial_overflow: f32,
    pub final_overflow: f32,
    pub initial_hpwl_um: f64,
    pub final_hpwl_um: f64,
}

pub fn run_nesterov_placement(
    design_db: &DesignDb,
    tech_db: &TechDb,
    config: &NesterovConfig,
) -> NesterovResult {
    let dbu_per_um = design_db.dbu_per_micron.max(1) as f32;
    let wl_precomp = WaWirelengthPrecomp::new(design_db, tech_db);
    let mut density_solver = DensitySolver::new(
        design_db,
        tech_db,
        config.target_density,
        config.grid_m_override,
    );

    let num_cells = design_db.num_cells();
    let num_nets = design_db.num_nets();
    let num_pins = design_db.num_pins();

    let core_xl = density_solver.core_xl_um;
    let core_xh = density_solver.core_xh_um;
    let core_yl = density_solver.core_yl_um;
    let core_yh = density_solver.core_yh_um;
    let core_cx = 0.5 * (core_xl + core_xh);
    let core_cy = 0.5 * (core_yl + core_yh);

    let bin_w_um = density_solver.bin_w_um;
    let bin_h_um = density_solver.bin_h_um;
    let step_scale = bin_w_um + bin_h_um;

    // 1. Initial coordinates u_x, u_y
    let mut u_x = vec![0.0f32; num_cells];
    let mut u_y = vec![0.0f32; num_cells];
    for c in 0..num_cells {
        if !wl_precomp.movable_cell_mask[c] {
            u_x[c] = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
            u_y[c] = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
        } else {
            let w = density_solver.cell_w_um[c];
            let h = density_solver.cell_h_um[c];
            u_x[c] = core_cx - 0.5 * w;
            u_y[c] = core_cy - 0.5 * h;
        }
    }

    // 15 iterations of star-model barycenter relaxation
    for _ in 0..15 {
        let mut net_cx = vec![0.0f32; num_nets];
        let mut net_cy = vec![0.0f32; num_nets];
        for e in 0..num_nets {
            let start = design_db.net_pin_offsets[e] as usize;
            let end = design_db.net_pin_offsets[e + 1] as usize;
            let deg = end - start;
            if deg < 2 {
                continue;
            }
            let mut sx = 0.0f32;
            let mut sy = 0.0f32;
            for &p in &design_db.net_pin_ids[start..end] {
                let pid = p.0 as usize;
                if let Some(cell) = design_db.pin_cells[pid] {
                    let cid = cell.0 as usize;
                    sx += u_x[cid] + wl_precomp.pin_dx_um[pid];
                    sy += u_y[cid] + wl_precomp.pin_dy_um[pid];
                } else {
                    sx += wl_precomp.pin_dx_um[pid];
                    sy += wl_precomp.pin_dy_um[pid];
                }
            }
            let inv_deg = 1.0f32 / deg as f32;
            net_cx[e] = sx * inv_deg;
            net_cy[e] = sy * inv_deg;
        }

        for c in 0..num_cells {
            if !wl_precomp.movable_cell_mask[c] {
                continue;
            }
            let p_start = design_db.cell_pin_offsets[c] as usize;
            let p_end = design_db.cell_pin_offsets[c + 1] as usize;
            let mut sum_tx = 0.0f32;
            let mut sum_ty = 0.0f32;
            let mut sum_w = 0.0f32;

            for &p in &design_db.cell_pin_ids[p_start..p_end] {
                let pid = p.0 as usize;
                if let Some(net) = design_db.pin_nets[pid] {
                    let e = net.0 as usize;
                    let n_start = design_db.net_pin_offsets[e] as usize;
                    let n_end = design_db.net_pin_offsets[e + 1] as usize;
                    let deg = n_end - n_start;
                    if deg >= 2 {
                        let target_x = net_cx[e] - wl_precomp.pin_dx_um[pid];
                        let target_y = net_cy[e] - wl_precomp.pin_dy_um[pid];
                        sum_tx += target_x;
                        sum_ty += target_y;
                        sum_w += 1.0;
                    }
                }
            }

            if sum_w > 0.0 {
                let tx = sum_tx / sum_w;
                let ty = sum_ty / sum_w;
                let w_c = density_solver.cell_w_um[c];
                let h_c = density_solver.cell_h_um[c];
                u_x[c] = (0.5 * u_x[c] + 0.5 * tx).clamp(core_xl, core_xh - w_c);
                u_y[c] = (0.5 * u_y[c] + 0.5 * ty).clamp(core_yl, core_yh - h_c);
            }
        }
    }

    // Small deterministic perturbation to movable cells
    for c in 0..num_cells {
        if !wl_precomp.movable_cell_mask[c] {
            continue;
        }
        let dx = (((c * 73) % 29) as f32 - 14.0) * 0.005 * bin_w_um;
        let dy = (((c * 37) % 29) as f32 - 14.0) * 0.005 * bin_h_um;
        let w_c = density_solver.cell_w_um[c];
        let h_c = density_solver.cell_h_um[c];
        u_x[c] = (u_x[c] + dx).clamp(core_xl, core_xh - w_c);
        u_y[c] = (u_y[c] + dy).clamp(core_yl, core_yh - h_c);
    }

    let init_hpwl_um = compute_hpwl_um(design_db, &wl_precomp, &u_x, &u_y);
    let dhpwl_ref = (0.015 * init_hpwl_um).max(50.0);

    let mut v_x = u_x.clone();
    let mut v_y = u_y.clone();
    let mut t_k = 1.0f32;

    let base_gamma = 0.25 * (bin_w_um + bin_h_um);

    // Initial force and overflow evaluation
    let mut force_x = vec![0.0f32; num_cells];
    let mut force_y = vec![0.0f32; num_cells];
    let (init_tau, _) =
        density_solver.compute_density_and_forces(&v_x, &v_y, &mut force_x, &mut force_y);

    let gamma_0 = base_gamma * 10.0f32.powf(2.0 * (init_tau - 0.10));

    let mut dummy_pin_gx = vec![0.0f32; num_pins];
    let mut dummy_pin_gy = vec![0.0f32; num_pins];
    let mut wl_gx = vec![0.0f32; num_cells];
    let mut wl_gy = vec![0.0f32; num_cells];
    compute_wa_wirelength_and_grad(
        design_db,
        &wl_precomp,
        &v_x,
        &v_y,
        gamma_0,
        &mut dummy_pin_gx,
        &mut dummy_pin_gy,
        &mut wl_gx,
        &mut wl_gy,
    );

    let norm_wl = l1_norm_f64(&wl_gx, &wl_gy);
    let norm_force = l1_norm_f64(&force_x, &force_y);
    let mut lambda_k = (0.05 * (norm_wl / norm_force.max(1e-9))) as f32;

    // Precompute cell pin counts and areas for diagonal preconditioner
    let mut pin_cnt = vec![0.0f32; num_cells];
    let mut cell_area = vec![0.0f32; num_cells];
    for c in 0..num_cells {
        pin_cnt[c] =
            (design_db.cell_pin_offsets[c + 1] - design_db.cell_pin_offsets[c]) as f32;
        cell_area[c] = density_solver.cell_w_um[c] * density_solver.cell_h_um[c];
    }

    let mut prev_v_x = v_x.clone();
    let mut prev_v_y = v_y.clone();
    let mut prev_g_x = vec![0.0f32; num_cells];
    let mut prev_g_y = vec![0.0f32; num_cells];
    let mut hpwl_cur = init_hpwl_um;
    let mut final_overflow = init_tau;
    let mut final_iters = 0;

    for k in 0..config.max_iters {
        final_iters = k;
        if k > 0 {
            let (tau_k, _) =
                density_solver.compute_density_and_forces(&v_x, &v_y, &mut force_x, &mut force_y);
            final_overflow = tau_k;
            if tau_k <= config.stop_overflow {
                break;
            }
            let gamma_k = base_gamma * 10.0f32.powf(2.0 * (tau_k - 0.10));
            compute_wa_wirelength_and_grad(
                design_db,
                &wl_precomp,
                &v_x,
                &v_y,
                gamma_k,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut wl_gx,
                &mut wl_gy,
            );
        } else {
            final_overflow = init_tau;
            if init_tau <= config.stop_overflow {
                break;
            }
        }

        // Compute preconditioned gradient g_k
        let mut g_x = vec![0.0f32; num_cells];
        let mut g_y = vec![0.0f32; num_cells];
        let mut g_inf = 1e-6f32;

        for c in 0..num_cells {
            if !wl_precomp.movable_cell_mask[c] {
                continue;
            }
            let denom = (pin_cnt[c] + lambda_k * cell_area[c]).max(1.0);
            let gx = (wl_gx[c] - lambda_k * force_x[c]) / denom;
            let gy = (wl_gy[c] - lambda_k * force_y[c]) / denom;
            g_x[c] = gx;
            g_y[c] = gy;
            g_inf = g_inf.max(gx.abs()).max(gy.abs());
        }

        // Step size alpha_k
        let alpha_raw = if k == 0 {
            0.05 * step_scale / g_inf
        } else {
            let dist_v = l2_dist_f64(&v_x, &v_y, &prev_v_x, &prev_v_y);
            let dist_g = l2_dist_f64(&g_x, &g_y, &prev_g_x, &prev_g_y);
            dist_v / dist_g.max(1e-9)
        };
        let alpha_k = alpha_raw.clamp(0.001 * step_scale / g_inf, 0.5 * step_scale / g_inf);

        prev_v_x.copy_from_slice(&v_x);
        prev_v_y.copy_from_slice(&v_y);
        prev_g_x.copy_from_slice(&g_x);
        prev_g_y.copy_from_slice(&g_y);

        // Update u_next and v_next
        let mut u_next_x = u_x.clone();
        let mut u_next_y = u_y.clone();
        let t_next = 0.5 * (1.0 + (1.0 + 4.0 * t_k * t_k).sqrt());
        let momentum = (t_k - 1.0) / t_next;

        for c in 0..num_cells {
            if !wl_precomp.movable_cell_mask[c] {
                continue;
            }
            let w_c = density_solver.cell_w_um[c];
            let h_c = density_solver.cell_h_um[c];
            let unx = (v_x[c] - alpha_k * g_x[c]).clamp(core_xl, core_xh - w_c);
            let uny = (v_y[c] - alpha_k * g_y[c]).clamp(core_yl, core_yh - h_c);
            u_next_x[c] = unx;
            u_next_y[c] = uny;
            v_x[c] = (unx + momentum * (unx - u_x[c])).clamp(core_xl, core_xh - w_c);
            v_y[c] = (uny + momentum * (uny - u_y[c])).clamp(core_yl, core_yh - h_c);
        }

        // Update penalty multiplier lambda
        let hpwl_next = compute_hpwl_um(design_db, &wl_precomp, &u_next_x, &u_next_y);
        let dhpwl = hpwl_next - hpwl_cur;
        let exponent = 1.0 - (dhpwl / dhpwl_ref);
        let mu_k = (1.05f64.powf(exponent) as f32).clamp(0.95, 1.05);
        lambda_k *= mu_k;

        u_x = u_next_x;
        u_y = u_next_y;
        t_k = t_next;
        hpwl_cur = hpwl_next;
    }

    let final_hpwl_um = compute_hpwl_um(design_db, &wl_precomp, &u_x, &u_y);

    NesterovResult {
        cell_x_um: u_x,
        cell_y_um: u_y,
        iterations: final_iters,
        initial_overflow: init_tau,
        final_overflow,
        initial_hpwl_um: init_hpwl_um,
        final_hpwl_um,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::design_db::PlacementStatus;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;

    #[test]
    fn test_gcd_nesterov_global_placement() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pdk_root = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".ciel"));

        let tlef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef");
        let floorplan_def_path = repo_root.join("tests/designs/gcd/gcd.floorplan.def");

        if !tlef_path.exists() || !lef_path.exists() || !floorplan_def_path.exists() {
            panic!("Required test files missing: tlef={:?}, lef={:?}, def={:?}",
                   tlef_path, lef_path, floorplan_def_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");
        let design_db = parse_def(&floorplan_def_path, &tech_db, None)
            .expect("Failed to parse floorplan DEF");

        let config = NesterovConfig::default();
        let t0 = std::time::Instant::now();
        let res = run_nesterov_placement(&design_db, &tech_db, &config);
        let elapsed = t0.elapsed();

        println!(
            "Nesterov placement converged in {} iters, time: {:?}, init_ovf: {:.4}, final_ovf: {:.4}, init_hpwl: {:.2} um, final_hpwl: {:.2} um",
            res.iterations, elapsed, res.initial_overflow, res.final_overflow, res.initial_hpwl_um, res.final_hpwl_um
        );

        // 1. Initial overflow > 0.50 and final overflow <= 0.10
        assert!(
            res.initial_overflow > 0.50,
            "Initial overflow must be > 0.50, got {}",
            res.initial_overflow
        );
        assert!(
            res.final_overflow <= 0.10,
            "Final overflow must be <= 0.10, got {}",
            res.final_overflow
        );

        // 2. Converged in < 300 iterations
        assert!(
            res.iterations < 300,
            "Must converge in < 300 iterations, took {}",
            res.iterations
        );

        // 3. Core bounds check
        let dbu_per_um = design_db.dbu_per_micron as f32;
        let core_xl = design_db.rows.iter().map(|r| r.orig_x_dbu).min().unwrap() as f32 / dbu_per_um;
        let core_xh = design_db.rows.iter().map(|r| r.orig_x_dbu + r.num_x * r.step_x_dbu).max().unwrap() as f32 / dbu_per_um;
        let core_yl = design_db.rows.iter().map(|r| r.orig_y_dbu).min().unwrap() as f32 / dbu_per_um;
        let core_yh = design_db.rows.iter().map(|r| r.orig_y_dbu + tech_db.site_height_dbu).max().unwrap() as f32 / dbu_per_um;

        let num_cells = design_db.num_cells();
        let wl_precomp = WaWirelengthPrecomp::new(&design_db, &tech_db);

        for c in 0..num_cells {
            let is_movable = wl_precomp.movable_cell_mask[c];
            let x = res.cell_x_um[c];
            let y = res.cell_y_um[c];

            if is_movable {
                let macro_id = design_db.cell_macros[c];
                let macro_def = &tech_db.macros[macro_id.0 as usize];
                let w = macro_def.width_dbu as f32 / dbu_per_um;
                let h = macro_def.height_dbu as f32 / dbu_per_um;

                assert!(
                    x >= core_xl - 1e-4 && x + w <= core_xh + 1e-4,
                    "Movable cell {} X out of bounds: x={}, w={}, core=[{}, {}]",
                    c, x, w, core_xl, core_xh
                );
                assert!(
                    y >= core_yl - 1e-4 && y + h <= core_yh + 1e-4,
                    "Movable cell {} Y out of bounds: y={}, h={}, core=[{}, {}]",
                    c, y, h, core_yl, core_yh
                );
            } else if design_db.cell_status[c] == PlacementStatus::Fixed {
                // Fixed cells must remain exactly at their initial coordinates
                let orig_x = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
                let orig_y = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
                assert!(
                    (x - orig_x).abs() < 1e-5,
                    "Fixed cell {} X changed from {} to {}",
                    c, orig_x, x
                );
                assert!(
                    (y - orig_y).abs() < 1e-5,
                    "Fixed cell {} Y changed from {} to {}",
                    c, orig_y, y
                );
            }
        }

        // 4. Final HPWL is reasonable (< 5000 um)
        assert!(
            res.final_hpwl_um < 5000.0,
            "Final HPWL must be < 5000 um, got {}",
            res.final_hpwl_um
        );
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
    }
}
