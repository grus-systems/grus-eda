use crate::design_db::{DesignDb, PlacementStatus};
use crate::interner::{CellId, NetId, PinId};
use crate::tech_db::{MacroClass, TechDb};
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct WaWirelengthPrecomp {
    pub dbu_per_um: f32,
    pub pin_dx_um: Vec<f32>, // length num_pins: offset from cell lower-left (or absolute I/O x_um)
    pub pin_dy_um: Vec<f32>, // length num_pins: offset from cell lower-left (or absolute I/O y_um)
    pub movable_cell_mask: Vec<bool>, // length num_cells: true if movable Core/CoreTie cell
}

impl WaWirelengthPrecomp {
    pub fn new(design_db: &DesignDb, tech_db: &TechDb) -> Self {
        let dbu_per_um = design_db.dbu_per_micron.max(1) as f32;
        let num_pins = design_db.num_pins();
        let mut pin_dx_um = vec![0.0f32; num_pins];
        let mut pin_dy_um = vec![0.0f32; num_pins];

        for p in 0..num_pins {
            match design_db.pin_cells[p] {
                Some(c) => {
                    let cell_idx = c.0 as usize;
                    if design_db.cell_status[cell_idx] == PlacementStatus::Unplaced {
                        pin_dx_um[p] = design_db.pin_offset_x_dbu[p] as f32 / dbu_per_um;
                        pin_dy_um[p] = design_db.pin_offset_y_dbu[p] as f32 / dbu_per_um;
                    } else {
                        let (px_dbu, py_dbu) = design_db.pin_pos_dbu(PinId(p as u32), tech_db);
                        pin_dx_um[p] =
                            (px_dbu - design_db.cell_x_dbu[cell_idx]) as f32 / dbu_per_um;
                        pin_dy_um[p] =
                            (py_dbu - design_db.cell_y_dbu[cell_idx]) as f32 / dbu_per_um;
                    }
                }
                None => {
                    pin_dx_um[p] = design_db.pin_offset_x_dbu[p] as f32 / dbu_per_um;
                    pin_dy_um[p] = design_db.pin_offset_y_dbu[p] as f32 / dbu_per_um;
                }
            }
        }

        let num_cells = design_db.num_cells();
        let mut movable_cell_mask = vec![false; num_cells];
        for (c, mask) in movable_cell_mask.iter_mut().enumerate() {
            let status = design_db.cell_status[c];
            let macro_id = design_db.cell_macros[c];
            let class = tech_db.macros[macro_id.0 as usize].class;
            *mask = matches!(status, PlacementStatus::Placed | PlacementStatus::Unplaced)
                && matches!(class, MacroClass::Core | MacroClass::CoreTie);
        }

        Self {
            dbu_per_um,
            pin_dx_um,
            pin_dy_um,
            movable_cell_mask,
        }
    }
}

/// Computes the exact Half-Perimeter Wirelength (HPWL) in microns.
pub fn compute_hpwl_um(
    design_db: &DesignDb,
    precomp: &WaWirelengthPrecomp,
    cell_x_um: &[f32],
    cell_y_um: &[f32],
) -> f64 {
    assert_eq!(cell_x_um.len(), design_db.num_cells());
    assert_eq!(cell_y_um.len(), design_db.num_cells());

    let mut total_hpwl: f64 = 0.0;
    for net in 0..design_db.num_nets() {
        let pins = design_db.net_pins(NetId(net as u32));
        if pins.len() <= 1 {
            continue;
        }

        let mut x_min = f32::INFINITY;
        let mut x_max = f32::NEG_INFINITY;
        let mut y_min = f32::INFINITY;
        let mut y_max = f32::NEG_INFINITY;

        for &pin_id in pins {
            let p = pin_id.0 as usize;
            let (xp, yp) = match design_db.pin_cells[p] {
                Some(cell_id) => {
                    let c = cell_id.0 as usize;
                    (
                        cell_x_um[c] + precomp.pin_dx_um[p],
                        cell_y_um[c] + precomp.pin_dy_um[p],
                    )
                }
                None => (precomp.pin_dx_um[p], precomp.pin_dy_um[p]),
            };
            x_min = x_min.min(xp);
            x_max = x_max.max(xp);
            y_min = y_min.min(yp);
            y_max = y_max.max(yp);
        }

        let dx = (x_max - x_min).max(0.0);
        let dy = (y_max - y_min).max(0.0);
        total_hpwl += (dx + dy) as f64;
    }
    total_hpwl
}

