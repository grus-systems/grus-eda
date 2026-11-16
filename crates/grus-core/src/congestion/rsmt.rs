use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque};
use rayon::prelude::*;
use crate::congestion::gcell::{GCellGrid, GPoint, SteinerSegment};
use crate::design_db::DesignDb;
use crate::interner::NetId;
use crate::tech_db::TechDb;

#[derive(Default)]
struct ExactRsmtScratch {
    dist: Vec<u32>,
    d: Vec<u32>,
    d_split: Vec<u32>,
    split_choice: Vec<(u16, u16)>,
    from_u: Vec<u16>,
}

thread_local! {
    static EXACT_RSMT_SCRATCH: RefCell<ExactRsmtScratch> = RefCell::new(ExactRsmtScratch::default());
}

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

/// Largest net degree solved exactly by [`build_precise_rsmt_points`].
pub const EXACT_MAX_DEGREE: usize = 9;

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

    heuristic_rsmt_points(&unique)
}

/// Like [`build_rsmt_points`] but optimal for nets of up to [`EXACT_MAX_DEGREE`] distinct points,
/// which is what FLUTE guarantees and what OpenROAD's parasitic estimate uses (E3). E1 keeps the
/// greedy tree: its congestion ranking was verified against OpenROAD with that tree, and the optimal
/// one produces a tie in the utilisation sweep (see docs/results.md, E3.4).
pub fn build_precise_rsmt_points(pts: &[Pt]) -> Vec<(Pt, Pt)> {
    let mut unique = pts.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if (4..=EXACT_MAX_DEGREE).contains(&unique.len()) {
        return exact_rsmt_points(&unique);
    }
    build_rsmt_points(&unique)
}

