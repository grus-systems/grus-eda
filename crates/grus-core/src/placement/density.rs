use std::f32::consts::PI;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use crate::design_db::{DesignDb, Orient, PlacementStatus};
use crate::tech_db::{MacroClass, TechDb};

/// 1D unnormalized Forward DCT-II:
/// out[u] = sum_{n=0}^{M-1} x[n] * cos(pi * u * (2n + 1) / (2M))
pub fn dct1d(x: &[f32], out: &mut [f32], fft_fwd: &dyn Fft<f32>, buf: &mut [Complex<f32>]) {
    let m = x.len();
    assert!(m.is_power_of_two(), "M must be a power of two");
    assert_eq!(out.len(), m);
    assert_eq!(buf.len(), m);

    let half = m / 2;
    for k in 0..half {
        buf[k] = Complex::new(x[2 * k], 0.0);
        buf[m - 1 - k] = Complex::new(x[2 * k + 1], 0.0);
    }

    fft_fwd.process(buf);

    let inv_2m = PI / (2.0 * m as f32);
    for u in 0..m {
        let angle = (u as f32) * inv_2m;
        let (sin_a, cos_a) = angle.sin_cos();
        out[u] = buf[u].re * cos_a + buf[u].im * sin_a;
    }
}

/// 1D Inverse DCT (DCT-III satisfying idct1d(dct1d(x)) == x):
/// out[n] = (1/M) * c[0] + (2/M) * sum_{u=1}^{M-1} c[u] * cos(pi * u * (2n + 1) / (2M))
pub fn idct1d(c: &[f32], out: &mut [f32], fft_inv: &dyn Fft<f32>, buf: &mut [Complex<f32>]) {
    let m = c.len();
    assert!(m.is_power_of_two(), "M must be a power of two");
    assert_eq!(out.len(), m);
    assert_eq!(buf.len(), m);

    let inv_m = 1.0f32 / m as f32;
    buf[0] = Complex::new(c[0] * inv_m, 0.0);

    let inv_2m = PI / (2.0 * m as f32);
    for u in 1..m {
        let angle = (u as f32) * inv_2m;
        let (sin_a, cos_a) = angle.sin_cos();
        let re = (c[u] * cos_a + c[m - u] * sin_a) * inv_m;
        let im = (c[u] * sin_a - c[m - u] * cos_a) * inv_m;
        buf[u] = Complex::new(re, im);
    }

    fft_inv.process(buf);

    let half = m / 2;
    for k in 0..half {
        out[2 * k] = buf[k].re;
        out[2 * k + 1] = buf[m - 1 - k].re;
    }
}

/// 1D Inverse Shifted Sine Transform:
/// out[n] = sum_{u=1}^{M-1} s[u] * sin(pi * u * (2n + 1) / (2M))
pub fn idxst1d(s: &[f32], out: &mut [f32], fft_inv: &dyn Fft<f32>, buf: &mut [Complex<f32>]) {
    let m = s.len();
    assert!(m.is_power_of_two(), "M must be a power of two");
    assert_eq!(out.len(), m);
    assert_eq!(buf.len(), m);

    let mut c = vec![0.0f32; m];
    c[0] = 0.0;
    for u in 1..m {
        c[u] = s[m - u];
    }

    idct1d(&c, out, fft_inv, buf);

    for n in (1..m).step_by(2) {
        out[n] = -out[n];
    }
}

#[derive(Clone, Debug)]
pub struct DensitySolver {
    pub m: usize, // power-of-2 grid dimension M x M
    pub core_xl_um: f32,
    pub core_yl_um: f32,
    pub core_xh_um: f32,
    pub core_yh_um: f32,
    pub bin_w_um: f32,
    pub bin_h_um: f32,
    pub bin_area_um2: f32,
    pub target_density: f32,
    pub total_movable_area_um2: f32,
    pub cell_w_um: Vec<f32>,
    pub cell_h_um: Vec<f32>,
    pub cell_stretched_w_um: Vec<f32>,
    pub cell_stretched_h_um: Vec<f32>,
    pub cell_density_scale: Vec<f32>, // s_c = (w_c * h_c) / (w'_c * h'_c)
    pub movable_cell_mask: Vec<bool>,
    pub fixed_area: Vec<f32>,   // length m * m
    pub movable_area: Vec<f32>, // length m * m
    pub rho: Vec<f32>,          // length m * m (zero-mean charge density)
    pub phi: Vec<f32>,          // length m * m (electrostatic potential)
    pub e_x: Vec<f32>,          // length m * m (horizontal electric field -dphi/dx)
    pub e_y: Vec<f32>,          // length m * m (vertical electric field -dphi/dy)
}

