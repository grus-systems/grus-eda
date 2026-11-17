//! Placement-estimate RC tree builder matching OpenROAD's `pdr::primDijkstra`
//! (stt module, `alpha = 0.3`) used by `estimate_parasitics -placement`.
//!
//! Semantics ported from OpenROAD @ dcf36133 (BSD-3-Clause); this is our own
//! implementation. The pinned LibreLane image builds the Steiner tree with
//! Prim-Dijkstra first and falls back to FLUTE only when `checkTree` rejects
//! the PD tree (non-adjacent edges with intersecting bounding boxes). FLUTE
//! itself is not implemented: `build_pd_tree_for_points` reports whether the
//! PD tree passed the check, and `estimate_placement_parasitics` counts the
//! failures into `NetParasitics::pd_check_failed_nets` so callers can see
//! where our tree would differ from OpenROAD's FLUTE fallback.

use super::rsmt::Pt;

const ALPHA: f32 = 0.3;
const INVALID: usize = usize::MAX;

/// Manograph point distance in DBU.
fn manhattan(a: Pt, b: Pt) -> i32 {
    a.0.abs_diff(b.0) as i32 + a.1.abs_diff(b.1) as i32
}

/// Undirected graph with lemon `ListGraph` semantics: each node keeps its
/// incident edge ids most-recent-first, `add_edge` prepends to both endpoint
/// lists, and `change_u`/`change_v` move the touched arc to the new node's head.
struct PdGraph {
    pts: Vec<Pt>,
    /// `(u, v)` endpoints as stored by lemon (`u` is `arcs[2e].target`).
    edges: Vec<(usize, usize)>,
    adj: Vec<Vec<usize>>,
}

impl PdGraph {
    fn with_terminals(pts: &[Pt]) -> Self {
        Self {
            pts: pts.to_vec(),
            edges: Vec::new(),
            adj: vec![Vec::new(); pts.len()],
        }
    }

    fn add_node(&mut self, p: Pt) -> usize {
        self.pts.push(p);
        self.adj.push(Vec::new());
        self.pts.len() - 1
    }

    fn add_edge(&mut self, u: usize, v: usize) -> usize {
        let id = self.edges.len();
        self.edges.push((u, v));
        self.adj[u].insert(0, id);
        self.adj[v].insert(0, id);
        id
    }

    /// `oppositeNode`/`runningNode`: the endpoint that is not `node`
    /// (`u` when `node` is not incident, matching lemon's unchecked lookup).
    fn other(&self, e: usize, node: usize) -> usize {
        let (u, v) = self.edges[e];
        if u == node { v } else { u }
    }

    fn change_u(&mut self, e: usize, n: usize) {
        let u = self.edges[e].0;
        if let Some(pos) = self.adj[u].iter().position(|&x| x == e) {
            self.adj[u].remove(pos);
        }
        self.edges[e].0 = n;
        self.adj[n].insert(0, e);
    }

    fn change_v(&mut self, e: usize, n: usize) {
        let v = self.edges[e].1;
        if let Some(pos) = self.adj[v].iter().position(|&x| x == e) {
            self.adj[v].remove(pos);
        }
        self.edges[e].1 = n;
        self.adj[n].insert(0, e);
    }
}

/// Indexed binary heap reproducing `boost::heap::d_ary_heap<arity<2>>` pop
/// order: `less(a, b)` is true when `a` has lower priority than `b`, so the
/// top is the maximum-priority element; equal elements keep heap order.
struct Heap<T> {
    data: Vec<T>,
    /// node id -> position in `data`; `None` while not in the heap.
    pos: Vec<Option<usize>>,
    key: fn(&T) -> usize,
    less: fn(&T, &T) -> bool,
}

impl<T: Clone> Heap<T> {
    fn new(key: fn(&T) -> usize, less: fn(&T, &T) -> bool) -> Self {
        Self { data: Vec::new(), pos: Vec::new(), key, less }
    }

    fn ensure_pos(&mut self, node: usize) {
        if self.pos.len() <= node {
            self.pos.resize(node + 1, None);
        }
    }

    fn pos_of(&self, node: usize) -> Option<usize> {
        self.pos.get(node).copied().flatten()
    }