/// Computes the smooth Weighted-Average (WA) wirelength and analytical gradients.
///
/// Implements the three mandatory f32 numerical safeguards:
/// 1. Exponent Shift: t_p^+ = x_p - x_max <= 0 and t_p^- = x_p - x_min >= 0,
///    so exp(t_p^+ / gamma) and exp(-t_p^- / gamma) are always in (0, 1] and never overflow.
/// 2. Shifted Coordinates Inside B_e^\pm and Gradient: evaluates (x_p - x_max) inside B_e^+
///    and the gradient, completely eliminating catastrophic cancellation.
/// 3. f64 Scalar Accumulator for global reductions.
#[derive(Clone, Copy, Default)]
struct PinWaCache {
    p: usize,
    xp: f64,
    yp: f64,
    t_x_plus: f64,
    t_x_minus: f64,
    ap_x: f64,
    am_x: f64,
    t_y_plus: f64,
    t_y_minus: f64,
    ap_y: f64,
    am_y: f64,
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn eval_net_wa(
    design_db: &DesignDb,
    precomp: &WaWirelengthPrecomp,
    cell_x_um: &[f32],
    cell_y_um: &[f32],
    net: usize,
    inv_gamma: f64,
    gamma_f64: f64,
    pin_cache: &mut Vec<PinWaCache>,
    mut emit_pin_grad: impl FnMut(usize, f32, f32),
) -> f64 {
    let pins = design_db.net_pins(NetId(net as u32));
    if pins.len() <= 1 {
        return 0.0;
    }

    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    pin_cache.clear();
    for &pin_id in pins {
        let p = pin_id.0 as usize;
        let (xp, yp) = match design_db.pin_cells[p] {
            Some(cell_id) => {
                let c = cell_id.0 as usize;
                (
                    cell_x_um[c] as f64 + precomp.pin_dx_um[p] as f64,
                    cell_y_um[c] as f64 + precomp.pin_dy_um[p] as f64,
                )
            }
            None => (precomp.pin_dx_um[p] as f64, precomp.pin_dy_um[p] as f64),
        };
        x_min = x_min.min(xp);
        x_max = x_max.max(xp);
        y_min = y_min.min(yp);
        y_max = y_max.max(yp);
        pin_cache.push(PinWaCache {
            p,
            xp,
            yp,
            ..Default::default()
        });
    }

    let mut a_x_plus = 0.0f64;
    let mut b_x_plus = 0.0f64;
    let mut a_x_minus = 0.0f64;
    let mut b_x_minus = 0.0f64;

    let mut a_y_plus = 0.0f64;
    let mut b_y_plus = 0.0f64;
    let mut a_y_minus = 0.0f64;
    let mut b_y_minus = 0.0f64;

    for entry in pin_cache.iter_mut() {
        let t_x_plus = entry.xp - x_max;
        let t_x_minus = entry.xp - x_min;
        let ap_x = (t_x_plus * inv_gamma).exp();
        let am_x = (-t_x_minus * inv_gamma).exp();
        a_x_plus += ap_x;
        b_x_plus += t_x_plus * ap_x;
        a_x_minus += am_x;
        b_x_minus += t_x_minus * am_x;

        let t_y_plus = entry.yp - y_max;
        let t_y_minus = entry.yp - y_min;
        let ap_y = (t_y_plus * inv_gamma).exp();
        let am_y = (-t_y_minus * inv_gamma).exp();
        a_y_plus += ap_y;
        b_y_plus += t_y_plus * ap_y;
        a_y_minus += am_y;
        b_y_minus += t_y_minus * am_y;

        entry.t_x_plus = t_x_plus;
        entry.t_x_minus = t_x_minus;
        entry.ap_x = ap_x;
        entry.am_x = am_x;
        entry.t_y_plus = t_y_plus;
        entry.t_y_minus = t_y_minus;
        entry.ap_y = ap_y;
        entry.am_y = am_y;
    }

    let inv_a_x_plus = 1.0 / a_x_plus;
    let inv_a_x_minus = 1.0 / a_x_minus;
    let inv_a_y_plus = 1.0 / a_y_plus;
    let inv_a_y_minus = 1.0 / a_y_minus;

    let wx = (x_max - x_min) + b_x_plus * inv_a_x_plus - b_x_minus * inv_a_x_minus;
    let wy = (y_max - y_min) + b_y_plus * inv_a_y_plus - b_y_minus * inv_a_y_minus;

    let inv_gamma_a_x_plus_sq = inv_gamma * (inv_a_x_plus * inv_a_x_plus);
    let inv_gamma_a_x_minus_sq = inv_gamma * (inv_a_x_minus * inv_a_x_minus);
    let inv_gamma_a_y_plus_sq = inv_gamma * (inv_a_y_plus * inv_a_y_plus);
    let inv_gamma_a_y_minus_sq = inv_gamma * (inv_a_y_minus * inv_a_y_minus);

    for entry in pin_cache.iter() {
        let grad_x_plus = entry.ap_x
            * inv_gamma_a_x_plus_sq
            * ((gamma_f64 + entry.t_x_plus) * a_x_plus - b_x_plus);
        let grad_x_minus = entry.am_x
            * inv_gamma_a_x_minus_sq
            * ((gamma_f64 - entry.t_x_minus) * a_x_minus + b_x_minus);

        let grad_y_plus = entry.ap_y
            * inv_gamma_a_y_plus_sq
            * ((gamma_f64 + entry.t_y_plus) * a_y_plus - b_y_plus);
        let grad_y_minus = entry.am_y
            * inv_gamma_a_y_minus_sq
            * ((gamma_f64 - entry.t_y_minus) * a_y_minus + b_y_minus);

        emit_pin_grad(
            entry.p,
            (grad_x_plus - grad_x_minus) as f32,
            (grad_y_plus - grad_y_minus) as f32,
        );
    }

    wx + wy
}

#[allow(clippy::too_many_arguments)]
pub fn compute_wa_wirelength_and_grad(
    design_db: &DesignDb,
    precomp: &WaWirelengthPrecomp,
    cell_x_um: &[f32],
    cell_y_um: &[f32],
    gamma: f32,
    pin_grad_x: &mut [f32],
    pin_grad_y: &mut [f32],
    cell_grad_x: &mut [f32],
    cell_grad_y: &mut [f32],
) -> f64 {
    let num_cells = design_db.num_cells();
    let num_pins = design_db.num_pins();
    let num_nets = design_db.num_nets();

    assert_eq!(cell_x_um.len(), num_cells);
    assert_eq!(cell_y_um.len(), num_cells);
    assert_eq!(pin_grad_x.len(), num_pins);
    assert_eq!(pin_grad_y.len(), num_pins);
    assert_eq!(cell_grad_x.len(), num_cells);
    assert_eq!(cell_grad_y.len(), num_cells);

    pin_grad_x.fill(0.0);
    pin_grad_y.fill(0.0);
    cell_grad_x.fill(0.0);
    cell_grad_y.fill(0.0);

    let gamma_f64 = gamma as f64;
    let inv_gamma = 1.0f64 / gamma_f64;

    let total_wl = if num_nets < 1024 {
        let mut total_wl: f64 = 0.0;
        let mut pin_cache = Vec::new();
        for net in 0..num_nets {
            total_wl += eval_net_wa(
                design_db,
                precomp,
                cell_x_um,
                cell_y_um,
                net,
                inv_gamma,
                gamma_f64,
                &mut pin_cache,
                |p, gx, gy| {
                    pin_grad_x[p] = gx;
                    pin_grad_y[p] = gy;
                },
            );
        }
        total_wl
    } else {
        const CHUNK: usize = 1024;
        let mut net_wl = vec![0.0f64; num_nets];
        let chunk_grads: Vec<Vec<(usize, f32, f32)>> = net_wl
            .par_chunks_mut(CHUNK)
            .enumerate()
            .map_init(Vec::new, |pin_cache, (chunk_idx, wl_slice)| {
                let net_start = chunk_idx * CHUNK;
                let mut grads = Vec::with_capacity(CHUNK * 4);
                for (offset, wl_slot) in wl_slice.iter_mut().enumerate() {
                    *wl_slot = eval_net_wa(
                        design_db,
                        precomp,
                        cell_x_um,
                        cell_y_um,
                        net_start + offset,
                        inv_gamma,
                        gamma_f64,
                        pin_cache,
                        |p, gx, gy| grads.push((p, gx, gy)),
                    );
                }
                grads
            })
            .collect();
        for grads in chunk_grads {
            for (p, gx, gy) in grads {
                pin_grad_x[p] = gx;
                pin_grad_y[p] = gy;
            }
        }
        net_wl.iter().sum()
    };

    // Cell gradient gathering across movable cells
    let gather_cell = |c: usize, gx_out: &mut f32, gy_out: &mut f32| {
        if precomp.movable_cell_mask[c] {
            let mut gx = 0.0f32;
            let mut gy = 0.0f32;
            for &pin_id in design_db.cell_pins(CellId(c as u32)) {
                let p = pin_id.0 as usize;
                gx += pin_grad_x[p];
                gy += pin_grad_y[p];
            }
            *gx_out = gx;
            *gy_out = gy;
        } else {
            *gx_out = 0.0;
            *gy_out = 0.0;
        }
    };

    if num_cells < 2048 {
        for c in 0..num_cells {
            gather_cell(c, &mut cell_grad_x[c], &mut cell_grad_y[c]);
        }
    } else {
        cell_grad_x
            .par_iter_mut()
            .zip(cell_grad_y.par_iter_mut())
            .enumerate()
            .for_each(|(c, (gx, gy))| gather_cell(c, gx, gy));
    }

    total_wl
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_db::Orient;
    use crate::interner::{MacroId, MacroPinId, PinDir, StringInterner};
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use crate::tech_db::{MacroClass, MacroDef, MacroPin, RectDbu};
    use crate::test_support::{design_dir, sky130_hd};

    #[test]
    fn test_synthetic_wa_wirelength_and_gradient() {
        // Construct a synthetic 3-pin net design with 3 movable cells
        let mut tech_db = TechDb::new();
        tech_db.dbu_per_micron = 1000;
        let mut interner = StringInterner::new();

        let m_name = interner.intern("TEST_MACRO");
        let mp_name = interner.intern("A");
        tech_db.macro_pins.push(MacroPin {
            name: mp_name,
            dir: PinDir::Input,
            is_power: false,
            offset_x_dbu: 200,
            offset_y_dbu: 300,
            offset_x_frac: false,
            offset_y_frac: false,
            layer: None,
        rects: Vec::new(),
        });

        let mac = MacroDef {
            name: m_name,
            class: MacroClass::Core,
            width_dbu: 1000,
            height_dbu: 2000,
            pin_offset: 0,
            pin_count: 1,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        };
        tech_db.macros.push(mac);
        tech_db.macro_by_name.insert(m_name, MacroId(0));

        let mut design_db = DesignDb::new();
        design_db.dbu_per_micron = 1000;
        design_db.die_area = RectDbu::new(0, 0, 10000000, 10000000);

        // 3 cells
        for i in 0..3 {
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

            // Pin
            let pname = interner.intern(&format!("p{}", i));
            design_db.pin_names.push(pname);
            design_db.pin_cells.push(Some(CellId(i as u32)));
            design_db.pin_nets.push(Some(NetId(0)));
            design_db.pin_macro_pins.push(Some(MacroPinId(0)));
            design_db.pin_lib_pins.push(None);
            design_db.pin_dirs.push(PinDir::Input);
            design_db.pin_offset_x_dbu.push(200); // 0.2 um
            design_db.pin_offset_y_dbu.push(300); // 0.3 um
            design_db.pin_io_layer.push(None);
            design_db.pin_io_rects.push(Vec::new());
            design_db.pin_is_driver.push(i == 0);
        }
        design_db.cell_pin_offsets.push(3);

        // 1 net connecting all 3 pins
        let nname = interner.intern("net0");
        design_db.net_names.push(nname);
        design_db.net_pin_offsets.push(0);
        design_db.net_pin_ids.extend([PinId(0), PinId(1), PinId(2)]);
        design_db.net_pin_offsets.push(3);

        let precomp = WaWirelengthPrecomp::new(&design_db, &tech_db);
        assert_eq!(precomp.movable_cell_mask, vec![true, true, true]);

        let mut cell_x_um = vec![10.0f32, 50.0f32, 100.0f32];
        let mut cell_y_um = vec![20.0f32, 80.0f32, 30.0f32];
        let gamma = 5.0f32;

        let mut pin_grad_x = vec![0.0f32; 3];
        let mut pin_grad_y = vec![0.0f32; 3];
        let mut cell_grad_x = vec![0.0f32; 3];
        let mut cell_grad_y = vec![0.0f32; 3];

        let wl_orig = compute_wa_wirelength_and_grad(
            &design_db,
            &precomp,
            &cell_x_um,
            &cell_y_um,
            gamma,
            &mut pin_grad_x,
            &mut pin_grad_y,
            &mut cell_grad_x,
            &mut cell_grad_y,
        );

        let pin_gx_orig = pin_grad_x.clone();
        let pin_gy_orig = pin_grad_y.clone();

        // 1. Shift Invariance Check: translating by (+5000.0, +5000.0) um
        let shift_x = vec![
            cell_x_um[0] + 5000.0,
            cell_x_um[1] + 5000.0,
            cell_x_um[2] + 5000.0,
        ];
        let shift_y = vec![
            cell_y_um[0] + 5000.0,
            cell_y_um[1] + 5000.0,
            cell_y_um[2] + 5000.0,
        ];

        let wl_shift = compute_wa_wirelength_and_grad(
            &design_db,
            &precomp,
            &shift_x,
            &shift_y,
            gamma,
            &mut pin_grad_x,
            &mut pin_grad_y,
            &mut cell_grad_x,
            &mut cell_grad_y,
        );

        println!(
            "wl_orig = {}, wl_shift = {}, diff = {}",
            wl_orig,
            wl_shift,
            (wl_orig - wl_shift).abs()
        );
        assert!(
            (wl_orig - wl_shift).abs() < 1e-4,
            "Shift invariance failed on wirelength: orig={}, shift={}, diff={}",
            wl_orig,
            wl_shift,
            (wl_orig - wl_shift).abs()
        );
        for i in 0..3 {
            assert!(
                (pin_gx_orig[i] - pin_grad_x[i]).abs() < 1e-6,
                "Shift invariance failed on pin_grad_x[{}]",
                i
            );
            assert!(
                (pin_gy_orig[i] - pin_grad_y[i]).abs() < 1e-6,
                "Shift invariance failed on pin_grad_y[{}]",
                i
            );
        }

        // 2. Central Finite Differences Check (h = 1.0 / 64.0)
        let h = 1.0f32 / 64.0f32;
        let mut dummy_pin_gx = vec![0.0f32; 3];
        let mut dummy_pin_gy = vec![0.0f32; 3];
        let mut dummy_cell_gx = vec![0.0f32; 3];
        let mut dummy_cell_gy = vec![0.0f32; 3];

        for c in 0..3 {
            // X direction
            let ox = cell_x_um[c];
            cell_x_um[c] = ox + h;
            let wl_p = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_x_um[c] = ox - h;
            let wl_m = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_x_um[c] = ox;

            let fd_x = (wl_p - wl_m) / ((2.0 * h) as f64);
            let ana_x = pin_gx_orig[c] as f64;
            let err_x = (ana_x - fd_x).abs();
            let rel_x = err_x / fd_x.abs().max(1e-6);
            assert!(
                rel_x < 1e-4,
                "Cell {} X grad mismatch: analytical={}, fd={}, rel_err={}",
                c,
                ana_x,
                fd_x,
                rel_x
            );

            // Y direction
            let oy = cell_y_um[c];
            cell_y_um[c] = oy + h;
            let wl_p = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_y_um[c] = oy - h;
            let wl_m = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_y_um[c] = oy;

            let fd_y = (wl_p - wl_m) / ((2.0 * h) as f64);
            let ana_y = pin_gy_orig[c] as f64;
            let err_y = (ana_y - fd_y).abs();
            let rel_y = err_y / fd_y.abs().max(1e-6);
            assert!(
                rel_y < 1e-4,
                "Cell {} Y grad mismatch: analytical={}, fd={}, rel_err={}",
                c,
                ana_y,
                fd_y,
                rel_y
            );
        }
    }

    #[test]
    fn test_gcd_wa_wirelength_and_gradient() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let def_path = design_dir("gcd").join("gcd.placed.def");

        if !tlef_path.exists() || !lef_path.exists() || !def_path.exists() {
            panic!(
                "Required integration test files missing: tlef={:?}, lef={:?}, def={:?}",
                tlef_path, lef_path, def_path
            );
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path).expect("Reference JSON");
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes).expect("Parse JSON");

