use crate::design_db::DesignDb;
use crate::placement::density::DensitySolver;
use crate::placement::wirelength::{
    WaWirelengthPrecomp, compute_hpwl_um, compute_wa_wirelength_and_grad,
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
    /// Gradient steps executed (0 when the initial overflow already meets the stop value).
    pub iterations: usize,
    pub converged: bool,
    pub initial_overflow: f32,
    /// Overflow of the returned `cell_x_um` / `cell_y_um` when `converged` is false.
    pub final_overflow: f32,
    pub initial_hpwl_um: f64,
    pub final_hpwl_um: f64,
}

/// Solves the regularized quadratic star-wirelength linear system via Jacobi-preconditioned
/// Conjugate Gradient so multi-hop interior cells follow I/O anchor geometry before GP begins.
#[allow(clippy::too_many_arguments)]
fn cg_initial_place(
    design_db: &DesignDb,
    wl_precomp: &WaWirelengthPrecomp,
    u_x: &mut [f32],
    u_y: &mut [f32],
    cell_w: &[f32],
    cell_h: &[f32],
    core: (f32, f32, f32, f32),
    max_iters: usize,
) {
    let n = design_db.num_cells();
    let num_nets = design_db.num_nets();
    let movable = &wl_precomp.movable_cell_mask;
    if !movable.iter().any(|&m| m) {
        return;
    }

    let mut fixed_b = [vec![0.0f64; n], vec![0.0f64; n]];
    let mut diag = vec![0.0f64; n];
    let mut net_w = vec![0.0f64; num_nets];

    for e in 0..num_nets {
        let s = design_db.net_pin_offsets[e] as usize;
        let en = design_db.net_pin_offsets[e + 1] as usize;
        let deg = en - s;
        if deg < 2 {
            continue;
        }
        let w = 1.0 / ((deg - 1) as f64);
        net_w[e] = w;
        let inv_deg = 1.0 / (deg as f64);

        let mut sum_fix_x = 0.0f64;
        let mut sum_fix_y = 0.0f64;
        for &p in &design_db.net_pin_ids[s..en] {
            let pid = p.0 as usize;
            if let Some(cell) = design_db.pin_cells[pid] {
                let cid = cell.0 as usize;
                if movable[cid] {
                    sum_fix_x += wl_precomp.pin_dx_um[pid] as f64;
                    sum_fix_y += wl_precomp.pin_dy_um[pid] as f64;
                } else {
                    sum_fix_x += u_x[cid] as f64 + wl_precomp.pin_dx_um[pid] as f64;
                    sum_fix_y += u_y[cid] as f64 + wl_precomp.pin_dy_um[pid] as f64;
                }
            } else {
                sum_fix_x += wl_precomp.pin_dx_um[pid] as f64;
                sum_fix_y += wl_precomp.pin_dy_um[pid] as f64;
            }
        }
        let bar_fix_x = sum_fix_x * inv_deg;
        let bar_fix_y = sum_fix_y * inv_deg;

        for &p in &design_db.net_pin_ids[s..en] {
            let pid = p.0 as usize;
            if let Some(cell) = design_db.pin_cells[pid] {
                let cid = cell.0 as usize;
                if movable[cid] {
                    diag[cid] += w * (1.0 - inv_deg);
                    fixed_b[0][cid] += w * (bar_fix_x - wl_precomp.pin_dx_um[pid] as f64);
                    fixed_b[1][cid] += w * (bar_fix_y - wl_precomp.pin_dy_um[pid] as f64);
                }
            }
        }
    }

    // Small diagonal anchor keeps unconstrained connected components positive definite.
    const REG: f64 = 1e-6;
    for c in 0..n {
        if movable[c] {
            diag[c] += REG;
            fixed_b[0][c] += REG * (u_x[c] as f64);
            fixed_b[1][c] += REG * (u_y[c] as f64);
        }
    }

    let apply_a = |v: &[f64], av: &mut [f64]| {
        av.fill(0.0);
        for e in 0..num_nets {
            let w = net_w[e];
            if w == 0.0 {
                continue;
            }
            let s = design_db.net_pin_offsets[e] as usize;
            let en = design_db.net_pin_offsets[e + 1] as usize;
            let inv_deg = 1.0 / ((en - s) as f64);
            let mut sum = 0.0f64;
            for &p in &design_db.net_pin_ids[s..en] {
                if let Some(cell) = design_db.pin_cells[p.0 as usize] {
                    let cid = cell.0 as usize;
                    if movable[cid] {
                        sum += v[cid];
                    }
                }
            }
            let bar = sum * inv_deg;
            for &p in &design_db.net_pin_ids[s..en] {
                if let Some(cell) = design_db.pin_cells[p.0 as usize] {
                    let cid = cell.0 as usize;
                    if movable[cid] {
                        av[cid] += w * (v[cid] - bar);
                    }
                }
            }
        }
        for c in 0..n {
            if movable[c] {
                av[c] += REG * v[c];
            }
        }
    };

    let mut x = vec![0.0f64; n];
    let mut ax = vec![0.0f64; n];
    let mut r = vec![0.0f64; n];
    let mut z = vec![0.0f64; n];
    let mut p_dir = vec![0.0f64; n];

    let axes = [
        (u_x, &fixed_b[0], cell_w, core.0, core.1),
        (u_y, &fixed_b[1], cell_h, core.2, core.3),
    ];
    for (u_axis, b, sizes, lo, hi) in axes {
        for c in 0..n {
            x[c] = u_axis[c] as f64;
        }
        apply_a(&x, &mut ax);
        let mut rz_old = 0.0f64;
        for c in 0..n {
            if movable[c] {
                r[c] = b[c] - ax[c];
                z[c] = r[c] / diag[c];
                p_dir[c] = z[c];
                rz_old += r[c] * z[c];
            }
        }
        for _ in 0..max_iters {
            if rz_old < 1e-12 {
                break;
            }
            apply_a(&p_dir, &mut ax);
            let mut pap = 0.0f64;
            for c in 0..n {
                if movable[c] {
                    pap += p_dir[c] * ax[c];
                }
            }
            if pap <= 1e-20 {
                break;
            }
            let alpha = rz_old / pap;
            let mut rz_new = 0.0f64;
            for c in 0..n {
                if movable[c] {
                    x[c] += alpha * p_dir[c];
                    r[c] -= alpha * ax[c];
                    z[c] = r[c] / diag[c];
                    rz_new += r[c] * z[c];
                }
            }
            let beta = rz_new / rz_old;
            for c in 0..n {
                if movable[c] {
                    p_dir[c] = z[c] + beta * p_dir[c];
                }
            }
            rz_old = rz_new;
        }
        for c in 0..n {
            if movable[c] {
                u_axis[c] = (x[c] as f32).clamp(lo, hi - sizes[c]);
            }
        }
    }
}