    fn fix_pos(&mut self, i: usize) {
        let node = (self.key)(&self.data[i]);
        self.ensure_pos(node);
        self.pos[node] = Some(i);
    }

    fn sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if (self.less)(&self.data[p], &self.data[i]) {
                self.data.swap(p, i);
                self.fix_pos(p);
                self.fix_pos(i);
                i = p;
            } else {
                break;
            }
        }
    }

    fn sift_down(&mut self, mut i: usize) {
        loop {
            let l = 2 * i + 1;
            if l >= self.data.len() {
                break;
            }
            let r = l + 1;
            let mut best = l;
            if r < self.data.len() && (self.less)(&self.data[best], &self.data[r]) {
                best = r;
            }
            if (self.less)(&self.data[i], &self.data[best]) {
                self.data.swap(i, best);
                self.fix_pos(i);
                self.fix_pos(best);
                i = best;
            } else {
                break;
            }
        }
    }

    fn push(&mut self, item: T) -> usize {
        let node = (self.key)(&item);
        self.ensure_pos(node);
        let i = self.data.len();
        self.data.push(item);
        self.fix_pos(i);
        self.sift_up(i);
        self.pos_of(node).expect("just pushed")
    }

    fn pop(&mut self) -> Option<T> {
        if self.data.is_empty() {
            return None;
        }
        let top = self.data[0].clone();
        let last = self.data.pop().expect("non-empty");
        if !self.data.is_empty() {
            self.data[0] = last;
            self.fix_pos(0);
            self.sift_down(0);
        }
        let node = (self.key)(&top);
        if let Some(p) = self.pos.get_mut(node) {
            *p = None;
        }
        Some(top)
    }

    /// `heap.update(handle)`: element already replaced at its position;
    /// sift up when its priority grew, otherwise down (no-op on ties).
    fn update_at(&mut self, i: usize) {
        if i > 0 {
            let p = (i - 1) / 2;
            if (self.less)(&self.data[p], &self.data[i]) {
                self.sift_up(i);
                return;
            }
        }
        self.sift_down(i);
    }

    /// Replace the element at `i` and sift toward the top only. This mirrors
    /// `heap.increase(handle)`, which the spanning-tree search uses after
    /// lowering a key: boost does not check whether the element actually
    /// belongs lower, so a worse-ordered key can stay near the top.
    fn assign_increase_at(&mut self, i: usize, item: T) {
        self.data[i] = item;
        self.sift_up(i);
    }

    fn replace_at(&mut self, i: usize, item: T) {
        self.data[i] = item;
        self.fix_pos(i);
        self.update_at(i);
    }
}

#[derive(Clone)]
struct SearchEdge {
    weight: f32,
    path_length: i32,
    parent: i32,
    node: usize,
}

fn search_edge_key(e: &SearchEdge) -> usize {
    e.node
}

/// `CmpEdge`: lower priority for the *greater* `(weight, parent, node)` tuple,
/// so the heap top is the smallest tuple (min-heap). `INVALID = -1` parents.
fn search_edge_less(a: &SearchEdge, b: &SearchEdge) -> bool {
    (a.weight, a.parent, a.node as i32) > (b.weight, b.parent, b.node as i32)
}

/// `get_nearest_neighbors` from pd.cpp: `vi` neighbors `vj` when the smallest
/// bounding box containing them holds no other points. O(n^2) y-sweep with
/// per-point quadrant bounds; neighbor lists stay in discovery order.
fn nearest_neighbors(pts: &[Pt]) -> Vec<Vec<usize>> {
    let n = pts.len();
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut sorted: Vec<usize> = (0..n).collect();
    sorted.sort_by_key(|&i| (pts[i].1, pts[i].0));

    let mut ur = vec![i32::MAX; n];
    let mut lr = vec![i32::MAX; n];
    let mut ul = vec![i32::MIN; n];
    let mut ll = vec![i32::MIN; n];

    for idx in 0..n {
        let pt_idx = sorted[idx];
        let pt_x = pts[pt_idx].0;
        for &below_idx in &sorted[..idx] {
            let below_x = pts[below_idx].0;
            if below_x <= pt_x && pt_x < ur[below_idx] {
                neighbors[below_idx].push(pt_idx);
                ur[below_idx] = pt_x;
            } else if ul[below_idx] < pt_x && pt_x < below_x {
                neighbors[below_idx].push(pt_idx);
                ul[below_idx] = pt_x;
            }
        }
        for &below_idx in sorted[..idx].iter().rev() {
            let below_x = pts[below_idx].0;
            if pt_x <= below_x && below_x < lr[pt_idx] {
                neighbors[pt_idx].push(below_idx);
                lr[pt_idx] = below_x;
            } else if ll[pt_idx] < below_x && below_x < pt_x {
                neighbors[pt_idx].push(below_idx);
                ll[pt_idx] = below_x;
            }
        }
    }
    neighbors
}