        let precomp = WaWirelengthPrecomp::new(&design_db, &tech_db);

        // 1. Verify movable cell count matches logic instances
        let logic_instances = ref_json["logic_instances"].as_u64().unwrap() as usize;
        let movable_count = precomp.movable_cell_mask.iter().filter(|&&b| b).count();
        assert_eq!(
            movable_count, logic_instances,
            "Movable cell count must equal logic instances (233)"
        );

        let num_cells = design_db.num_cells();
        let mut cell_x_um = vec![0.0f32; num_cells];
        let mut cell_y_um = vec![0.0f32; num_cells];
        let dbu_per_um = design_db.dbu_per_micron as f32;

        for c in 0..num_cells {
            cell_x_um[c] = design_db.cell_x_dbu[c] as f32 / dbu_per_um;
            cell_y_um[c] = design_db.cell_y_dbu[c] as f32 / dbu_per_um;
        }

        // 2. Verify compute_hpwl_um matches reference placed_hpwl_um
        let placed_hpwl_um = ref_json["placed_hpwl_um"].as_f64().unwrap();
        let calc_hpwl_um = compute_hpwl_um(&design_db, &precomp, &cell_x_um, &cell_y_um);
        assert!(
            (calc_hpwl_um - placed_hpwl_um).abs() < 1e-3,
            "Calculated HPWL {} um must match reference {} um within 1e-3",
            calc_hpwl_um,
            placed_hpwl_um
        );

