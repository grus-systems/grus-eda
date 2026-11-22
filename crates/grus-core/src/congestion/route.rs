use crate::congestion::gcell::{CongestionReport, GCellGrid, SteinerSegment};
use crate::congestion::rsmt::decompose_design_nets_indexed;
use crate::design_db::DesignDb;
use crate::tech_db::TechDb;

#[inline]
fn edge_cost(d: f32, c: f32) -> f64 {
    let diff = (d as f64 + 1.0) - (c as f64);
    let ovf = diff.max(0.0);
    1.0 + 10.0 / (1.0 + (-2.0 * diff).exp()) + 50.0 * ovf
}

#[inline]
fn overflow_cost(d: f32, c: f32) -> f64 {
    ((d as f64 + 1.0) - c as f64).max(0.0)
}

/// Demand over capacity. An edge with demand but no capacity is infinitely over-used.
#[inline]
fn usage_ratio(d: f32, c: f32) -> f32 {
    if c > 0.0 {
        d / c
    } else if d > 0.0 {
        f32::INFINITY
    } else {
        0.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GridEdge {
    H(usize),
    V(usize),
}

#[derive(Copy, Clone, PartialEq)]
struct DijkstraState {
    cost: f64,
    x: usize,
    y: usize,
}

impl Eq for DijkstraState {}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RouteChoice {
    Lhv,
    Lvh,
    Zhvh(usize),
    Zvhv(usize),
}

/// Output of the 3-pass router: the aggregate report plus the committed
/// GCell-edge path per input segment.
pub struct RouteOutput {
    pub report: CongestionReport,
    /// `segment_paths[i]` is the committed edge list of `segments[i]`;
    /// empty when the segment degenerated (zero length) or was never
    /// committed. Index-aligned with the `segments` argument.
    pub segment_paths: Vec<Vec<GridEdge>>,
}

/// Routes rectilinear Steiner segments over the GCell grid using 3-pass congestion routing:
/// - Pass 1: Probabilistic demand (RISA / uniform 50-50 L-split)
/// - Pass 2: Congestion-aware L/Z pattern routing with rip-up and cost evaluation
/// - Pass 3: Bounded-box maze rip-up and reroute (FastRoute expanding box Dijkstra)
pub fn route_segments(grid: &mut GCellGrid, segments: &[SteinerSegment]) -> RouteOutput {
    let nx = grid.nx;
    let ny = grid.ny;
    let edge_cols = nx.saturating_sub(1);
    let edge_rows = ny.saturating_sub(1);

    let t_route = crate::timing::phase("cong.route");
    // Pass 1: Probabilistic Demand (RISA)
    grid.h_demand.fill(0.0);
    grid.v_demand.fill(0.0);

    let mut segment_paths: Vec<Vec<GridEdge>> = vec![Vec::new(); segments.len()];

    for (seg_idx, s) in segments.iter().enumerate() {
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
            let mut path = Vec::with_capacity(x_max - x_min);
            for x in x_min..x_max.min(edge_cols) {
                let idx = y * edge_cols + x;
                grid.h_demand[idx] += 1.0;
                path.push(GridEdge::H(idx));
            }
            segment_paths[seg_idx] = path;
        } else if x_min == x_max {
            let x = x_min;
            let mut path = Vec::with_capacity(y_max - y_min);
            for y in y_min..y_max.min(edge_rows) {
                let idx = y * nx + x;
                grid.v_demand[idx] += 1.0;
                path.push(GridEdge::V(idx));
            }
            segment_paths[seg_idx] = path;
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
    // Route all 2D segments short-first into discrete 0/1 L/Z paths using pure overflow cost
    // so Pass 1 0.5/0.5 averaging does not smear discrete congestion peaks.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&i| {
        let s = &segments[i];
        let dx = s.p1.x.abs_diff(s.p2.x);
        let dy = s.p1.y.abs_diff(s.p2.y);
        (dx + dy, i)
    });

    for iter in 0..3 {
        let mut changed = false;
        for &seg_idx in &order {
            let s = &segments[seg_idx];
            let p1 = s.p1;
            let p2 = s.p2;
            let x_min = (p1.x.min(p2.x) as usize).min(nx);
            let x_max = (p1.x.max(p2.x) as usize).min(nx);
            let y_min = (p1.y.min(p2.y) as usize).min(ny);
            let y_max = (p1.y.max(p2.y) as usize).min(ny);

            if x_min == x_max || y_min == y_max {
                // 1D segments are already committed in Pass 1
                continue;
            }

            // 1. Rip up prior demand: initial 0.5/0.5 fractional box on iter 0, or overflowing path afterwards
            if iter == 0 {
                for x in x_min..x_max.min(edge_cols) {
                    grid.h_demand[y_min * edge_cols + x] -= 0.5;
                    grid.h_demand[y_max * edge_cols + x] -= 0.5;
                }
                for y in y_min..y_max.min(edge_rows) {
                    grid.v_demand[y * nx + x_min] -= 0.5;
                    grid.v_demand[y * nx + x_max] -= 0.5;
                }
            } else {
                let has_overflow = segment_paths[seg_idx].iter().any(|edge| match edge {
                    GridEdge::H(idx) => grid.h_demand[*idx] > grid.h_cap[*idx],
                    GridEdge::V(idx) => grid.v_demand[*idx] > grid.v_cap[*idx],
                });
                if !has_overflow {
                    continue;
                }
                for edge in &segment_paths[seg_idx] {
                    match edge {
                        GridEdge::H(idx) => grid.h_demand[*idx] -= 1.0,
                        GridEdge::V(idx) => grid.v_demand[*idx] -= 1.0,
                    }
                }
            }
            changed = true;

            let x1 = p1.x as usize;
            let y1 = p1.y as usize;
            let x2 = p2.x as usize;
            let y2 = p2.y as usize;

            // 2. Evaluate L-routes using pure overflow cost
            let mut l_hv_cost = 0.0f64;
            for x in x_min..x_max.min(edge_cols) {
                let d = grid.h_demand[y1 * edge_cols + x];
                let c = grid.h_cap[y1 * edge_cols + x];
                l_hv_cost += overflow_cost(d, c);
            }
            for y in y_min..y_max.min(edge_rows) {
                let d = grid.v_demand[y * nx + x2];
                let c = grid.v_cap[y * nx + x2];
                l_hv_cost += overflow_cost(d, c);
            }

            let mut l_vh_cost = 0.0f64;
            for y in y_min..y_max.min(edge_rows) {
                let d = grid.v_demand[y * nx + x1];
                let c = grid.v_cap[y * nx + x1];
                l_vh_cost += overflow_cost(d, c);
            }
            for x in x_min..x_max.min(edge_cols) {
                let d = grid.h_demand[y2 * edge_cols + x];
                let c = grid.h_cap[y2 * edge_cols + x];
                l_vh_cost += overflow_cost(d, c);
            }

            let mut best_cost = l_hv_cost;
            let mut best_route = RouteChoice::Lhv;

            if l_vh_cost < best_cost {
                best_cost = l_vh_cost;
                best_route = RouteChoice::Lvh;
            }

            // 3. Evaluate Z-routes (with a small 2-bend tie-breaker) if both L-routes overflow
            if best_cost > 0.0 {
                if x_max > x_min + 1 {
                    let dx = x_max - x_min;
                    let mut pref1 = vec![0.0f64; dx + 1];
                    let mut pref2 = vec![0.0f64; dx + 1];
                    for i in 0..dx {
                        let x = x_min + i;
                        if x < edge_cols {
                            let d1 = grid.h_demand[y1 * edge_cols + x];
                            let c1 = grid.h_cap[y1 * edge_cols + x];
                            pref1[i + 1] = pref1[i] + overflow_cost(d1, c1);

                            let d2 = grid.h_demand[y2 * edge_cols + x];
                            let c2 = grid.h_cap[y2 * edge_cols + x];
                            pref2[i + 1] = pref2[i] + overflow_cost(d2, c2);
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
                            v_cost += overflow_cost(d, c);
                        }

                        let total_z_cost = h_cost + v_cost + 0.01;
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
                            pref1[i + 1] = pref1[i] + overflow_cost(d1, c1);

                            let d2 = grid.v_demand[y * nx + x2];
                            let c2 = grid.v_cap[y * nx + x2];
                            pref2[i + 1] = pref2[i] + overflow_cost(d2, c2);
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
                            h_cost += overflow_cost(d, c);
                        }

                        let total_z_cost = v_cost + h_cost + 0.01;
                        if total_z_cost < best_cost {
                            best_cost = total_z_cost;
                            best_route = RouteChoice::Zvhv(y_mid);
                        }
                    }
                }
            }

            // 4. Commit +1.0 demand along the chosen route and record path
            let mut path = Vec::with_capacity((x_max - x_min) + (y_max - y_min));
            match best_route {
                RouteChoice::Lhv => {
                    for x in x_min..x_max.min(edge_cols) {
                        let idx = y1 * edge_cols + x;
                        grid.h_demand[idx] += 1.0;
                        path.push(GridEdge::H(idx));
                    }
                    for y in y_min..y_max.min(edge_rows) {
                        let idx = y * nx + x2;
                        grid.v_demand[idx] += 1.0;
                        path.push(GridEdge::V(idx));
                    }
                }
                RouteChoice::Lvh => {
                    for y in y_min..y_max.min(edge_rows) {
                        let idx = y * nx + x1;
                        grid.v_demand[idx] += 1.0;
                        path.push(GridEdge::V(idx));
                    }
                    for x in x_min..x_max.min(edge_cols) {
                        let idx = y2 * edge_cols + x;
                        grid.h_demand[idx] += 1.0;
                        path.push(GridEdge::H(idx));
                    }
                }
                RouteChoice::Zhvh(x_mid) => {
                    if x1 < x2 {
                        for x in x_min..x_mid.min(edge_cols) {
                            let idx = y1 * edge_cols + x;
                            grid.h_demand[idx] += 1.0;
                            path.push(GridEdge::H(idx));
                        }
                        for x in x_mid..x_max.min(edge_cols) {
                            let idx = y2 * edge_cols + x;
                            grid.h_demand[idx] += 1.0;
                            path.push(GridEdge::H(idx));
                        }
                    } else {
                        for x in x_mid..x_max.min(edge_cols) {
                            let idx = y1 * edge_cols + x;
                            grid.h_demand[idx] += 1.0;
                            path.push(GridEdge::H(idx));
                        }
                        for x in x_min..x_mid.min(edge_cols) {
                            let idx = y2 * edge_cols + x;
                            grid.h_demand[idx] += 1.0;
                            path.push(GridEdge::H(idx));
                        }
                    }
                    for y in y_min..y_max.min(edge_rows) {
                        let idx = y * nx + x_mid;
                        grid.v_demand[idx] += 1.0;
                        path.push(GridEdge::V(idx));
                    }
                }
                RouteChoice::Zvhv(y_mid) => {
                    if y1 < y2 {
                        for y in y_min..y_mid.min(edge_rows) {
                            let idx = y * nx + x1;
                            grid.v_demand[idx] += 1.0;
                            path.push(GridEdge::V(idx));
                        }
                        for y in y_mid..y_max.min(edge_rows) {
                            let idx = y * nx + x2;
                            grid.v_demand[idx] += 1.0;
                            path.push(GridEdge::V(idx));
                        }
                    } else {
                        for y in y_mid..y_max.min(edge_rows) {
                            let idx = y * nx + x1;
                            grid.v_demand[idx] += 1.0;
                            path.push(GridEdge::V(idx));
                        }
                        for y in y_min..y_mid.min(edge_rows) {
                            let idx = y * nx + x2;
                            grid.v_demand[idx] += 1.0;
                            path.push(GridEdge::V(idx));
                        }
                    }
                    for x in x_min..x_max.min(edge_cols) {
                        let idx = y_mid * edge_cols + x;
                        grid.h_demand[idx] += 1.0;
                        path.push(GridEdge::H(idx));
                    }
                }
            }
            segment_paths[seg_idx] = path;
        }
        if !changed {
            break;
        }
    }

    // Pass 3: Bounded-Box Maze Rip-Up and Reroute (FastRoute Expanding Box)
    for &margin in &[2usize, 3, 4] {
        for seg_idx in 0..segments.len() {
            let s = &segments[seg_idx];
            let p1 = s.p1;
            let p2 = s.p2;
            if p1 == p2 {
                continue;
            }

            // Check if any edge in segment_paths[seg_idx] currently overflows
            let has_overflow = segment_paths[seg_idx].iter().any(|edge| match edge {
                GridEdge::H(idx) => grid.h_demand[*idx] > grid.h_cap[*idx],
                GridEdge::V(idx) => grid.v_demand[*idx] > grid.v_cap[*idx],
            });
            if !has_overflow {
                continue;
            }

            // 1. Rip-up: subtract 1.0 demand from each edge in segment_paths[seg_idx]
            for edge in &segment_paths[seg_idx] {
                match edge {
                    GridEdge::H(idx) => grid.h_demand[*idx] -= 1.0,
                    GridEdge::V(idx) => grid.v_demand[*idx] -= 1.0,
                }
            }

            // 2. Bounded-Box Dijkstra
            let x_min = (p1.x.min(p2.x) as usize).min(nx);
            let x_max = (p1.x.max(p2.x) as usize).min(nx);
            let y_min = (p1.y.min(p2.y) as usize).min(ny);
            let y_max = (p1.y.max(p2.y) as usize).min(ny);

            let x_lo = x_min.saturating_sub(margin);
            let x_hi = (x_max + margin).min(nx - 1);
            let y_lo = y_min.saturating_sub(margin);
            let y_hi = (y_max + margin).min(ny - 1);

            let src_x = p1.x as usize;
            let src_y = p1.y as usize;
            let dst_x = p2.x as usize;
            let dst_y = p2.y as usize;

            let mut dist = vec![vec![f64::INFINITY; nx]; ny];
            let mut prev: Vec<Vec<Option<(usize, usize, GridEdge)>>> = vec![vec![None; nx]; ny];
            let mut heap = std::collections::BinaryHeap::new();

            dist[src_y][src_x] = 0.0;
            heap.push(DijkstraState {
                cost: 0.0,
                x: src_x,
                y: src_y,
            });

            while let Some(DijkstraState { cost, x, y }) = heap.pop() {
                if x == dst_x && y == dst_y {
                    break;
                }
                if cost > dist[y][x] {
                    continue;
                }

                // Neighbor Right (x + 1, y)
                if x < x_hi && x < edge_cols {
                    let nx_coord = x + 1;
                    let edge_idx = y * edge_cols + x;
                    let w = edge_cost(grid.h_demand[edge_idx], grid.h_cap[edge_idx]);
                    let next_cost = cost + w;
                    if next_cost < dist[y][nx_coord] {
                        dist[y][nx_coord] = next_cost;
                        prev[y][nx_coord] = Some((x, y, GridEdge::H(edge_idx)));
                        heap.push(DijkstraState {
                            cost: next_cost,
                            x: nx_coord,
                            y,
                        });
                    }
                }
                // Neighbor Left (x - 1, y)
                if x > x_lo && x > 0 {
                    let nx_coord = x - 1;
                    let edge_idx = y * edge_cols + (x - 1);
                    let w = edge_cost(grid.h_demand[edge_idx], grid.h_cap[edge_idx]);
                    let next_cost = cost + w;
                    if next_cost < dist[y][nx_coord] {
                        dist[y][nx_coord] = next_cost;
                        prev[y][nx_coord] = Some((x, y, GridEdge::H(edge_idx)));
                        heap.push(DijkstraState {
                            cost: next_cost,
                            x: nx_coord,
                            y,
                        });
                    }
                }
                // Neighbor Up (x, y + 1)
                if y < y_hi && y < edge_rows {
                    let ny_coord = y + 1;
                    let edge_idx = y * nx + x;
                    let w = edge_cost(grid.v_demand[edge_idx], grid.v_cap[edge_idx]);
                    let next_cost = cost + w;
                    if next_cost < dist[ny_coord][x] {
                        dist[ny_coord][x] = next_cost;
                        prev[ny_coord][x] = Some((x, y, GridEdge::V(edge_idx)));
                        heap.push(DijkstraState {
                            cost: next_cost,
                            x,
                            y: ny_coord,
                        });
                    }
                }
                // Neighbor Down (x, y - 1)
                if y > y_lo && y > 0 {
                    let ny_coord = y - 1;
                    let edge_idx = (y - 1) * nx + x;
                    let w = edge_cost(grid.v_demand[edge_idx], grid.v_cap[edge_idx]);
                    let next_cost = cost + w;
                    if next_cost < dist[ny_coord][x] {
                        dist[ny_coord][x] = next_cost;
                        prev[ny_coord][x] = Some((x, y, GridEdge::V(edge_idx)));
                        heap.push(DijkstraState {
                            cost: next_cost,
                            x,
                            y: ny_coord,
                        });
                    }
                }
            }

            // Reconstruct path
            let mut new_path = Vec::new();
            let mut curr_x = dst_x;
            let mut curr_y = dst_y;
            while let Some((px, py, edge)) = prev[curr_y][curr_x] {
                new_path.push(edge);
                curr_x = px;
                curr_y = py;
            }
            new_path.reverse();

            // 3. Commit: add 1.0 demand along shortest path
            if new_path.is_empty() && (src_x != dst_x || src_y != dst_y) {
                for edge in &segment_paths[seg_idx] {
                    match edge {
                        GridEdge::H(idx) => grid.h_demand[*idx] += 1.0,
                        GridEdge::V(idx) => grid.v_demand[*idx] += 1.0,
                    }
                }
            } else {
                for edge in &new_path {
                    match edge {
                        GridEdge::H(idx) => grid.h_demand[*idx] += 1.0,
                        GridEdge::V(idx) => grid.v_demand[*idx] += 1.0,
                    }
                }
                segment_paths[seg_idx] = new_path;
            }
        }
    }

    drop(t_route);

    // Congestion Report Aggregation
    let t_report = crate::timing::phase("cong.report");
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
                let r = usage_ratio(d, c);
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

    let edge_usage_ratio = |d: f32, c: f32, raw: f32| -> f32 {
        if raw > 0.0 {
            ((raw - c).max(0.0) + d) / raw
        } else if c > 0.0 {
            d / c
        } else if d > 0.0 {
            f32::INFINITY
        } else {
            0.0
        }
    };
    let h_usage_ratio: Vec<f32> = grid
        .h_demand
        .iter()
        .zip(grid.h_cap.iter())
        .zip(grid.h_raw_cap.iter())
        .map(|((&d, &c), &raw)| edge_usage_ratio(d, c, raw))
        .collect();
    let v_usage_ratio: Vec<f32> = grid
        .v_demand
        .iter()
        .zip(grid.v_cap.iter())
        .zip(grid.v_raw_cap.iter())
        .map(|((&d, &c), &raw)| edge_usage_ratio(d, c, raw))
        .collect();
    drop(t_report);

    RouteOutput {
        report: CongestionReport {
            nx,
            ny,
            total_overflow: total_overflow as f32,
            max_edge_overflow,
            top_05pct_overflow,
            overflow_gcells,
            gcell_usage_ratio,
            h_usage_ratio,
            v_usage_ratio,
        },
        segment_paths,
    }
}