/// `buildSpanningTree`: Prim-style growth over the neighbor graph where the
/// heap key is `edge_length + alpha * path_length_from_driver`.
fn build_spanning_tree(g: &mut PdGraph, driver: usize, nn: &[Vec<usize>]) {
    let n = g.pts.len();
    let mut heap = Heap::new(search_edge_key, search_edge_less);
    heap.push(SearchEdge { weight: 0.0, path_length: 0, parent: -1, node: driver });
    let mut visited = vec![false; n];
    let mut visited_count = 0;

    while let Some(edge) = heap.pop() {
        visited[edge.node] = true;
        visited_count += 1;
        if edge.parent >= 0 {
            g.add_edge(edge.parent as usize, edge.node);
        }
        if visited_count == n {
            break;
        }
        for &nb in &nn[edge.node] {
            if visited[nb] {
                continue;
            }
            let edge_length = manhattan(g.pts[nb], g.pts[edge.node]);
            let neighbor_path_length = edge_length + edge.path_length;
            let neighbor_weight = edge_length as f32 + ALPHA * edge.path_length as f32;
            match heap.pos_of(nb) {
                Some(pos) => {
                    if neighbor_weight <= heap.data[pos].weight {
                        heap.assign_increase_at(
                            pos,
                            SearchEdge {
                                weight: neighbor_weight,
                                path_length: neighbor_path_length,
                                parent: edge.node as i32,
                                node: nb,
                            },
                        );
                    }
                }
                None => {
                    heap.push(SearchEdge {
                        weight: neighbor_weight,
                        path_length: neighbor_path_length,
                        parent: edge.node as i32,
                        node: nb,
                    });
                }
            }
        }
    }
}

#[derive(Clone)]
struct CandidateSteiner {
    gain: i32,
    node: usize,
    edge1: usize,
    edge2: usize,
    pt: Pt,
}

fn candidate_key(c: &CandidateSteiner) -> usize {
    c.node
}

/// `CmpCandidate`: lower priority for the smaller `(gain, x, y)` tuple, so the
/// heap top is the largest tuple.
fn candidate_less(a: &CandidateSteiner, b: &CandidateSteiner) -> bool {
    (a.gain, a.pt.0, a.pt.1) < (b.gain, b.pt.0, b.pt.1)
}

/// `best_steiner_for_node`: best pair of incident edges improvable by moving
/// `node`'s point to the corner toward both neighbors. First strict maximum
/// wins; edge pairs iterate the node's incident list most-recent-first.
fn best_steiner_for_node(g: &PdGraph, node: usize) -> CandidateSteiner {
    let pt_node = g.pts[node];
    let mut best = CandidateSteiner {
        gain: 0,
        node,
        edge1: INVALID,
        edge2: INVALID,
        pt: pt_node,
    };
    let adj = &g.adj[node];
    for (i1, &e1) in adj.iter().enumerate() {
        let n1 = g.other(e1, node);
        let pt1 = g.pts[n1];
        for &e2 in &adj[i1 + 1..] {
            let n2 = g.other(e2, node);
            let pt2 = g.pts[n2];
            let mut sx = pt_node.0;
            let mut sy = pt_node.1;
            if pt1.0.min(pt2.0) > pt_node.0 {
                sx = pt1.0.min(pt2.0);
            } else if pt1.0.max(pt2.0) < pt_node.0 {
                sx = pt1.0.max(pt2.0);
            }
            if pt1.1.min(pt2.1) > pt_node.1 {
                sy = pt1.1.min(pt2.1);
            } else if pt1.1.max(pt2.1) < pt_node.1 {
                sy = pt1.1.max(pt2.1);
            }
            let gain = (sx - pt_node.0).abs() + (sy - pt_node.1).abs();
            if gain > best.gain {
                best = CandidateSteiner { gain, node, edge1: e1, edge2: e2, pt: (sx, sy) };
            }
        }
    }
    best
}