        // 3. Verify monotonic convergence of WA wirelength as gamma decreases
        let gammas = [10.0f32, 2.0, 0.5, 0.05, 0.001];
        let mut prev_wa = 0.0f64;
        let num_pins = design_db.num_pins();
        let mut pin_grad_x = vec![0.0f32; num_pins];
        let mut pin_grad_y = vec![0.0f32; num_pins];
        let mut cell_grad_x = vec![0.0f32; num_cells];
        let mut cell_grad_y = vec![0.0f32; num_cells];

        for &gamma in &gammas {
            let wa = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                gamma,
                &mut pin_grad_x,
                &mut pin_grad_y,
                &mut cell_grad_x,
                &mut cell_grad_y,
            );
            assert!(
                wa <= placed_hpwl_um + 1e-3,
                "WA wirelength ({}) must be bounded above by HPWL ({})",
                wa,
                placed_hpwl_um
            );
            assert!(
                wa > prev_wa,
                "WA wirelength must strictly increase as gamma decreases: gamma {} ({}) <= prev ({})",
                gamma,
                wa,
                prev_wa
            );
            prev_wa = wa;
        }

        // At gamma = 0.001 um, WA wirelength must be within 0.01 um of HPWL
        assert!(
            (prev_wa - placed_hpwl_um).abs() < 0.01,
            "WA wirelength at gamma=0.001 ({}) must be within 0.01 um of HPWL ({})",
            prev_wa,
            placed_hpwl_um
        );

        // 4. Verify analytical cell gradients against central finite differences at gamma = 5.0 um
        let eval_gamma = 5.0f32;
        let t0 = std::time::Instant::now();
        compute_wa_wirelength_and_grad(
            &design_db,
            &precomp,
            &cell_x_um,
            &cell_y_um,
            eval_gamma,
            &mut pin_grad_x,
            &mut pin_grad_y,
            &mut cell_grad_x,
            &mut cell_grad_y,
        );
        let eval_time = t0.elapsed();
        println!(
            "WA Wirelength + Gradient single evaluation time: {:?}",
            eval_time
        );

        let ana_gx = cell_grad_x.clone();
        let ana_gy = cell_grad_y.clone();

        let h = 1.0f32 / 128.0f32;
        let mut dummy_pin_gx = vec![0.0f32; num_pins];
        let mut dummy_pin_gy = vec![0.0f32; num_pins];
        let mut dummy_cell_gx = vec![0.0f32; num_cells];
        let mut dummy_cell_gy = vec![0.0f32; num_cells];

        for c in 0..num_cells {
            if !precomp.movable_cell_mask[c] {
                // Assert fixed cells have 0.0 gradient
                assert_eq!(
                    ana_gx[c], 0.0,
                    "Fixed cell {} must have cell_grad_x == 0",
                    c
                );
                assert_eq!(
                    ana_gy[c], 0.0,
                    "Fixed cell {} must have cell_grad_y == 0",
                    c
                );
                continue;
            }

            // X direction finite difference
            let ox = cell_x_um[c];
            let xp = ox + h;
            let xm = ox - h;

            cell_x_um[c] = xp;
            let wl_p = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                eval_gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_x_um[c] = xm;
            let wl_m = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                eval_gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_x_um[c] = ox;

            let fd_x = (wl_p - wl_m) / ((xp - xm) as f64);
            let ana_x = ana_gx[c] as f64;
            let diff_x = (ana_x - fd_x).abs();
            if fd_x.abs() > 1e-3 {
                let rel_x = diff_x / fd_x.abs();
                assert!(
                    rel_x < 1e-4,
                    "Cell {} X grad relative error too high: ana={}, fd={}, rel_err={}",
                    c,
                    ana_x,
                    fd_x,
                    rel_x
                );
            } else {
                assert!(
                    diff_x < 1e-6,
                    "Cell {} X grad absolute error too high: ana={}, fd={}, diff={}",
                    c,
                    ana_x,
                    fd_x,
                    diff_x
                );
            }

            // Y direction finite difference
            let oy = cell_y_um[c];
            let yp = oy + h;
            let ym = oy - h;

            cell_y_um[c] = yp;
            let wl_p = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                eval_gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_y_um[c] = ym;
            let wl_m = compute_wa_wirelength_and_grad(
                &design_db,
                &precomp,
                &cell_x_um,
                &cell_y_um,
                eval_gamma,
                &mut dummy_pin_gx,
                &mut dummy_pin_gy,
                &mut dummy_cell_gx,
                &mut dummy_cell_gy,
            );
            cell_y_um[c] = oy;

            let fd_y = (wl_p - wl_m) / ((yp - ym) as f64);
            let ana_y = ana_gy[c] as f64;
            let diff_y = (ana_y - fd_y).abs();
            if fd_y.abs() > 1e-3 {
                let rel_y = diff_y / fd_y.abs();
                assert!(
                    rel_y < 1e-4,
                    "Cell {} Y grad relative error too high: ana={}, fd={}, rel_err={}",
                    c,
                    ana_y,
                    fd_y,
                    rel_y
                );
            } else {
                assert!(
                    diff_y < 1e-6,
                    "Cell {} Y grad absolute error too high: ana={}, fd={}, diff={}",
                    c,
                    ana_y,
                    fd_y,
                    diff_y
                );
            }
        }
    }
}