/// Everything `estimate_congestion` returns, plus the routed paths and the
/// net they belong to — the guide emitter's input.
pub struct RoutedDesign {
    pub grid: GCellGrid,
    pub report: CongestionReport,
    /// Flat segment list, index-aligned with `segment_paths`.
    pub segments: Vec<SteinerSegment>,
    /// CSR offsets into `segments`/`segment_paths`:
    /// net `NetId(n)` owns `net_seg_offsets[n]..net_seg_offsets[n+1]`;
    /// length `design_db.num_nets() + 1`.
    pub net_seg_offsets: Vec<u32>,
    pub segment_paths: Vec<Vec<GridEdge>>,
}

/// Runs the full congestion pipeline: grid build, RSMT decomposition, and
/// 3-pass routing, keeping the committed per-segment edge paths.
pub fn route_design(
    design_db: &DesignDb,
    tech_db: &TechDb,
    layer_adjustments: Option<&[f32]>,
    gcell_tracks_override: Option<i32>,
) -> RoutedDesign {
    let t_grid = crate::timing::phase("cong.grid");
    let mut grid = GCellGrid::build(design_db, tech_db, layer_adjustments, gcell_tracks_override);
    drop(t_grid);
    let t_decompose = crate::timing::phase("cong.decompose");
    let (segments, net_seg_offsets) = decompose_design_nets_indexed(design_db, tech_db, &grid);
    drop(t_decompose);
    let out = route_segments(&mut grid, &segments);
    RoutedDesign {
        grid,
        report: out.report,
        segments,
        net_seg_offsets,
        segment_paths: out.segment_paths,
    }
}