/// `steinerize`: repeatedly apply the maximum-gain Steiner insertion until no
/// candidate improves. Candidate entries are refreshed only for the touched
/// nodes, exactly like the reference.
fn steinerize(g: &mut PdGraph) {
    let mut heap = Heap::new(candidate_key, candidate_less);
    // NodeIt order is newest-first, i.e. descending node id.
    for node in (0..g.pts.len()).rev() {
        let cand = best_steiner_for_node(g, node);
        heap.push(cand);
    }

    while let Some(best) = heap.pop() {
        if best.gain == 0 {
            break;
        }
        let opp1 = g.other(best.edge1, best.node);
        let opp2 = g.other(best.edge2, best.node);

        let mut new_node = false;
        let steiner_node = if best.pt == g.pts[opp1] {
            opp1
        } else if best.pt == g.pts[opp2] {
            opp2
        } else {
            new_node = true;
            g.add_node(best.pt)
        };

        if steiner_node != opp1 {
            if g.edges[best.edge1].0 == best.node {
                g.change_u(best.edge1, steiner_node);
            } else {
                g.change_v(best.edge1, steiner_node);
            }
        }
        if steiner_node != opp2 {
            if g.edges[best.edge2].0 == best.node {
                g.change_u(best.edge2, steiner_node);
            } else {
                g.change_v(best.edge2, steiner_node);
            }
        }
        if new_node {
            g.add_edge(steiner_node, best.node);
            let cand = best_steiner_for_node(g, steiner_node);
            heap.push(cand);
        }

        // best.node's entry was popped: push a fresh candidate.
        let cand = best_steiner_for_node(g, best.node);
        heap.push(cand);
        for &node in &[opp1, opp2] {
            if let Some(pos) = heap.pos_of(node) {
                let cand = best_steiner_for_node(g, node);
                heap.replace_at(pos, cand);
            }
        }
    }
}

/// `splitDegree4Nodes`: nodes above degree three are split into degree-3
/// chains linked by zero-length edges, breadth-first in node-id-descending
/// seed order.
fn split_degree4_nodes(g: &mut PdGraph) {
    let mut to_process: std::collections::VecDeque<usize> =
        (0..g.pts.len()).rev().collect();
    while let Some(node) = to_process.pop_front() {
        if g.adj[node].len() <= 3 {
            continue;
        }
        let new_node = g.add_node(g.pts[node]);
        // The reference iterates the live incident list; removals never
        // disturb the not-yet-visited suffix, so a snapshot is equivalent.
        let seq = g.adj[node].clone();
        for (i, e) in seq.into_iter().enumerate() {
            if i >= 2 {
                if g.edges[e].0 == node {
                    g.change_u(e, new_node);
                } else {
                    g.change_v(e, new_node);
                }
            }
        }
        g.add_edge(node, new_node);
        to_process.push_back(new_node);
    }
}

/// `makeTreeRecursive`: parent of every node toward the driver.
fn make_tree_parents(g: &PdGraph, driver: usize) -> Vec<usize> {
    fn rec(g: &PdGraph, node: usize, par: usize, parent: &mut [usize]) {
        parent[node] = par;
        for &e in &g.adj[node] {
            let c = g.other(e, node);
            if c != par {
                rec(g, c, node, parent);
            }
        }
    }
    let mut parent = vec![INVALID; g.pts.len()];
    rec(g, driver, driver, &mut parent);
    parent
}

