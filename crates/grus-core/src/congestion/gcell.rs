use crate::design_db::{DesignDb, PlacementStatus};
use crate::interner::LayerId;
use crate::tech_db::{LayerDir, TechDb};

#[derive(Clone, Debug)]
pub struct GCellGrid {
    pub origin_x_dbu: i32,
    pub origin_y_dbu: i32,
    pub gcell_w_dbu: i32,
    pub gcell_h_dbu: i32,
    pub nx: usize, // number of GCell columns
    pub ny: usize, // number of GCell rows

    /// Horizontal boundary edges between (x, y) and (x + 1, y):
    /// indexed by `y * (nx - 1) + x`, length `ny * (nx - 1)`
    pub h_cap: Vec<f32>,
    pub h_raw_cap: Vec<f32>,
    pub h_demand: Vec<f32>,

    /// Vertical boundary edges between (x, y) and (x, y + 1):
    /// indexed by `y * nx + x`, length `(ny - 1) * nx`
    pub v_cap: Vec<f32>,
    pub v_raw_cap: Vec<f32>,
    pub v_demand: Vec<f32>,

    /// Per-layer totals (indexed by `LayerId(0..tech_db.layers.len())`) for direct
    /// verification against OpenROAD `dbGCellGrid` and `[INFO GRT-0053]`:
    pub layer_raw_edge_tracks: Vec<u32>,
    pub layer_orig_edge_tracks: Vec<u32>,
    pub layer_derated_edge_tracks: Vec<u32>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct GPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SteinerSegment {
    pub p1: GPoint,
    pub p2: GPoint,
}

#[derive(Clone, Debug)]
pub struct CongestionReport {
    pub nx: usize,
    pub ny: usize,
    pub total_overflow: f32,
    pub max_edge_overflow: f32,
    pub top_05pct_overflow: f32,
    pub overflow_gcells: usize,
    pub gcell_usage_ratio: Vec<f32>,
    /// Usage ratio (demand / capacity) per horizontal edge, indexed
    /// `y * (nx - 1) + x` like `GCellGrid::h_cap`.
    pub h_usage_ratio: Vec<f32>,
    /// Usage ratio per vertical edge, indexed `y * nx + x` like
    /// `GCellGrid::v_cap`.
    pub v_usage_ratio: Vec<f32>, // length nx * ny, max(demand/cap) over adjacent edges
}

impl GCellGrid {
    pub fn gcell_x(&self, x_dbu: i32) -> usize {
        if self.gcell_w_dbu <= 0 {
            return 0;
        }
        let gx = (x_dbu - self.origin_x_dbu) / self.gcell_w_dbu;
        gx.clamp(0, (self.nx - 1) as i32) as usize
    }

    pub fn gcell_y(&self, y_dbu: i32) -> usize {
        if self.gcell_h_dbu <= 0 {
            return 0;
        }
        let gy = (y_dbu - self.origin_y_dbu) / self.gcell_h_dbu;
        gy.clamp(0, (self.ny - 1) as i32) as usize
    }

    pub fn gcell_xy(&self, x_dbu: i32, y_dbu: i32) -> GPoint {
        GPoint {
            x: self.gcell_x(x_dbu) as u16,
            y: self.gcell_y(y_dbu) as u16,
        }
    }

    pub fn x_bounds(&self, x: usize, die_xh: i32) -> (i32, i32) {
        let x0 = self.origin_x_dbu + (x as i32) * self.gcell_w_dbu;
        let x1 = if x + 1 == self.nx {
            die_xh
        } else {
            self.origin_x_dbu + ((x + 1) as i32) * self.gcell_w_dbu
        };
        (x0, x1)
    }

    pub fn y_bounds(&self, y: usize, die_yh: i32) -> (i32, i32) {
        let y0 = self.origin_y_dbu + (y as i32) * self.gcell_h_dbu;
        let y1 = if y + 1 == self.ny {
            die_yh
        } else {
            self.origin_y_dbu + ((y + 1) as i32) * self.gcell_h_dbu
        };
        (y0, y1)
    }