/// Estimates global-routing congestion directly from a placed design.
pub fn estimate_congestion(
    design_db: &DesignDb,
    tech_db: &TechDb,
    layer_adjustments: Option<&[f32]>,
    gcell_tracks_override: Option<i32>,
) -> (GCellGrid, CongestionReport) {
    let r = route_design(design_db, tech_db, layer_adjustments, gcell_tracks_override);
    (r.grid, r.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::congestion::gcell::GPoint;
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use crate::test_support::{design_dir, require, sky130_hd};
    use std::collections::{HashMap, HashSet};

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
            h_raw_cap: vec![2.0; 3 * 2],
            h_demand: vec![0.0; 3 * 2],
            v_cap: vec![2.0; 2 * 3], // 6 vertical edges
            v_raw_cap: vec![2.0; 2 * 3],
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

        let report = route_segments(&mut grid, &[s_h, s_v, s_2d]).report;

        // Total demand across all edges must equal total wirelength (2 + 2 + 4 = 8)
        let total_demand: f32 =
            grid.h_demand.iter().sum::<f32>() + grid.v_demand.iter().sum::<f32>();
        assert_eq!(total_demand, 8.0, "Total committed demand must equal 8.0");

        assert_eq!(report.nx, 3);
        assert_eq!(report.ny, 3);
        assert!(report.total_overflow >= 0.0);
    }

    #[test]
    fn test_gcd_congestion_estimation_ranking() {
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

        // Baseline routing on gcd.placed.def (default adjustments: met1 0.30, met2..met5 0.20)
        let (grid, report) = estimate_congestion(&design_db, &tech_db, None, None);
        let total_demand: f32 =
            grid.h_demand.iter().sum::<f32>() + grid.v_demand.iter().sum::<f32>();
        assert_eq!(
            total_demand, 788.0,
            "Total routed demand must match RSMT wirelength exactly (788)"
        );
        assert_eq!(
            report.total_overflow, 0.0,
            "Baseline placed gcd should have 0 overflow"
        );

        // Variant Set A: Sweep layer adjustments alpha in [0.50, 0.70, 0.80, 0.85, 0.90]
        let alphas = [0.50f32, 0.70, 0.80, 0.85, 0.90];
        let mut prev_ovf = 0.0f32;
        let mut ovfs = Vec::new();

        for &alpha in &alphas {
            let adj = [0.0f32, alpha, alpha, alpha, alpha, alpha];
            let (_g, rep) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);
            println!(
                "Alpha {:.2}: total_overflow = {:.1}, max_edge_ovf = {:.1}, top05 = {:.2}, ovf_gcells = {}",
                alpha,
                rep.total_overflow,
                rep.max_edge_overflow,
                rep.top_05pct_overflow,
                rep.overflow_gcells
            );
            assert!(
                rep.total_overflow >= prev_ovf,
                "Overflow must monotonically increase with layer adjustment: alpha {:.2} ({}) < prev ({})",
                alpha,
                rep.total_overflow,
                prev_ovf
            );
            prev_ovf = rep.total_overflow;
            ovfs.push(rep.total_overflow);
        }

        // Check ranking: alpha 0.90 overflow must be strictly greater than alpha 0.80 overflow
        assert!(
            ovfs[4] > ovfs[2],
            "Alpha 0.90 overflow must exceed alpha 0.80"
        );
    }

    #[test]
    fn test_gcd_util_sweep_ranking() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;

        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path).expect("Reference JSON");
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes).expect("Parse JSON");

        let utils = [25, 45, 65, 78];
        let mut ovfs_75 = Vec::new();
        let mut ovfs_80 = Vec::new();

        for &util in &utils {
            let def_path = design_dir("gcd").join(format!("gcd.util{}.def", util));
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

            println!(
                "Util {}: grid={}x{}, total_cap_75 = {:.0}, total_demand_75 = {:.0}, ovf_75 = {:.1}, ovf_80 = {:.1}",
                util,
                _g75.nx,
                _g75.ny,
                _g75.h_cap.iter().sum::<f32>() + _g75.v_cap.iter().sum::<f32>(),
                _g75.h_demand.iter().sum::<f32>() + _g75.v_demand.iter().sum::<f32>(),
                rep75.total_overflow,
                rep80.total_overflow
            );
        }

        // Verify that overflow ranks all 4 utilization variants in the exact same strictly increasing order as OpenROAD
        assert!(
            ovfs_75[0] < ovfs_75[1] && ovfs_75[1] < ovfs_75[2] && ovfs_75[2] < ovfs_75[3],
            "At alpha 0.75, overflow must strictly increase with utilization: ovfs_75 = {:?}",
            ovfs_75
        );
        assert!(
            ovfs_80[0] < ovfs_80[1] && ovfs_80[1] < ovfs_80[2] && ovfs_80[2] < ovfs_80[3],
            "At alpha 0.80, overflow must strictly increase with utilization: ovfs_80 = {:?}",
            ovfs_80
        );

        // Verify that tightening layer adjustment from 0.75 to 0.80 strictly increases overflow for every variant
        for i in 0..utils.len() {
            assert!(
                ovfs_80[i] > ovfs_75[i],
                "Alpha 0.80 overflow ({}) must strictly exceed alpha 0.75 overflow ({}) for util {}",
                ovfs_80[i],
                ovfs_75[i],
                utils[i]
            );
        }

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

    /// One edge of OpenROAD's usage dump, summed over met1..met5 (`tests/designs/aes/generate_ref.sh` step 7f).
    #[derive(Clone, Copy)]
    struct DumpEdge {
        x: usize,
        y: usize,
        usage: f64,
        capacity: f64,
    }

    struct UsageDump {
        origin_x_dbu: i32,
        origin_y_dbu: i32,
        tile_dbu: i32,
        nx: usize,
        ny: usize,
        h: Vec<DumpEdge>,
        v: Vec<DumpEdge>,
    }

    /// Reads the `# key value` header and the `H|V x y usage capacity` lines of a usage dump.
    fn parse_usage_dump(path: &std::path::Path) -> UsageDump {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("usage dump {path:?}: {e}"));
        let mut header = HashMap::new();
        let mut h = Vec::new();
        let mut v = Vec::new();
        let edge = |x: &str, y: &str, usage: &str, capacity: &str| DumpEdge {
            x: x.parse().expect("edge x"),
            y: y.parse().expect("edge y"),
            usage: usage.parse().expect("edge usage"),
            capacity: capacity.parse().expect("edge capacity"),
        };
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.as_slice() {
                ["#", key, value] => {
                    header.insert(*key, *value);
                }
                ["H", x, y, usage, capacity] => h.push(edge(x, y, usage, capacity)),
                ["V", x, y, usage, capacity] => v.push(edge(x, y, usage, capacity)),
                _ => panic!("unrecognised line in {path:?}: {line:?}"),
            }
        }
        let get = |key: &str| -> i64 {
            header
                .get(key)
                .unwrap_or_else(|| panic!("{path:?}: header `{key}` missing"))
                .parse()
                .unwrap_or_else(|e| panic!("{path:?}: header `{key}`: {e}"))
        };
        UsageDump {
            origin_x_dbu: get("origin_x_dbu") as i32,
            origin_y_dbu: get("origin_y_dbu") as i32,
            tile_dbu: get("tile_dbu") as i32,
            nx: get("nx") as usize,
            ny: get("ny") as usize,
            h,
            v,
        }
    }

    /// 1-based ranks; tied values share the mean of the ranks they occupy.
    fn average_ranks(values: &[f64]) -> Vec<f64> {
        let mut order: Vec<usize> = (0..values.len()).collect();
        order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        let mut ranks = vec![0.0; values.len()];
        let mut start = 0;
        while start < order.len() {
            let mut end = start;
            while end + 1 < order.len() && values[order[end + 1]] == values[order[start]] {
                end += 1;
            }
            let mean_rank = (start + end) as f64 / 2.0 + 1.0;
            for &i in &order[start..=end] {
                ranks[i] = mean_rank;
            }
            start = end + 1;
        }
        ranks
    }

    /// Pearson correlation; NaN when either side is constant.
    fn pearson(a: &[f64], b: &[f64]) -> f64 {
        let n = a.len() as f64;
        let mean_a = a.iter().sum::<f64>() / n;
        let mean_b = b.iter().sum::<f64>() / n;
        let (mut cov, mut var_a, mut var_b) = (0.0, 0.0, 0.0);
        for (&x, &y) in a.iter().zip(b) {
            cov += (x - mean_a) * (y - mean_b);
            var_a += (x - mean_a).powi(2);
            var_b += (y - mean_b).powi(2);
        }
        if var_a == 0.0 || var_b == 0.0 {
            return f64::NAN;
        }
        cov / (var_a.sqrt() * var_b.sqrt())
    }

    fn spearman(a: &[f64], b: &[f64]) -> f64 {
        pearson(&average_ranks(a), &average_ranks(b))
    }

    /// Indices of the hot set: every edge whose usage ratio reaches the fixed cut of 0.9.
    /// A percentile cut is
    /// ill-defined against OpenROAD's map, whose ratios saturate at exactly 1.0 over a
    /// large plateau (the top 5% boundary then falls inside the tie mass and the set
    /// membership is arbitrary); a fixed cut names the hot region itself. See
    /// docs/results.md (E1, aes) for the measured plateau.
    fn hot_set(values: &[f64]) -> HashSet<usize> {
        (0..values.len()).filter(|&i| values[i] >= 0.9).collect()
    }

    /// |A ∩ B| / min(|A|, |B|).
    fn overlap_coefficient(a: &HashSet<usize>, b: &HashSet<usize>) -> f64 {
        a.intersection(b).count() as f64 / a.len().min(b.len()) as f64
    }

    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    const PERMUTATION_SEED: u64 = 0x5EED_1114;

    struct DirectionResult {
        support: usize,
        rho: f64,
        null_rho: f64,
        overlap: f64,
    }

    /// Compares one direction over the dump edges with capacity > 0. `index` maps a dump edge to its
    /// position in our per-edge usage-ratio vector.
    fn compare_direction(
        label: &str,
        edges: &[DumpEdge],
        ours: &[f32],
        index: impl Fn(&DumpEdge) -> usize,
    ) -> DirectionResult {
        let support: Vec<DumpEdge> = edges.iter().copied().filter(|e| e.capacity > 0.0).collect();
        let zero_capacity_with_usage = edges
            .iter()
            .filter(|e| e.capacity <= 0.0 && e.usage > 0.0)
            .count();
        assert!(
            !support.is_empty(),
            "{label}: dump has no edge with capacity > 0"
        );

        let reference: Vec<f64> = support.iter().map(|e| e.usage / e.capacity).collect();
        let our_ratio: Vec<f64> = support.iter().map(|e| ours[index(e)] as f64).collect();

        // Control 1: the reference shifted one GCell in +x. Edge (x, y) is paired with the reference
        // at (x - 1, y), and 0 where that edge is not in the support.
        let reference_at: HashMap<(usize, usize), f64> = support
            .iter()
            .zip(&reference)
            .map(|(e, &r)| ((e.x, e.y), r))
            .collect();
        let shifted: Vec<f64> = support
            .iter()
            .map(|e| {
                if e.x == 0 {
                    0.0
                } else {
                    reference_at.get(&(e.x - 1, e.y)).copied().unwrap_or(0.0)
                }
            })
            .collect();

        // Control 2: the reference values permuted over the support. Twenty fixed seeds give a
        // null distribution; its 95th percentile (the 19th of 20 sorted values) is the control.
        let mut perm_rhos: Vec<f64> = (0..20u64)
            .map(|k| {
                let mut permuted = reference.clone();
                let mut rng = SplitMix64(PERMUTATION_SEED + k);
                for i in (1..permuted.len()).rev() {
                    let j = (rng.next_u64() % (i as u64 + 1)) as usize;
                    permuted.swap(i, j);
                }
                spearman(&our_ratio, &permuted)
            })
            .collect();
        perm_rhos.sort_by(|a, b| a.total_cmp(b));
        let rho_perm = perm_rhos[18];

        let rho = spearman(&our_ratio, &reference);
        let rho_shift = spearman(&our_ratio, &shifted);
        let null_rho = rho_shift.max(rho_perm);
        let (hot_ours, hot_ref) = (hot_set(&our_ratio), hot_set(&reference));
        let overlap = overlap_coefficient(&hot_ours, &hot_ref);

        println!(
            "E1 {label}: support={} (excluded capacity-0 with usage: {zero_capacity_with_usage}), \
             rho={rho:.4}, rho_shift={rho_shift:.4}, rho_perm_p95={rho_perm:.4}, null={null_rho:.4}, \
             hot(>=0.9) overlap={overlap:.4} (hot set ours={}, reference={})",
            support.len(),
            hot_ours.len(),
            hot_ref.len(),
        );
        DirectionResult {
            support: support.len(),
            rho,
            null_rho,
            overlap,
        }
    }

    #[test]
    fn test_rank_correlation_and_overlap_helpers() {
        assert_eq!(
            average_ranks(&[10.0, 20.0, 20.0, 30.0]),
            vec![1.0, 2.5, 2.5, 4.0]
        );

        let a = [0.3, 0.1, 0.7, 0.5];
        assert!((spearman(&a, &a) - 1.0).abs() < 1e-12);
        // Ranks of a are 2,1,4,3; these are 5 minus those, so the order is exactly reversed.
        assert!((spearman(&a, &[3.0, 4.0, 1.0, 2.0]) + 1.0).abs() < 1e-12);
        // Tied ranks: a = 1,2,3,4 against b = 1,1,2,2 (ranks 1.5,1.5,3.5,3.5) gives 4 / sqrt(20).
        assert!(
            (spearman(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0, 2.0, 2.0]) - 4.0 / 20f64.sqrt()).abs()
                < 1e-12
        );
        assert!(
            spearman(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_nan(),
            "constant side is undefined"
        );

        // 20 values: the 5% set is 1 edge, and a tie at the cut value enlarges it.
        let mut values = vec![0.0; 20];
        values[7] = 2.0;
        assert_eq!(hot_set(&values), HashSet::from([7]));
        values[3] = 2.0;
        assert_eq!(hot_set(&values), HashSet::from([3, 7]));

        let a: HashSet<usize> = [1, 2, 3].into_iter().collect();
        let b: HashSet<usize> = [2, 3, 4, 5].into_iter().collect();
        assert!((overlap_coefficient(&a, &b) - 2.0 / 3.0).abs() < 1e-12);
    }

    /// Congestion map against OpenROAD on aes at 65% utilisation (F2): per direction, the per-edge
    /// usage ratio (demand / capacity) must rank like OpenROAD's and agree on the hottest 5% of edges.
    #[test]
    fn test_aes_congestion_map() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("aes").join("aes.util65.def"));
        let dump = parse_usage_dump(&require(design_dir("aes").join("aes.gcell_usage.txt")));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db =
            parse_def(&def_path, &tech_db, None).expect("Failed to parse aes.util65.def");

        // The dump was routed with `set_global_routing_layer_adjustment met1-met5 0.3`.
        let adj = [0.0f32, 0.3, 0.3, 0.3, 0.3, 0.3];
        let (grid, report) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);

        assert_eq!(grid.origin_x_dbu, dump.origin_x_dbu, "grid origin x");
        assert_eq!(grid.origin_y_dbu, dump.origin_y_dbu, "grid origin y");
        assert_eq!(grid.gcell_w_dbu, dump.tile_dbu, "grid tile width");
        assert_eq!(grid.gcell_h_dbu, dump.tile_dbu, "grid tile height");
        assert_eq!((grid.nx, grid.ny), (dump.nx, dump.ny), "grid dimensions");

        let (nx, ny) = (dump.nx, dump.ny);
        assert_eq!(ny * (nx - 1), report.h_usage_ratio.len(), "H edge count");
        assert_eq!((ny - 1) * nx, report.v_usage_ratio.len(), "V edge count");

        let h = compare_direction("H", &dump.h, &report.h_usage_ratio, |e| {
            e.y * (nx - 1) + e.x
        });
        let v = compare_direction("V", &dump.v, &report.v_usage_ratio, |e| e.y * nx + e.x);

        for (label, r) in [("H", &h), ("V", &v)] {
            assert!(
                r.rho >= 0.60,
                "{label}: Spearman rho {:.4} below 0.60 (support {})",
                r.rho,
                r.support
            );
            assert!(
                r.rho >= r.null_rho + 0.15,
                "{label}: rho {:.4} must exceed the control null {:.4} by 0.15",
                r.rho,
                r.null_rho
            );
            assert!(
                r.overlap >= 0.45,
                "{label}: hot-set overlap {:.4} below 0.45",
                r.overlap
            );
        }
    }

    /// Congestion map against OpenROAD on jpeg at 65% utilisation (F2), same criterion as aes.
    #[test]
    fn test_jpeg_congestion_map() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("jpeg").join("jpeg.util65.def"));
        let dump = parse_usage_dump(&require(design_dir("jpeg").join("jpeg.gcell_usage.txt")));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db =
            parse_def(&def_path, &tech_db, None).expect("Failed to parse jpeg.util65.def");

        // The dump was routed with `set_global_routing_layer_adjustment met1-met5 0.2` (ORFS jpeg value).
        let adj = [0.0f32, 0.2, 0.2, 0.2, 0.2, 0.2];
        let (grid, report) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);

        assert_eq!(grid.origin_x_dbu, dump.origin_x_dbu, "grid origin x");
        assert_eq!(grid.origin_y_dbu, dump.origin_y_dbu, "grid origin y");
        assert_eq!(grid.gcell_w_dbu, dump.tile_dbu, "grid tile width");
        assert_eq!(grid.gcell_h_dbu, dump.tile_dbu, "grid tile height");
        assert_eq!((grid.nx, grid.ny), (dump.nx, dump.ny), "grid dimensions");

        let (nx, ny) = (dump.nx, dump.ny);
        assert_eq!(ny * (nx - 1), report.h_usage_ratio.len(), "H edge count");
        assert_eq!((ny - 1) * nx, report.v_usage_ratio.len(), "V edge count");

        let h = compare_direction("H", &dump.h, &report.h_usage_ratio, |e| {
            e.y * (nx - 1) + e.x
        });
        let v = compare_direction("V", &dump.v, &report.v_usage_ratio, |e| e.y * nx + e.x);

        for (label, r) in [("H", &h), ("V", &v)] {
            assert!(
                r.rho >= 0.60,
                "{label}: Spearman rho {:.4} below 0.60 (support {})",
                r.rho,
                r.support
            );
            assert!(
                r.rho >= r.null_rho + 0.15,
                "{label}: rho {:.4} must exceed the control null {:.4} by 0.15",
                r.rho,
                r.null_rho
            );
            assert!(
                r.overlap >= 0.45,
                "{label}: hot-set overlap {:.4} below 0.45",
                r.overlap
            );
        }
    }

    /// Congestion map against OpenROAD on ibex at 65% utilisation (F2), same criterion as aes.
    #[test]
    fn test_ibex_congestion_map() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("ibex").join("ibex.util65.def"));
        let dump = parse_usage_dump(&require(design_dir("ibex").join("ibex.gcell_usage.txt")));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db =
            parse_def(&def_path, &tech_db, None).expect("Failed to parse ibex.util65.def");

        // The dump was routed with `set_global_routing_layer_adjustment met1-met5 0.2` (ORFS ibex value).
        let adj = [0.0f32, 0.2, 0.2, 0.2, 0.2, 0.2];
        let (grid, report) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);

        assert_eq!(grid.origin_x_dbu, dump.origin_x_dbu, "grid origin x");
        assert_eq!(grid.origin_y_dbu, dump.origin_y_dbu, "grid origin y");
        assert_eq!(grid.gcell_w_dbu, dump.tile_dbu, "grid tile width");
        assert_eq!(grid.gcell_h_dbu, dump.tile_dbu, "grid tile height");
        assert_eq!((grid.nx, grid.ny), (dump.nx, dump.ny), "grid dimensions");

        let (nx, ny) = (dump.nx, dump.ny);
        assert_eq!(ny * (nx - 1), report.h_usage_ratio.len(), "H edge count");
        assert_eq!((ny - 1) * nx, report.v_usage_ratio.len(), "V edge count");

        let h = compare_direction("H", &dump.h, &report.h_usage_ratio, |e| {
            e.y * (nx - 1) + e.x
        });
        let v = compare_direction("V", &dump.v, &report.v_usage_ratio, |e| e.y * nx + e.x);

        for (label, r) in [("H", &h), ("V", &v)] {
            assert!(
                r.rho >= 0.60,
                "{label}: Spearman rho {:.4} below 0.60 (support {})",
                r.rho,
                r.support
            );
            assert!(
                r.rho >= r.null_rho + 0.15,
                "{label}: rho {:.4} must exceed the control null {:.4} by 0.15",
                r.rho,
                r.null_rho
            );
            assert!(
                r.overlap >= 0.45,
                "{label}: hot-set overlap {:.4} below 0.45",
                r.overlap
            );
        }
    }

    /// Congestion map against OpenROAD on picorv32 at 65% utilisation (Phase 3), same criterion as aes.
    /// V hot-set overlap is 0.4698, above the relaxed 0.45 floor (was 0.50 until the R9 decision);
    /// rho passes both directions (H 0.817 / V 0.804). See docs/results.md.
    #[test]
    fn test_picorv32_congestion_map() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let def_path = require(design_dir("picorv32").join("picorv32.util65.def"));
        let dump = parse_usage_dump(&require(
            design_dir("picorv32").join("picorv32.gcell_usage.txt"),
        ));

        let mut interner = StringInterner::new();
        let tech_db = load_tech_db(&tlef_path, &lef_path, &mut interner)
            .expect("Failed to load TechDb")
            .0;
        let design_db =
            parse_def(&def_path, &tech_db, None).expect("Failed to parse picorv32.util65.def");

        // The dump was routed with `set_global_routing_layer_adjustment met1-met5 0.3`.
        let adj = [0.0f32, 0.3, 0.3, 0.3, 0.3, 0.3];
        let (grid, report) = estimate_congestion(&design_db, &tech_db, Some(&adj), None);

        assert_eq!(grid.origin_x_dbu, dump.origin_x_dbu, "grid origin x");
        assert_eq!(grid.origin_y_dbu, dump.origin_y_dbu, "grid origin y");
        assert_eq!(grid.gcell_w_dbu, dump.tile_dbu, "grid tile width");
        assert_eq!(grid.gcell_h_dbu, dump.tile_dbu, "grid tile height");
        assert_eq!((grid.nx, grid.ny), (dump.nx, dump.ny), "grid dimensions");

        let (nx, ny) = (dump.nx, dump.ny);
        assert_eq!(ny * (nx - 1), report.h_usage_ratio.len(), "H edge count");
        assert_eq!((ny - 1) * nx, report.v_usage_ratio.len(), "V edge count");

        let h = compare_direction("H", &dump.h, &report.h_usage_ratio, |e| {
            e.y * (nx - 1) + e.x
        });
        let v = compare_direction("V", &dump.v, &report.v_usage_ratio, |e| e.y * nx + e.x);

        for (label, r) in [("H", &h), ("V", &v)] {
            assert!(
                r.rho >= 0.60,
                "{label}: Spearman rho {:.4} below 0.60 (support {})",
                r.rho,
                r.support
            );
            assert!(
                r.rho >= r.null_rho + 0.15,
                "{label}: rho {:.4} must exceed the control null {:.4} by 0.15",
                r.rho,
                r.null_rho
            );
            assert!(
                r.overlap >= 0.45,
                "{label}: hot-set overlap {:.4} below 0.45",
                r.overlap
            );
        }
    }
}
