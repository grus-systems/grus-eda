//! Shared polygon substrate for the area families: per-layer
//! connected components of member metal, union area, and enclosed
//! void (hole) extraction.
//!
//! Components are merged purely on geometry — a rect touching another
//! on any edge or corner is one plane polygon regardless of net, which
//! is how Magic's tile plane reads the paint (a two-net touch is a
//! short for the spacing family, not something the area check splits
//! over). `MacroObs` is never a member — blockage abstracts are not
//! fabricated metal — but `drc::area` consults them separately when
//! deciding whether an enclosed void is real.

use crate::drc::{LayerShapes, ShapeSrc};
use crate::tech_db::RectDbu;

/// Member-source filter: rects that are real metal on the plane.
/// `MacroObs` is not a member (blockage abstract, not fabricated
/// metal) but is consulted separately for hole discards.
pub fn is_member(src: ShapeSrc) -> bool {
    matches!(
        src,
        ShapeSrc::SignalWire | ShapeSrc::SpecialWire | ShapeSrc::Via | ShapeSrc::IoPort | ShapeSrc::MacroPin
    )
}

/// Connected components of member rects on one layer — Magic's
/// merged plane polygons. `comp_of[i] == u32::MAX` marks non-members.
pub struct LayerComponents {
    /// `rects` index → component id.
    pub comp_of: Vec<u32>,
    /// Component id → member `rects` indices (insertion order).
    pub members: Vec<Vec<u32>>,
}

/// Components of all member rects: `for_each_pair_within(1)` — the
/// same closed-touch predicate the spacing same-layer pass uses
/// (touch or overlap counts, corner contact included) — merged
/// regardless of net.
pub fn components(ls: &LayerShapes) -> LayerComponents {
    let n = ls.rects.len();
    let member: Vec<bool> = ls.rects.iter().map(|s| is_member(s.src)).collect();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(p: &mut [u32], mut x: u32) -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    }
    ls.for_each_pair_within(1, |a, b| {
        if member[a as usize] && member[b as usize] {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra as usize] = rb;
            }
        }
    });
    let mut comp_of = vec![u32::MAX; n];
    let mut members: Vec<Vec<u32>> = Vec::new();
    // Index-order scan keeps component ids deterministic; a
    // root→id table over `u32::MAX`-init slots avoids hashing.
    let mut id_of = vec![u32::MAX; n];
    for i in 0..n {
        if !member[i] {
            continue;
        }
        let root = find(&mut parent, i as u32) as usize;
        let id = if id_of[root] == u32::MAX {
            id_of[root] = members.len() as u32;
            members.push(Vec::new());
            id_of[root]
        } else {
            id_of[root]
        };
        comp_of[i] = id;
        members[id as usize].push(i as u32);
    }
    LayerComponents { comp_of, members }
}

/// Covered-length segment tree over compressed y endpoints — the
/// classic union-of-rectangles sweep needs O(log n) interval
/// add/remove, not an O(active) rescan per slab.
struct Cover {
    ys: Vec<i32>,
    cnt: Vec<i32>,
    len: Vec<i64>,
}

impl Cover {
    fn new(ys: Vec<i32>) -> Self {
        let m = ys.len().max(2);
        Self {
            ys,
            cnt: vec![0; 4 * m],
            len: vec![0; 4 * m],
        }
    }
    /// `+1`/`-1` coverage of `[l, r)`.
    fn add(&mut self, l: i32, r: i32, d: i32) {
        self.upd(0, 0, self.ys.len() - 1, l, r, d);
    }
    fn upd(&mut self, i: usize, nl: usize, nr: usize, l: i32, r: i32, d: i32) {
        if r <= self.ys[nl] || l >= self.ys[nr] {
            return;
        }
        if l <= self.ys[nl] && self.ys[nr] <= r {
            self.cnt[i] += d;
        } else if nr - nl > 1 {
            let mid = (nl + nr) / 2;
            self.upd(2 * i + 1, nl, mid, l, r, d);
            self.upd(2 * i + 2, mid, nr, l, r, d);
        }
        self.len[i] = if self.cnt[i] > 0 {
            i64::from(self.ys[nr]) - i64::from(self.ys[nl])
        } else if nr - nl == 1 {
            0
        } else {
            self.len[2 * i + 1] + self.len[2 * i + 2]
        };
    }
    fn covered(&self) -> i64 {
        self.len[0]
    }
}