impl DensitySolver {
    pub fn new_empty(
        m: usize,
        core_xl_um: f32,
        core_yl_um: f32,
        core_xh_um: f32,
        core_yh_um: f32,
        target_density: f32,
    ) -> Self {
        assert!(m.is_power_of_two(), "Grid dimension M must be a power of 2");
        let bin_w_um = (core_xh_um - core_xl_um) / m as f32;
        let bin_h_um = (core_yh_um - core_yl_um) / m as f32;
        let bin_area_um2 = bin_w_um * bin_h_um;
        let grid_size = m * m;

        Self {
            m,
            core_xl_um,
            core_yl_um,
            core_xh_um,
            core_yh_um,
            bin_w_um,
            bin_h_um,
            bin_area_um2,
            target_density,
            total_movable_area_um2: 0.0,
            cell_w_um: Vec::new(),
            cell_h_um: Vec::new(),
            cell_stretched_w_um: Vec::new(),
            cell_stretched_h_um: Vec::new(),
            cell_density_scale: Vec::new(),
            movable_cell_mask: Vec::new(),
            fixed_area: vec![0.0; grid_size],
            movable_area: vec![0.0; grid_size],
            rho: vec![0.0; grid_size],
            phi: vec![0.0; grid_size],
            e_x: vec![0.0; grid_size],
            e_y: vec![0.0; grid_size],
        }
    }