pub(crate) fn exact_rsmt_points(unique: &[Pt]) -> Vec<(Pt, Pt)> {
    let k = unique.len();
    let mut xs: Vec<i32> = unique.iter().map(|p| p.0).collect();
    let mut ys: Vec<i32> = unique.iter().map(|p| p.1).collect();
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();

    if xs.len() == 1 {
        let mut sorted = unique.to_vec();
        sorted.sort_by_key(|p| p.1);
        return sorted.windows(2).map(|w| (w[0], w[1])).collect();
    }
    if ys.len() == 1 {
        let mut sorted = unique.to_vec();
        sorted.sort_by_key(|p| p.0);
        return sorted.windows(2).map(|w| (w[0], w[1])).collect();
    }

    let nx = xs.len();
    let ny = ys.len();
    let nv = nx * ny;

    let mut v_coords = Vec::with_capacity(nv);
    for &x in &xs {
        for &y in &ys {
            v_coords.push((x, y));
        }
    }

    let mut term_v = Vec::with_capacity(k);
    for p in unique {
        let idx = v_coords.iter().position(|v| v == p).unwrap();
        term_v.push(idx);
    }

    let num_subsets = 1 << k;
    let inf = u32::MAX / 2;

    let edges = EXACT_RSMT_SCRATCH.with(|cell| {
        let mut sc = cell.borrow_mut();
        let dist_len = nv * nv;
        if sc.dist.len() < dist_len {
            sc.dist.resize(dist_len, 0);
        }
        let table_len = num_subsets * nv;
        if sc.d.len() < table_len {
            sc.d.resize(table_len, inf);
            sc.d_split.resize(table_len, inf);
            sc.split_choice.resize(table_len, (0, 0));
            sc.from_u.resize(table_len, 0);
        }

        let ExactRsmtScratch { dist, d, d_split, split_choice, from_u } = &mut *sc;
        let dist = &mut dist[..dist_len];
        let d = &mut d[..table_len];
        let d_split = &mut d_split[..table_len];
        let split_choice = &mut split_choice[..table_len];
        let from_u = &mut from_u[..table_len];

        for i in 0..nv {
            let vi = v_coords[i];
            let row = &mut dist[i * nv..(i + 1) * nv];
            for j in 0..nv {
                row[j] = vi.0.abs_diff(v_coords[j].0) + vi.1.abs_diff(v_coords[j].1);
            }
        }

        d.fill(inf);
        d_split.fill(inf);

        for (i, &tv) in term_v.iter().enumerate() {
            let base = (1 << i) * nv;
            let dist_row = &dist[tv * nv..(tv + 1) * nv];
            d[base..base + nv].copy_from_slice(dist_row);
            d_split[base..base + nv].copy_from_slice(dist_row);
        }

        for mask in 1..num_subsets {
            if (mask & (mask - 1)) == 0 {
                continue;
            }
            let base = mask * nv;

            let mut sub = (mask - 1) & mask;
            while sub > (mask ^ sub) {
                let s1 = sub;
                let s2 = mask ^ sub;
                let row1 = &d[s1 * nv..(s1 + 1) * nv];
                let row2 = &d[s2 * nv..(s2 + 1) * nv];
                for v in 0..nv {
                    let val = row1[v] + row2[v];
                    if val < d_split[base + v] {
                        d_split[base + v] = val;
                        split_choice[base + v] = (s1 as u16, s2 as u16);
                    }
                }
                sub = (sub - 1) & mask;
            }

            let d_split_row = &d_split[base..base + nv];
            for v in 0..nv {
                let mut best_val = d_split_row[v];
                let mut best_u = v;
                // dist is symmetric (dist[v][u] == dist[u][v]), allowing stride-1 reads across u.
                let dist_v = &dist[v * nv..(v + 1) * nv];
                for u in 0..nv {
                    let val = d_split_row[u] + dist_v[u];
                    if val < best_val {
                        best_val = val;
                        best_u = u;
                    }
                }
                d[base + v] = best_val;
                from_u[base + v] = best_u as u16;
            }
        }

        let mut edges: BTreeSet<(Pt, Pt)> = BTreeSet::new();

        let mut add_path = |p1: Pt, p2: Pt| {
            let (x1, y1) = p1;
            let (x2, y2) = p2;
            let ix1 = xs.binary_search(&x1).unwrap();
            let iy1 = ys.binary_search(&y1).unwrap();
            let ix2 = xs.binary_search(&x2).unwrap();
            let iy2 = ys.binary_search(&y2).unwrap();

            if ix1 != ix2 {
                let (start, end) = (ix1.min(ix2), ix1.max(ix2));
                for ix in start..end {
                    let a = (xs[ix], ys[iy1]);
                    let b = (xs[ix + 1], ys[iy1]);
                    edges.insert((a.min(b), a.max(b)));
                }
            }
            if iy1 != iy2 {
                let (start, end) = (iy1.min(iy2), iy1.max(iy2));
                for iy in start..end {
                    let a = (xs[ix2], ys[iy]);
                    let b = (xs[ix2], ys[iy + 1]);
                    edges.insert((a.min(b), a.max(b)));
                }
            }
        };

        #[allow(clippy::too_many_arguments)]
        fn trace_dp(
            mask: usize,
            v: usize,
            nv: usize,
            pts: &[Pt],
            v_coords: &[Pt],
            split_choice: &[(u16, u16)],
            from_u: &[u16],
            add_path: &mut impl FnMut(Pt, Pt),
        ) {
            if (mask & (mask - 1)) == 0 {
                let i = mask.trailing_zeros() as usize;
                if pts[i] != v_coords[v] {
                    add_path(v_coords[v], pts[i]);
                }
                return;
            }
            let u = from_u[mask * nv + v] as usize;
            if u != v {
                add_path(v_coords[v], v_coords[u]);
            }
            let (s1, s2) = split_choice[mask * nv + u];
            trace_dp(s1 as usize, u, nv, pts, v_coords, split_choice, from_u, add_path);
            trace_dp(s2 as usize, u, nv, pts, v_coords, split_choice, from_u, add_path);
        }

        let all = num_subsets - 1;
        let all_base = all * nv;
        let mut best_root = 0;
        let mut min_total = inf;
        for (v, &val) in d[all_base..all_base + nv].iter().enumerate() {
            if val < min_total {
                min_total = val;
                best_root = v;
            }
        }

        trace_dp(all, best_root, nv, unique, &v_coords, split_choice, from_u, &mut add_path);
        edges
    });

    // Build BFS spanning tree to guarantee zero cycles and exact minimum length
    let mut adj: BTreeMap<Pt, Vec<Pt>> = BTreeMap::new();
    for &(a, b) in &edges {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }

    let root = unique[0];
    let mut visited: BTreeSet<Pt> = BTreeSet::new();
    let mut queue = VecDeque::new();
    visited.insert(root);
    queue.push_back(root);

    let mut tree_edges: Vec<(Pt, Pt)> = Vec::new();
    while let Some(u) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&u) {
            for &v in neighbors {
                if visited.insert(v) {
                    tree_edges.push((u.min(v), u.max(v)));
                    queue.push_back(v);
                }
            }
        }
    }

    // Merge collinear degree-2 non-terminal nodes
    let mut tree_adj: BTreeMap<Pt, Vec<Pt>> = BTreeMap::new();
    for &(a, b) in &tree_edges {
        tree_adj.entry(a).or_default().push(b);
        tree_adj.entry(b).or_default().push(a);
    }

    let term_set: BTreeSet<Pt> = unique.iter().copied().collect();
    let mut changed = true;
    while changed {
        changed = false;
        let nodes: Vec<Pt> = tree_adj.keys().copied().collect();
        for node in nodes {
            if term_set.contains(&node) {
                continue;
            }
            let Some(&[n1, n2]) = tree_adj.get(&node).map(Vec::as_slice) else {
                continue;
            };
            if (n1.0 == node.0 && node.0 == n2.0) || (n1.1 == node.1 && node.1 == n2.1) {
                tree_adj.remove(&node);
                if let Some(v1) = tree_adj.get_mut(&n1) {
                    v1.retain(|&x| x != node);
                    v1.push(n2);
                }
                if let Some(v2) = tree_adj.get_mut(&n2) {
                    v2.retain(|&x| x != node);
                    v2.push(n1);
                }
                changed = true;
                break;
            }
        }
    }

    let mut result = Vec::new();
    let mut seen: BTreeSet<(Pt, Pt)> = BTreeSet::new();
    for (&u, neighbors) in &tree_adj {
        for &v in neighbors {
            let edge = (u.min(v), u.max(v));
            if seen.insert(edge) {
                result.push(edge);
            }
        }
    }
    result
}