/// Union area of a rect set, DBU² — one x-sweep over the coverage
/// tree. Called per component; member sets are small and x-disjoint
/// in aggregate, so total work is ~O(n log n) per layer even with a
/// die-spanning rail component.
pub fn union_area(rects: &[RectDbu]) -> i64 {
    if rects.is_empty() {
        return 0;
    }
    let mut ys: Vec<i32> = Vec::with_capacity(2 * rects.len());
    let mut edges: Vec<(i32, i32, i32, i32)> = Vec::with_capacity(2 * rects.len());
    for r in rects {
        if r.xl >= r.xh || r.yl >= r.yh {
            continue;
        }
        ys.push(r.yl);
        ys.push(r.yh);
        edges.push((r.xl, r.yl, r.yh, 1));
        edges.push((r.xh, r.yl, r.yh, -1));
    }
    ys.sort_unstable();
    ys.dedup();
    if ys.len() < 2 {
        return 0;
    }
    // Removal before insertion at a shared x: a rect ending exactly
    // where another starts covers the gap slab only once — event
    // order within a coordinate must not affect `covered` at slab
    // time, and applying all events before the slab handles that.
    edges.sort_unstable();
    let mut tree = Cover::new(ys);
    let mut area = 0i64;
    let mut prev_x = edges[0].0;
    let mut i = 0;
    while i < edges.len() {
        let x = edges[i].0;
        area += tree.covered() * i64::from(x - prev_x);
        while i < edges.len() && edges[i].0 == x {
            tree.add(edges[i].1, edges[i].2, edges[i].3);
            i += 1;
        }
        prev_x = x;
    }
    area
}

/// One enclosed void: the free-space region plus its totals.
#[derive(Debug)]
pub struct Hole {
    /// Free rects of the component (for OBS adjacency tests).
    pub rects: Vec<RectDbu>,
    pub bbox: RectDbu,
    pub area_dbu2: i64,
}