    pub fn new(
        design_db: &DesignDb,
        tech_db: &TechDb,
        target_density: f32,
        grid_m_override: Option<usize>,
    ) -> Self {
        let dbu_per_um = design_db.dbu_per_micron.max(1) as f32;

        let (core_xl_um, core_yl_um, core_xh_um, core_yh_um) = if !design_db.rows.is_empty() {
            let core_xl_dbu = design_db.rows.iter().map(|r| r.orig_x_dbu).min().unwrap();
            let core_xh_dbu = design_db
                .rows
                .iter()
                .map(|r| r.orig_x_dbu + r.num_x * r.step_x_dbu)
                .max()
                .unwrap();
            let core_yl_dbu = design_db.rows.iter().map(|r| r.orig_y_dbu).min().unwrap();
            let core_yh_dbu = design_db
                .rows
                .iter()
                .map(|r| r.orig_y_dbu + tech_db.site_height_dbu)
                .max()
                .unwrap();
            (
                core_xl_dbu as f32 / dbu_per_um,
                core_yl_dbu as f32 / dbu_per_um,
                core_xh_dbu as f32 / dbu_per_um,
                core_yh_dbu as f32 / dbu_per_um,
            )
        } else {
            (
                design_db.die_area.xl as f32 / dbu_per_um,
                design_db.die_area.yl as f32 / dbu_per_um,
                design_db.die_area.xh as f32 / dbu_per_um,
                design_db.die_area.yh as f32 / dbu_per_um,
            )
        };

        let m = grid_m_override.unwrap_or_else(|| {
            let core_h_um = core_yh_um - core_yl_um;
            let site_h_um = tech_db.site_height_dbu as f32 / dbu_per_um;
            let max_bin_h = 2.0 * site_h_um;
            let candidates = [16, 32, 64, 128, 256, 512];
            for &c in &candidates {
                if core_h_um / (c as f32) <= max_bin_h {
                    return c;
                }
            }
            16
        });

        assert!(m.is_power_of_two(), "Grid dimension M must be a power of 2");

        let bin_w_um = (core_xh_um - core_xl_um) / m as f32;
        let bin_h_um = (core_yh_um - core_yl_um) / m as f32;
        let bin_area_um2 = bin_w_um * bin_h_um;
        let grid_size = m * m;

        let num_cells = design_db.num_cells();
        let mut cell_w_um = vec![0.0f32; num_cells];
        let mut cell_h_um = vec![0.0f32; num_cells];
        let mut cell_stretched_w_um = vec![0.0f32; num_cells];
        let mut cell_stretched_h_um = vec![0.0f32; num_cells];
        let mut cell_density_scale = vec![0.0f32; num_cells];
        let mut movable_cell_mask = vec![false; num_cells];
        let mut total_movable_area_um2 = 0.0f32;

        let sqrt2 = std::f32::consts::SQRT_2;
        let min_stretched_w = sqrt2 * bin_w_um;
        let min_stretched_h = sqrt2 * bin_h_um;

        for c in 0..num_cells {
            let macro_id = design_db.cell_macros[c];
            if macro_id.0 as usize >= tech_db.macros.len() {
                continue;
            }
            let macro_def = &tech_db.macros[macro_id.0 as usize];

            let is_rotated = matches!(
                design_db.cell_orient[c],
                Orient::E | Orient::W | Orient::FE | Orient::FW
            );
            let (w_dbu, h_dbu) = if is_rotated {
                (macro_def.height_dbu, macro_def.width_dbu)
            } else {
                (macro_def.width_dbu, macro_def.height_dbu)
            };
            let w = w_dbu as f32 / dbu_per_um;
            let h = h_dbu as f32 / dbu_per_um;
            cell_w_um[c] = w;
            cell_h_um[c] = h;

            let status = design_db.cell_status[c];
            let class = macro_def.class;

            if class == MacroClass::CoreSpacer {
                continue;
            }

            let is_movable = (status == PlacementStatus::Placed
                || status == PlacementStatus::Unplaced)
                && (class == MacroClass::Core || class == MacroClass::CoreTie);

            if is_movable {
                movable_cell_mask[c] = true;
                let area = w * h;
                total_movable_area_um2 += area;

                let sw = w.max(min_stretched_w);
                let sh = h.max(min_stretched_h);
                cell_stretched_w_um[c] = sw;
                cell_stretched_h_um[c] = sh;
                cell_density_scale[c] = area / (sw * sh);
            } else {
                cell_stretched_w_um[c] = w;
                cell_stretched_h_um[c] = h;
                cell_density_scale[c] = 0.0;
            }
        }

        // Precompute fixed_area from PlacementStatus::Fixed cells clipped to bins
        let mut fixed_area = vec![0.0f32; grid_size];
        for c in 0..num_cells {
            if design_db.cell_status[c] != PlacementStatus::Fixed {
                continue;
            }
            let macro_id = design_db.cell_macros[c];
            if macro_id.0 as usize >= tech_db.macros.len() {
                continue;
            }
            if tech_db.macros[macro_id.0 as usize].class == MacroClass::CoreSpacer {
                continue;
            }

            let cx0 = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
            let cy0 = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
            let cx1 = cx0 + cell_w_um[c];
            let cy1 = cy0 + cell_h_um[c];

            // Clip against core bounds
            let x0 = cx0.max(core_xl_um);
            let y0 = cy0.max(core_yl_um);
            let x1 = cx1.min(core_xh_um);
            let y1 = cy1.min(core_yh_um);

            if x1 <= x0 || y1 <= y0 {
                continue;
            }

            let u_min = (((x0 - core_xl_um) / bin_w_um).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let u_max = (((x1 - core_xl_um) / bin_w_um).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_min = (((y0 - core_yl_um) / bin_h_um).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_max = (((y1 - core_yl_um) / bin_h_um).floor() as isize)
                .clamp(0, m as isize - 1) as usize;

            for v in v_min..=v_max {
                let by0 = core_yl_um + v as f32 * bin_h_um;
                let by1 = by0 + bin_h_um;
                let dy = (y1.min(by1) - y0.max(by0)).max(0.0);
                if dy <= 0.0 {
                    continue;
                }
                for u in u_min..=u_max {
                    let bx0 = core_xl_um + u as f32 * bin_w_um;
                    let bx1 = bx0 + bin_w_um;
                    let dx = (x1.min(bx1) - x0.max(bx0)).max(0.0);
                    if dx > 0.0 {
                        fixed_area[v * m + u] += dx * dy;
                    }
                }
            }
        }

        // Clamp fixed_area to bin_area_um2
        for val in &mut fixed_area {
            *val = val.min(bin_area_um2);
        }

        Self {
            m,
            core_xl_um,
            core_yl_um,
            core_xh_um,
            core_yh_um,
            bin_w_um,
            bin_h_um,
            bin_area_um2,
            target_density,
            total_movable_area_um2,
            cell_w_um,
            cell_h_um,
            cell_stretched_w_um,
            cell_stretched_h_um,
            cell_density_scale,
            movable_cell_mask,
            fixed_area,
            movable_area: vec![0.0; grid_size],
            rho: vec![0.0; grid_size],
            phi: vec![0.0; grid_size],
            e_x: vec![0.0; grid_size],
            e_y: vec![0.0; grid_size],
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn solve_poisson_from_rho(&mut self) {
        let m = self.m;
        let mut planner = FftPlanner::new();
        let fft_fwd = planner.plan_fft_forward(m);
        let fft_inv = planner.plan_fft_inverse(m);
        let mut buf = vec![Complex::new(0.0f32, 0.0f32); m];

        let mut a = vec![0.0f32; m * m];

        // 1. 2D DCT of self.rho -> a
        // Row transform along u
        for v in 0..m {
            let row_start = v * m;
            dct1d(
                &self.rho[row_start..row_start + m],
                &mut a[row_start..row_start + m],
                &*fft_fwd,
                &mut buf,
            );
        }

        // Column transform along v
        let mut col_in = vec![0.0f32; m];
        let mut col_out = vec![0.0f32; m];
        for u in 0..m {
            for v in 0..m {
                col_in[v] = a[v * m + u];
            }
            dct1d(&col_in, &mut col_out, &*fft_fwd, &mut buf);
            for v in 0..m {
                a[v * m + u] = col_out[v];
            }
        }

        // 2. Frequency division
        let mut phi_hat = vec![0.0f32; m * m];
        let mut ex_hat = vec![0.0f32; m * m];
        let mut ey_hat = vec![0.0f32; m * m];

        let wu_base = PI / (m as f32 * self.bin_w_um);
        let wv_base = PI / (m as f32 * self.bin_h_um);

        for v in 0..m {
            let wv = v as f32 * wv_base;
            let wv2 = wv * wv;
            for u in 0..m {
                let idx = v * m + u;
                if u == 0 && v == 0 {
                    phi_hat[idx] = 0.0;
                    ex_hat[idx] = 0.0;
                    ey_hat[idx] = 0.0;
                } else {
                    let wu = u as f32 * wu_base;
                    let denom = wu * wu + wv2;
                    let p = a[idx] / denom;
                    phi_hat[idx] = p;
                    ex_hat[idx] = wu * p;
                    ey_hat[idx] = wv * p;
                }
            }
        }

        // 3. Inversion
        // phi: idct rows along u, idct cols along v
        for v in 0..m {
            let row_start = v * m;
            idct1d(
                &phi_hat[row_start..row_start + m],
                &mut self.phi[row_start..row_start + m],
                &*fft_inv,
                &mut buf,
            );
        }
        for u in 0..m {
            for v in 0..m {
                col_in[v] = self.phi[v * m + u];
            }
            idct1d(&col_in, &mut col_out, &*fft_inv, &mut buf);
            for v in 0..m {
                self.phi[v * m + u] = col_out[v];
            }
        }

        // e_x: idxst rows along u, idct cols along v
        for v in 0..m {
            let row_start = v * m;
            idxst1d(
                &ex_hat[row_start..row_start + m],
                &mut self.e_x[row_start..row_start + m],
                &*fft_inv,
                &mut buf,
            );
        }
        for u in 0..m {
            for v in 0..m {
                col_in[v] = self.e_x[v * m + u];
            }
            idct1d(&col_in, &mut col_out, &*fft_inv, &mut buf);
            for v in 0..m {
                self.e_x[v * m + u] = col_out[v];
            }
        }

        // e_y: idct rows along u, idxst cols along v
        for v in 0..m {
            let row_start = v * m;
            idct1d(
                &ey_hat[row_start..row_start + m],
                &mut self.e_y[row_start..row_start + m],
                &*fft_inv,
                &mut buf,
            );
        }
        for u in 0..m {
            for v in 0..m {
                col_in[v] = self.e_y[v * m + u];
            }
            idxst1d(&col_in, &mut col_out, &*fft_inv, &mut buf);
            for v in 0..m {
                self.e_y[v * m + u] = col_out[v];
            }
        }
    }

    pub fn compute_density_and_forces(
        &mut self,
        cell_x_um: &[f32],
        cell_y_um: &[f32],
        force_x: &mut [f32],
        force_y: &mut [f32],
    ) -> (f32, f64) {
        let m = self.m;
        let grid_size = m * m;

        self.movable_area.fill(0.0);
        force_x.fill(0.0);
        force_y.fill(0.0);

        let num_cells = self.movable_cell_mask.len();
        let bin_w = self.bin_w_um;
        let bin_h = self.bin_h_um;
        let core_xl = self.core_xl_um;
        let core_xh = self.core_xh_um;
        let core_yl = self.core_yl_um;
        let core_yh = self.core_yh_um;

        // 1. Accumulate movable cell area into self.movable_area
        for c in 0..num_cells {
            if !self.movable_cell_mask[c] {
                continue;
            }

            let orig_w = self.cell_w_um[c];
            let orig_h = self.cell_h_um[c];
            let sw = self.cell_stretched_w_um[c];
            let sh = self.cell_stretched_h_um[c];
            let scale = self.cell_density_scale[c];

            let cx = cell_x_um[c] + orig_w * 0.5;
            let cy = cell_y_um[c] + orig_h * 0.5;

            let x0 = cx - sw * 0.5;
            let x1 = cx + sw * 0.5;
            let y0 = cy - sh * 0.5;
            let y1 = cy + sh * 0.5;

            // Clip against core bounds
            let x0_clip = x0.max(core_xl);
            let y0_clip = y0.max(core_yl);
            let x1_clip = x1.min(core_xh);
            let y1_clip = y1.min(core_yh);

            if x1_clip <= x0_clip || y1_clip <= y0_clip {
                continue;
            }

            let u_min = (((x0_clip - core_xl) / bin_w).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let u_max = (((x1_clip - core_xl) / bin_w).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_min = (((y0_clip - core_yl) / bin_h).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_max = (((y1_clip - core_yl) / bin_h).floor() as isize)
                .clamp(0, m as isize - 1) as usize;

            for v in v_min..=v_max {
                let by0 = core_yl + v as f32 * bin_h;
                let by1 = by0 + bin_h;
                let dy = (y1_clip.min(by1) - y0_clip.max(by0)).max(0.0);
                if dy <= 0.0 {
                    continue;
                }
                for u in u_min..=u_max {
                    let bx0 = core_xl + u as f32 * bin_w;
                    let bx1 = bx0 + bin_w;
                    let dx = (x1_clip.min(bx1) - x0_clip.max(bx0)).max(0.0);
                    if dx > 0.0 {
                        self.movable_area[v * m + u] += scale * dx * dy;
                    }
                }
            }
        }

        // 2. Compute overflow ratio tau
        let mut overflow_sum = 0.0f64;
        let bin_area = self.bin_area_um2;
        let target_density = self.target_density;

        for i in 0..grid_size {
            let free_area = (bin_area - self.fixed_area[i]).max(0.0);
            let target_cap = target_density * free_area;
            let ovf = (self.movable_area[i] - target_cap).max(0.0);
            overflow_sum += ovf as f64;
        }

        let tau = if self.total_movable_area_um2 > 0.0 {
            (overflow_sum / self.total_movable_area_um2 as f64) as f32
        } else {
            0.0
        };

        // 3. Compute charge rho(u, v) = (movable_area + target_density * fixed_area) / A_b - rho_bar
        let inv_bin_area = 1.0f32 / bin_area;
        let mut sum_raw_rho = 0.0f64;
        for ((rho, &movable), &fixed) in self.rho.iter_mut().zip(&self.movable_area).zip(&self.fixed_area) {
            let raw = (movable + target_density * fixed) * inv_bin_area;
            *rho = raw;
            sum_raw_rho += raw as f64;
        }
        let mean_rho = (sum_raw_rho / grid_size as f64) as f32;
        for rho in self.rho.iter_mut() {
            *rho -= mean_rho;
        }

        // 4. Solve Poisson equation
        self.solve_poisson_from_rho();

        // 5. Gather repulsive forces
        for c in 0..num_cells {
            if !self.movable_cell_mask[c] {
                continue;
            }

            let orig_w = self.cell_w_um[c];
            let orig_h = self.cell_h_um[c];
            let sw = self.cell_stretched_w_um[c];
            let sh = self.cell_stretched_h_um[c];
            let scale = self.cell_density_scale[c];

            let cx = cell_x_um[c] + orig_w * 0.5;
            let cy = cell_y_um[c] + orig_h * 0.5;

            let x0 = cx - sw * 0.5;
            let x1 = cx + sw * 0.5;
            let y0 = cy - sh * 0.5;
            let y1 = cy + sh * 0.5;

            let x0_clip = x0.max(core_xl);
            let y0_clip = y0.max(core_yl);
            let x1_clip = x1.min(core_xh);
            let y1_clip = y1.min(core_yh);

            if x1_clip <= x0_clip || y1_clip <= y0_clip {
                continue;
            }

            let u_min = (((x0_clip - core_xl) / bin_w).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let u_max = (((x1_clip - core_xl) / bin_w).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_min = (((y0_clip - core_yl) / bin_h).floor() as isize)
                .clamp(0, m as isize - 1) as usize;
            let v_max = (((y1_clip - core_yl) / bin_h).floor() as isize)
                .clamp(0, m as isize - 1) as usize;

            let mut fx = 0.0f32;
            let mut fy = 0.0f32;

            for v in v_min..=v_max {
                let by0 = core_yl + v as f32 * bin_h;
                let by1 = by0 + bin_h;
                let dy = (y1_clip.min(by1) - y0_clip.max(by0)).max(0.0);
                if dy <= 0.0 {
                    continue;
                }
                for u in u_min..=u_max {
                    let bx0 = core_xl + u as f32 * bin_w;
                    let bx1 = bx0 + bin_w;
                    let dx = (x1_clip.min(bx1) - x0_clip.max(bx0)).max(0.0);
                    if dx > 0.0 {
                        let weight = dx * dy;
                        let idx = v * m + u;
                        fx += weight * self.e_x[idx];
                        fy += weight * self.e_y[idx];
                    }
                }
            }

            force_x[c] = scale * fx;
            force_y[c] = scale * fy;
        }

        // 6. Compute potential energy psi = 0.5 * sum(rho * phi * bin_area)
        let mut psi = 0.0f64;
        for i in 0..grid_size {
            psi += (self.rho[i] as f64) * (self.phi[i] as f64);
        }
        psi *= 0.5 * (bin_area as f64);

        (tau, psi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, sky130_hd};
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;

    #[test]
    fn test_dct1d_idct1d_roundtrip() {
        let m = 64;
        let mut planner = FftPlanner::new();
        let fft_fwd = planner.plan_fft_forward(m);
        let fft_inv = planner.plan_fft_inverse(m);
        let mut buf = vec![Complex::new(0.0f32, 0.0f32); m];

        let mut x = vec![0.0f32; m];
        for (i, val) in x.iter_mut().enumerate() {
            *val = ((i as f32) * 1.7 + 0.3).sin() + ((i as f32) * 0.4).cos();
        }

        let mut c = vec![0.0f32; m];
        dct1d(&x, &mut c, &*fft_fwd, &mut buf);

        let mut x_recon = vec![0.0f32; m];
        idct1d(&c, &mut x_recon, &*fft_inv, &mut buf);

        for i in 0..m {
            let diff = (x[i] - x_recon[i]).abs();
            assert!(
                diff < 1e-5,
                "DCT/IDCT roundtrip mismatch at index {}: orig={}, recon={}, diff={}",
                i, x[i], x_recon[i], diff
            );
        }
    }

    #[test]
    fn test_2d_poisson_analytical_eigenmode() {
        let m = 64;
        let bin_w = PI / (m as f32);
        let bin_h = PI / (m as f32);
        let mut solver = DensitySolver::new_empty(
            m,
            0.0,
            0.0,
            m as f32 * bin_w,
            m as f32 * bin_h,
            0.60,
        );

        let wu1 = PI / (m as f32 * bin_w);
        let wv2 = 2.0 * PI / (m as f32 * bin_h);

        // rho(u, v) = cos(pi * (2u + 1) / (2M)) + cos(2 * pi * (2v + 1) / (2M))
        for v in 0..m {
            let y_term = (wv2 * (v as f32 + 0.5) * bin_h).cos();
            for u in 0..m {
                let x_term = (wu1 * (u as f32 + 0.5) * bin_w).cos();
                solver.rho[v * m + u] = x_term + y_term;
            }
        }

        solver.solve_poisson_from_rho();

        let inv_wu1_2 = 1.0 / (wu1 * wu1);
        let inv_wv2_2 = 1.0 / (wv2 * wv2);
        let inv_wu1 = 1.0 / wu1;
        let inv_wv2 = 1.0 / wv2;

        let mut sum_ey = 0.0f64;

        for v in 0..m {
            let cos_y = (wv2 * (v as f32 + 0.5) * bin_h).cos();
            let sin_y = (wv2 * (v as f32 + 0.5) * bin_h).sin();
            for u in 0..m {
                let cos_x = (wu1 * (u as f32 + 0.5) * bin_w).cos();
                let sin_x = (wu1 * (u as f32 + 0.5) * bin_w).sin();

                let expected_phi = inv_wu1_2 * cos_x + inv_wv2_2 * cos_y;
                let expected_ex = inv_wu1 * sin_x;
                let expected_ey = inv_wv2 * sin_y;

                let idx = v * m + u;
                let actual_phi = solver.phi[idx];
                let actual_ex = solver.e_x[idx];
                let actual_ey = solver.e_y[idx];

                sum_ey += actual_ey as f64;

                let err_phi = (actual_phi - expected_phi).abs();
                assert!(
                    err_phi < 1e-4,
                    "phi mismatch at ({}, {}): expected={}, actual={}, err={}",
                    u, v, expected_phi, actual_phi, err_phi
                );

                let err_ex = (actual_ex - expected_ex).abs();
                assert!(
                    err_ex < 1e-4,
                    "e_x mismatch at ({}, {}): expected={}, actual={}, err={}",
                    u, v, expected_ex, actual_ex, err_ex
                );

                let err_ey = (actual_ey - expected_ey).abs();
                assert!(
                    err_ey < 1e-4,
                    "e_y mismatch at ({}, {}): expected={}, actual={}, err={}",
                    u, v, expected_ey, actual_ey, err_ey
                );
            }
        }

        // Ey completes a full 2pi period across rows, so its sum is strictly 0
        assert!(
            sum_ey.abs() < 1e-3,
            "Net Ey must sum to 0 within 1e-3, got {}",
            sum_ey
        );

        // Also test mode (2, 2) where both Ex and Ey complete full 2pi periods and sum to 0
        let wu2 = 2.0 * PI / (m as f32 * bin_w);
        for v in 0..m {
            let y_term = (wv2 * (v as f32 + 0.5) * bin_h).cos();
            for u in 0..m {
                let x_term = (wu2 * (u as f32 + 0.5) * bin_w).cos();
                solver.rho[v * m + u] = x_term + y_term;
            }
        }
        solver.solve_poisson_from_rho();

        let mut sum_ex2 = 0.0f64;
        let mut sum_ey2 = 0.0f64;
        for i in 0..(m * m) {
            sum_ex2 += solver.e_x[i] as f64;
            sum_ey2 += solver.e_y[i] as f64;
        }
        assert!(sum_ex2.abs() < 1e-3, "Net Ex on mode (2, 2) must sum to 0, got {}", sum_ex2);
        assert!(sum_ey2.abs() < 1e-3, "Net Ey on mode (2, 2) must sum to 0, got {}", sum_ey2);
    }

    #[test]
    fn test_gcd_density_solver() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let def_path = design_dir("gcd").join("gcd.placed.def");

        if !tlef_path.exists() || !lef_path.exists() || !def_path.exists() {
            panic!("Required integration test files missing: tlef={:?}, lef={:?}, def={:?}",
                   tlef_path, lef_path, def_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");
        let design_db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        let mut solver = DensitySolver::new(&design_db, &tech_db, 0.60, Some(64));

        // 1. Verify core bounds from rows match (5.52, 10.88) to (86.02, 89.76) um
        assert!((solver.core_xl_um - 5.52).abs() < 1e-3, "core_xl mismatch: {}", solver.core_xl_um);
        assert!((solver.core_yl_um - 10.88).abs() < 1e-3, "core_yl mismatch: {}", solver.core_yl_um);
        assert!((solver.core_xh_um - 86.02).abs() < 1e-3, "core_xh mismatch: {}", solver.core_xh_um);
        assert!((solver.core_yh_um - 89.76).abs() < 1e-3, "core_yh mismatch: {}", solver.core_yh_um);

        // 2. Movable cells count is 233
        let movable_count = solver.movable_cell_mask.iter().filter(|&&b| b).count();
        assert_eq!(movable_count, 233, "Movable cell count must equal 233 logic instances");

        // 3. Test placed DEF density & forces
        let num_cells = design_db.num_cells();
        let dbu_per_um = design_db.dbu_per_micron as f32;
        let mut placed_x_um = vec![0.0f32; num_cells];
        let mut placed_y_um = vec![0.0f32; num_cells];
        for c in 0..num_cells {
            placed_x_um[c] = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
            placed_y_um[c] = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
        }

        let mut force_x = vec![0.0f32; num_cells];
        let mut force_y = vec![0.0f32; num_cells];

        let t0 = std::time::Instant::now();
        let (tau_placed, psi_placed) = solver.compute_density_and_forces(
            &placed_x_um,
            &placed_y_um,
            &mut force_x,
            &mut force_y,
        );
        let eval_time = t0.elapsed();
        println!(
            "Density solve time on gcd: {:?}, tau_placed: {}, psi_placed: {}",
            eval_time, tau_placed, psi_placed
        );

        // Legalized placement has low overflow compared to clustered center (0.293 vs 0.998).
        // Note: In row-legalized placement, standard cells abut each other with local density 1.0.
        // At M = 64, bins (1.23 um tall) are smaller than cell height (2.72 um), so bins covered
        // by cell silicon have density 1.0 > target_density (0.60), giving tau_placed ≈ 0.293.
        assert!(
            tau_placed < 0.35,
            "Legalized placement overflow must be < 0.35, got {}",
            tau_placed
        );

        // Also test default grid sizing (None -> M = 16 on gcd)
        let mut solver_16 = DensitySolver::new(&design_db, &tech_db, 0.60, None);
        assert_eq!(solver_16.m, 16, "Default grid sizing for gcd must choose M = 16");
        let mut force_x_16 = vec![0.0f32; num_cells];
        let mut force_y_16 = vec![0.0f32; num_cells];
        let (tau_placed_16, _) = solver_16.compute_density_and_forces(
            &placed_x_um,
            &placed_y_um,
            &mut force_x_16,
            &mut force_y_16,
        );
        assert!(
            tau_placed_16 < 0.05,
            "Placed overflow on M=16 grid must be < 0.05, got {}",
            tau_placed_16
        );

        // Mass conservation test: sum of movable_area equals total_movable_area_um2
        let sum_movable_area: f64 = solver.movable_area.iter().map(|&a| a as f64).sum();
        let area_diff = (sum_movable_area - solver.total_movable_area_um2 as f64).abs();
        let rel_diff = area_diff / (solver.total_movable_area_um2 as f64);
        assert!(
            rel_diff < 1e-4,
            "Mass conservation failed: sum_movable_area={}, total_movable_area={}, rel_diff={}",
            sum_movable_area, solver.total_movable_area_um2, rel_diff
        );

        // 4. Clustered center test
        let x_center = (solver.core_xl_um + solver.core_xh_um) * 0.5;
        let y_center = (solver.core_yl_um + solver.core_yh_um) * 0.5;

        let mut center_x_um = vec![0.0f32; num_cells];
        let mut center_y_um = vec![0.0f32; num_cells];
        for c in 0..num_cells {
            if solver.movable_cell_mask[c] {
                // Distribute slightly around center (-0.2 um to +0.2 um)
                let offset_x = ((c % 7) as f32 - 3.0) * 0.05;
                let offset_y = (((c / 7) % 7) as f32 - 3.0) * 0.05;
                center_x_um[c] = x_center + offset_x - solver.cell_w_um[c] * 0.5;
                center_y_um[c] = y_center + offset_y - solver.cell_h_um[c] * 0.5;
            }
        }

        let (tau_center, _psi_center) = solver.compute_density_and_forces(
            &center_x_um,
            &center_y_um,
            &mut force_x,
            &mut force_y,
        );

        assert!(
            tau_center > 0.85,
            "Clustered center overflow must be > 0.85, got {}",
            tau_center
        );

        // Verify repulsive force signs: cells with center < x_center have Fx < 0, cells with center > x_center have Fx > 0
        let mut left_count = 0;
        let mut right_count = 0;
        for c in 0..num_cells {
            if !solver.movable_cell_mask[c] {
                assert_eq!(force_x[c], 0.0, "Fixed cell {} must have force_x == 0", c);
                assert_eq!(force_y[c], 0.0, "Fixed cell {} must have force_y == 0", c);
                continue;
            }
            let cell_cx = center_x_um[c] + solver.cell_w_um[c] * 0.5;
            if cell_cx < x_center - 0.01 {
                assert!(
                    force_x[c] < 0.0,
                    "Cell {} left of center (cx={}) must have negative Fx (repelled left), got {}",
                    c, cell_cx, force_x[c]
                );
                left_count += 1;
            } else if cell_cx > x_center + 0.01 {
                assert!(
                    force_x[c] > 0.0,
                    "Cell {} right of center (cx={}) must have positive Fx (repelled right), got {}",
                    c, cell_cx, force_x[c]
                );
                right_count += 1;
            }
        }
        assert!(left_count > 0 && right_count > 0, "Must have cells on both left and right of center");
    }
}