pub fn run_nesterov_placement(
    design_db: &DesignDb,
    tech_db: &TechDb,
    config: &NesterovConfig,
) -> NesterovResult {
    let dbu_per_um = design_db.dbu_per_micron.max(1) as f32;
    let t_init = crate::timing::phase("place.gp.init");
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

    cg_initial_place(
        design_db,
        &wl_precomp,
        &mut u_x,
        &mut u_y,
        &density_solver.cell_w_um,
        &density_solver.cell_h_um,
        (core_xl, core_xh, core_yl, core_yh),
        60,
    );
    drop(t_init);

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
    let est_hpwl_um =
        (num_nets as f64) * 0.05 * ((core_xh - core_xl) as f64 + (core_yh - core_yl) as f64);

    let mut v_x = u_x.clone();
    let mut v_y = u_y.clone();
    let mut t_k = 1.0f32;

    let base_gamma = 0.25 * (bin_w_um + bin_h_um);

    // Initial force and overflow evaluation
    let mut force_x = vec![0.0f32; num_cells];
    let mut force_y = vec![0.0f32; num_cells];
    let (init_tau, _) = {
        let _d = crate::timing::phase("place.gp.density");
        density_solver.compute_density_and_forces(&v_x, &v_y, &mut force_x, &mut force_y)
    };

    let gamma_0 = base_gamma * 10.0f32.powf(2.0 * (init_tau - 0.10));

    let mut dummy_pin_gx = vec![0.0f32; num_pins];
    let mut dummy_pin_gy = vec![0.0f32; num_pins];
    let mut wl_gx = vec![0.0f32; num_cells];
    let mut wl_gy = vec![0.0f32; num_cells];
    {
        let _w = crate::timing::phase("place.gp.wl");
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
    }

    let norm_wl = l1_norm_f64(&wl_gx, &wl_gy);
    let norm_force = l1_norm_f64(&force_x, &force_y);
    let mut lambda_k = (0.15 * (norm_wl / norm_force.max(1e-9))) as f32;

    // Precompute cell pin counts and areas for diagonal preconditioner
    let mut pin_cnt = vec![0.0f32; num_cells];
    let mut cell_area = vec![0.0f32; num_cells];
    for c in 0..num_cells {
        pin_cnt[c] = (design_db.cell_pin_offsets[c + 1] - design_db.cell_pin_offsets[c]) as f32;
        cell_area[c] = density_solver.cell_w_um[c] * density_solver.cell_h_um[c];
    }

    let mut prev_v_x = v_x.clone();
    let mut prev_v_y = v_y.clone();
    let mut prev_g_x = vec![0.0f32; num_cells];
    let mut prev_g_y = vec![0.0f32; num_cells];
    let mut g_x = vec![0.0f32; num_cells];
    let mut g_y = vec![0.0f32; num_cells];
    let mut u_next_x = u_x.clone();
    let mut u_next_y = u_y.clone();
    let mut alpha_prev = 0.0f32;
    let mut hpwl_cur = init_hpwl_um;
    let mut final_overflow = init_tau;
    let mut iterations = 0;
    let mut converged = false;

    for k in 0..config.max_iters {
        let tau_k = if k > 0 {
            let (t, _) = {
                let _d = crate::timing::phase("place.gp.density");
                density_solver.compute_density_and_forces(&v_x, &v_y, &mut force_x, &mut force_y)
            };
            final_overflow = t;
            if t <= config.stop_overflow {
                converged = true;
                break;
            }
            let gamma_k = base_gamma * 10.0f32.powf(2.0 * (t - 0.10));
            {
                let _w = crate::timing::phase("place.gp.wl");
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
            }
            t
        } else {
            final_overflow = init_tau;
            if init_tau <= config.stop_overflow {
                converged = true;
                break;
            }
            init_tau
        };

        let _step = crate::timing::phase("place.gp.step");
        // Compute preconditioned gradient g_k
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

        // Barzilai-Borwein step size bounded relative to alpha_prev so single-cell outliers do not stall large designs
        let alpha_k = if k == 0 {
            0.05 * step_scale / g_inf
        } else {
            let dist_v = l2_dist_f64(&v_x, &v_y, &prev_v_x, &prev_v_y);
            let dist_g = l2_dist_f64(&g_x, &g_y, &prev_g_x, &prev_g_y);
            let alpha_raw = dist_v / dist_g.max(1e-9);
            alpha_raw.clamp(0.25 * alpha_prev, 2.0 * alpha_prev)
        };
        alpha_prev = alpha_k;

        prev_v_x.copy_from_slice(&v_x);
        prev_v_y.copy_from_slice(&v_y);
        prev_g_x.copy_from_slice(&g_x);
        prev_g_y.copy_from_slice(&g_y);

        // Update u_next and v_next
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

        // Update penalty multiplier lambda using dynamic wirelength scale and two-phase upper bound
        let hpwl_next = compute_hpwl_um(design_db, &wl_precomp, &u_next_x, &u_next_y);
        let dhpwl = hpwl_next - hpwl_cur;
        let dhpwl_ref = (0.015 * hpwl_cur.max(est_hpwl_um)).max(50.0);
        let exponent = 1.0 - (dhpwl / dhpwl_ref);
        let mu_max: f64 = if tau_k > 0.70 { 1.090 } else { 1.055 };
        let mu_k = (mu_max.powf(exponent) as f32).clamp(0.95, mu_max as f32);
        lambda_k *= mu_k;

        std::mem::swap(&mut u_x, &mut u_next_x);
        std::mem::swap(&mut u_y, &mut u_next_y);
        t_k = t_next;
        hpwl_cur = hpwl_next;
        iterations += 1;
    }

    // The loop exits without a stop check after its last step, so the overflow
    // it last saw belongs to v, not to the returned u.
    if !converged {
        let (tau_u, _) = {
            let _d = crate::timing::phase("place.gp.density");
            density_solver.compute_density_and_forces(&u_x, &u_y, &mut force_x, &mut force_y)
        };
        final_overflow = tau_u;
        converged = tau_u <= config.stop_overflow;
    }

    let final_hpwl_um = compute_hpwl_um(design_db, &wl_precomp, &u_x, &u_y);

    NesterovResult {
        cell_x_um: u_x,
        cell_y_um: u_y,
        iterations,
        converged,
        initial_overflow: init_tau,
        final_overflow,
        initial_hpwl_um: init_hpwl_um,
        final_hpwl_um,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::PlacementStatus;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use crate::test_support::{design_dir, sky130_hd};

    #[test]
    fn test_gcd_nesterov_global_placement() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let floorplan_def_path = design_dir("gcd").join("gcd.floorplan.def");

        if !tlef_path.exists() || !lef_path.exists() || !floorplan_def_path.exists() {
            panic!(
                "Required test files missing: tlef={:?}, lef={:?}, def={:?}",
                tlef_path, lef_path, floorplan_def_path
            );
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        let config = NesterovConfig::default();
        let t0 = std::time::Instant::now();
        let res = run_nesterov_placement(&design_db, &tech_db, &config);
        let elapsed = t0.elapsed();

        let status = if res.converged {
            "converged"
        } else {
            "did not converge"
        };
        println!(
            "Nesterov placement {} in {} iters, time: {:?}, init_ovf: {:.4}, final_ovf: {:.4}, init_hpwl: {:.2} um, final_hpwl: {:.2} um",
            status,
            res.iterations,
            elapsed,
            res.initial_overflow,
            res.final_overflow,
            res.initial_hpwl_um,
            res.final_hpwl_um
        );

        // 1. CG initial placement yields ~0.40 initial overflow on gcd; GP drives it <= 0.10
        assert!(
            res.initial_overflow > 0.30,
            "Initial overflow must be > 0.30, got {}",
            res.initial_overflow
        );
        assert!(
            res.final_overflow <= 0.10,
            "Final overflow must be <= 0.10, got {}",
            res.final_overflow
        );

        // 2. Converged in < 300 iterations
        assert!(res.converged, "gcd must report convergence");
        assert!(
            res.iterations < 300,
            "Must converge in < 300 iterations, took {}",
            res.iterations
        );

        // 3. Core bounds check
        let dbu_per_um = design_db.dbu_per_micron as f32;
        let core_xl =
            design_db.rows.iter().map(|r| r.orig_x_dbu).min().unwrap() as f32 / dbu_per_um;
        let core_xh = design_db
            .rows
            .iter()
            .map(|r| r.orig_x_dbu + r.num_x * r.step_x_dbu)
            .max()
            .unwrap() as f32
            / dbu_per_um;
        let core_yl =
            design_db.rows.iter().map(|r| r.orig_y_dbu).min().unwrap() as f32 / dbu_per_um;
        let core_yh = design_db
            .rows
            .iter()
            .map(|r| r.orig_y_dbu + tech_db.site_height_dbu)
            .max()
            .unwrap() as f32
            / dbu_per_um;

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
                    c,
                    x,
                    w,
                    core_xl,
                    core_xh
                );
                assert!(
                    y >= core_yl - 1e-4 && y + h <= core_yh + 1e-4,
                    "Movable cell {} Y out of bounds: y={}, h={}, core=[{}, {}]",
                    c,
                    y,
                    h,
                    core_yl,
                    core_yh
                );
            } else if design_db.cell_status[c] == PlacementStatus::Fixed {
                // Fixed cells must remain exactly at their initial coordinates
                let orig_x = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
                let orig_y = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
                assert!(
                    (x - orig_x).abs() < 1e-5,
                    "Fixed cell {} X changed from {} to {}",
                    c,
                    orig_x,
                    x
                );
                assert!(
                    (y - orig_y).abs() < 1e-5,
                    "Fixed cell {} Y changed from {} to {}",
                    c,
                    orig_y,
                    y
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

    /// PDK-free fixture: 40 Core cells of 4 um x 2 um in a 20 um x 20 um die.
    /// Movable area (320 um^2) exceeds the target density (0.60) of the die (400 um^2),
    /// so the density overflow starts above zero. Cells are grouped into 10 nets of 4.
    fn synthetic_design() -> (DesignDb, TechDb) {
        use crate::design_db::Orient;
        use crate::interner::{CellId, MacroId, MacroPinId, NetId, PinDir, PinId, StringInterner};
        use crate::tech_db::{MacroClass, MacroDef, MacroPin, RectDbu};

        let num_cells = 40usize;
        let mut tech_db = TechDb::new();
        tech_db.dbu_per_micron = 1000;
        tech_db.site_height_dbu = 2000;
        let mut interner = StringInterner::new();

        let m_name = interner.intern("TEST_MACRO");
        let mp_name = interner.intern("A");
        tech_db.macro_pins.push(MacroPin {
            name: mp_name,
            dir: PinDir::Input,
            is_power: false,
            offset_x_dbu: 2000,
            offset_y_dbu: 1000,
            offset_x_frac: false,
            offset_y_frac: false,
            layer: None,
        });
        tech_db.macros.push(MacroDef {
            name: m_name,
            class: MacroClass::Core,
            width_dbu: 4000,
            height_dbu: 2000,
            pin_offset: 0,
            pin_count: 1,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        });
        tech_db.macro_by_name.insert(m_name, MacroId(0));

        let mut design_db = DesignDb::new();
        design_db.dbu_per_micron = 1000;
        design_db.die_area = RectDbu::new(0, 0, 20000, 20000);

        for i in 0..num_cells {
            let cname = interner.intern(&format!("c{}", i));
            design_db.cell_names.push(cname);
            design_db.cell_macros.push(MacroId(0));
            design_db.cell_lib_ids.push(None);
            design_db.cell_orient.push(Orient::N);
            design_db.cell_status.push(PlacementStatus::Placed);
            design_db.cell_x_dbu.push(0);
            design_db.cell_y_dbu.push(0);
            design_db.cell_pin_offsets.push(i as u32);
            design_db.cell_pin_ids.push(PinId(i as u32));

            let pname = interner.intern(&format!("p{}", i));
            design_db.pin_names.push(pname);
            design_db.pin_cells.push(Some(CellId(i as u32)));
            design_db.pin_nets.push(Some(NetId((i / 4) as u32)));
            design_db.pin_macro_pins.push(Some(MacroPinId(0)));
            design_db.pin_lib_pins.push(None);
            design_db.pin_dirs.push(PinDir::Input);
            design_db.pin_offset_x_dbu.push(2000);
            design_db.pin_offset_y_dbu.push(1000);
            design_db.pin_is_driver.push(i % 4 == 0);
        }
        design_db.cell_pin_offsets.push(num_cells as u32);

        let num_nets = num_cells / 4;
        for e in 0..num_nets {
            let nname = interner.intern(&format!("net{}", e));
            design_db.net_names.push(nname);
            design_db.net_pin_offsets.push((e * 4) as u32);
            design_db
                .net_pin_ids
                .extend((e * 4..e * 4 + 4).map(|p| PinId(p as u32)));
        }
        design_db.net_pin_offsets.push((num_nets * 4) as u32);

        (design_db, tech_db)
    }

    #[test]
    fn test_convergence_accounting_iteration_limit() {
        let (design_db, tech_db) = synthetic_design();
        // A negative stop value can never be met, so the loop must exhaust max_iters.
        let config = NesterovConfig {
            max_iters: 3,
            stop_overflow: -1.0,
            ..NesterovConfig::default()
        };
        let res = run_nesterov_placement(&design_db, &tech_db, &config);

        assert!(
            !res.converged,
            "run that hit max_iters must not report convergence"
        );
        assert_eq!(res.iterations, 3, "3 gradient steps were executed");
        assert_eq!(res.cell_x_um.len(), design_db.num_cells());
        assert!(res.final_overflow.is_finite() && res.final_overflow >= 0.0);
    }

    #[test]
    fn test_convergence_accounting_initial_overflow_satisfies_stop() {
        let (design_db, tech_db) = synthetic_design();
        let config = NesterovConfig {
            max_iters: 3,
            stop_overflow: 1.0e6,
            ..NesterovConfig::default()
        };
        let res = run_nesterov_placement(&design_db, &tech_db, &config);

        assert!(res.converged, "initial overflow already below stop value");
        assert_eq!(res.iterations, 0, "no gradient step should run");
        assert_eq!(res.final_overflow, res.initial_overflow);
    }
}