pub(crate) fn heuristic_rsmt_points(unique: &[Pt]) -> Vec<(Pt, Pt)> {
    let k = unique.len();
    let mut nodes = unique.to_vec();
    let mut visited = vec![false; k];
    let mut min_dist = vec![u32::MAX; k];
    let mut best_dy = vec![u32::MAX; k];
    let mut parent = vec![0usize; k];

    visited[0] = true;
    for j in 1..k {
        min_dist[j] = manhattan(nodes[0], nodes[j]);
        best_dy[j] = (nodes[0].1 - nodes[j].1).unsigned_abs();
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
                let dy = (nodes[v].1 - nodes[j].1).unsigned_abs();
                // Prefer same-row (smaller |dy|) connections on equal-distance Prim ties to match horizontal std-cell topology.
                if d < min_dist[j]
                    || (d == min_dist[j] && (dy < best_dy[j] || (dy == best_dy[j] && v < parent[j])))
                {
                    min_dist[j] = d;
                    best_dy[j] = dy;
                    parent[j] = v;
                }
            }
        }
    }

    // Iterative L-flip Steiner refinement.
    // For tiny trees (<= 32 edges), a direct O(E^2) loop is fastest; for larger trees,
    // track per-vertex best incident pair in a max-heap and only refresh the <= 4 touched
    // vertices per flip, using monotonic edge sequence numbers to preserve exact tie-breaking.
    if edges.len() <= 32 {
        heuristic_lflip_small(&mut nodes, &mut edges);
    } else {
        heuristic_lflip_heap(&mut nodes, &mut edges);
    }

    let mut segs = Vec::with_capacity(edges.len());
    for (a, b) in edges {
        if nodes[a] != nodes[b] {
            segs.push((nodes[a], nodes[b]));
        }
    }
    segs
}