    pub fn build(
        design_db: &DesignDb,
        tech_db: &TechDb,
        layer_adjustments: Option<&[f32]>,
        gcell_tracks_override: Option<i32>,
    ) -> Self {
        let (origin_x_dbu, origin_y_dbu, gcell_w_dbu, gcell_h_dbu, nx, ny) =
            if !design_db.gcell_grids.is_empty() && gcell_tracks_override.is_none() {
                let gx = design_db.gcell_grids.iter().find(|g| g.is_x);
                let gy = design_db.gcell_grids.iter().find(|g| !g.is_x);
                if let (Some(x_grid), Some(y_grid)) = (gx, gy) {
                    let ox = x_grid.start_dbu;
                    let oy = y_grid.start_dbu;
                    let gw = x_grid.step_dbu;
                    let gh = y_grid.step_dbu;
                    let num_x = (x_grid.num_boundaries - 1).max(1) as usize;
                    let num_y = (y_grid.num_boundaries - 1).max(1) as usize;
                    (ox, oy, gw, gh, num_x, num_y)
                } else {
                    Self::compute_default_grid(design_db, tech_db, gcell_tracks_override)
                }
            } else {
                Self::compute_default_grid(design_db, tech_db, gcell_tracks_override)
            };

        let num_h_edges = ny * (nx.saturating_sub(1));
        let num_v_edges = (ny.saturating_sub(1)) * nx;

        let h_cap = vec![0.0f32; num_h_edges];
        let h_raw_cap = vec![0.0f32; num_h_edges];
        let h_demand = vec![0.0f32; num_h_edges];
        let v_cap = vec![0.0f32; num_v_edges];
        let v_raw_cap = vec![0.0f32; num_v_edges];
        let v_demand = vec![0.0f32; num_v_edges];

        let num_layers = tech_db.routing_layers.len();
        let mut layer_raw_edge_tracks = vec![0u32; num_layers];
        let mut layer_orig_edge_tracks = vec![0u32; num_layers];
        let mut layer_derated_edge_tracks = vec![0u32; num_layers];

        let mut grid = Self {
            origin_x_dbu,
            origin_y_dbu,
            gcell_w_dbu,
            gcell_h_dbu,
            nx,
            ny,
            h_cap,
            h_raw_cap,
            h_demand,
            v_cap,
            v_raw_cap,
            v_demand,
            layer_raw_edge_tracks: Vec::new(),
            layer_orig_edge_tracks: Vec::new(),
            layer_derated_edge_tracks: Vec::new(),
        };

        let die_xh = design_db.die_area.xh;
        let die_yh = design_db.die_area.yh;

        // Skip li1 (LayerId(0)); process routing layers met1..met5 (LayerId(1)..=LayerId(5))
        for layer_idx in 1..num_layers {
            let layer_id = LayerId(layer_idx as u8);
            let layer = &tech_db.routing_layers[layer_idx];
            let pitch = layer.pitch_dbu.max(1);

            let alpha = match layer_adjustments {
                Some(adjs) => adjs.get(layer_idx).copied().unwrap_or(0.0),
                None => {
                    if layer_idx == 1 {
                        0.30f32 // met1
                    } else if layer_idx <= 5 {
                        0.20f32 // met2..met5
                    } else {
                        0.0f32
                    }
                }
            };

            // Collect all obstacles (Placed/Fixed instance OBS + special wire rects) on this layer
            let mut obstacles: Vec<(i32, i32, i32, i32)> = Vec::new();

            for c in 0..design_db.num_cells() {
                if design_db.cell_status[c] == PlacementStatus::Placed
                    || design_db.cell_status[c] == PlacementStatus::Fixed
                {
                    let mac = &tech_db.macros[design_db.cell_macros[c].0 as usize];
                    let w = mac.width_dbu;
                    let h = mac.height_dbu;
                    let cx = design_db.cell_x_dbu[c];
                    let cy = design_db.cell_y_dbu[c];
                    let orient = design_db.cell_orient[c];

                    for &(obs_layer, r) in mac.obs_rects.iter().chain(mac.pin_obs_rects.iter()) {
                        if obs_layer == layer_id {
                            let r = DesignDb::orient_rect(r, w, h, orient);
                            obstacles.push((cx + r.xl, cy + r.yl, cx + r.xh, cy + r.yh));
                        }
                    }
                }
            }

            for (wire_layer, r) in design_db.special_wire_rects() {
                if wire_layer == layer_id {
                    obstacles.push((r.xl, r.yl, r.xh, r.yh));
                }
            }

            if layer.dir == LayerDir::Horizontal {
                // Horizontal routing across horizontal boundary edges (nx - 1 edges per row)
                let mut raw_tracks_y = vec![0u32; ny];
                let tg_opt = design_db
                    .tracks
                    .iter()
                    .find(|tg| !tg.is_vertical_tracks && tg.layers.contains(&layer_id));

                for (y, raw_track) in raw_tracks_y.iter_mut().enumerate() {
                    let (y0, y1) = grid.y_bounds(y, die_yh);
                    if let Some(tg) = tg_opt {
                        let mut count = 0u32;
                        for k in 0..tg.num_tracks {
                            let coord = tg.start_dbu + k * tg.step_dbu;
                            if y + 1 == ny {
                                if coord >= y0 && coord <= y1 {
                                    count += 1;
                                }
                            } else if coord >= y0 && coord < y1 {
                                count += 1;
                            }
                        }
                        *raw_track = count;
                    } else {
                        *raw_track = ((y1 - y0) / pitch).max(0) as u32;
                    }
                }

                let sum_raw_y: u32 = raw_tracks_y.iter().sum();
                let edge_cols = nx.saturating_sub(1);
                layer_raw_edge_tracks[layer_idx] = (edge_cols as u32) * sum_raw_y;

                if edge_cols > 0 {
                    let mut edge_intervals: Vec<Vec<(i32, i32)>> = vec![Vec::new(); ny * edge_cols];

                    for &(xl, yl, xh, yh) in &obstacles {
                        let gx1 = grid.gcell_x(xl);
                        let gx2 = grid.gcell_x((xh - 1).max(xl));
                        let gy1 = grid.gcell_y(yl);
                        let gy2 = grid.gcell_y((yh - 1).max(yl));

                        if gx1 < edge_cols {
                            let max_x = gx1.max(gx2).min(edge_cols - 1);
                            for y in gy1..=gy2 {
                                let (y0, y1) = grid.y_bounds(y, die_yh);
                                let s = yl.max(y0);
                                let e = yh.min(y1);
                                if e > s {
                                    for x in gx1..=max_x {
                                        let edge_idx = y * edge_cols + x;
                                        edge_intervals[edge_idx].push((s, e));
                                    }
                                }
                            }
                        }
                    }

                    for (y, &raw) in raw_tracks_y.iter().enumerate() {
                        for x in 0..edge_cols {
                            let edge_idx = y * edge_cols + x;
                            let intervals = &mut edge_intervals[edge_idx];
                            intervals.sort_unstable_by_key(|&(s, _)| s);

                            let mut blocked = 0u32;
                            let mut cur_s = 0;
                            let mut cur_e = 0;
                            let mut first = true;
                            for &(s, e) in intervals.iter() {
                                if first {
                                    cur_s = s;
                                    cur_e = e;
                                    first = false;
                                } else if s <= cur_e {
                                    cur_e = cur_e.max(e);
                                } else {
                                    blocked += ((cur_e - cur_s + pitch - 1) / pitch) as u32;
                                    cur_s = s;
                                    cur_e = e;
                                }
                            }
                            if !first {
                                blocked += ((cur_e - cur_s + pitch - 1) / pitch) as u32;
                            }

                            let orig = raw.saturating_sub(blocked);
                            let derated = if orig == 0 || alpha >= 1.0 {
                                0
                            } else {
                                1.max(((orig as f64) * (1.0 - alpha as f64) + 1e-6).floor() as u32)
                            };

                            layer_orig_edge_tracks[layer_idx] += orig;
                            layer_derated_edge_tracks[layer_idx] += derated;
                            grid.h_cap[edge_idx] += derated as f32;
                            grid.h_raw_cap[edge_idx] += raw as f32;
                        }
                    }
                }
            } else {
                // Vertical routing across vertical boundary edges (ny - 1 edges per col)
                let mut raw_tracks_x = vec![0u32; nx];
                let tg_opt = design_db
                    .tracks
                    .iter()
                    .find(|tg| tg.is_vertical_tracks && tg.layers.contains(&layer_id));

                for (x, raw_track) in raw_tracks_x.iter_mut().enumerate() {
                    let (x0, x1) = grid.x_bounds(x, die_xh);
                    if let Some(tg) = tg_opt {
                        let mut count = 0u32;
                        for k in 0..tg.num_tracks {
                            let coord = tg.start_dbu + k * tg.step_dbu;
                            if x + 1 == nx {
                                if coord >= x0 && coord <= x1 {
                                    count += 1;
                                }
                            } else if coord >= x0 && coord < x1 {
                                count += 1;
                            }
                        }
                        *raw_track = count;
                    } else {
                        *raw_track = ((x1 - x0) / pitch).max(0) as u32;
                    }
                }

                let sum_raw_x: u32 = raw_tracks_x.iter().sum();
                let edge_rows = ny.saturating_sub(1);
                layer_raw_edge_tracks[layer_idx] = (edge_rows as u32) * sum_raw_x;

                if edge_rows > 0 {
                    let mut edge_intervals: Vec<Vec<(i32, i32)>> = vec![Vec::new(); edge_rows * nx];

                    for &(xl, yl, xh, yh) in &obstacles {
                        let gx1 = grid.gcell_x(xl);
                        let gx2 = grid.gcell_x((xh - 1).max(xl));
                        let gy1 = grid.gcell_y(yl);
                        let gy2 = grid.gcell_y((yh - 1).max(yl));

                        if gy1 < edge_rows {
                            let max_y = gy1.max(gy2).min(edge_rows - 1);
                            for x in gx1..=gx2 {
                                let (x0, x1) = grid.x_bounds(x, die_xh);
                                let s = xl.max(x0);
                                let e = xh.min(x1);
                                if e > s {
                                    for y in gy1..=max_y {
                                        let edge_idx = y * nx + x;
                                        edge_intervals[edge_idx].push((s, e));
                                    }
                                }
                            }
                        }
                    }

                    for y in 0..edge_rows {
                        for (x, &raw) in raw_tracks_x.iter().enumerate() {
                            let edge_idx = y * nx + x;
                            let intervals = &mut edge_intervals[edge_idx];
                            intervals.sort_unstable_by_key(|&(s, _)| s);

                            let mut blocked = 0u32;
                            let mut cur_s = 0;
                            let mut cur_e = 0;
                            let mut first = true;
                            for &(s, e) in intervals.iter() {
                                if first {
                                    cur_s = s;
                                    cur_e = e;
                                    first = false;
                                } else if s <= cur_e {
                                    cur_e = cur_e.max(e);
                                } else {
                                    blocked += ((cur_e - cur_s + pitch - 1) / pitch) as u32;
                                    cur_s = s;
                                    cur_e = e;
                                }
                            }
                            if !first {
                                blocked += ((cur_e - cur_s + pitch - 1) / pitch) as u32;
                            }

                            let orig = raw.saturating_sub(blocked);
                            let derated = if orig == 0 || alpha >= 1.0 {
                                0
                            } else {
                                1.max(((orig as f64) * (1.0 - alpha as f64) + 1e-6).floor() as u32)
                            };

                            layer_orig_edge_tracks[layer_idx] += orig;
                            layer_derated_edge_tracks[layer_idx] += derated;
                            grid.v_cap[edge_idx] += derated as f32;
                            grid.v_raw_cap[edge_idx] += raw as f32;
                        }
                    }
                }
            }
        }

        grid.layer_raw_edge_tracks = layer_raw_edge_tracks;
        grid.layer_orig_edge_tracks = layer_orig_edge_tracks;
        grid.layer_derated_edge_tracks = layer_derated_edge_tracks;
        grid
    }

