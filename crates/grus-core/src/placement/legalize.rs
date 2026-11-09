use std::collections::HashSet;
use anyhow::{bail, Context, Result};
use crate::design_db::{DesignDb, Orient, PlacementStatus};
use crate::interner::NetId;
use crate::tech_db::{MacroClass, TechDb};

#[derive(Clone, Debug)]
pub struct LegalizeResult {
    pub avg_displacement_um: f64,
    pub max_displacement_um: f64,
    pub pre_legal_hpwl_dbu: i64,
    pub post_abacus_hpwl_dbu: i64,
    pub final_hpwl_dbu: i64,
}

#[derive(Clone, Debug)]
pub struct AbacusCluster {
    pub x_dbu: i32,
    pub width_dbu: i32,
    pub weight_sum: f32,
    pub q_sum: f32,
    pub cells: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct SubRow {
    pub row_idx: usize,
    pub orig_x_dbu: i32,
    pub orig_y_dbu: i32,
    pub step_x_dbu: i32,
    pub orient: Orient,
    pub x_min_dbu: i32,
    pub x_max_dbu: i32,
    pub free_width: i32,
    pub clusters: Vec<AbacusCluster>,
    pub cells: Vec<usize>,
}

#[inline]
fn snap_to_site(x: i32, orig_x: i32, step_x: i32, min_x: i32, max_x: i32) -> i32 {
    let offset = x - orig_x;
    let k = (offset as f64 / step_x as f64).round() as i32;
    let snapped = orig_x + k * step_x;
    snapped.clamp(min_x, max_x)
}

#[inline]
fn snap_down(x: i32, orig_x: i32, step_x: i32) -> i32 {
    let offset = x - orig_x;
    let k = (offset as f64 / step_x as f64).floor() as i32;
    orig_x + k * step_x
}

#[inline]
fn snap_up(x: i32, orig_x: i32, step_x: i32) -> i32 {
    let offset = x - orig_x;
    let k = (offset as f64 / step_x as f64).ceil() as i32;
    orig_x + k * step_x
}

fn add_cell_to_clusters(
    clusters: &mut Vec<AbacusCluster>,
    cell: usize,
    w_i: i32,
    x_gp: i32,
    sub_row: &SubRow,
) {
    let x_init = snap_to_site(
        x_gp,
        sub_row.orig_x_dbu,
        sub_row.step_x_dbu,
        sub_row.x_min_dbu,
        sub_row.x_max_dbu - w_i,
    );
    let singleton = AbacusCluster {
        x_dbu: x_init,
        width_dbu: w_i,
        weight_sum: 1.0,
        q_sum: x_gp as f32,
        cells: vec![cell],
    };
    clusters.push(singleton);

    while clusters.len() >= 2 {
        let n = clusters.len();
        if clusters[n - 2].x_dbu + clusters[n - 2].width_dbu <= clusters[n - 1].x_dbu {
            break;
        }
        let c_last = clusters.pop().unwrap();
        let c_prev = clusters.last_mut().unwrap();
        c_prev.q_sum += c_last.q_sum - c_last.weight_sum * (c_prev.width_dbu as f32);
        c_prev.weight_sum += c_last.weight_sum;
        c_prev.width_dbu += c_last.width_dbu;
        c_prev.x_dbu = snap_to_site(
            (c_prev.q_sum / c_prev.weight_sum).round() as i32,
            sub_row.orig_x_dbu,
            sub_row.step_x_dbu,
            sub_row.x_min_dbu,
            sub_row.x_max_dbu - c_prev.width_dbu,
        );
        c_prev.cells.extend(c_last.cells);
    }
}

fn net_hpwl_dbu(db: &DesignDb, tech_db: &TechDb, net_idx: usize) -> i64 {
    let nid = NetId(net_idx as u32);
    let pins = db.net_pins(nid);
    if pins.len() <= 1 {
        return 0;
    }
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for &pin in pins {
        let (px, py) = db.pin_pos_dbu(pin, tech_db);
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
    }
    if max_x >= min_x && max_y >= min_y {
        (max_x - min_x) as i64 + (max_y - min_y) as i64
    } else {
        0
    }
}

pub fn legalize_placement(
    design_db: &mut DesignDb,
    tech_db: &TechDb,
    gp_x_um: &[f32],
    gp_y_um: &[f32],
) -> Result<LegalizeResult> {
    if design_db.rows.is_empty() {
        bail!("Cannot legalize placement: design has no PlacementRows");
    }

    let dbu_per_um = design_db.dbu_per_micron.max(1) as f32;
    let site_height = tech_db.site_height_dbu;
    let num_cells = design_db.num_cells();

    // Precompute cell dimensions in DBU
    let mut cell_w_dbu = vec![0i32; num_cells];
    let mut cell_h_dbu = vec![0i32; num_cells];
    let mut movable_mask = vec![false; num_cells];

    for c in 0..num_cells {
        let macro_id = design_db.cell_macros[c];
        if macro_id.0 as usize >= tech_db.macros.len() {
            continue;
        }
        let mac = &tech_db.macros[macro_id.0 as usize];
        let is_rot = matches!(
            design_db.cell_orient[c],
            Orient::E | Orient::W | Orient::FE | Orient::FW
        );
        let (w, h) = if is_rot {
            (mac.height_dbu, mac.width_dbu)
        } else {
            (mac.width_dbu, mac.height_dbu)
        };
        cell_w_dbu[c] = w;
        cell_h_dbu[c] = h;

        let status = design_db.cell_status[c];
        let class = mac.class;
        if (status == PlacementStatus::Placed || status == PlacementStatus::Unplaced)
            && (class == MacroClass::Core || class == MacroClass::CoreTie)
        {
            movable_mask[c] = true;
        }
    }

    // Step 1: Collect row sub-segments by splitting around fixed obstacles
    let mut row_indices: Vec<usize> = (0..design_db.rows.len()).collect();
    row_indices.sort_by_key(|&r| design_db.rows[r].orig_y_dbu);

    let mut sub_rows = Vec::new();
    for &r_idx in &row_indices {
        let r = &design_db.rows[r_idx];
        let row_xl = r.orig_x_dbu;
        let row_xh = r.orig_x_dbu + r.num_x * r.step_x_dbu;
        let row_yl = r.orig_y_dbu;
        let row_yh = r.orig_y_dbu + site_height;

        // Collect non-movable cells overlapping this row
        let mut obs_intervals = Vec::new();
        for f in 0..num_cells {
            if movable_mask[f] {
                continue;
            }
            if design_db.cell_status[f] != PlacementStatus::Fixed
                && design_db.cell_status[f] != PlacementStatus::Cover
            {
                continue;
            }
            let cy = design_db.cell_y_dbu[f];
            let ch = cell_h_dbu[f];
            if cy < row_yh && cy + ch > row_yl {
                let cx = design_db.cell_x_dbu[f];
                let cw = cell_w_dbu[f];
                let ox_l = snap_down(cx, r.orig_x_dbu, r.step_x_dbu).max(row_xl);
                let ox_h = snap_up(cx + cw, r.orig_x_dbu, r.step_x_dbu).min(row_xh);
                if ox_h > ox_l {
                    obs_intervals.push((ox_l, ox_h));
                }
            }
        }

        // Merge overlapping obstacle intervals
        obs_intervals.sort_unstable();
        let mut merged_obs: Vec<(i32, i32)> = Vec::new();
        for (ox_l, ox_h) in obs_intervals {
            match merged_obs.last_mut() {
                Some(last) if ox_l <= last.1 => {
                    last.1 = last.1.max(ox_h);
                }
                _ => {
                    merged_obs.push((ox_l, ox_h));
                }
            }
        }

        // Extract complementary free SubRow intervals
        let mut cur_x = row_xl;
        for (ox_l, ox_h) in merged_obs {
            if ox_l > cur_x {
                sub_rows.push(SubRow {
                    row_idx: r_idx,
                    orig_x_dbu: r.orig_x_dbu,
                    orig_y_dbu: r.orig_y_dbu,
                    step_x_dbu: r.step_x_dbu,
                    orient: r.orient,
                    x_min_dbu: cur_x,
                    x_max_dbu: ox_l,
                    free_width: ox_l - cur_x,
                    clusters: Vec::new(),
                    cells: Vec::new(),
                });
            }
            cur_x = cur_x.max(ox_h);
        }
        if cur_x < row_xh {
            sub_rows.push(SubRow {
                row_idx: r_idx,
                orig_x_dbu: r.orig_x_dbu,
                orig_y_dbu: r.orig_y_dbu,
                step_x_dbu: r.step_x_dbu,
                orient: r.orient,
                x_min_dbu: cur_x,
                x_max_dbu: row_xh,
                free_width: row_xh - cur_x,
                clusters: Vec::new(),
                cells: Vec::new(),
            });
        }
    }

    if sub_rows.is_empty() {
        bail!("No free SubRows available for placement");
    }

    // Step 2: Convert global placement coordinates to DBU and measure pre-legal HPWL
    let mut x_gp_dbu = vec![0i32; num_cells];
    let mut y_gp_dbu = vec![0i32; num_cells];
    for c in 0..num_cells {
        if movable_mask[c] {
            x_gp_dbu[c] = (gp_x_um[c] * dbu_per_um).round() as i32;
            y_gp_dbu[c] = (gp_y_um[c] * dbu_per_um).round() as i32;
            design_db.cell_x_dbu[c] = x_gp_dbu[c];
            design_db.cell_y_dbu[c] = y_gp_dbu[c];
            design_db.cell_status[c] = PlacementStatus::Placed;
        } else {
            x_gp_dbu[c] = design_db.cell_x_dbu[c];
            y_gp_dbu[c] = design_db.cell_y_dbu[c];
        }
    }
    let pre_legal_hpwl_dbu = design_db.compute_hpwl_dbu(tech_db);

    // Sort movable cells by x_gp_dbu ascending
    let mut movable_cells: Vec<usize> = (0..num_cells).filter(|&c| movable_mask[c]).collect();
    movable_cells.sort_by_key(|&c| (x_gp_dbu[c], y_gp_dbu[c], c));

    // Step 3: Abacus Single-Row Dynamic Clustering
    for &cell in &movable_cells {
        let w_i = cell_w_dbu[cell];
        let x_gp = x_gp_dbu[cell];
        let y_gp = y_gp_dbu[cell];

        // Search candidate sub-rows within +/- 10 rows that have capacity
        let max_y_dist = 10 * site_height;
        let mut best_subrow_idx = None;
        let mut best_disp = i64::MAX;
        let mut best_trial_clusters = Vec::new();

        let mut pass_candidates: Vec<usize> = sub_rows
            .iter()
            .enumerate()
            .filter(|(_, sr)| sr.free_width >= w_i && (sr.orig_y_dbu - y_gp).abs() <= max_y_dist)
            .map(|(idx, _)| idx)
            .collect();

        if pass_candidates.is_empty() {
            // Fallback to all sub-rows with capacity
            pass_candidates = sub_rows
                .iter()
                .enumerate()
                .filter(|(_, sr)| sr.free_width >= w_i)
                .map(|(idx, _)| idx)
                .collect();
        }

        if pass_candidates.is_empty() {
            bail!("Unable to place cell {}: no SubRow with remaining capacity {}", cell, w_i);
        }

        for &sr_idx in &pass_candidates {
            let sr = &sub_rows[sr_idx];
            let mut trial_clusters = sr.clusters.clone();
            add_cell_to_clusters(&mut trial_clusters, cell, w_i, x_gp, sr);

            let c_last = trial_clusters.last().unwrap();
            let x_legal = c_last.x_dbu + c_last.width_dbu - w_i;
            let y_legal = sr.orig_y_dbu;

            let dx = (x_legal - x_gp) as i64;
            let dy = (y_legal - y_gp) as i64;
            let disp = dx * dx + dy * dy;

            if disp < best_disp {
                best_disp = disp;
                best_subrow_idx = Some(sr_idx);
                best_trial_clusters = trial_clusters;
            }
        }

        let chosen_idx = best_subrow_idx.unwrap();
        sub_rows[chosen_idx].clusters = best_trial_clusters;
        sub_rows[chosen_idx].free_width -= w_i;
    }

    // Unpack Abacus clusters into sub-row cell ordering and write positions to design_db
    for sub_row in &mut sub_rows {
        sub_row.cells.clear();
        for cluster in &sub_row.clusters {
            let mut cur_x = cluster.x_dbu;
            for &c in &cluster.cells {
                design_db.cell_x_dbu[c] = cur_x;
                design_db.cell_y_dbu[c] = sub_row.orig_y_dbu;
                design_db.cell_orient[c] = sub_row.orient;
                design_db.cell_status[c] = PlacementStatus::Placed;
                sub_row.cells.push(c);
                cur_x += cell_w_dbu[c];
            }
        }
    }
    let post_abacus_hpwl_dbu = design_db.compute_hpwl_dbu(tech_db);
    verify_legality(design_db, tech_db).context("verify_legality failed right after Abacus")?;

    // Step 4: Post-Abacus Local Detailed Placement Refinement (4 Rounds)
    // Precompute net-to-cells and cell-to-nets mappings
    let mut cell_nets: Vec<Vec<usize>> = vec![Vec::new(); num_cells];
    for (c, nets_entry) in cell_nets.iter_mut().enumerate() {
        let start = design_db.cell_pin_offsets[c] as usize;
        let end = design_db.cell_pin_offsets[c + 1] as usize;
        let mut nets = HashSet::new();
        for &p in &design_db.cell_pin_ids[start..end] {
            if let Some(net) = design_db.pin_nets[p.0 as usize] {
                nets.insert(net.0 as usize);
            }
        }
        *nets_entry = nets.into_iter().collect();
    }

    for _round in 0..4 {
        // Pass 1: Equal-width cell swaps
        for i in 0..movable_cells.len() {
            let c1 = movable_cells[i];
            let w1 = cell_w_dbu[c1];
            let x1 = design_db.cell_x_dbu[c1];
            let y1 = design_db.cell_y_dbu[c1];
            let o1 = design_db.cell_orient[c1];

            for &c2 in &movable_cells[(i + 1)..] {
                if cell_w_dbu[c2] != w1 {
                    continue;
                }
                let x2 = design_db.cell_x_dbu[c2];
                let y2 = design_db.cell_y_dbu[c2];
                let o2 = design_db.cell_orient[c2];

                if (y1 - y2).abs() > 4 * site_height || (x1 - x2).abs() > 25_000 {
                    continue;
                }

                // Collect incident nets
                let mut incident: Vec<usize> = cell_nets[c1].to_vec();
                for &net in &cell_nets[c2] {
                    if !incident.contains(&net) {
                        incident.push(net);
                    }
                }
                if incident.is_empty() {
                    continue;
                }

                let hpwl_before: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();

                design_db.cell_x_dbu[c1] = x2;
                design_db.cell_y_dbu[c1] = y2;
                design_db.cell_orient[c1] = o2;
                design_db.cell_x_dbu[c2] = x1;
                design_db.cell_y_dbu[c2] = y1;
                design_db.cell_orient[c2] = o1;

                let hpwl_after: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();

                if hpwl_after < hpwl_before {
                    // Keep swap!
                    break;
                } else {
                    // Revert swap
                    design_db.cell_x_dbu[c1] = x1;
                    design_db.cell_y_dbu[c1] = y1;
                    design_db.cell_orient[c1] = o1;
                    design_db.cell_x_dbu[c2] = x2;
                    design_db.cell_y_dbu[c2] = y2;
                    design_db.cell_orient[c2] = o2;
                }
            }
        }

        // Re-synchronize sub_row.cells according to current design_db positions
        for sr in &mut sub_rows {
            let row_y = sr.orig_y_dbu;
            let sr_min = sr.x_min_dbu;
            let sr_max = sr.x_max_dbu;
            sr.cells.clear();
            for &c in &movable_cells {
                if design_db.cell_y_dbu[c] == row_y
                    && design_db.cell_x_dbu[c] >= sr_min
                    && design_db.cell_x_dbu[c] + cell_w_dbu[c] <= sr_max
                {
                    sr.cells.push(c);
                }
            }
            sr.cells.sort_by_key(|&c| design_db.cell_x_dbu[c]);
        }

        // Pass 2: Adjacent sub-row cell swaps
        for sr in &mut sub_rows {
            let k = sr.cells.len();
            if k < 2 {
                continue;
            }
            for j in 0..(k - 1) {
                let ca = sr.cells[j];
                let cb = sr.cells[j + 1];
                let xa = design_db.cell_x_dbu[ca];
                let xb = design_db.cell_x_dbu[cb];
                let wa = cell_w_dbu[ca];
                let wb = cell_w_dbu[cb];

                let new_xb = xa;
                let new_xa = xa + wb;
                if new_xa + wa > xb + wb {
                    continue;
                }

                let mut incident: Vec<usize> = cell_nets[ca].to_vec();
                for &net in &cell_nets[cb] {
                    if !incident.contains(&net) {
                        incident.push(net);
                    }
                }
                if incident.is_empty() {
                    continue;
                }

                let hpwl_before: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();

                design_db.cell_x_dbu[ca] = new_xa;
                design_db.cell_x_dbu[cb] = new_xb;

                let hpwl_after: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();

                if hpwl_after < hpwl_before {
                    sr.cells.swap(j, j + 1);
                } else {
                    design_db.cell_x_dbu[ca] = xa;
                    design_db.cell_x_dbu[cb] = xb;
                }
            }
        }

        // Pass 3: Single-cell site sliding within sub-row
        for sr in &sub_rows {
            let k = sr.cells.len();
            for j in 0..k {
                let c = sr.cells[j];
                let wc = cell_w_dbu[c];
                let orig_x = design_db.cell_x_dbu[c];

                let left_limit = if j > 0 {
                    design_db.cell_x_dbu[sr.cells[j - 1]] + cell_w_dbu[sr.cells[j - 1]]
                } else {
                    sr.x_min_dbu
                };
                let right_limit = if j + 1 < k {
                    design_db.cell_x_dbu[sr.cells[j + 1]] - wc
                } else {
                    sr.x_max_dbu - wc
                };

                if right_limit <= left_limit {
                    continue;
                }

                // Compute median x of other pins
                let mut other_xs = Vec::new();
                for &net in &cell_nets[c] {
                    let pins = design_db.net_pins(NetId(net as u32));
                    for &pin in pins {
                        if design_db.pin_cells[pin.0 as usize] != Some(crate::interner::CellId(c as u32)) {
                            let (px, _) = design_db.pin_pos_dbu(pin, tech_db);
                            other_xs.push(px);
                        }
                    }
                }

                let mut candidates = vec![left_limit, right_limit];
                if !other_xs.is_empty() {
                    other_xs.sort_unstable();
                    let med = other_xs[other_xs.len() / 2];
                    let snapped = snap_to_site(med - wc / 2, sr.orig_x_dbu, sr.step_x_dbu, left_limit, right_limit);
                    candidates.push(snapped);
                }

                candidates.sort_unstable();
                candidates.dedup();

                let incident = &cell_nets[c];
                if incident.is_empty() {
                    continue;
                }
                let hpwl_before: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();

                let mut best_x = orig_x;
                let mut best_hpwl = hpwl_before;

                for &cand_x in &candidates {
                    if cand_x == orig_x {
                        continue;
                    }
                    design_db.cell_x_dbu[c] = cand_x;
                    let h: i64 = incident.iter().map(|&n| net_hpwl_dbu(design_db, tech_db, n)).sum();
                    if h < best_hpwl {
                        best_hpwl = h;
                        best_x = cand_x;
                    }
                }
                design_db.cell_x_dbu[c] = best_x;
            }
        }
    }

    let final_hpwl_dbu = design_db.compute_hpwl_dbu(tech_db);

    // Compute average and maximum displacement
    let mut total_disp_um = 0.0f64;
    let mut max_disp_um = 0.0f64;
    let mut count = 0;

    for &c in &movable_cells {
        let final_x_um = design_db.cell_x_dbu[c] as f64 / dbu_per_um as f64;
        let final_y_um = design_db.cell_y_dbu[c] as f64 / dbu_per_um as f64;
        let orig_x_um = gp_x_um[c] as f64;
        let orig_y_um = gp_y_um[c] as f64;

        let dx = final_x_um - orig_x_um;
        let dy = final_y_um - orig_y_um;
        let dist = (dx * dx + dy * dy).sqrt();

        total_disp_um += dist;
        max_disp_um = max_disp_um.max(dist);
        count += 1;
    }

    let avg_displacement_um = if count > 0 {
        total_disp_um / count as f64
    } else {
        0.0
    };

    // Self-verification
    verify_legality(design_db, tech_db).context("verify_legality failed on output placement")?;

    Ok(LegalizeResult {
        avg_displacement_um,
        max_displacement_um: max_disp_um,
        pre_legal_hpwl_dbu,
        post_abacus_hpwl_dbu,
        final_hpwl_dbu,
    })
}

pub fn verify_legality(design_db: &DesignDb, tech_db: &TechDb) -> Result<()> {
    let site_height = tech_db.site_height_dbu;
    let num_cells = design_db.num_cells();

    // 1. Verify all movable cells are Placed and on valid rows
    for c in 0..num_cells {
        let macro_id = design_db.cell_macros[c];
        if macro_id.0 as usize >= tech_db.macros.len() {
            continue;
        }
        let mac = &tech_db.macros[macro_id.0 as usize];
        if mac.class != MacroClass::Core && mac.class != MacroClass::CoreTie {
            continue;
        }
        if design_db.cell_status[c] == PlacementStatus::Fixed
            || design_db.cell_status[c] == PlacementStatus::Cover
        {
            continue;
        }

        if design_db.cell_status[c] != PlacementStatus::Placed {
            bail!("Movable cell {} status is not Placed: {:?}", c, design_db.cell_status[c]);
        }

        let cy = design_db.cell_y_dbu[c];
        let cx = design_db.cell_x_dbu[c];
        let orient = design_db.cell_orient[c];

        let matched_row = design_db.rows.iter().find(|r| r.orig_y_dbu == cy);
        let row = match matched_row {
            Some(r) => r,
            None => bail!("Cell {} y={} does not match any PlacementRow", c, cy),
        };

        if orient != row.orient {
            bail!("Cell {} orient {:?} does not match row orient {:?}", c, orient, row.orient);
        }

        if (cx - row.orig_x_dbu) % row.step_x_dbu != 0 {
            bail!("Cell {} x={} is not snapped to row site grid (orig_x={}, step={})", c, cx, row.orig_x_dbu, row.step_x_dbu);
        }

        let is_rot = matches!(orient, Orient::E | Orient::W | Orient::FE | Orient::FW);
        let cw = if is_rot { mac.height_dbu } else { mac.width_dbu };
        let row_xh = row.orig_x_dbu + row.num_x * row.step_x_dbu;

        if cx < row.orig_x_dbu || cx + cw > row_xh {
            bail!("Cell {} [{}, {}] exceeds row horizontal bounds [{}, {}]", c, cx, cx + cw, row.orig_x_dbu, row_xh);
        }
    }

    // 2. Zero-overlap check across all rows
    for row in &design_db.rows {
        let row_yl = row.orig_y_dbu;
        let row_yh = row.orig_y_dbu + site_height;

        let mut row_cells = Vec::new();
        for c in 0..num_cells {
            let macro_id = design_db.cell_macros[c];
            if macro_id.0 as usize >= tech_db.macros.len() {
                continue;
            }
            let mac = &tech_db.macros[macro_id.0 as usize];
            if mac.class == MacroClass::CoreSpacer {
                continue;
            }
            let cy = design_db.cell_y_dbu[c];
            let is_rot = matches!(design_db.cell_orient[c], Orient::E | Orient::W | Orient::FE | Orient::FW);
            let (cw, ch) = if is_rot { (mac.height_dbu, mac.width_dbu) } else { (mac.width_dbu, mac.height_dbu) };

            if cy < row_yh && cy + ch > row_yl {
                let cx = design_db.cell_x_dbu[c];
                row_cells.push((cx, cx + cw, c));
            }
        }

        row_cells.sort_by_key(|&(x1, _, _)| x1);

        for i in 0..row_cells.len().saturating_sub(1) {
            let (x1_l, x1_h, c1) = row_cells[i];
            let (x2_l, _, c2) = row_cells[i + 1];
            if x1_h > x2_l {
                bail!(
                    "Overlap on row y={}: cell {} [{}, {}] overlaps cell {} [{}, {}] by {} DBU",
                    row_yl, c1, x1_l, x1_h, c2, x2_l, row_cells[i + 1].1, x1_h - x2_l
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::interner::StringInterner;
    use crate::placement::nesterov::{run_nesterov_placement, NesterovConfig};
    use crate::readers::def::{parse_def, write_def};
    use crate::readers::lef::load_tech_db;

    #[test]
    fn test_gcd_place_and_legalize() {
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
        let mut design_db = parse_def(&floorplan_def_path, &tech_db, None)
            .expect("Failed to parse floorplan DEF");

        // 1. Run global placement
        let config = NesterovConfig::default();
        let gp_res = run_nesterov_placement(&design_db, &tech_db, &config);

        // 2. Run greedy Abacus legalization + local refinement
        let t0 = std::time::Instant::now();
        let leg_res = legalize_placement(&mut design_db, &tech_db, &gp_res.cell_x_um, &gp_res.cell_y_um)
            .expect("Legalization must succeed");
        let elapsed = t0.elapsed();

        println!(
            "Legalization completed in {:?}: pre_legal_hpwl={} dbu, post_abacus_hpwl={} dbu, final_hpwl={} dbu, avg_disp={:.3} um, max_disp={:.3} um",
            elapsed, leg_res.pre_legal_hpwl_dbu, leg_res.post_abacus_hpwl_dbu, leg_res.final_hpwl_dbu, leg_res.avg_displacement_um, leg_res.max_displacement_um
        );

        // 3. Verify legality self-check passes
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        // 4. Assert final HPWL is within 5% of reference placed_hpwl_dbu
        let ref_path = repo_root.join("tests/designs/gcd/gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path).expect("Reference JSON");
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes).expect("Parse JSON");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();

        let rel_diff = (leg_res.final_hpwl_dbu - ref_hpwl_dbu).abs() as f64 / ref_hpwl_dbu as f64;
        println!(
            "HPWL comparison: reference={} dbu, ours={} dbu, diff={:.2}%",
            ref_hpwl_dbu, leg_res.final_hpwl_dbu, rel_diff * 100.0
        );
        assert!(
            rel_diff <= 0.05,
            "Legalized HPWL {} must be within 5% of OpenROAD {}, got {:.2}%",
            leg_res.final_hpwl_dbu, ref_hpwl_dbu, rel_diff * 100.0
        );

        // 5. Test DEF write and re-parse roundtrip
        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_gcd_legalized_test.def");
        write_def(&design_db, &tech_db, &floorplan_def_path, &out_def_path)
            .expect("Failed to write legalized DEF");

        let reloaded_db = parse_def(&out_def_path, &tech_db, None)
            .expect("Failed to re-parse written DEF");

        verify_legality(&reloaded_db, &tech_db).expect("Reloaded DEF must pass verify_legality");
        let reloaded_hpwl = reloaded_db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            reloaded_hpwl, leg_res.final_hpwl_dbu,
            "Reloaded DEF HPWL {} must match legalized HPWL {}",
            reloaded_hpwl, leg_res.final_hpwl_dbu
        );
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
    }
}