fn heuristic_lflip_small(nodes: &mut Vec<Pt>, edges: &mut Vec<(usize, usize)>) {
    loop {
        let mut best_candidate: Option<(u32, usize, usize, usize, usize, usize, Pt)> = None;
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
                                gain > best_gain || (gain == best_gain && (e1, e2) < (best_e1, best_e2))
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
}

fn heuristic_lflip_heap(nodes: &mut Vec<Pt>, edges: &mut Vec<(usize, usize)>) {
    let mut node_map: HashMap<Pt, usize> = HashMap::with_capacity(nodes.len() * 2);
    for (idx, &p) in nodes.iter().enumerate() {
        node_map.entry(p).or_insert(idx);
    }

    let mut edge_endpoints: Vec<(usize, usize)> = Vec::with_capacity(edges.len() * 2);
    let mut edge_alive: Vec<bool> = Vec::with_capacity(edges.len() * 2);
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];

    for &(a, b) in edges.iter() {
        let eid = edge_endpoints.len();
        edge_endpoints.push((a, b));
        edge_alive.push(true);
        adj[a].push(eid);
        adj[b].push(eid);
    }

    let mut vertex_ver: Vec<u32> = vec![0; nodes.len()];
    // (gain, Reverse(eid1), Reverse(eid2), ver, u, v, w, s_pt)
    #[derive(Eq, PartialEq)]
    struct HeapCand {
        gain: u32,
        seq1: Reverse<usize>,
        seq2: Reverse<usize>,
        ver: u32,
        u: usize,
        v: usize,
        w: usize,
        s_pt: Pt,
    }
    impl Ord for HeapCand {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            (self.gain, self.seq1, self.seq2)
                .cmp(&(other.gain, other.seq1, other.seq2))
        }
    }
    impl PartialOrd for HeapCand {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let push_vertex_best = |u: usize,
                            nodes: &[Pt],
                            edge_endpoints: &[(usize, usize)],
                            adj: &[Vec<usize>],
                            vertex_ver: &[u32],
                            heap: &mut BinaryHeap<HeapCand>| {
        let inc = &adj[u];
        if inc.len() < 2 {
            return;
        }
        let pu = nodes[u];
        let mut best: Option<(u32, usize, usize, usize, usize, Pt)> = None;
        for i in 0..inc.len() {
            let e_a = inc[i];
            let (a1, b1) = edge_endpoints[e_a];
            let other_a = if a1 == u { b1 } else { a1 };
            let pa = nodes[other_a];
            for &e_b in &inc[(i + 1)..] {
                let (a2, b2) = edge_endpoints[e_b];
                let other_b = if a2 == u { b2 } else { a2 };
                let pb = nodes[other_b];
                let s_pt = (median3(pu.0, pa.0, pb.0), median3(pu.1, pa.1, pb.1));
                let gain = manhattan(pu, s_pt);
                if gain == 0 {
                    continue;
                }
                let (seq1, v, seq2, w) = if e_a < e_b {
                    (e_a, other_a, e_b, other_b)
                } else {
                    (e_b, other_b, e_a, other_a)
                };
                let is_better = match best {
                    None => true,
                    Some((bg, bs1, bs2, ..)) => {
                        gain > bg || (gain == bg && (seq1, seq2) < (bs1, bs2))
                    }
                };
                if is_better {
                    best = Some((gain, seq1, seq2, v, w, s_pt));
                }
            }
        }
        if let Some((gain, seq1, seq2, v, w, s_pt)) = best {
            heap.push(HeapCand {
                gain,
                seq1: Reverse(seq1),
                seq2: Reverse(seq2),
                ver: vertex_ver[u],
                u,
                v,
                w,
                s_pt,
            });
        }
    };

    let mut heap = BinaryHeap::with_capacity(nodes.len());
    for u in 0..nodes.len() {
        push_vertex_best(u, nodes, &edge_endpoints, &adj, &vertex_ver, &mut heap);
    }

    while let Some(top) = heap.pop() {
        let eid1 = top.seq1.0;
        let eid2 = top.seq2.0;
        if top.ver != vertex_ver[top.u] || !edge_alive[eid1] || !edge_alive[eid2] {
            continue;
        }
        let (u, v, w, s_pt) = (top.u, top.v, top.w, top.s_pt);
        let s_idx = match node_map.get(&s_pt) {
            Some(&idx) => idx,
            None => {
                let idx = nodes.len();
                nodes.push(s_pt);
                node_map.insert(s_pt, idx);
                adj.push(Vec::new());
                vertex_ver.push(0);
                idx
            }
        };

        edge_alive[eid1] = false;
        edge_alive[eid2] = false;
        adj[u].retain(|&eid| eid != eid1 && eid != eid2);
        adj[v].retain(|&eid| eid != eid1);
        adj[w].retain(|&eid| eid != eid2);

        let candidates = [
            (u.min(s_idx), u.max(s_idx)),
            (v.min(s_idx), v.max(s_idx)),
            (w.min(s_idx), w.max(s_idx)),
        ];
        for &(n1, n2) in &candidates {
            if n1 != n2
                && nodes[n1] != nodes[n2]
                && !adj[n1].iter().any(|&eid| edge_endpoints[eid] == (n1, n2))
            {
                let eid = edge_endpoints.len();
                edge_endpoints.push((n1, n2));
                edge_alive.push(true);
                adj[n1].push(eid);
                adj[n2].push(eid);
            }
        }

        let dirty = [u, v, w, s_idx];
        for (i, &z) in dirty.iter().enumerate() {
            if dirty[..i].contains(&z) {
                continue;
            }
            vertex_ver[z] += 1;
            push_vertex_best(z, nodes, &edge_endpoints, &adj, &vertex_ver, &mut heap);
        }
    }

    edges.clear();
    for (eid, &ep) in edge_endpoints.iter().enumerate() {
        if edge_alive[eid] {
            edges.push(ep);
        }
    }
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
    let n = design_db.net_names.len();
    if n < 512 {
        let mut all_segs = Vec::new();
        for net_idx in 0..n {
            all_segs.extend(decompose_net(design_db, tech_db, grid, NetId(net_idx as u32)));
        }
        return all_segs;
    }
    let per_net: Vec<Vec<SteinerSegment>> = (0..n)
        .into_par_iter()
        .map(|net_idx| decompose_net(design_db, tech_db, grid, NetId(net_idx as u32)))
        .collect();
    let total: usize = per_net.iter().map(Vec::len).sum();
    let mut all_segs = Vec::with_capacity(total);
    for segs in per_net {
        all_segs.extend(segs);
    }
    all_segs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{design_dir, sky130_hd};
    use crate::interner::StringInterner;
    use crate::readers::def::parse_def;
    use crate::readers::lef::load_tech_db;
    use std::collections::{HashMap, HashSet, VecDeque};

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

        let ref_path = design_dir("gcd").join("gcd.reference.json");
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
        assert_eq!(total_rsmt_wl, 788, "Total RSMT wirelength on gcd must be 788");
        assert!(total_rsmt_wl > 0);
        // Compare with OpenROAD's global routing edge demand
        let diff = (total_rsmt_wl as f64 - ref_total_demand as f64).abs();
        let pct_diff = (diff / ref_total_demand as f64) * 100.0;
        println!("RSMT WL vs OpenROAD GRT Demand diff: {:.2}%", pct_diff);
    }

    fn total_length(segs: &[(Pt, Pt)]) -> u32 {
        segs.iter().map(|&(a, b)| manhattan(a, b)).sum()
    }

    /// Deterministic pseudo-random points (no external crate).
    fn points(seed: u64, k: usize) -> Vec<Pt> {
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) % 60) as i32
        };
        (0..k).map(|_| (next(), next())).collect()
    }

    #[test]
    fn test_precise_rsmt_is_connected_never_longer_than_greedy_and_deterministic() {
        for seed in 0..40u64 {
            let k = 4 + (seed % 6) as usize; // 4..=9
            let pts = points(seed, k);
            let mut unique = pts.clone();
            unique.sort_unstable();
            unique.dedup();
            let precise = build_precise_rsmt_points(&pts);
            let greedy = build_rsmt_points(&pts);
            assert!(total_length(&precise) <= total_length(&greedy), "seed {seed}");
            let (min_x, max_x) = (unique.iter().map(|p| p.0).min().unwrap(), unique.iter().map(|p| p.0).max().unwrap());
            let (min_y, max_y) = (unique.iter().map(|p| p.1).min().unwrap(), unique.iter().map(|p| p.1).max().unwrap());
            assert!(total_length(&precise) >= ((max_x - min_x) + (max_y - min_y)) as u32, "below HPWL, seed {seed}");
            // Every terminal is reachable from the first one.
            let mut reached = std::collections::HashSet::from([unique[0]]);
            let mut grew = true;
            while grew {
                grew = false;
                for &(a, b) in &precise {
                    if reached.contains(&a) != reached.contains(&b) {
                        reached.insert(a);
                        reached.insert(b);
                        grew = true;
                    }
                }
            }
            assert!(unique.iter().all(|p| reached.contains(p)), "disconnected, seed {seed}");
            assert_eq!(precise, build_precise_rsmt_points(&pts), "not deterministic, seed {seed}");
        }
    }

    #[test]
    fn test_precise_rsmt_finds_the_plus_shape() {
        // Four points around a centre: the optimal tree is a plus of length 4 * 10.
        let pts = [(10, 0), (0, 10), (20, 10), (10, 20)];
        assert_eq!(total_length(&build_precise_rsmt_points(&pts)), 40);
        assert!(total_length(&build_rsmt_points(&pts)) >= 40);
    }

    #[test]
    fn test_heap_lflip_matches_quadratic_exactly() {
        for seed in 0..8u64 {
            let pts = points(seed + 100, 50);
            let mut unique = pts;
            unique.sort_unstable();
            unique.dedup();
            // Build the same initial MST edge list from `heuristic_rsmt_points` by copying Prim setup,
            // then compare `heuristic_lflip_small` vs `heuristic_lflip_heap` directly.
            let k = unique.len();
            let mut visited = vec![false; k];
            let mut min_dist = vec![u32::MAX; k];
            let mut best_dy = vec![u32::MAX; k];
            let mut parent = vec![0usize; k];
            visited[0] = true;
            for j in 1..k {
                min_dist[j] = manhattan(unique[0], unique[j]);
                best_dy[j] = (unique[0].1 - unique[j].1).unsigned_abs();
            }
            let mut mst_edges = Vec::with_capacity(k);
            for _ in 1..k {
                let mut best_v = 0;
                let mut best_d = u32::MAX;
                for v in 0..k {
                    if !visited[v] && min_dist[v] < best_d {
                        best_d = min_dist[v];
                        best_v = v;
                    }
                }
                visited[best_v] = true;
                let u = parent[best_v];
                mst_edges.push((u.min(best_v), u.max(best_v)));
                for j in 0..k {
                    if !visited[j] {
                        let d = manhattan(unique[best_v], unique[j]);
                        let dy = (unique[best_v].1 - unique[j].1).unsigned_abs();
                        if d < min_dist[j]
                            || (d == min_dist[j] && (dy < best_dy[j] || (dy == best_dy[j] && best_v < parent[j])))
                        {
                            min_dist[j] = d;
                            best_dy[j] = dy;
                            parent[j] = best_v;
                        }
                    }
                }
            }
            let (mut n1, mut e1) = (unique.clone(), mst_edges.clone());
            let (mut n2, mut e2) = (unique, mst_edges);
            heuristic_lflip_small(&mut n1, &mut e1);
            heuristic_lflip_heap(&mut n2, &mut e2);
            let segs1: Vec<_> = e1.into_iter().filter(|&(a, b)| n1[a] != n1[b]).map(|(a, b)| (n1[a], n1[b])).collect();
            let segs2: Vec<_> = e2.into_iter().filter(|&(a, b)| n2[a] != n2[b]).map(|(a, b)| (n2[a], n2[b])).collect();
            assert_eq!(segs1, segs2, "seed {seed} mismatch between heap and quadratic L-flip");
        }
    }
}