/// Free-space components inside `window` that never touch its
/// boundary — enclosed by the `solid` union. `solid` is every member
/// rect overlapping the window (any component, any net).
///
/// Same x-sweep shape as `union_area`, but per slab the *free*
/// intervals — the metal-union complement within
/// `[window.yl, window.yh]` — become free rects that merge across
/// adjacent slabs on positive y-overlap (edge-connected only; a void
/// reaching outside through a point-pinched channel counts as
/// enclosed). A component with no free rect touching any `window`
/// edge is a hole. The boundary test is what separates exterior
/// notches from holes: a C-shaped polygon's gap reaches the window
/// edge; a donut's void never does.
pub fn enclosed_voids(solid: &[RectDbu], window: RectDbu) -> Vec<Hole> {
    let mut xs = vec![window.xl, window.xh];
    let mut edges: Vec<(i32, i32, i32, i32)> = Vec::new();
    for r in solid {
        let (xl, xh) = (r.xl.max(window.xl), r.xh.min(window.xh));
        let (yl, yh) = (r.yl.max(window.yl), r.yh.min(window.yh));
        if xl < xh && yl < yh {
            xs.push(xl);
            xs.push(xh);
            edges.push((xl, yl, yh, 1));
            edges.push((xh, yl, yh, -1));
        }
    }
    xs.sort_unstable();
    xs.dedup();
    edges.sort_unstable();

    let mut free: Vec<RectDbu> = Vec::new();
    let mut parent: Vec<u32> = Vec::new();
    // Free rects of the previous slab: (index into `free`) — the
    // merge candidate set for the current slab.
    let mut prev: Vec<u32> = Vec::new();
    let mut active: Vec<(i32, i32)> = Vec::new();
    let mut ei = 0usize;

    let find = |p: &mut [u32], mut x: u32| -> u32 {
        while p[x as usize] != x {
            p[x as usize] = p[p[x as usize] as usize];
            x = p[x as usize];
        }
        x
    };

    for w in xs.windows(2) {
        let (x0, x1) = (w[0], w[1]);
        // Apply all events at x0: an interval ending at x0 covers
        // nothing inside [x0, x1); one starting at x0 covers all of it.
        while ei < edges.len() && edges[ei].0 == x0 {
            let (_, yl, yh, d) = edges[ei];
            if d > 0 {
                active.push((yl, yh));
            } else if let Some(pos) = active.iter().position(|&a| a == (yl, yh)) {
                active.swap_remove(pos);
            }
            ei += 1;
        }
        // Free intervals = complement of the active union inside the
        // window's y span. Merge active first so abutting coverage
        // does not fabricate zero-width voids.
        active.sort_unstable();
        let mut merged: Vec<(i32, i32)> = Vec::with_capacity(active.len());
        for &(a0, a1) in &active {
            match merged.last_mut() {
                Some(m) if a0 <= m.1 => m.1 = m.1.max(a1),
                _ => merged.push((a0, a1)),
            }
        }
        let mut gaps: Vec<(i32, i32)> = Vec::with_capacity(merged.len() + 1);
        let mut y = window.yl;
        for &(a0, a1) in &merged {
            if a0 > y {
                gaps.push((y, a0.min(window.yh)));
            }
            y = y.max(a1);
        }
        if y < window.yh {
            gaps.push((y, window.yh));
        }
        // Emit this slab's free rects; merge with the previous slab's
        // on positive y-overlap — both lists are y-sorted and
        // disjoint, so a two-pointer join is O(a + b).
        let mut cur: Vec<u32> = Vec::with_capacity(gaps.len());
        let (mut p, mut c) = (0usize, 0usize);
        for &(g0, g1) in &gaps {
            let idx = free.len() as u32;
            free.push(RectDbu::new(x0, g0, x1, g1));
            parent.push(idx);
            cur.push(idx);
        }
        while p < prev.len() && c < cur.len() {
            let (pr, cr) = (free[prev[p] as usize], free[cur[c] as usize]);
            if pr.yh <= cr.yl {
                p += 1;
            } else if cr.yh <= pr.yl {
                c += 1;
            } else {
                let (rp, rc) = (find(&mut parent, prev[p]), find(&mut parent, cur[c]));
                if rp != rc {
                    parent[rp as usize] = rc;
                }
                if pr.yh < cr.yh {
                    p += 1;
                } else {
                    c += 1;
                }
            }
        }
        prev = cur;
    }

    // Group free rects by component; a component with any member
    // touching a window edge is exterior space, not a hole.
    let mut holes: Vec<Hole> = Vec::new();
    let mut idx_of = vec![u32::MAX; free.len()];
    for (i, &r) in free.iter().enumerate() {
        let root = find(&mut parent, i as u32) as usize;
        let hid = if idx_of[root] == u32::MAX {
            idx_of[root] = holes.len() as u32;
            holes.push(Hole {
                rects: Vec::new(),
                bbox: r,
                area_dbu2: 0,
            });
            idx_of[root]
        } else {
            idx_of[root]
        };
        let h = &mut holes[hid as usize];
        h.rects.push(r);
        h.area_dbu2 += i64::from(r.xh - r.xl) * i64::from(r.yh - r.yl);
        h.bbox = RectDbu::new(
            h.bbox.xl.min(r.xl),
            h.bbox.yl.min(r.yl),
            h.bbox.xh.max(r.xh),
            h.bbox.yh.max(r.yh),
        );
    }
    holes.retain(|h| {
        h.rects.iter().all(|r| {
            r.xl > window.xl && r.xh < window.xh && r.yl > window.yl && r.yh < window.yh
        })
    });
    holes
}

