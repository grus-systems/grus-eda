use crate::congestion::gcell::{GCellGrid, GPoint, SteinerSegment};
use crate::design_db::DesignDb;
use crate::interner::NetId;
use crate::tech_db::TechDb;

/// Returns the wirelength of a Steiner segment in GCell grid units.
#[inline]
pub fn segment_wl(s: &SteinerSegment) -> u32 {
    s.p1.x.abs_diff(s.p2.x) as u32 + s.p1.y.abs_diff(s.p2.y) as u32
}

/// Integer point used by the generic RSMT core: GCell coordinates in E1, DBU in E3.
pub type Pt = (i32, i32);

#[inline]
fn manhattan(a: Pt, b: Pt) -> u32 {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

#[inline]
fn median3(a: i32, b: i32, c: i32) -> i32 {
    let mut v = [a, b, c];
    v.sort_unstable();
    v[1]
}

/// Constructs a Rectilinear Steiner Minimal Tree (RSMT) for a set of GCell points.
///
/// Degree handling:
/// - $k \le 1$: Intra-GCell net, returns 0 segments.
/// - $k = 2$: Single direct segment between the two points.
/// - $k = 3$: Computes median Steiner point $(x_s, y_s)$ and connects terminals,
///   guaranteeing total wirelength strictly equals HPWL.
/// - $k \ge 4$: Prim's rectilinear MST in $O(k^2)$ followed by iterative greedy
///   L-flip Steiner point insertions on pairs of edges sharing an endpoint with
///   maximum wirelength reduction (`gain = manhattan(u, s)`), breaking ties
///   deterministically by edge indices.
pub fn build_rsmt_points(pts: &[Pt]) -> Vec<(Pt, Pt)> {
    let mut unique = pts.to_vec();
    unique.sort_unstable();
    unique.dedup();

    let k = unique.len();
    if k <= 1 {
        return Vec::new();
    }
    if k == 2 {
        return vec![(unique[0], unique[1])];
    }
    if k == 3 {
        let sp = (
            median3(unique[0].0, unique[1].0, unique[2].0),
            median3(unique[0].1, unique[1].1, unique[2].1),
        );
        let mut segs = Vec::with_capacity(3);
        for &p in &unique {
            if p != sp {
                segs.push((p, sp));
            }
        }
        return segs;
    }

    // k >= 4: Prim's Rectilinear MST + Iterative L-flip Steiner Refinement
    let mut nodes = unique;
    let mut visited = vec![false; k];
    let mut min_dist = vec![u32::MAX; k];
    let mut parent = vec![0usize; k];

    visited[0] = true;
    for j in 1..k {
        min_dist[j] = manhattan(nodes[0], nodes[j]);
        parent[j] = 0;
    }

    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(2 * k);
    for _ in 1..k {
        let mut best_v = None;
        let mut best_d = u32::MAX;
        for (v, (&is_vis, &d)) in visited.iter().zip(min_dist.iter()).enumerate() {
            if !is_vis && d < best_d {
                best_d = d;
                best_v = Some(v);
            }
        }
        let v = best_v.expect("Connected MST vertex exists");
        visited[v] = true;
        let u = parent[v];
        edges.push((u.min(v), u.max(v)));

        for j in 0..k {
            if !visited[j] {
                let d = manhattan(nodes[v], nodes[j]);
                if d < min_dist[j] || (d == min_dist[j] && v < parent[j]) {
                    min_dist[j] = d;
                    parent[j] = v;
                }
            }
        }
    }

    // Iterative L-flip Steiner refinement
    loop {
        let mut best_candidate: Option<(u32, usize, usize, usize, usize, usize, Pt)> = None;
        // (gain, e1_idx, e2_idx, u, v, w, s_pt)

        let num_edges = edges.len();
        for e1 in 0..num_edges {
            let (a1, b1) = edges[e1];
            for (e2_offset, &(a2, b2)) in edges[(e1 + 1)..num_edges].iter().enumerate() {
                let e2 = e1 + 1 + e2_offset;

                let shared = if a1 == a2 {
                    Some((a1, b1, b2))
                } else if a1 == b2 {
                    Some((a1, b1, a2))
                } else if b1 == a2 {
                    Some((b1, a1, b2))
                } else if b1 == b2 {
                    Some((b1, a1, a2))
                } else {
                    None
                };

                if let Some((u, v, w)) = shared {
                    let pu = nodes[u];
                    let pv = nodes[v];
                    let pw = nodes[w];

                    let s_pt = (median3(pu.0, pv.0, pw.0), median3(pu.1, pv.1, pw.1));
                    let gain = manhattan(pu, s_pt);

                    if gain > 0 {
                        let is_better = match best_candidate {
                            None => true,
                            Some((best_gain, best_e1, best_e2, ..)) => {
                                if gain > best_gain {
                                    true
                                } else if gain == best_gain {
                                    (e1, e2) < (best_e1, best_e2)
                                } else {
                                    false
                                }
                            }
                        };
                        if is_better {
                            best_candidate = Some((gain, e1, e2, u, v, w, s_pt));
                        }
                    }
                }
            }
        }

        match best_candidate {
            None => break,
            Some((_gain, e1, e2, u, v, w, s_pt)) => {
                let s_idx = match nodes.iter().position(|&p| p == s_pt) {
                    Some(idx) => idx,
                    None => {
                        nodes.push(s_pt);
                        nodes.len() - 1
                    }
                };

                let candidates = [
                    (u.min(s_idx), u.max(s_idx)),
                    (v.min(s_idx), v.max(s_idx)),
                    (w.min(s_idx), w.max(s_idx)),
                ];

                edges.remove(e2);
                edges.remove(e1);

                for &(n1, n2) in &candidates {
                    if n1 != n2 && nodes[n1] != nodes[n2] && !edges.contains(&(n1, n2)) {
                        edges.push((n1, n2));
                    }
                }
            }
        }
    }

    let mut segs = Vec::with_capacity(edges.len());
    for (a, b) in edges {
        if nodes[a] != nodes[b] {
            segs.push((nodes[a], nodes[b]));
        }
    }
    segs
}

/// Constructs a Rectilinear Steiner Minimal Tree over GCell points (see `build_rsmt_points`).
pub fn build_net_rsmt(pts: &[GPoint]) -> Vec<SteinerSegment> {
    let pts: Vec<Pt> = pts.iter().map(|p| (p.x as i32, p.y as i32)).collect();
    let to_g = |p: Pt| GPoint { x: p.0 as u16, y: p.1 as u16 };
    build_rsmt_points(&pts)
        .into_iter()
        .map(|(a, b)| SteinerSegment { p1: to_g(a), p2: to_g(b) })
        .collect()
}

/// Decomposes a single net into rectilinear Steiner segments over the GCell grid.
pub fn decompose_net(
    design_db: &DesignDb,
    tech_db: &TechDb,
    grid: &GCellGrid,
    net_id: NetId,
) -> Vec<SteinerSegment> {
    let pins = design_db.net_pins(net_id);
    let mut pts = Vec::with_capacity(pins.len());
    for &pin in pins {
        let (px, py) = design_db.pin_pos_dbu(pin, tech_db);
        let gp = grid.gcell_xy(px, py);
        pts.push(gp);
    }
    build_net_rsmt(&pts)
}

/// Decomposes all nets in the design into rectilinear Steiner segments.
pub fn decompose_design_nets(
    design_db: &DesignDb,
    tech_db: &TechDb,
    grid: &GCellGrid,
) -> Vec<SteinerSegment> {
    let mut all_segs = Vec::new();
    for net_idx in 0..design_db.net_names.len() {
        let net_id = NetId(net_idx as u32);
        all_segs.extend(decompose_net(design_db, tech_db, grid, net_id));
    }
    all_segs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::path::PathBuf;

    fn verify_connectivity(pts: &[GPoint], segs: &[SteinerSegment]) {
        let mut unique: Vec<GPoint> = pts.to_vec();
        unique.sort_unstable_by_key(|a| (a.x, a.y));
        unique.dedup();

        if unique.len() <= 1 {
            assert!(segs.is_empty());
            return;
        }

        let mut adj: HashMap<GPoint, Vec<GPoint>> = HashMap::new();
        for s in segs {
            adj.entry(s.p1).or_default().push(s.p2);
            adj.entry(s.p2).or_default().push(s.p1);
        }

        let mut visited: HashSet<GPoint> = HashSet::new();
        let mut queue: VecDeque<GPoint> = VecDeque::new();
        queue.push_back(unique[0]);
        visited.insert(unique[0]);

        while let Some(curr) = queue.pop_front() {
            if let Some(neighbors) = adj.get(&curr) {
                for &nxt in neighbors {
                    if visited.insert(nxt) {
                        queue.push_back(nxt);
                    }
                }
            }
        }

        for p in &unique {
            assert!(
                visited.contains(p),
                "Terminal point {:?} is not reachable in Steiner tree",
                p
            );
        }
    }

    #[test]
    fn test_rsmt_degrees_1_to_3() {
        // Degree 1
        let pts1 = [GPoint { x: 5, y: 5 }];
        let segs1 = build_net_rsmt(&pts1);
        assert!(segs1.is_empty());

        // Degree 2
        let pts2 = [GPoint { x: 2, y: 3 }, GPoint { x: 7, y: 9 }];
        let segs2 = build_net_rsmt(&pts2);
        assert_eq!(segs2.len(), 1);
        let wl2: u32 = segs2.iter().map(segment_wl).sum();
        let hpwl2 = (7 - 2) + (9 - 3);
        assert_eq!(wl2, hpwl2);
        verify_connectivity(&pts2, &segs2);

        // Degree 3: triangle
        let pts3 = [
            GPoint { x: 1, y: 8 },
            GPoint { x: 6, y: 2 },
            GPoint { x: 9, y: 5 },
        ];
        let segs3 = build_net_rsmt(&pts3);
        let wl3: u32 = segs3.iter().map(segment_wl).sum();
        let hpwl3 = (9 - 1) + (8 - 2);
        assert_eq!(wl3, hpwl3, "Degree 3 RSMT wirelength must equal HPWL");
        verify_connectivity(&pts3, &segs3);

        // Degree 3: collinear / L-aligned with median at a terminal
        let pts3_collinear = [
            GPoint { x: 2, y: 4 },
            GPoint { x: 5, y: 4 },
            GPoint { x: 8, y: 4 },
        ];
        let segs3_c = build_net_rsmt(&pts3_collinear);
        let wl3_c: u32 = segs3_c.iter().map(segment_wl).sum();
        assert_eq!(wl3_c, 8 - 2);
        verify_connectivity(&pts3_collinear, &segs3_c);
    }

    #[test]
    fn test_rsmt_degree_4_overlapping_l_branches() {
        // 4 points with overlapping L-branches:
        // A(0, 2), B(2, 0), C(2, 4), D(5, 2)
        let pts = [
            GPoint { x: 0, y: 2 },
            GPoint { x: 2, y: 0 },
            GPoint { x: 2, y: 4 },
            GPoint { x: 5, y: 2 },
        ];
        let hpwl = 5 + 4; // 9
        let segs = build_net_rsmt(&pts);
        let rsmt_wl: u32 = segs.iter().map(segment_wl).sum();

        // MST on these 4 points has wirelength 13; optimal RSMT introduces Steiner point (2, 2)
        // reducing wirelength to 9.
        assert_eq!(hpwl, 9);
        assert_eq!(rsmt_wl, 9, "RSMT wirelength should be 9 for optimal Steiner point at (2, 2)");
        assert!(rsmt_wl < 13, "RSMT wirelength must be strictly less than MST wirelength 13");
        verify_connectivity(&pts, &segs);
    }

    #[test]
    fn test_gcd_rsmt_decomposition() {
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

        let ref_path = repo_root.join("tests/designs/gcd/gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path)
            .unwrap_or_else(|e| panic!("Reference file missing at {:?}: {}", ref_path, e));
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse {:?}: {}", ref_path, e));

        let grid = GCellGrid::build(&design_db, &tech_db, None, None);

        let ref_total_demand = ref_json["grt_total_edge_demand"]
            .as_u64()
            .expect("grt_total_edge_demand in reference JSON") as u32;
        assert_eq!(ref_total_demand, 812, "OpenROAD reference demand is 812");

        let mut total_rsmt_wl = 0u32;
        let mut degree_le3_count = 0usize;
        let mut multi_gcell_nets = 0usize;

        for net_idx in 0..design_db.net_names.len() {
            let net_id = NetId(net_idx as u32);
            let segs = decompose_net(&design_db, &tech_db, &grid, net_id);

            let pins = design_db.net_pins(net_id);
            let mut pts = Vec::with_capacity(pins.len());
            for &pin in pins {
                let (px, py) = design_db.pin_pos_dbu(pin, &tech_db);
                pts.push(grid.gcell_xy(px, py));
            }

            let mut unique = pts.clone();
            unique.sort_unstable_by_key(|a| (a.x, a.y));
            unique.dedup();
            let k = unique.len();

            if k <= 1 {
                assert!(segs.is_empty(), "Intra-GCell net {} should produce 0 segments", net_idx);
                continue;
            }

            multi_gcell_nets += 1;
            let min_x = unique.iter().map(|p| p.x).min().unwrap();
            let max_x = unique.iter().map(|p| p.x).max().unwrap();
            let min_y = unique.iter().map(|p| p.y).min().unwrap();
            let max_y = unique.iter().map(|p| p.y).max().unwrap();
            let gcell_hpwl = (max_x - min_x) as u32 + (max_y - min_y) as u32;

            let net_rsmt_wl: u32 = segs.iter().map(segment_wl).sum();
            assert!(
                net_rsmt_wl >= gcell_hpwl,
                "Net {} RSMT WL ({}) must be >= GCell HPWL ({})",
                net_idx, net_rsmt_wl, gcell_hpwl
            );

            if k <= 3 {
                degree_le3_count += 1;
                assert_eq!(
                    net_rsmt_wl, gcell_hpwl,
                    "Net {} (degree {}) RSMT WL must exactly equal HPWL",
                    net_idx, k
                );
            }

            verify_connectivity(&pts, &segs);
            total_rsmt_wl += net_rsmt_wl;
        }

        println!("Multi-GCell nets: {}, Degree <= 3 nets: {}", multi_gcell_nets, degree_le3_count);
        println!("Total RSMT wirelength (GCell units): {}", total_rsmt_wl);
        println!("OpenROAD GRT total edge demand: {}", ref_total_demand);

        // RSMT total edge length on gcd
        assert_eq!(total_rsmt_wl, 790, "Total RSMT wirelength on gcd must be 790");
        assert!(total_rsmt_wl > 0);
        // Compare with OpenROAD's global routing edge demand
        let diff = (total_rsmt_wl as f64 - ref_total_demand as f64).abs();
        let pct_diff = (diff / ref_total_demand as f64) * 100.0;
        println!("RSMT WL vs OpenROAD GRT Demand diff: {:.2}%", pct_diff);
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
    }
}