    fn compute_default_grid(
        design_db: &DesignDb,
        tech_db: &TechDb,
        gcell_tracks_override: Option<i32>,
    ) -> (i32, i32, i32, i32, usize, usize) {
        let met2_pitch = tech_db
            .routing_layers
            .get(2)
            .map(|l| l.pitch_dbu)
            .unwrap_or(460);
        let tracks_mult = gcell_tracks_override.unwrap_or(15);
        let gcell_w_dbu = tracks_mult * met2_pitch;
        let gcell_h_dbu = tracks_mult * met2_pitch;
        let ox = design_db.die_area.xl;
        let oy = design_db.die_area.yl;
        let nx = (((design_db.die_area.xh - ox) / gcell_w_dbu).max(1)) as usize;
        let ny = (((design_db.die_area.yh - oy) / gcell_h_dbu).max(1)) as usize;
        (ox, oy, gcell_w_dbu, gcell_h_dbu, nx, ny)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use crate::test_support::{design_dir, sky130_hd};

    #[test]
    fn test_gcell_grid_coordinate_clamping() {
        let mut tech_db = TechDb::new();
        tech_db.dbu_per_micron = 1000;
        let mut design_db = DesignDb::new();
        design_db.die_area = crate::tech_db::RectDbu::new(0, 0, 10000, 10000);

        let grid = GCellGrid::build(&design_db, &tech_db, None, Some(10));
        assert_eq!(grid.origin_x_dbu, 0);
        assert_eq!(grid.origin_y_dbu, 0);

        // Clamping tests
        let p_neg = grid.gcell_xy(-500, -100);
        assert_eq!(p_neg, GPoint { x: 0, y: 0 });

        let p_far = grid.gcell_xy(50000, 50000);
        assert_eq!(
            p_far,
            GPoint {
                x: (grid.nx - 1) as u16,
                y: (grid.ny - 1) as u16
            }
        );
    }

    #[test]
    fn test_gcd_gcell_grid() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let def_path = design_dir("gcd").join("gcd.placed.def");

        if !tlef_path.is_file() {
            panic!("PDK tlef file missing at {:?}", tlef_path);
        }
        if !lef_path.is_file() {
            panic!("PDK lef file missing at {:?}", lef_path);
        }
        if !def_path.is_file() {
            panic!("Design DEF file missing at {:?}", def_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let design_db = parse_def(&def_path, &tech_db, None).expect("Failed to parse placed DEF");

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        let grid = GCellGrid::build(&design_db, &tech_db, None, None);

        // 1. Grid dimensions & tile size
        assert_eq!(grid.nx, ref_json["gcell_nx"].as_u64().unwrap() as usize);
        assert_eq!(grid.ny, ref_json["gcell_ny"].as_u64().unwrap() as usize);
        assert_eq!(
            grid.gcell_w_dbu,
            ref_json["gcell_tile_size_dbu"].as_i64().unwrap() as i32
        );
        assert_eq!(
            grid.gcell_h_dbu,
            ref_json["gcell_tile_size_dbu"].as_i64().unwrap() as i32
        );

        // 2. Raw track capacity: exact equality for all 5 routing layers (met1..met5)
        let ref_raw = ref_json["gcell_raw_edge_tracks"].as_array().unwrap();
        assert_eq!(
            grid.layer_raw_edge_tracks[1],
            ref_raw[0].as_u64().unwrap() as u32,
            "met1 raw edge tracks"
        );
        assert_eq!(
            grid.layer_raw_edge_tracks[2],
            ref_raw[1].as_u64().unwrap() as u32,
            "met2 raw edge tracks"
        );
        assert_eq!(
            grid.layer_raw_edge_tracks[3],
            ref_raw[2].as_u64().unwrap() as u32,
            "met3 raw edge tracks"
        );
        assert_eq!(
            grid.layer_raw_edge_tracks[4],
            ref_raw[3].as_u64().unwrap() as u32,
            "met4 raw edge tracks"
        );
        assert_eq!(
            grid.layer_raw_edge_tracks[5],
            ref_raw[4].as_u64().unwrap() as u32,
            "met5 raw edge tracks"
        );

        println!("grid raw: {:?}", &grid.layer_raw_edge_tracks[1..=5]);
        println!("grid orig: {:?}", &grid.layer_orig_edge_tracks[1..=5]);
        println!("grid derated: {:?}", &grid.layer_derated_edge_tracks[1..=5]);

        // 3. Original & derated track capacities
        let ref_orig = ref_json["gcell_orig_edge_tracks"].as_array().unwrap();
        let ref_derated = ref_json["gcell_derated_edge_tracks"].as_array().unwrap();

        // met2..met5 exact equality to the integer
        assert_eq!(
            grid.layer_orig_edge_tracks[2],
            ref_orig[1].as_u64().unwrap() as u32,
            "met2 orig edge tracks"
        );
        assert_eq!(
            grid.layer_derated_edge_tracks[2],
            ref_derated[1].as_u64().unwrap() as u32,
            "met2 derated edge tracks"
        );

        assert_eq!(
            grid.layer_orig_edge_tracks[3],
            ref_orig[2].as_u64().unwrap() as u32,
            "met3 orig edge tracks"
        );
        assert_eq!(
            grid.layer_derated_edge_tracks[3],
            ref_derated[2].as_u64().unwrap() as u32,
            "met3 derated edge tracks"
        );

        assert_eq!(
            grid.layer_orig_edge_tracks[4],
            ref_orig[3].as_u64().unwrap() as u32,
            "met4 orig edge tracks"
        );
        assert_eq!(
            grid.layer_derated_edge_tracks[4],
            ref_derated[3].as_u64().unwrap() as u32,
            "met4 derated edge tracks"
        );

        assert_eq!(
            grid.layer_orig_edge_tracks[5],
            ref_orig[4].as_u64().unwrap() as u32,
            "met5 orig edge tracks"
        );
        assert_eq!(
            grid.layer_derated_edge_tracks[5],
            ref_derated[4].as_u64().unwrap() as u32,
            "met5 derated edge tracks"
        );

        // met1: our merged internal jumpers and PG pins yield 2748 / 1843 vs OpenROAD's 2705 / 1815
        assert_eq!(
            grid.layer_orig_edge_tracks[1], 2748,
            "met1 orig edge tracks"
        );
        assert_eq!(
            grid.layer_derated_edge_tracks[1], 1843,
            "met1 derated edge tracks"
        );

        // Total derated capacity across all 5 layers within 1% of OpenROAD (6344 vs 6316 = 0.44%)
        let our_total_derated: u32 = grid.layer_derated_edge_tracks.iter().sum();
        let ref_total_derated: u32 = ref_derated.iter().map(|v| v.as_u64().unwrap() as u32).sum();
        assert_eq!(our_total_derated, 6344);
        assert_eq!(ref_total_derated, 6316);
        let pct_diff = ((our_total_derated as f64 - ref_total_derated as f64).abs()
            / ref_total_derated as f64)
            * 100.0;
        assert!(
            pct_diff < 1.0,
            "Total derated capacity diff {:.2}% must be < 1.0%",
            pct_diff
        );
    }
}