/// `SteinerTreeBuilder::checkTree`: every pair of branch-edge bounding boxes
/// must not intersect unless they share a corner or one is degenerate.
/// `deg > 100` skips the check entirely. Verified against OpenROAD @ dcf36133
/// (`src/stt/src/SteinerTreeBuilder.cpp`): `rectAreaZero` is point-degenerate
/// only, `odb::Rect::intersects` is inclusive, and all branch pairs are
/// compared (adjacent edges always share a corner, so only non-adjacent
/// crossings fail).
fn check_tree(g: &PdGraph, parent: &[usize], deg: usize) -> bool {
    if deg > 100 {
        return true;
    }
    let n = g.pts.len();
    let mut rects = Vec::with_capacity(n);
    for i in 0..n {
        let (x1, y1) = g.pts[i];
        let (x2, y2) = g.pts[parent[i]];
        rects.push((x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)));
    }
    let area_zero = |r: (i32, i32, i32, i32)| r.0 == r.2 && r.1 == r.3;
    let intersects = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| {
        a.2 >= b.0 && a.0 <= b.2 && a.3 >= b.1 && a.1 <= b.3
    };
    let is_corner = |r: (i32, i32, i32, i32), x: i32, y: i32| {
        (x == r.0 || x == r.2) && (y == r.1 || y == r.3)
    };
    let share_corner = |a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)| {
        [(b.0, b.1), (b.0, b.3), (b.2, b.1), (b.2, b.3)]
            .iter()
            .any(|&(x, y)| is_corner(a, x, y))
    };
    for i in 0..n {
        for j in i + 1..n {
            let (r1, r2) = (rects[i], rects[j]);
            if area_zero(r1) || area_zero(r2) {
                continue;
            }
            if intersects(r1, r2) && !share_corner(r1, r2) {
                return false;
            }
        }
    }
    true
}

/// Result of the PD pass: tree segments plus whether `checkTree` accepted
/// them (false means OpenROAD would have used FLUTE instead).
pub(crate) struct PdTree {
    pub segments: Vec<(Pt, Pt)>,
    pub check_passed: bool,
}

/// `pdr::primDijkstra` + `checkTree` on the sorted pin locations.
/// `sorted_pts` must be the pins sorted by (x, y) with the driver somewhere
/// inside; `driver` is its index in that array.
pub(crate) fn build_pd_tree(sorted_pts: &[Pt], driver: usize) -> PdTree {
    let mut g = PdGraph::with_terminals(sorted_pts);
    let nn = nearest_neighbors(sorted_pts);
    build_spanning_tree(&mut g, driver, &nn);
    steinerize(&mut g);
    split_degree4_nodes(&mut g);
    let parent = make_tree_parents(&g, driver);
    let check_passed = check_tree(&g, &parent, sorted_pts.len());
    let mut segments = Vec::new();
    for i in 0..g.pts.len() {
        let p = parent[i];
        if p != i && p != INVALID {
            segments.push((g.pts[i], g.pts[p]));
        }
    }
    PdTree { segments, check_passed }
}

/// Segments-only entry point: `pts[0]` is the driver pin location, remaining
/// points are the loads in netlist order. Callers that need the `checkTree`
/// result use `build_pd_tree_for_points`.
pub fn build_pd_points(pts: &[Pt]) -> Vec<(Pt, Pt)> {
    build_pd_tree_for_points(pts).segments
}

/// Sort the pins by (x, y) like `EstimateParasitics::makeSteinerTree` and run
/// the PD pass. `pts[0]` is the driver pin location. Coincident points keep
/// input order (the reference's `std::sort` order among equals is unspecified,
/// and any choice yields an isomorphic tree).
pub(crate) fn build_pd_tree_for_points(pts: &[Pt]) -> PdTree {
    if pts.len() < 2 {
        return PdTree { segments: Vec::new(), check_passed: true };
    }
    let mut order: Vec<usize> = (0..pts.len()).collect();
    order.sort_by_key(|&i| (pts[i].0, pts[i].1));
    let sorted_pts: Vec<Pt> = order.iter().map(|&i| pts[i]).collect();
    // The driver's sorted position: first index sharing the driver location.
    // Any coincident index gives an isomorphic tree.
    let driver_pt = pts[0];
    let driver = sorted_pts.iter().position(|&p| p == driver_pt).unwrap_or(0);
    build_pd_tree(&sorted_pts, driver)
}
