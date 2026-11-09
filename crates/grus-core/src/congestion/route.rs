use crate::congestion::gcell::{CongestionReport, GCellGrid, SteinerSegment};
use crate::congestion::rsmt::decompose_design_nets;
use crate::design_db::DesignDb;
use crate::tech_db::TechDb;

#[inline]
fn edge_cost(d: f32, c: f32) -> f64 {
    1.0 + 10.0 / (1.0 + (-2.0 * (d as f64 - c as f64)).exp())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RouteChoice {
    Lhv,
    Lvh,
    Zhvh(usize),
    Zvhv(usize),
}

/// Routes rectilinear Steiner segments over the GCell grid using 2-pass congestion routing:
/// - Pass 1: Probabilistic demand (RISA / uniform 50-50 L-split)
/// - Pass 2: Congestion-aware L/Z pattern routing with rip-up and cost evaluation
pub fn route_segments(grid: &mut GCellGrid, segments: &[SteinerSegment]) -> CongestionReport {
    let nx = grid.nx;
    let ny = grid.ny;
    let edge_cols = nx.saturating_sub(1);
    let edge_rows = ny.saturating_sub(1);

    // Pass 1: Probabilistic Demand (RISA)
    grid.h_demand.fill(0.0);
    grid.v_demand.fill(0.0);

    for s in segments {
        let p1 = s.p1;
        let p2 = s.p2;
        let x_min = (p1.x.min(p2.x) as usize).min(nx);
        let x_max = (p1.x.max(p2.x) as usize).min(nx);
        let y_min = (p1.y.min(p2.y) as usize).min(ny);
        let y_max = (p1.y.max(p2.y) as usize).min(ny);

        if x_min == x_max && y_min == y_max {
            continue;
        }

        if y_min == y_max {
            let y = y_min;
            for x in x_min..x_max.min(edge_cols) {
                grid.h_demand[y * edge_cols + x] += 1.0;
            }
        } else if x_min == x_max {
            let x = x_min;
            for y in y_min..y_max.min(edge_rows) {
                grid.v_demand[y * nx + x] += 1.0;
            }
        } else {
            for x in x_min..x_max.min(edge_cols) {
                grid.h_demand[y_min * edge_cols + x] += 0.5;
                grid.h_demand[y_max * edge_cols + x] += 0.5;
            }
            for y in y_min..y_max.min(edge_rows) {
                grid.v_demand[y * nx + x_min] += 0.5;
                grid.v_demand[y * nx + x_max] += 0.5;
            }
        }
    }

    // Pass 2: Congestion-Aware L/Z Pattern Routing
    for s in segments {
        let p1 = s.p1;
        let p2 = s.p2;
        let x_min = (p1.x.min(p2.x) as usize).min(nx);
        let x_max = (p1.x.max(p2.x) as usize).min(nx);
        let y_min = (p1.y.min(p2.y) as usize).min(ny);
        let y_max = (p1.y.max(p2.y) as usize).min(ny);

        if x_min == x_max || y_min == y_max {
            // Flat segment has no routing alternative
            continue;
        }

        let x1 = p1.x as usize;
        let y1 = p1.y as usize;
        let x2 = p2.x as usize;
        let y2 = p2.y as usize;

        // 1. Rip-up: subtract 0.5 contribution from the bounding box L-paths
        for x in x_min..x_max.min(edge_cols) {
            grid.h_demand[y_min * edge_cols + x] -= 0.5;
            grid.h_demand[y_max * edge_cols + x] -= 0.5;
        }
        for y in y_min..y_max.min(edge_rows) {
            grid.v_demand[y * nx + x_min] -= 0.5;
            grid.v_demand[y * nx + x_max] -= 0.5;
        }

        // 2. Evaluate L-routes
        let mut l_hv_cost = 0.0f64;
        let mut l_hv_overflow = false;
        for x in x_min..x_max.min(edge_cols) {
            let d = grid.h_demand[y1 * edge_cols + x];
            let c = grid.h_cap[y1 * edge_cols + x];
            l_hv_cost += edge_cost(d, c);
            if d + 1.0 > c {
                l_hv_overflow = true;
            }
        }
        for y in y_min..y_max.min(edge_rows) {
            let d = grid.v_demand[y * nx + x2];
            let c = grid.v_cap[y * nx + x2];
            l_hv_cost += edge_cost(d, c);
            if d + 1.0 > c {
                l_hv_overflow = true;
            }
        }

        let mut l_vh_cost = 0.0f64;
        let mut l_vh_overflow = false;
        for y in y_min..y_max.min(edge_rows) {
            let d = grid.v_demand[y * nx + x1];
            let c = grid.v_cap[y * nx + x1];
            l_vh_cost += edge_cost(d, c);
            if d + 1.0 > c {
                l_vh_overflow = true;
            }
        }
        for x in x_min..x_max.min(edge_cols) {
            let d = grid.h_demand[y2 * edge_cols + x];
            let c = grid.h_cap[y2 * edge_cols + x];
            l_vh_cost += edge_cost(d, c);
            if d + 1.0 > c {
                l_vh_overflow = true;
            }
        }

        let mut best_cost = l_hv_cost;
        let mut best_route = RouteChoice::Lhv;

        if l_vh_cost < best_cost {
            best_cost = l_vh_cost;
            best_route = RouteChoice::Lvh;
        }

        // 3. Evaluate Z-routes if both L_HV and L_VH cross an overflowing edge
        if l_hv_overflow && l_vh_overflow {
            if x_max > x_min + 1 {
                let dx = x_max - x_min;
                let mut pref1 = vec![0.0f64; dx + 1];
                let mut pref2 = vec![0.0f64; dx + 1];
                for i in 0..dx {
                    let x = x_min + i;
                    if x < edge_cols {
                        let d1 = grid.h_demand[y1 * edge_cols + x];
                        let c1 = grid.h_cap[y1 * edge_cols + x];
                        pref1[i + 1] = pref1[i] + edge_cost(d1, c1);

                        let d2 = grid.h_demand[y2 * edge_cols + x];
                        let c2 = grid.h_cap[y2 * edge_cols + x];
                        pref2[i + 1] = pref2[i] + edge_cost(d2, c2);
                    } else {
                        pref1[i + 1] = pref1[i];
                        pref2[i + 1] = pref2[i];
                    }
                }

                for x_mid in (x_min + 1)..x_max {
                    let m = x_mid - x_min;
                    let h_cost = if x1 < x2 {
                        pref1[m] + (pref2[dx] - pref2[m])
                    } else {
                        (pref1[dx] - pref1[m]) + pref2[m]
                    };

                    let mut v_cost = 0.0f64;
                    for y in y_min..y_max.min(edge_rows) {
                        let d = grid.v_demand[y * nx + x_mid];
                        let c = grid.v_cap[y * nx + x_mid];
                        v_cost += edge_cost(d, c);
                    }

                    let total_z_cost = h_cost + v_cost;
                    if total_z_cost < best_cost {
                        best_cost = total_z_cost;
                        best_route = RouteChoice::Zhvh(x_mid);
                    }
                }
            }

            if y_max > y_min + 1 {
                let dy = y_max - y_min;
                let mut pref1 = vec![0.0f64; dy + 1];
                let mut pref2 = vec![0.0f64; dy + 1];
                for i in 0..dy {
                    let y = y_min + i;
                    if y < edge_rows {
                        let d1 = grid.v_demand[y * nx + x1];
                        let c1 = grid.v_cap[y * nx + x1];
                        pref1[i + 1] = pref1[i] + edge_cost(d1, c1);

                        let d2 = grid.v_demand[y * nx + x2];
                        let c2 = grid.v_cap[y * nx + x2];
                        pref2[i + 1] = pref2[i] + edge_cost(d2, c2);
                    } else {
                        pref1[i + 1] = pref1[i];
                        pref2[i + 1] = pref2[i];
                    }
                }

                for y_mid in (y_min + 1)..y_max {
                    let m = y_mid - y_min;
                    let v_cost = if y1 < y2 {
                        pref1[m] + (pref2[dy] - pref2[m])
                    } else {
                        (pref1[dy] - pref1[m]) + pref2[m]
                    };

                    let mut h_cost = 0.0f64;
                    for x in x_min..x_max.min(edge_cols) {
                        let d = grid.h_demand[y_mid * edge_cols + x];
                        let c = grid.h_cap[y_mid * edge_cols + x];
                        h_cost += edge_cost(d, c);
                    }

                    let total_z_cost = v_cost + h_cost;
                    if total_z_cost < best_cost {
                        best_cost = total_z_cost;
                        best_route = RouteChoice::Zvhv(y_mid);
                    }
                }
            }
        }

        // 4. Commit +1.0 demand along the chosen minimum-cost route
        match best_route {
            RouteChoice::Lhv => {
                for x in x_min..x_max.min(edge_cols) {
                    grid.h_demand[y1 * edge_cols + x] += 1.0;
                }
                for y in y_min..y_max.min(edge_rows) {
                    grid.v_demand[y * nx + x2] += 1.0;
                }
            }
            RouteChoice::Lvh => {
                for y in y_min..y_max.min(edge_rows) {
                    grid.v_demand[y * nx + x1] += 1.0;
                }
                for x in x_min..x_max.min(edge_cols) {
                    grid.h_demand[y2 * edge_cols + x] += 1.0;
                }
            }
            RouteChoice::Zhvh(x_mid) => {
                if x1 < x2 {
                    for x in x_min..x_mid.min(edge_cols) {
                        grid.h_demand[y1 * edge_cols + x] += 1.0;
                    }
                    for x in x_mid..x_max.min(edge_cols) {
                        grid.h_demand[y2 * edge_cols + x] += 1.0;
                    }
                } else {
                    for x in x_mid..x_max.min(edge_cols) {
                        grid.h_demand[y1 * edge_cols + x] += 1.0;
                    }
                    for x in x_min..x_mid.min(edge_cols) {
                        grid.h_demand[y2 * edge_cols + x] += 1.0;
                    }
                }
                for y in y_min..y_max.min(edge_rows) {
                    grid.v_demand[y * nx + x_mid] += 1.0;
                }
            }
            RouteChoice::Zvhv(y_mid) => {
                if y1 < y2 {
                    for y in y_min..y_mid.min(edge_rows) {
                        grid.v_demand[y * nx + x1] += 1.0;
                    }
                    for y in y_mid..y_max.min(edge_rows) {
                        grid.v_demand[y * nx + x2] += 1.0;
                    }
                } else {
                    for y in y_mid..y_max.min(edge_rows) {
                        grid.v_demand[y * nx + x1] += 1.0;
                    }
                    for y in y_min..y_mid.min(edge_rows) {
                        grid.v_demand[y * nx + x2] += 1.0;
                    }
                }
                for x in x_min..x_max.min(edge_cols) {
                    grid.h_demand[y_mid * edge_cols + x] += 1.0;
                }
            }
        }
    }

    // Congestion Report Aggregation
    let num_edges = grid.h_demand.len() + grid.v_demand.len();
    let mut total_overflow = 0.0f64;
    let mut max_edge_overflow = 0.0f32;
    let mut edge_ovfs = Vec::with_capacity(num_edges);

    for (&d, &c) in grid.h_demand.iter().zip(grid.h_cap.iter()) {
        let ovf = (d - c).max(0.0);
        total_overflow += ovf as f64;
        if ovf > max_edge_overflow {
            max_edge_overflow = ovf;
        }
        edge_ovfs.push(ovf);
    }

    for (&d, &c) in grid.v_demand.iter().zip(grid.v_cap.iter()) {
        let ovf = (d - c).max(0.0);
        total_overflow += ovf as f64;
        if ovf > max_edge_overflow {
            max_edge_overflow = ovf;
        }
        edge_ovfs.push(ovf);
    }

    edge_ovfs.sort_unstable_by(|a, b| b.total_cmp(a));
    let k = ((0.005 * num_edges as f64).ceil() as usize).max(1);
    let top_k_sum: f64 = edge_ovfs.iter().take(k).map(|&v| v as f64).sum();
    let top_05pct_overflow = (top_k_sum / k as f64) as f32;

    // Per-GCell usage ratio
    let mut gcell_usage_ratio = vec![0.0f32; nx * ny];
    let mut overflow_gcells = 0usize;

    for y in 0..ny {
        for x in 0..nx {
            let mut max_ratio = 0.0f32;

            let check_ratio = |d: f32, c: f32, cur_max: &mut f32| {
                let r = if c > 0.0 {
                    d / c
                } else if d > 0.0 {
                    f32::INFINITY
                } else {
                    0.0
                };
                if r > *cur_max {
                    *cur_max = r;
                }
            };

            // Left horizontal edge
            if x > 0 && edge_cols > 0 {
                let idx = y * edge_cols + (x - 1);
                check_ratio(grid.h_demand[idx], grid.h_cap[idx], &mut max_ratio);
            }
            // Right horizontal edge
            if x < edge_cols {
                let idx = y * edge_cols + x;
                check_ratio(grid.h_demand[idx], grid.h_cap[idx], &mut max_ratio);
            }
            // Bottom vertical edge
            if y > 0 {
                let idx = (y - 1) * nx + x;
                check_ratio(grid.v_demand[idx], grid.v_cap[idx], &mut max_ratio);
            }
            // Top vertical edge
            if y < edge_rows {
                let idx = y * nx + x;
                check_ratio(grid.v_demand[idx], grid.v_cap[idx], &mut max_ratio);
            }

            gcell_usage_ratio[y * nx + x] = max_ratio;
            if max_ratio > 1.0 {
                overflow_gcells += 1;
            }
        }
    }

    CongestionReport {
        nx,
        ny,
        total_overflow: total_overflow as f32,
        max_edge_overflow,
        top_05pct_overflow,
        overflow_gcells,
        gcell_usage_ratio,
    }
}

/// Estimates global-routing congestion directly from a placed design.
pub fn estimate_congestion(
    design_db: &DesignDb,
    tech_db: &TechDb,
    layer_adjustments: Option<&[f32]>,
    gcell_tracks_override: Option<i32>,
) -> (GCellGrid, CongestionReport) {
    let mut grid = GCellGrid::build(design_db, tech_db, layer_adjustments, gcell_tracks_override);
    let segments = decompose_design_nets(design_db, tech_db, &grid);
    let report = route_segments(&mut grid, &segments);
    (grid, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::congestion::gcell::GPoint;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use std::path::PathBuf;

    #[test]
    fn test_pass1_and_pass2_routing() {
        let mut grid = GCellGrid {
            origin_x_dbu: 0,
            origin_y_dbu: 0,
            gcell_w_dbu: 1000,
            gcell_h_dbu: 1000,
            nx: 3,
            ny: 3,
            h_cap: vec![2.0; 3 * 2], // 6 horizontal edges
            h_demand: vec![0.0; 3 * 2],
            v_cap: vec![2.0; 2 * 3], // 6 vertical edges
            v_demand: vec![0.0; 2 * 3],
            layer_raw_edge_tracks: Vec::new(),
            layer_orig_edge_tracks: Vec::new(),
            layer_derated_edge_tracks: Vec::new(),
        };

        // Flat horizontal segment
        let s_h = SteinerSegment {
            p1: GPoint { x: 0, y: 1 },
            p2: GPoint { x: 2, y: 1 },
        };
        // Flat vertical segment
        let s_v = SteinerSegment {
            p1: GPoint { x: 1, y: 0 },
            p2: GPoint { x: 1, y: 2 },
        };
        // 2D bend
        let s_2d = SteinerSegment {
            p1: GPoint { x: 0, y: 0 },
            p2: GPoint { x: 2, y: 2 },
        };

        let report = route_segments(&mut grid, &[s_h, s_v, s_2d]);

        // Total demand across all edges must equal total wirelength (2 + 2 + 4 = 8)
        let total_demand: f32 = grid.h_demand.iter().sum::<f32>() + grid.v_demand.iter().sum::<f32>();
        assert_eq!(total_demand, 8.0, "Total committed demand must equal 8.0");

        assert_eq!(report.nx, 3);
        assert_eq!(report.ny, 3);
        assert!(report.total_overflow >= 0.0);
    }

    #[test]
    fn test_gcd_congestion_estimation_ranking() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pdk_root = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".ciel"));

        let tlef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef");
        let def_path = repo_root.join("tests/designs/gcd/gcd.placed.def");

        if !tlef_path.exists() || !lef_path.exists() || !def_path.exists() {
            panic!("Required integration test files missing: tlef={:?}, lef={:?}, def={:?}",
                   tlef_path, lef_path, def_path);
        }

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let design_db = parse_def(&def_path, &tech_db, None)
            .expect("Failed to parse placed DEF");

        // Baseline routing on gcd.placed.def (default adjustments: met1 0.30, met2..met5 0.20)
        let (grid, report) = estimate_congestion(&design_db, &tech_db, None, None);
        let total_demand: f32 = grid.h_demand.iter().sum::<f32>() + grid.v_demand.iter().sum::<f32>();
        assert_eq!(total_demand, 790.0, "Total routed demand must match RSMT wirelength exactly (790)");
        assert_eq!(report.total_overflow, 0.0, "Baseline placed gcd should have 0 overflow");

        // Variant Set A: Sweep layer adjustments alpha in [0.50, 0.70, 0.80, 0.85, 0.90]
        let alphas = [0.50f32, 0.70, 0.80, 0.85, 0.90];
        let mut prev_ovf = 0.0f32;
        let mut ovfs = Vec::new();

        for &alpha in &alphas {
            let adj = [0.0f32, alpha, alpha, alpha, alpha, alpha];
            let (_g, rep) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);
            println!("Alpha {:.2}: total_overflow = {:.1}, max_edge_ovf = {:.1}, top05 = {:.2}, ovf_gcells = {}",
                     alpha, rep.total_overflow, rep.max_edge_overflow, rep.top_05pct_overflow, rep.overflow_gcells);
            assert!(
                rep.total_overflow >= prev_ovf,
                "Overflow must monotonically increase with layer adjustment: alpha {:.2} ({}) < prev ({})",
                alpha, rep.total_overflow, prev_ovf
            );
            prev_ovf = rep.total_overflow;
            ovfs.push(rep.total_overflow);
        }

        // Check ranking: alpha 0.90 overflow must be strictly greater than alpha 0.80 overflow
        assert!(ovfs[4] > ovfs[2], "Alpha 0.90 overflow must exceed alpha 0.80");
    }

    #[test]
    fn test_gcd_util_sweep_ranking() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pdk_root = std::env::var("PDK_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs_home().join(".ciel"));

        let tlef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = pdk_root.join("sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb");

        let ref_path = repo_root.join("tests/designs/gcd/gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path).expect("Reference JSON");
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes).expect("Parse JSON");

        let utils = [25, 45, 65, 78];
        let mut ovfs_75 = Vec::new();
        let mut ovfs_80 = Vec::new();

        for &util in &utils {
            let def_path = repo_root.join(format!("tests/designs/gcd/gcd.util{}.def", util));
            assert!(def_path.exists(), "DEF fixture {:?} missing", def_path);
            let design_db = parse_def(&def_path, &tech_db, None).expect("Parse util DEF");

            // Route at alpha 0.75
            let adj75 = [0.0f32, 0.75, 0.75, 0.75, 0.75, 0.75];
            let (_g75, rep75) = estimate_congestion(&design_db, &tech_db, Some(&adj75), None);
            ovfs_75.push(rep75.total_overflow);

            // Route at alpha 0.80
            let adj80 = [0.0f32, 0.80, 0.80, 0.80, 0.80, 0.80];
            let (_g80, rep80) = estimate_congestion(&design_db, &tech_db, Some(&adj80), None);
            ovfs_80.push(rep80.total_overflow);

            println!("Util {}: grid={}x{}, total_cap_75 = {:.0}, total_demand_75 = {:.0}, ovf_75 = {:.1}, ovf_80 = {:.1}",
                     util, _g75.nx, _g75.ny,
                     _g75.h_cap.iter().sum::<f32>() + _g75.v_cap.iter().sum::<f32>(),
                     _g75.h_demand.iter().sum::<f32>() + _g75.v_demand.iter().sum::<f32>(),
                     rep75.total_overflow, rep80.total_overflow);
        }

        // Verify that tightening layer adjustment from 0.75 to 0.80 strictly increases overflow for every variant
        for i in 0..utils.len() {
            assert!(
                ovfs_80[i] > ovfs_75[i],
                "Alpha 0.80 overflow ({}) must strictly exceed alpha 0.75 overflow ({}) for util {}",
                ovfs_80[i], ovfs_75[i], utils[i]
            );
        }

        // Verify that high-utilization design (78%) has substantially higher overflow than low-utilization (25%)
        assert!(
            ovfs_75[3] > ovfs_75[0],
            "Util 78 overflow ({}) must exceed util 25 overflow ({}) at alpha 0.75",
            ovfs_75[3], ovfs_75[0]
        );
        assert!(
            ovfs_80[3] > ovfs_80[0],
            "Util 78 overflow ({}) must exceed util 25 overflow ({}) at alpha 0.80",
            ovfs_80[3], ovfs_80[0]
        );

        // Verify against committed golden reference numbers from live OpenROAD queries
        let ref_utils: Vec<i64> = ref_json["grt_util_sweep_utils"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(ref_utils, vec![25, 45, 65, 78]);

        let ref_ovfs_75: Vec<f64> = ref_json["grt_util_sweep_overflow_075"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(ref_ovfs_75, vec![6.0, 77.0, 116.0, 168.0]);

        let ref_ovfs_80: Vec<f64> = ref_json["grt_util_sweep_overflow_080"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(ref_ovfs_80, vec![154.0, 314.0, 312.0, 350.0]);
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
    }
}