/// `cur` minus each rect of `sub` — the residual slabs. Iterating `sub`
/// in order splits off up to four axis-aligned slabs per subtraction;
/// the result order is fixed by input order (determinism), overlaps
/// between `sub` rects are harmless (a slab can only shrink).
pub fn rect_minus(mut cur: Vec<RectDbu>, sub: &[RectDbu]) -> Vec<RectDbu> {
    for s in sub {
        let mut next = Vec::with_capacity(cur.len() * 2);
        for r in cur.drain(..) {
            let (ixl, ixh) = (r.xl.max(s.xl), r.xh.min(s.xh));
            let (iyl, iyh) = (r.yl.max(s.yl), r.yh.min(s.yh));
            if ixl >= ixh || iyl >= iyh {
                next.push(r);
                continue;
            }
            // left / right full-height slabs
            if r.xl < ixl {
                next.push(RectDbu::new(r.xl, r.yl, ixl, r.yh));
            }
            if ixh < r.xh {
                next.push(RectDbu::new(ixh, r.yl, r.xh, r.yh));
            }
            // top / bottom slabs inside the x-overlap
            if ixl < ixh {
                if r.yl < iyl {
                    next.push(RectDbu::new(ixl, r.yl, ixh, iyl));
                }
                if iyh < r.yh {
                    next.push(RectDbu::new(ixl, iyh, ixh, r.yh));
                }
            }
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    cur
}

/// Is `[lo, hi]` covered by the union of `ivs` — boundary contact
/// counts as connected (two abutting cover rects hold the paint
/// between them just as one merged rect would)?
fn intervals_cover(ivs: &mut [(i32, i32)], lo: i32, hi: i32) -> bool {
    ivs.sort_unstable();
    let mut cur = lo;
    for &(a, b) in ivs.iter() {
        if a > cur {
            break;
        }
        cur = cur.max(b);
        if cur >= hi {
            return true;
        }
    }
    cur >= hi
}

/// `part ⊆ ⋃ rects` — union-exact containment (the r3 confinement
/// test): a part spanning joined same-pin sub-rects is inside the
/// cover even when no single rect holds it (the L-shaped-pin class).
/// Degenerate parts are exact too: a line/point part is covered iff
/// the varying axis's interval is covered by the rects containing the
/// fixed coordinate.
pub fn rect_covered(rects: &[RectDbu], part: &RectDbu) -> bool {
    if part.xl == part.xh {
        // Point or vertical-segment part at x: the y-interval must be
        // covered by rects spanning x.
        let mut ivs: Vec<(i32, i32)> = rects
            .iter()
            .filter(|r| part.xl >= r.xl && part.xh <= r.xh)
            .map(|r| (r.yl, r.yh))
            .collect();
        return intervals_cover(&mut ivs, part.yl, part.yh);
    }
    if part.yl == part.yh {
        let mut ivs: Vec<(i32, i32)> = rects
            .iter()
            .filter(|r| part.yl >= r.yl && part.yh <= r.yh)
            .map(|r| (r.xl, r.xh))
            .collect();
        return intervals_cover(&mut ivs, part.xl, part.xh);
    }
    rect_minus(vec![*part], rects).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc::{LayerShapes, ShapeRect};

    fn sh(rect: RectDbu, src: ShapeSrc) -> ShapeRect {
        ShapeRect {
            rect,
            src,
            net: None,
            group: 0,
        }
    }

    /// Different-net touching rects merge (the plane model), `MacroObs`
    /// never joins, corner-only contact still counts, and disjoint
    /// islands keep separate ids.
    #[test]
    fn test_components_merge_semantics() {
        let net = |n: u32| crate::interner::SymbolId(n);
        let mut a = sh(RectDbu::new(0, 0, 100, 100), ShapeSrc::SignalWire);
        a.net = Some(net(1));
        let mut b = sh(RectDbu::new(100, 0, 200, 100), ShapeSrc::SignalWire);
        b.net = Some(net(2)); // different net, edge touch → same comp
        let c = sh(RectDbu::new(200, 100, 300, 200), ShapeSrc::SignalWire);
        // c touches b's (200,100) corner only → still one component.
        let obs = sh(RectDbu::new(150, 0, 250, 100), ShapeSrc::MacroObs);
        // obs overlaps a and b but is never a member.
        let far = sh(RectDbu::new(1000, 0, 1100, 100), ShapeSrc::Via);
        let l = LayerShapes::from_rects(vec![a, b, c, obs, far]);
        let lc = components(&l);
        assert_eq!(lc.members.len(), 2);
        assert_eq!(lc.comp_of[0], lc.comp_of[1]);
        assert_eq!(lc.comp_of[1], lc.comp_of[2]);
        assert_eq!(lc.comp_of[3], u32::MAX, "MacroObs is not a member");
        assert_ne!(lc.comp_of[0], lc.comp_of[4]);
    }

    /// Two overlapping rects dedupe (a∪b, not a+b); disjoint sets sum.
    #[test]
    fn test_union_area() {
        let a = RectDbu::new(0, 0, 100, 100);
        let b = RectDbu::new(50, 50, 150, 150);
        assert_eq!(union_area(&[a, b]), 100 * 100 + 100 * 100 - 50 * 50);
        let c = RectDbu::new(1000, 0, 1100, 50);
        assert_eq!(union_area(&[a, b, c]), 100 * 100 + 100 * 100 - 50 * 50 + 5000);
        assert_eq!(union_area(&[]), 0);
        // Edge-touching only → no overlap to dedupe.
        let d = RectDbu::new(100, 0, 200, 100);
        assert_eq!(union_area(&[a, d]), 100 * 100 + 100 * 100);
    }

    /// Donut of 4 rects → exactly 1 void with the inner area; a
    /// C-open shape's gap reaches the window edge → 0.
    #[test]
    fn test_enclosed_voids_donut_and_c_open() {
        // Ring: outer 0..400 × 0..400, void 100..300 × 100..300.
        let ring = [
            RectDbu::new(0, 0, 400, 100),   // bottom
            RectDbu::new(0, 300, 400, 400), // top
            RectDbu::new(0, 100, 100, 300), // left
            RectDbu::new(300, 100, 400, 300), // right
        ];
        let w = RectDbu::new(0, 0, 400, 400);
        let holes = enclosed_voids(&ring, w);
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].area_dbu2, 200 * 200);
        assert_eq!(holes[0].bbox, RectDbu::new(100, 100, 300, 300));

        // Remove the left wall → the gap reaches the left edge: open.
        let holes = enclosed_voids(&ring[..3], w);
        assert!(holes.is_empty(), "{holes:?}");
    }

    /// Two stacked donuts → 2 holes; a point-pinched channel (the
    /// void exits only through a corner) is still a hole.
    #[test]
    fn test_enclosed_voids_stacked_and_pinched() {
        // Two 0..400 × 0..400 rings stacked 400 apart in y.
        let ring = |dy: i32| {
            [
                RectDbu::new(0, dy, 400, dy + 100),
                RectDbu::new(0, dy + 300, 400, dy + 400),
                RectDbu::new(0, dy + 100, 100, dy + 300),
                RectDbu::new(300, dy + 100, 400, dy + 300),
            ]
        };
        let w = RectDbu::new(0, 0, 400, 1000);
        let mut solid = ring(0).to_vec();
        solid.extend_from_slice(&ring(500));
        let holes = enclosed_voids(&solid, w);
        assert_eq!(holes.len(), 2);
        assert!(holes.iter().all(|h| h.area_dbu2 == 200 * 200));

        // Point-pinched: bottom wall split at x=200, the two halves
        // shift 1 dbu in y so the channel closes only at the corner
        // (200,100) — edge contact, void stays enclosed.
        let pinched = [
            RectDbu::new(0, 0, 200, 100),     // bottom-left
            RectDbu::new(200, 99, 400, 199),  // bottom-right, overlaps x=200 corner
            RectDbu::new(0, 300, 400, 400),   // top
            RectDbu::new(0, 100, 100, 300),   // left
            RectDbu::new(300, 100, 400, 300), // right
        ];
        let holes = enclosed_voids(&pinched, RectDbu::new(0, 0, 400, 400));
        assert_eq!(holes.len(), 1, "{holes:?}");
    }

    /// A foreign rect inside the void shrinks its area.
    #[test]
    fn test_enclosed_voids_foreign_fill() {
        let ring = [
            RectDbu::new(0, 0, 400, 100),
            RectDbu::new(0, 300, 400, 400),
            RectDbu::new(0, 100, 100, 300),
            RectDbu::new(300, 100, 400, 300),
        ];
        let mut solid = ring.to_vec();
        solid.push(RectDbu::new(150, 150, 250, 250)); // inside the void
        let holes = enclosed_voids(&solid, RectDbu::new(0, 0, 400, 400));
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].area_dbu2, 200 * 200 - 100 * 100);
    }
}
