use crate::design_db::{DesignDb, Orient, PlacementStatus};
use crate::interner::{CellId, NetId, PinId};
use crate::tech_db::{MacroClass, TechDb};
use anyhow::{Context, Result, bail};
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct LegalizeResult {
    pub avg_displacement_um: f64,
    pub max_displacement_um: f64,
    pub pre_legal_hpwl_dbu: i64,
    pub post_abacus_hpwl_dbu: i64,
    pub final_hpwl_dbu: i64,
    /// Full net rescans done by pass 1 trial evaluations, summed over all rounds.
    pub pass1_eval_fallbacks: usize,
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

/// Simulate appending `cell` of width `w_i` at `x_gp` onto `sub_row.clusters` and return
/// the resulting legal x coordinate without cloning `sub_row.clusters`.
#[inline]
fn trial_add_cell_x(w_i: i32, x_gp: i32, sub_row: &SubRow) -> i32 {
    let mut cur_x = snap_to_site(
        x_gp,
        sub_row.orig_x_dbu,
        sub_row.step_x_dbu,
        sub_row.x_min_dbu,
        sub_row.x_max_dbu - w_i,
    );
    let mut cur_w = w_i;
    let mut cur_weight = 1.0f32;
    let mut cur_q = x_gp as f32;

    for c_prev in sub_row.clusters.iter().rev() {
        if c_prev.x_dbu + c_prev.width_dbu <= cur_x {
            break;
        }
        cur_q = c_prev.q_sum + (cur_q - cur_weight * (c_prev.width_dbu as f32));
        cur_weight += c_prev.weight_sum;
        cur_w += c_prev.width_dbu;
        cur_x = snap_to_site(
            (cur_q / cur_weight).round() as i32,
            sub_row.orig_x_dbu,
            sub_row.step_x_dbu,
            sub_row.x_min_dbu,
            sub_row.x_max_dbu - cur_w,
        );
    }
    cur_x + cur_w - w_i
}

/// Merge two sorted unique net slices into `out` without duplicates.
#[inline]
fn merge_sorted_nets(a: &[usize], b: &[usize], out: &mut Vec<usize>) {
    out.clear();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
}

#[derive(Clone, Copy)]
struct PinGeom {
    owner: u32, // u32::MAX if primary I/O port
    w: i32,
    h: i32,
    ox: i32,
    oy: i32,
    fx: bool,
    fy: bool,
}

#[inline(always)]
fn truncated_axis_fast(base: i32, off: i32, frac: bool, mirrored: bool) -> i32 {
    if !frac {
        if mirrored { base - off } else { base + off }
    } else {
        let floor = if mirrored { base - off - 1 } else { base + off };
        if floor < 0 { floor + 1 } else { floor }
    }
}

#[inline(always)]
fn pin_pos_at_cell(g: PinGeom, cx: i32, cy: i32, orient: Orient) -> (i32, i32) {
    let x = |base, off, frac| truncated_axis_fast(base, off, frac, false);
    let mx = |base, off, frac| truncated_axis_fast(base, off, frac, true);
    match orient {
        Orient::N => (x(cx, g.ox, g.fx), x(cy, g.oy, g.fy)),
        Orient::S => (mx(cx + g.w, g.ox, g.fx), mx(cy + g.h, g.oy, g.fy)),
        Orient::W => (mx(cx + g.h, g.oy, g.fy), x(cy, g.ox, g.fx)),
        Orient::E => (x(cx, g.oy, g.fy), mx(cy + g.w, g.ox, g.fx)),
        Orient::FN => (mx(cx + g.w, g.ox, g.fx), x(cy, g.oy, g.fy)),
        Orient::FS => (x(cx, g.ox, g.fx), mx(cy + g.h, g.oy, g.fy)),
        Orient::FW => (x(cx, g.oy, g.fy), x(cy, g.ox, g.fx)),
        Orient::FE => (mx(cx + g.h, g.oy, g.fy), mx(cy + g.w, g.ox, g.fx)),
    }
}

struct PinGeomCache {
    pins: Vec<PinGeom>,
}

impl PinGeomCache {
    fn new(db: &DesignDb, tech_db: &TechDb) -> Self {
        let num_pins = db.num_pins();
        let mut pins = Vec::with_capacity(num_pins);
        for p in 0..num_pins {
            match db.pin_cells[p] {
                None => pins.push(PinGeom {
                    owner: u32::MAX,
                    w: 0,
                    h: 0,
                    ox: db.pin_offset_x_dbu[p],
                    oy: db.pin_offset_y_dbu[p],
                    fx: false,
                    fy: false,
                }),
                Some(cell_id) => {
                    let c = cell_id.0 as usize;
                    let mac = &tech_db.macros[db.cell_macros[c].0 as usize];
                    let (fx, fy) = match db.pin_macro_pins[p] {
                        Some(mp) => {
                            let mpin = &tech_db.macro_pins[mp.0 as usize];
                            (mpin.offset_x_frac, mpin.offset_y_frac)
                        }
                        None => (false, false),
                    };
                    pins.push(PinGeom {
                        owner: cell_id.0,
                        w: mac.width_dbu,
                        h: mac.height_dbu,
                        ox: db.pin_offset_x_dbu[p],
                        oy: db.pin_offset_y_dbu[p],
                        fx,
                        fy,
                    });
                }
            }
        }
        Self { pins }
    }

    #[inline(always)]
    fn pin_pos(&self, db: &DesignDb, pin: PinId) -> (i32, i32) {
        let g = self.pins[pin.0 as usize];
        if g.owner == u32::MAX {
            (g.ox, g.oy)
        } else {
            let c = g.owner as usize;
            pin_pos_at_cell(g, db.cell_x_dbu[c], db.cell_y_dbu[c], db.cell_orient[c])
        }
    }
}

fn net_hpwl_dbu_fast(db: &DesignDb, geom: &PinGeomCache, net_idx: usize) -> i64 {
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
        let (px, py) = geom.pin_pos(db, pin);
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

#[cfg(test)]
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

#[derive(Clone, Copy)]
struct Extremum {
    val: i32,
    pin: PinId,
}

// Placeholder for the unused slots of nets with fewer than two pins; never read.
const UNUSED_EXTREMUM: Extremum = Extremum {
    val: 0,
    pin: PinId(0),
};

// Two smallest (`lo`) and two largest (`hi`) pin coordinates per axis (0 = x, 1 = y), best
// first, each with its pin. The slots hold real pins only when `pin_count >= 2`.
#[derive(Clone, Copy)]
struct NetExtrema {
    pin_count: usize,
    lo: [[Extremum; 2]; 2],
    hi: [[Extremum; 2]; 2],
}

// Keeps the two best of the values offered so far. `seen` is the number of values before `e`.
// `ahead` is strict, so a tie keeps the earlier pin in front and a 3+-way tie still fills both slots.
fn offer_extremum(top: &mut [Extremum; 2], e: Extremum, seen: usize, ahead: fn(i32, i32) -> bool) {
    match seen {
        0 => top[0] = e,
        1 => {
            if ahead(e.val, top[0].val) {
                top[1] = top[0];
                top[0] = e;
            } else {
                top[1] = e;
            }
        }
        _ => {
            if ahead(e.val, top[0].val) {
                top[1] = top[0];
                top[0] = e;
            } else if ahead(e.val, top[1].val) {
                top[1] = e;
            }
        }
    }
}

#[cfg(test)]
fn build_net_extrema(db: &DesignDb, tech_db: &TechDb, net: usize) -> NetExtrema {
    let geom = PinGeomCache::new(db, tech_db);
    build_net_extrema_fast(db, &geom, net)
}

fn build_net_extrema_fast(db: &DesignDb, geom: &PinGeomCache, net: usize) -> NetExtrema {
    let pins = db.net_pins(NetId(net as u32));
    let mut ext = NetExtrema {
        pin_count: pins.len(),
        lo: [[UNUSED_EXTREMUM; 2]; 2],
        hi: [[UNUSED_EXTREMUM; 2]; 2],
    };
    if pins.len() < 2 {
        return ext;
    }
    let less: fn(i32, i32) -> bool = |a, b| a < b;
    let greater: fn(i32, i32) -> bool = |a, b| a > b;
    for (seen, &pin) in pins.iter().enumerate() {
        let (px, py) = geom.pin_pos(db, pin);
        for (axis, val) in [px, py].into_iter().enumerate() {
            let e = Extremum { val, pin };
            offer_extremum(&mut ext.lo[axis], e, seen, less);
            offer_extremum(&mut ext.hi[axis], e, seen, greater);
        }
    }
    ext
}

// Same expression as `net_hpwl_dbu`, read from the cache.
fn cached_hpwl(ext: &NetExtrema) -> i64 {
    if ext.pin_count <= 1 {
        return 0;
    }
    (ext.hi[0][0].val - ext.lo[0][0].val) as i64 + (ext.hi[1][0].val - ext.lo[1][0].val) as i64
}

// Extremum over the pins that did not move, or None when both top-2 pins moved and the
// caller must rescan the net.
#[cfg(test)]
fn surviving_extremum(top: &[Extremum; 2], moved: &[PinId]) -> Option<i32> {
    if !moved.contains(&top[0].pin) {
        Some(top[0].val)
    } else if !moved.contains(&top[1].pin) {
        Some(top[1].val)
    } else {
        None
    }
}

#[inline(always)]
fn surviving_extremum_pos(top: &[Extremum; 2], moved: &[(PinId, i32, i32)]) -> Option<i32> {
    if !moved.iter().any(|&(p, _, _)| p == top[0].pin) {
        Some(top[0].val)
    } else if !moved.iter().any(|&(p, _, _)| p == top[1].pin) {
        Some(top[1].val)
    } else {
        None
    }
}

// Pins of cells `c1` and `c2` on `net`. Those are the only pins a trial swap moves.
#[cfg(test)]
fn moved_pins_on_net(db: &DesignDb, net: usize, c1: usize, c2: usize, moved: &mut Vec<PinId>) {
    moved.clear();
    for c in [c1, c2] {
        for &pin in db.cell_pins(CellId(c as u32)) {
            if db.pin_nets[pin.0 as usize] == Some(NetId(net as u32)) {
                moved.push(pin);
            }
        }
    }
}

// HPWL of `net` with the trial positions already in `db`. `moved` are the pins of the swapped
// cells on this net. Each direction is read from the cache; `net_hpwl_dbu` is the fallback.
#[cfg(test)]
fn net_hpwl_after_swap(
    db: &DesignDb,
    tech_db: &TechDb,
    net: usize,
    ext: &NetExtrema,
    moved: &[PinId],
    fallbacks: &mut usize,
) -> i64 {
    if ext.pin_count <= 1 {
        return 0;
    }
    let mut lo = [i32::MAX; 2];
    let mut hi = [i32::MIN; 2];
    if moved.len() < ext.pin_count {
        for axis in 0..2 {
            let (Some(l), Some(h)) = (
                surviving_extremum(&ext.lo[axis], moved),
                surviving_extremum(&ext.hi[axis], moved),
            ) else {
                *fallbacks += 1;
                return net_hpwl_dbu(db, tech_db, net);
            };
            lo[axis] = l;
            hi[axis] = h;
        }
    }
    for &pin in moved {
        let (px, py) = db.pin_pos_dbu(pin, tech_db);
        lo[0] = lo[0].min(px);
        hi[0] = hi[0].max(px);
        lo[1] = lo[1].min(py);
        hi[1] = hi[1].max(py);
    }
    (hi[0] - lo[0]) as i64 + (hi[1] - lo[1]) as i64
}

// Scratch state for trial evaluations: overflow Vec for >8 moved pins on one net, and fallback count.
#[derive(Default)]
struct Pass1Eval {
    #[cfg(test)]
    moved: Vec<PinId>,
    overflow_moved: Vec<(PinId, i32, i32)>,
    fallbacks: usize,
}

#[cfg(test)]
fn evaluate_trial_swap(
    db: &mut DesignDb,
    tech_db: &TechDb,
    net_ext: &[NetExtrema],
    incident: &[usize],
    c1: usize,
    c2: usize,
    eval: &mut Pass1Eval,
) -> (i64, i64) {
    let hpwl_before: i64 = incident.iter().map(|&n| cached_hpwl(&net_ext[n])).sum();
    exchange_cells(db, c1, c2);
    let mut hpwl_after = 0i64;
    for &n in incident {
        moved_pins_on_net(db, n, c1, c2, &mut eval.moved);
        hpwl_after += net_hpwl_after_swap(
            db,
            tech_db,
            n,
            &net_ext[n],
            &eval.moved,
            &mut eval.fallbacks,
        );
    }
    (hpwl_before, hpwl_after)
}

/// Pure read-only swap evaluator over pre-sorted `(net, pin, geom)` slices for `c1` and `c2`:
/// computes `(hpwl_before, hpwl_after)` without mutating `db` on rejected swaps.
#[allow(clippy::too_many_arguments)]
fn evaluate_trial_swap_ro(
    db: &mut DesignDb,
    geom: &PinGeomCache,
    net_ext: &[NetExtrema],
    p1: &[(usize, PinId, PinGeom)],
    p2: &[(usize, PinId, PinGeom)],
    c1: usize,
    x1: i32,
    y1: i32,
    o1: Orient,
    c2: usize,
    x2: i32,
    y2: i32,
    o2: Orient,
    eval: &mut Pass1Eval,
) -> (i64, i64) {
    let mut hpwl_before = 0i64;
    let mut hpwl_after = 0i64;
    let (mut i, mut j) = (0usize, 0usize);

    while i < p1.len() || j < p2.len() {
        let net = match (p1.get(i), p2.get(j)) {
            (Some(a), Some(b)) => a.0.min(b.0),
            (Some(a), None) => a.0,
            (None, Some(b)) => b.0,
            (None, None) => break,
        };

        let mut stack_buf = [(PinId(0), 0i32, 0i32); 8];
        let mut count = 0usize;
        let mut using_overflow = false;

        let mut push_pin = |pin: PinId, px: i32, py: i32| {
            if !using_overflow && count < 8 {
                stack_buf[count] = (pin, px, py);
                count += 1;
            } else {
                if !using_overflow {
                    eval.overflow_moved.clear();
                    eval.overflow_moved.extend_from_slice(&stack_buf[..8]);
                    using_overflow = true;
                }
                eval.overflow_moved.push((pin, px, py));
            }
        };

        while i < p1.len() && p1[i].0 == net {
            let (_, pin, g) = p1[i];
            let (px, py) = pin_pos_at_cell(g, x2, y2, o2);
            push_pin(pin, px, py);
            i += 1;
        }
        while j < p2.len() && p2[j].0 == net {
            let (_, pin, g) = p2[j];
            let (px, py) = pin_pos_at_cell(g, x1, y1, o1);
            push_pin(pin, px, py);
            j += 1;
        }

        let ext = &net_ext[net];
        if ext.pin_count <= 1 {
            continue;
        }
        hpwl_before += cached_hpwl(ext);

        let moved = if using_overflow {
            eval.overflow_moved.as_slice()
        } else {
            &stack_buf[..count]
        };

        if moved.len() < ext.pin_count {
            if let (Some(lx), Some(hx), Some(ly), Some(hy)) = (
                surviving_extremum_pos(&ext.lo[0], moved),
                surviving_extremum_pos(&ext.hi[0], moved),
                surviving_extremum_pos(&ext.lo[1], moved),
                surviving_extremum_pos(&ext.hi[1], moved),
            ) {
                let (mut lox, mut hix, mut loy, mut hiy) = (lx, hx, ly, hy);
                for &(_, px, py) in moved {
                    lox = lox.min(px);
                    hix = hix.max(px);
                    loy = loy.min(py);
                    hiy = hiy.max(py);
                }
                hpwl_after += (hix - lox) as i64 + (hiy - loy) as i64;
            } else {
                eval.fallbacks += 1;
                exchange_cells(db, c1, c2);
                hpwl_after += net_hpwl_dbu_fast(db, geom, net);
                exchange_cells(db, c1, c2);
            }
        } else {
            let (mut lox, mut hix) = (i32::MAX, i32::MIN);
            let (mut loy, mut hiy) = (i32::MAX, i32::MIN);
            for &(_, px, py) in moved {
                lox = lox.min(px);
                hix = hix.max(px);
                loy = loy.min(py);
                hiy = hiy.max(py);
            }
            hpwl_after += (hix - lox) as i64 + (hiy - loy) as i64;
        }
    }

    (hpwl_before, hpwl_after)
}

// Exchanges position and orientation of two cells. Applying it twice restores both.
fn exchange_cells(db: &mut DesignDb, c1: usize, c2: usize) {
    db.cell_x_dbu.swap(c1, c2);
    db.cell_y_dbu.swap(c1, c2);
    db.cell_orient.swap(c1, c2);
}

const SWAP_MAX_DX_DBU: i32 = 25_000;

// (x_dbu, movable index, cell): sorted, so the x window is a binary-searched range.
type BandEntry = (i32, usize, usize);

// Invariant: at the start of every pass 1 round each movable cell sits on a sub-row y.
// Abacus writes sub-row y, pass 1 swaps exchange positions (y stays a sub-row y), and
// passes 2/3 never change y. A cell missing from `band_ys` would be missing from the
// index and silently drop candidate pairs, so this panics instead.
fn band_of(band_ys: &[i32], y: i32) -> usize {
    band_ys
        .binary_search(&y)
        .unwrap_or_else(|_| panic!("movable cell at y={} is not on a sub-row", y))
}

fn band_remove(band: &mut Vec<BandEntry>, entry: BandEntry) {
    let pos = band
        .binary_search(&entry)
        .expect("pass 1 index lost a cell");
    band.remove(pos);
}

fn band_insert(band: &mut Vec<BandEntry>, entry: BandEntry) {
    let pos = band.partition_point(|e| *e < entry);
    band.insert(pos, entry);
}

// Equal-width swaps, each cell against later cells in `movable_cells` order.
// The index is rebuilt here, once per round, and updated on every accepted swap.
// Returns the number of trial evaluations that rescanned a net in full.
#[cfg(test)]
fn equal_width_swap_pass(
    design_db: &mut DesignDb,
    tech_db: &TechDb,
    movable_cells: &[usize],
    cell_w_dbu: &[i32],
    cell_nets: &[Vec<usize>],
    band_ys: &[i32],
) -> usize {
    let geom = PinGeomCache::new(design_db, tech_db);
    let cell_pin_nets = build_cell_pin_nets(design_db, &geom, design_db.num_cells());
    equal_width_swap_pass_fast(
        design_db,
        tech_db,
        &geom,
        &cell_pin_nets,
        movable_cells,
        cell_w_dbu,
        cell_nets,
        band_ys,
    )
}

fn build_cell_pin_nets(
    design_db: &DesignDb,
    geom: &PinGeomCache,
    num_cells: usize,
) -> Vec<Vec<(usize, PinId, PinGeom)>> {
    let mut out = vec![Vec::new(); num_cells];
    for (c, entry) in out.iter_mut().enumerate() {
        let start = design_db.cell_pin_offsets[c] as usize;
        let end = design_db.cell_pin_offsets[c + 1] as usize;
        for &pin in &design_db.cell_pin_ids[start..end] {
            let p = pin.0 as usize;
            if let Some(net) = design_db.pin_nets[p] {
                entry.push((net.0 as usize, pin, geom.pins[p]));
            }
        }
        entry.sort_unstable_by_key(|&(net, pin, _)| (net, pin.0));
    }
    out
}

// Recomputes the cached x of every netted pin of `c` after the cell moved.
// Passes 2 and 3 slide cells along x only, so the cached y/orientation inputs stay valid.
fn refresh_cell_pin_x(
    db: &DesignDb,
    cell_pin_nets: &[Vec<(usize, PinId, PinGeom)>],
    pin_x: &mut [i32],
    c: usize,
) {
    let cx = db.cell_x_dbu[c];
    let cy = db.cell_y_dbu[c];
    let orient = db.cell_orient[c];
    for &(_, pin, g) in &cell_pin_nets[c] {
        pin_x[pin.0 as usize] = pin_pos_at_cell(g, cx, cy, orient).0;
    }
}

#[allow(clippy::too_many_arguments)]
fn equal_width_swap_pass_fast(
    design_db: &mut DesignDb,
    tech_db: &TechDb,
    geom: &PinGeomCache,
    cell_pin_nets: &[Vec<(usize, PinId, PinGeom)>],
    movable_cells: &[usize],
    cell_w_dbu: &[i32],
    cell_nets: &[Vec<usize>],
    band_ys: &[i32],
) -> usize {
    let max_dy = 4 * tech_db.site_height_dbu;

    let mut bands: Vec<Vec<BandEntry>> = vec![Vec::new(); band_ys.len()];
    for (mi, &c) in movable_cells.iter().enumerate() {
        let b = band_of(band_ys, design_db.cell_y_dbu[c]);
        bands[b].push((design_db.cell_x_dbu[c], mi, c));
    }
    for band in &mut bands {
        band.sort_unstable();
    }

    // Extrema of every net at the committed positions. Rebuilt for a pair's incident nets on accept.
    let n_nets = design_db.num_nets();
    let mut net_ext: Vec<NetExtrema> = if n_nets < 1024 {
        (0..n_nets)
            .map(|net| build_net_extrema_fast(design_db, geom, net))
            .collect()
    } else {
        (0..n_nets)
            .into_par_iter()
            .map(|net| build_net_extrema_fast(design_db, geom, net))
            .collect()
    };
    let mut eval = Pass1Eval::default();

    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut incident: Vec<usize> = Vec::new();
    for (i, &c1) in movable_cells.iter().enumerate() {
        let w1 = cell_w_dbu[c1];
        let x1 = design_db.cell_x_dbu[c1];
        let y1 = design_db.cell_y_dbu[c1];
        let o1 = design_db.cell_orient[c1];
        let p1 = &cell_pin_nets[c1];

        candidates.clear();
        let band_lo = band_ys.partition_point(|&by| by < y1.saturating_sub(max_dy));
        let band_hi = band_ys.partition_point(|&by| by <= y1.saturating_add(max_dy));
        for band in &bands[band_lo..band_hi] {
            let lo = band.partition_point(|e| e.0 < x1.saturating_sub(SWAP_MAX_DX_DBU));
            let hi = band.partition_point(|e| e.0 <= x1.saturating_add(SWAP_MAX_DX_DBU));
            for &(_, mi2, c2) in &band[lo..hi] {
                if mi2 > i && cell_w_dbu[c2] == w1 {
                    candidates.push((mi2, c2));
                }
            }
        }
        candidates.sort_unstable();

        for &(mi2, c2) in &candidates {
            if cell_w_dbu[c2] != w1 {
                continue;
            }
            let x2 = design_db.cell_x_dbu[c2];
            let y2 = design_db.cell_y_dbu[c2];

            if (y1 - y2).abs() > max_dy || (x1 - x2).abs() > SWAP_MAX_DX_DBU {
                continue;
            }

            let p2 = &cell_pin_nets[c2];
            if p1.is_empty() && p2.is_empty() {
                continue;
            }
            let o2 = design_db.cell_orient[c2];

            let (hpwl_before, hpwl_after) = evaluate_trial_swap_ro(
                design_db, geom, &net_ext, p1, p2, c1, x1, y1, o1, c2, x2, y2, o2, &mut eval,
            );

            if hpwl_after < hpwl_before {
                exchange_cells(design_db, c1, c2);
                let b1 = band_of(band_ys, y1);
                let b2 = band_of(band_ys, y2);
                band_remove(&mut bands[b1], (x1, i, c1));
                band_remove(&mut bands[b2], (x2, mi2, c2));
                band_insert(&mut bands[b2], (x2, i, c1));
                band_insert(&mut bands[b1], (x1, mi2, c2));
                merge_sorted_nets(&cell_nets[c1], &cell_nets[c2], &mut incident);
                for &n in &incident {
                    net_ext[n] = build_net_extrema_fast(design_db, geom, n);
                }
                break;
            }
        }
    }
    eval.fallbacks
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
    let row_orig_ys: Vec<i32> = row_indices
        .iter()
        .map(|&r_idx| design_db.rows[r_idx].orig_y_dbu)
        .collect();

    let mut row_obs: Vec<Vec<(i32, i32)>> = vec![Vec::new(); row_indices.len()];
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
        let cx = design_db.cell_x_dbu[f];
        let cw = cell_w_dbu[f];

        // Row k vertically overlaps [cy, cy + ch) iff ry + site_height > cy && ry < cy + ch.
        let min_ry = cy.saturating_sub(site_height);
        let lo = row_orig_ys.partition_point(|&ry| ry <= min_ry);
        let hi = row_orig_ys.partition_point(|&ry| ry < cy.saturating_add(ch));
        for k in lo..hi {
            let r = &design_db.rows[row_indices[k]];
            let row_xl = r.orig_x_dbu;
            let row_xh = r.orig_x_dbu + r.num_x * r.step_x_dbu;
            let ox_l = snap_down(cx, r.orig_x_dbu, r.step_x_dbu).max(row_xl);
            let ox_h = snap_up(cx + cw, r.orig_x_dbu, r.step_x_dbu).min(row_xh);
            if ox_h > ox_l {
                row_obs[k].push((ox_l, ox_h));
            }
        }
    }

    let mut sub_rows = Vec::new();
    for (k, &r_idx) in row_indices.iter().enumerate() {
        let r = &design_db.rows[r_idx];
        let row_xl = r.orig_x_dbu;
        let row_xh = r.orig_x_dbu + r.num_x * r.step_x_dbu;
        let obs_intervals = &mut row_obs[k];

        // Merge overlapping obstacle intervals
        obs_intervals.sort_unstable();
        let mut merged_obs: Vec<(i32, i32)> = Vec::new();
        for &(ox_l, ox_h) in obs_intervals.iter() {
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

    // Index sub_rows by sorted unique orig_y_dbu bands (preserving contiguous sub_row slices)
    // and track each band's max free_width for fast Abacus search + O(1) resync.
    let mut band_ys: Vec<i32> = Vec::new();
    let mut band_ranges: Vec<(usize, usize)> = Vec::new();
    let mut band_max_free: Vec<i32> = Vec::new();
    let mut subrow_band = vec![0usize; sub_rows.len()];
    {
        let mut i = 0;
        while i < sub_rows.len() {
            let y = sub_rows[i].orig_y_dbu;
            let b_idx = band_ys.len();
            let mut j = i;
            let mut max_fw = 0;
            while j < sub_rows.len() && sub_rows[j].orig_y_dbu == y {
                max_fw = max_fw.max(sub_rows[j].free_width);
                subrow_band[j] = b_idx;
                j += 1;
            }
            band_ys.push(y);
            band_ranges.push((i, j));
            band_max_free.push(max_fw);
            i = j;
        }
    }

    // Step 3: Abacus Single-Row Dynamic Clustering
    let max_y_dist = 10 * site_height;
    for &cell in &movable_cells {
        let w_i = cell_w_dbu[cell];
        let x_gp = x_gp_dbu[cell];
        let y_gp = y_gp_dbu[cell];

        // Search candidate sub-rows within +/- 10 rows first, then fall back to all rows.
        let mut best_subrow_idx: Option<usize> = None;
        let mut best_disp = i64::MAX;

        for &limited in &[true, false] {
            let (band_lo, band_hi) = if limited {
                let y_lo = y_gp as i64 - max_y_dist as i64;
                let y_hi = y_gp as i64 + max_y_dist as i64;
                (
                    band_ys.partition_point(|&by| (by as i64) < y_lo),
                    band_ys.partition_point(|&by| (by as i64) <= y_hi),
                )
            } else {
                (0, band_ys.len())
            };

            let mid = band_ys
                .partition_point(|&by| by < y_gp)
                .clamp(band_lo, band_hi);
            let mut left = mid;
            let mut right = mid;

            while left > band_lo || right < band_hi {
                let pick_right = match (left > band_lo, right < band_hi) {
                    (true, true) => {
                        let dl = y_gp as i64 - band_ys[left - 1] as i64;
                        let dr = band_ys[right] as i64 - y_gp as i64;
                        dr <= dl
                    }
                    (false, true) => true,
                    (true, false) => false,
                    (false, false) => break,
                };
                let b = if pick_right {
                    let b = right;
                    right += 1;
                    b
                } else {
                    left -= 1;
                    left
                };

                let dy = band_ys[b] as i64 - y_gp as i64;
                let dy2 = dy * dy;
                if dy2 > best_disp {
                    // Outward search guarantees |dy'| >= |dy| for all remaining bands.
                    break;
                }
                if band_max_free[b] < w_i {
                    continue;
                }

                let (sr_start, sr_end) = band_ranges[b];
                for (rel_idx, sr) in sub_rows[sr_start..sr_end].iter().enumerate() {
                    let sr_idx = sr_start + rel_idx;
                    if sr.free_width < w_i {
                        continue;
                    }
                    let max_start = sr.x_max_dbu - w_i;
                    let min_dx = if x_gp < sr.x_min_dbu {
                        (sr.x_min_dbu - x_gp) as i64
                    } else if x_gp > max_start {
                        (x_gp - max_start) as i64
                    } else {
                        0
                    };
                    if min_dx * min_dx + dy2 > best_disp {
                        continue;
                    }

                    let x_legal = trial_add_cell_x(w_i, x_gp, sr);
                    let dx = (x_legal - x_gp) as i64;
                    let disp = dx * dx + dy2;
                    if disp < best_disp
                        || (disp == best_disp && best_subrow_idx.is_none_or(|best| sr_idx < best))
                    {
                        best_disp = disp;
                        best_subrow_idx = Some(sr_idx);
                    }
                }
            }

            if best_subrow_idx.is_some() {
                break;
            }
        }

        let chosen_idx = best_subrow_idx.with_context(|| {
            format!(
                "Unable to place cell {}: no SubRow with remaining capacity {}",
                cell, w_i
            )
        })?;
        let sr = &mut sub_rows[chosen_idx];
        let sr_meta = SubRow {
            row_idx: sr.row_idx,
            orig_x_dbu: sr.orig_x_dbu,
            orig_y_dbu: sr.orig_y_dbu,
            step_x_dbu: sr.step_x_dbu,
            orient: sr.orient,
            x_min_dbu: sr.x_min_dbu,
            x_max_dbu: sr.x_max_dbu,
            free_width: sr.free_width,
            clusters: Vec::new(),
            cells: Vec::new(),
        };
        add_cell_to_clusters(&mut sr.clusters, cell, w_i, x_gp, &sr_meta);
        sr.free_width -= w_i;

        let b = subrow_band[chosen_idx];
        let (sr_start, sr_end) = band_ranges[b];
        band_max_free[b] = sub_rows[sr_start..sr_end]
            .iter()
            .map(|s| s.free_width)
            .max()
            .unwrap_or(0);
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
    // Precompute sorted unique incident nets per cell for fast deterministic merging.
    let mut cell_nets: Vec<Vec<usize>> = vec![Vec::new(); num_cells];
    for (c, nets_entry) in cell_nets.iter_mut().enumerate() {
        let start = design_db.cell_pin_offsets[c] as usize;
        let end = design_db.cell_pin_offsets[c + 1] as usize;
        for &p in &design_db.cell_pin_ids[start..end] {
            if let Some(net) = design_db.pin_nets[p.0 as usize] {
                nets_entry.push(net.0 as usize);
            }
        }
        nets_entry.sort_unstable();
        nets_entry.dedup();
    }

    let pin_geom = PinGeomCache::new(design_db, tech_db);
    let cell_pin_nets = build_cell_pin_nets(design_db, &pin_geom, num_cells);

    let mut pass1_eval_fallbacks = 0usize;
    let mut incident_buf: Vec<usize> = Vec::new();
    let mut other_xs_buf: Vec<i32> = Vec::new();
    let mut net_spans_buf: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut candidates_buf: Vec<i32> = Vec::new();
    for _round in 0..4 {
        // Pass 1: Equal-width cell swaps
        pass1_eval_fallbacks += equal_width_swap_pass_fast(
            design_db,
            tech_db,
            &pin_geom,
            &cell_pin_nets,
            &movable_cells,
            &cell_w_dbu,
            &cell_nets,
            &band_ys,
        );

        // Re-synchronize sub_row.cells by bucketing each movable cell into its exact sub_row.
        for sr in &mut sub_rows {
            sr.cells.clear();
        }
        for &c in &movable_cells {
            let cy = design_db.cell_y_dbu[c];
            let cx = design_db.cell_x_dbu[c];
            let cw = cell_w_dbu[c];
            if let Ok(b) = band_ys.binary_search(&cy) {
                let (sr_start, sr_end) = band_ranges[b];
                let slice = &mut sub_rows[sr_start..sr_end];
                let pos = slice.partition_point(|sr| sr.x_min_dbu <= cx);
                if pos > 0 {
                    let sr = &mut slice[pos - 1];
                    if cx + cw <= sr.x_max_dbu {
                        sr.cells.push(c);
                    }
                }
            }
        }
        for sr in &mut sub_rows {
            sr.cells.sort_unstable_by_key(|&c| design_db.cell_x_dbu[c]);
        }

        // Flat x-position cache for the x-only sweeps below; Pass 1 has already committed its
        // swaps, and every later accept refreshes the moved cells' entries.
        let mut pin_x = vec![0i32; design_db.num_pins()];
        for (p, slot) in pin_x.iter_mut().enumerate() {
            *slot = pin_geom.pin_pos(design_db, PinId(p as u32)).0;
        }

        // Pass 2: Adjacent sub-row cell swaps (single net scan per pair since only x shifts)
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

                merge_sorted_nets(&cell_nets[ca], &cell_nets[cb], &mut incident_buf);
                if incident_buf.is_empty() {
                    continue;
                }

                let dxa = new_xa - xa;
                let dxb = new_xb - xb;
                let ca_id = CellId(ca as u32);
                let cb_id = CellId(cb as u32);
                let mut hpwl_x_before = 0i64;
                let mut hpwl_x_after = 0i64;
                for &n in &incident_buf {
                    let pins = design_db.net_pins(NetId(n as u32));
                    if pins.len() <= 1 {
                        continue;
                    }
                    let (mut min_b, mut max_b) = (i32::MAX, i32::MIN);
                    let (mut min_a, mut max_a) = (i32::MAX, i32::MIN);
                    for &pin in pins {
                        let px = pin_x[pin.0 as usize];
                        min_b = min_b.min(px);
                        max_b = max_b.max(px);
                        let owner = design_db.pin_cells[pin.0 as usize];
                        let px_a = if owner == Some(ca_id) {
                            px + dxa
                        } else if owner == Some(cb_id) {
                            px + dxb
                        } else {
                            px
                        };
                        min_a = min_a.min(px_a);
                        max_a = max_a.max(px_a);
                    }
                    hpwl_x_before += (max_b - min_b) as i64;
                    hpwl_x_after += (max_a - min_a) as i64;
                }

                if hpwl_x_after < hpwl_x_before {
                    design_db.cell_x_dbu[ca] = new_xa;
                    design_db.cell_x_dbu[cb] = new_xb;
                    refresh_cell_pin_x(design_db, &cell_pin_nets, &mut pin_x, ca);
                    refresh_cell_pin_x(design_db, &cell_pin_nets, &mut pin_x, cb);
                    sr.cells.swap(j, j + 1);
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

                let incident = &cell_nets[c];
                if incident.is_empty() {
                    continue;
                }

                // Collect non-cell x positions + per-net (other_lo, other_hi, c_lo, c_hi) in one scan
                // so candidates evaluate without rescanning any net (y coordinates do not change).
                other_xs_buf.clear();
                net_spans_buf.clear();
                let c_id = CellId(c as u32);
                for &net in incident {
                    let pins = design_db.net_pins(NetId(net as u32));
                    if pins.len() <= 1 {
                        continue;
                    }
                    let (mut o_lo, mut o_hi) = (i32::MAX, i32::MIN);
                    let (mut c_lo, mut c_hi) = (i32::MAX, i32::MIN);
                    for &pin in pins {
                        let px = pin_x[pin.0 as usize];
                        if design_db.pin_cells[pin.0 as usize] == Some(c_id) {
                            c_lo = c_lo.min(px);
                            c_hi = c_hi.max(px);
                        } else {
                            other_xs_buf.push(px);
                            o_lo = o_lo.min(px);
                            o_hi = o_hi.max(px);
                        }
                    }
                    if c_lo <= c_hi {
                        net_spans_buf.push((o_lo, o_hi, c_lo, c_hi));
                    }
                }

                candidates_buf.clear();
                candidates_buf.push(left_limit);
                candidates_buf.push(right_limit);
                if !other_xs_buf.is_empty() {
                    let med_idx = other_xs_buf.len() / 2;
                    other_xs_buf.select_nth_unstable(med_idx);
                    let med = other_xs_buf[med_idx];
                    let snapped = snap_to_site(
                        med - wc / 2,
                        sr.orig_x_dbu,
                        sr.step_x_dbu,
                        left_limit,
                        right_limit,
                    );
                    candidates_buf.push(snapped);
                }

                candidates_buf.sort_unstable();
                candidates_buf.dedup();

                let eval_x_span = |dx: i32| -> i64 {
                    net_spans_buf
                        .iter()
                        .map(|&(o_lo, o_hi, c_lo, c_hi)| {
                            (o_hi.max(c_hi + dx) - o_lo.min(c_lo + dx)) as i64
                        })
                        .sum()
                };

                let mut best_x = orig_x;
                let mut best_span = eval_x_span(0);

                for &cand_x in &candidates_buf {
                    if cand_x == orig_x {
                        continue;
                    }
                    let h = eval_x_span(cand_x - orig_x);
                    if h < best_span {
                        best_span = h;
                        best_x = cand_x;
                    }
                }
                if best_x != orig_x {
                    design_db.cell_x_dbu[c] = best_x;
                    refresh_cell_pin_x(design_db, &cell_pin_nets, &mut pin_x, c);
                }
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
        pass1_eval_fallbacks,
    })
}

pub fn verify_legality(design_db: &DesignDb, tech_db: &TechDb) -> Result<()> {
    let site_height = tech_db.site_height_dbu;
    let num_cells = design_db.num_cells();

    let mut row_order: Vec<usize> = (0..design_db.rows.len()).collect();
    row_order.sort_by_key(|&r| design_db.rows[r].orig_y_dbu);
    let row_orig_ys: Vec<i32> = row_order
        .iter()
        .map(|&r| design_db.rows[r].orig_y_dbu)
        .collect();

    // 1. Verify all movable cells are Placed and on valid rows, and bucket all non-spacer cells
    // into overlapping rows in a single pass instead of scanning all cells per row.
    let mut row_cells: Vec<Vec<(i32, i32, usize)>> = vec![Vec::new(); row_order.len()];
    for c in 0..num_cells {
        let macro_id = design_db.cell_macros[c];
        if macro_id.0 as usize >= tech_db.macros.len() {
            continue;
        }
        let mac = &tech_db.macros[macro_id.0 as usize];
        let cy = design_db.cell_y_dbu[c];
        let cx = design_db.cell_x_dbu[c];
        let orient = design_db.cell_orient[c];
        let is_rot = matches!(orient, Orient::E | Orient::W | Orient::FE | Orient::FW);
        let (cw, ch) = if is_rot {
            (mac.height_dbu, mac.width_dbu)
        } else {
            (mac.width_dbu, mac.height_dbu)
        };

        let status = design_db.cell_status[c];
        if (mac.class == MacroClass::Core || mac.class == MacroClass::CoreTie)
            && status != PlacementStatus::Fixed
            && status != PlacementStatus::Cover
        {
            if status != PlacementStatus::Placed {
                bail!("Movable cell {} status is not Placed: {:?}", c, status);
            }

            let pos = row_orig_ys.partition_point(|&ry| ry < cy);
            if pos >= row_orig_ys.len() || row_orig_ys[pos] != cy {
                bail!("Cell {} y={} does not match any PlacementRow", c, cy);
            }
            // Match rows.iter().find() order when multiple rows share orig_y_dbu: stable sort keeps
            // the earliest row index first.
            let row = &design_db.rows[row_order[pos]];

            if orient != row.orient {
                bail!(
                    "Cell {} orient {:?} does not match row orient {:?}",
                    c,
                    orient,
                    row.orient
                );
            }

            if (cx - row.orig_x_dbu) % row.step_x_dbu != 0 {
                bail!(
                    "Cell {} x={} is not snapped to row site grid (orig_x={}, step={})",
                    c,
                    cx,
                    row.orig_x_dbu,
                    row.step_x_dbu
                );
            }

            let row_xh = row.orig_x_dbu + row.num_x * row.step_x_dbu;
            if cx < row.orig_x_dbu || cx + cw > row_xh {
                bail!(
                    "Cell {} [{}, {}] exceeds row horizontal bounds [{}, {}]",
                    c,
                    cx,
                    cx + cw,
                    row.orig_x_dbu,
                    row_xh
                );
            }
        }

        if mac.class == MacroClass::CoreSpacer {
            continue;
        }
        let min_ry = cy.saturating_sub(site_height);
        let lo = row_orig_ys.partition_point(|&ry| ry <= min_ry);
        let hi = row_orig_ys.partition_point(|&ry| ry < cy.saturating_add(ch));
        for row_vec in &mut row_cells[lo..hi] {
            row_vec.push((cx, cx + cw, c));
        }
    }

    // 2. Zero-overlap check across all rows
    for (k, cells) in row_cells.iter_mut().enumerate() {
        cells.sort_unstable_by_key(|&(x1, _, _)| x1);
        let row_yl = row_orig_ys[k];
        for i in 0..cells.len().saturating_sub(1) {
            let (x1_l, x1_h, c1) = cells[i];
            let (x2_l, _, c2) = cells[i + 1];
            if x1_h > x2_l {
                bail!(
                    "Overlap on row y={}: cell {} [{}, {}] overlaps cell {} [{}, {}] by {} DBU",
                    row_yl,
                    c1,
                    x1_l,
                    x1_h,
                    c2,
                    x2_l,
                    cells[i + 1].1,
                    x1_h - x2_l
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::{CellId, MacroId, NetId, PinDir, PinId, StringInterner, SymbolId};
    use crate::placement::nesterov::{NesterovConfig, run_nesterov_placement};
    use crate::readers::def::{parse_def, write_def};
    use crate::readers::lef::load_tech_db;
    use crate::tech_db::MacroDef;
    use crate::test_support::{design_dir, require, sky130_hd};

    #[test]
    fn test_gcd_place_and_legalize() {
        let tlef_path = sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef");
        let lef_path = sky130_hd().join("lef/sky130_fd_sc_hd.lef");
        let floorplan_def_path = design_dir("gcd").join("gcd.floorplan.def");

        if !tlef_path.exists() || !lef_path.exists() || !floorplan_def_path.exists() {
            panic!(
                "Required test files missing: tlef={:?}, lef={:?}, def={:?}",
                tlef_path, lef_path, floorplan_def_path
            );
        }

        let mut interner = StringInterner::new();
        let tech_db =
            load_tech_db(&tlef_path, &lef_path, &mut interner).expect("Failed to load TechDb");
        let mut design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        // 1. Run global placement
        let config = NesterovConfig::default();
        let gp_res = run_nesterov_placement(&design_db, &tech_db, &config);

        // 2. Run greedy Abacus legalization + local refinement
        let t0 = std::time::Instant::now();
        let leg_res = legalize_placement(
            &mut design_db,
            &tech_db,
            &gp_res.cell_x_um,
            &gp_res.cell_y_um,
        )
        .expect("Legalization must succeed");
        let elapsed = t0.elapsed();

        println!(
            "Legalization completed in {:?}: pre_legal_hpwl={} dbu, post_abacus_hpwl={} dbu, final_hpwl={} dbu, avg_disp={:.3} um, max_disp={:.3} um",
            elapsed,
            leg_res.pre_legal_hpwl_dbu,
            leg_res.post_abacus_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            leg_res.avg_displacement_um,
            leg_res.max_displacement_um
        );

        // 3. Verify legality self-check passes
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        // 4. Assert final HPWL is within 5% of reference placed_hpwl_dbu
        let ref_path = design_dir("gcd").join("gcd.reference.json");
        let ref_bytes = std::fs::read(&ref_path).expect("Reference JSON");
        let ref_json: serde_json::Value = serde_json::from_slice(&ref_bytes).expect("Parse JSON");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();

        let rel_diff = (leg_res.final_hpwl_dbu - ref_hpwl_dbu).abs() as f64 / ref_hpwl_dbu as f64;
        println!(
            "HPWL comparison: reference={} dbu, ours={} dbu, diff={:.2}%",
            ref_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            rel_diff * 100.0
        );
        assert!(
            rel_diff <= 0.05,
            "Legalized HPWL {} must be within 5% of OpenROAD {}, got {:.2}%",
            leg_res.final_hpwl_dbu,
            ref_hpwl_dbu,
            rel_diff * 100.0
        );

        // 5. Test DEF write and re-parse roundtrip
        let tmp_dir = std::env::temp_dir();
        let out_def_path = tmp_dir.join("grus_gcd_legalized_test.def");
        write_def(&design_db, &tech_db, &floorplan_def_path, &out_def_path)
            .expect("Failed to write legalized DEF");

        let reloaded_db =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");

        verify_legality(&reloaded_db, &tech_db).expect("Reloaded DEF must pass verify_legality");
        let reloaded_hpwl = reloaded_db.compute_hpwl_dbu(&tech_db);
        assert_eq!(
            reloaded_hpwl, leg_res.final_hpwl_dbu,
            "Reloaded DEF HPWL {} must match legalized HPWL {}",
            reloaded_hpwl, leg_res.final_hpwl_dbu
        );
    }

    // PDK-free pass 1 fixture: one 200 x 100 dbu Core macro, 12 cells in movable order.
    // Cell 0 sits at the origin; net 0 joins its pin to a fixed IO pin at (anchor_x, 0).
    // Cells 1..=10 are unconnected fillers far to the right. Cell 11, last in movable
    // order, is the only same-width partner near cell 0 and sits at `partner_xy`.
    fn pass1_fixture(anchor_x: i32, partner_xy: (i32, i32)) -> (DesignDb, TechDb) {
        let mut tech_db = TechDb::new();
        tech_db.site_height_dbu = 100;
        tech_db.macros.push(MacroDef {
            name: SymbolId(0),
            class: MacroClass::Core,
            width_dbu: 200,
            height_dbu: 100,
            pin_offset: 0,
            pin_count: 0,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        });

        let n = 12;
        let mut cell_x_dbu: Vec<i32> = (0..n as i32).map(|k| 200_000 + 1_000 * k).collect();
        let mut cell_y_dbu = vec![0; n];
        cell_x_dbu[0] = 0;
        cell_x_dbu[11] = partner_xy.0;
        cell_y_dbu[11] = partner_xy.1;

        let mut cell_pin_offsets = vec![1u32; n + 1];
        cell_pin_offsets[0] = 0;

        let mut db = DesignDb::new();
        db.cell_names = vec![SymbolId(0); n];
        db.cell_macros = vec![MacroId(0); n];
        db.cell_status = vec![PlacementStatus::Placed; n];
        db.cell_x_dbu = cell_x_dbu;
        db.cell_y_dbu = cell_y_dbu;
        db.cell_orient = vec![Orient::N; n];
        db.cell_pin_offsets = cell_pin_offsets;
        db.cell_pin_ids = vec![PinId(0)];
        db.pin_names = vec![SymbolId(0); 2];
        db.pin_dirs = vec![PinDir::Input; 2];
        db.pin_cells = vec![Some(CellId(0)), None];
        db.pin_macro_pins = vec![None; 2];
        db.pin_lib_pins = vec![None; 2];
        db.pin_nets = vec![Some(NetId(0)); 2];
        db.pin_offset_x_dbu = vec![0, anchor_x];
        db.pin_offset_y_dbu = vec![0, 0];
        db.pin_is_driver = vec![false; 2];
        db.io_pins = vec![PinId(1)];
        db.net_names = vec![SymbolId(0)];
        db.net_pin_offsets = vec![0, 2];
        db.net_pin_ids = vec![PinId(1), PinId(0)];
        (db, tech_db)
    }

    fn run_pass1(db: &mut DesignDb, tech_db: &TechDb) {
        let n = db.num_cells();
        let movable: Vec<usize> = (0..n).collect();
        let widths = vec![200; n];
        let mut nets = vec![Vec::new(); n];
        nets[0] = vec![0];
        equal_width_swap_pass(db, tech_db, &movable, &widths, &nets, &[0, 100]);
    }

    #[test]
    fn test_pass1_swaps_with_partner_far_in_movable_order() {
        // Anchor at x=20_000 pulls cell 0 right. The partner at (15_000, 100) is within
        // the window (|dx| = 15_000, |dy| = 100) and is 11 positions after cell 0.
        let (mut db, tech_db) = pass1_fixture(20_000, (15_000, 100));
        run_pass1(&mut db, &tech_db);
        assert_eq!((db.cell_x_dbu[0], db.cell_y_dbu[0]), (15_000, 100));
        assert_eq!((db.cell_x_dbu[11], db.cell_y_dbu[11]), (0, 0));
    }

    #[test]
    fn test_pass1_skips_partner_outside_x_window() {
        // The swap would cut net HPWL from 60_000 to 20_100, but the partner at
        // x=40_000 is 40_000 away, beyond the 25_000 dbu window, so pass 1 must not try it.
        let (mut db, tech_db) = pass1_fixture(60_000, (40_000, 100));
        run_pass1(&mut db, &tech_db);
        assert_eq!((db.cell_x_dbu[0], db.cell_y_dbu[0]), (0, 0));
        assert_eq!((db.cell_x_dbu[11], db.cell_y_dbu[11]), (40_000, 100));
    }

    // xorshift64: deterministic random inputs without a dev-dependency.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    fn pass1_tech() -> TechDb {
        let mut tech_db = TechDb::new();
        tech_db.site_height_dbu = 100;
        tech_db.macros.push(MacroDef {
            name: SymbolId(0),
            class: MacroClass::Core,
            width_dbu: 200,
            height_dbu: 100,
            pin_offset: 0,
            pin_count: 0,
            obs_rects: Vec::new(),
            pin_obs_rects: Vec::new(),
        });
        tech_db
    }

    // `cells[c]` is (x, y, orient). `pins[p]` is (cell, net, offset_x, offset_y) in pin id
    // order; a pin with no cell is a primary I/O port whose offset is absolute.
    fn build_pass1_db(
        cells: &[(i32, i32, Orient)],
        pins: &[(Option<usize>, Option<usize>, i32, i32)],
        num_nets: usize,
    ) -> DesignDb {
        let n = cells.len();
        let p = pins.len();
        let mut cell_pin_ids = Vec::new();
        let mut cell_pin_offsets = vec![0u32];
        for c in 0..n {
            cell_pin_ids.extend(
                (0..p)
                    .filter(|&i| pins[i].0 == Some(c))
                    .map(|i| PinId(i as u32)),
            );
            cell_pin_offsets.push(cell_pin_ids.len() as u32);
        }
        let mut net_pin_ids = Vec::new();
        let mut net_pin_offsets = vec![0u32];
        for net in 0..num_nets {
            net_pin_ids.extend(
                (0..p)
                    .filter(|&i| pins[i].1 == Some(net))
                    .map(|i| PinId(i as u32)),
            );
            net_pin_offsets.push(net_pin_ids.len() as u32);
        }

        let mut db = DesignDb::new();
        db.cell_names = vec![SymbolId(0); n];
        db.cell_macros = vec![MacroId(0); n];
        db.cell_status = vec![PlacementStatus::Placed; n];
        db.cell_x_dbu = cells.iter().map(|c| c.0).collect();
        db.cell_y_dbu = cells.iter().map(|c| c.1).collect();
        db.cell_orient = cells.iter().map(|c| c.2).collect();
        db.cell_pin_offsets = cell_pin_offsets;
        db.cell_pin_ids = cell_pin_ids;
        db.pin_names = vec![SymbolId(0); p];
        db.pin_dirs = vec![PinDir::Input; p];
        db.pin_cells = pins.iter().map(|q| q.0.map(|c| CellId(c as u32))).collect();
        db.pin_macro_pins = vec![None; p];
        db.pin_lib_pins = vec![None; p];
        db.pin_nets = pins
            .iter()
            .map(|q| q.1.map(|net| NetId(net as u32)))
            .collect();
        db.pin_offset_x_dbu = pins.iter().map(|q| q.2).collect();
        db.pin_offset_y_dbu = pins.iter().map(|q| q.3).collect();
        db.pin_is_driver = vec![false; p];
        db.io_pins = (0..p)
            .filter(|&i| pins[i].0.is_none())
            .map(|i| PinId(i as u32))
            .collect();
        db.net_names = vec![SymbolId(0); num_nets];
        db.net_pin_offsets = net_pin_offsets;
        db.net_pin_ids = net_pin_ids;
        db
    }

    fn random_pass1_db(rng: &mut Rng) -> DesignDb {
        // Coordinates come from a small grid so that ties at the extrema are common.
        let orients = [Orient::N, Orient::S, Orient::FN, Orient::FS];
        let n_cells = 3 + rng.below(6) as usize;
        let n_nets = 2 + rng.below(4) as usize;
        let cells: Vec<(i32, i32, Orient)> = (0..n_cells)
            .map(|_| {
                let x = 100 * rng.below(6) as i32;
                let y = 100 * rng.below(3) as i32;
                (x, y, orients[rng.below(4) as usize])
            })
            .collect();
        let mut pins = Vec::new();
        for c in 0..n_cells {
            for _ in 0..1 + rng.below(4) {
                let net = (rng.below(5) != 0).then(|| rng.below(n_nets as u64) as usize);
                let ox = 50 * rng.below(5) as i32;
                let oy = 50 * rng.below(3) as i32;
                pins.push((Some(c), net, ox, oy));
            }
        }
        for _ in 0..rng.below(3) {
            let net = rng.below(n_nets as u64) as usize;
            let x = 100 * rng.below(6) as i32;
            let y = 100 * rng.below(3) as i32;
            pins.push((None, Some(net), x, y));
        }
        build_pass1_db(&cells, &pins, n_nets)
    }

    fn incident_nets(db: &DesignDb, c1: usize, c2: usize) -> Vec<usize> {
        let mut nets: Vec<usize> = [c1, c2]
            .into_iter()
            .flat_map(|c| db.cell_pins(CellId(c as u32)).iter())
            .filter_map(|p| db.pin_nets[p.0 as usize])
            .map(|n| n.0 as usize)
            .collect();
        nets.sort_unstable();
        nets.dedup();
        nets
    }

    // Tries every cell pair as a swap, like pass 1 does, and checks the cached sums against
    // full scans on the same positions. An improving swap is accepted and the cache of its
    // incident nets rebuilt, so later trials start from the accepted state.
    // Returns the number of full rescans.
    fn differential_sweep(db: &mut DesignDb, tech_db: &TechDb) -> usize {
        let mut net_ext: Vec<NetExtrema> = Vec::new();
        for net in 0..db.num_nets() {
            net_ext.push(build_net_extrema(db, tech_db, net));
        }
        let mut eval = Pass1Eval::default();
        for c1 in 0..db.num_cells() {
            for c2 in (c1 + 1)..db.num_cells() {
                let incident = incident_nets(db, c1, c2);
                if incident.is_empty() {
                    continue;
                }
                let full_before: i64 = incident.iter().map(|&n| net_hpwl_dbu(db, tech_db, n)).sum();
                let (cached_before, cached_after) =
                    evaluate_trial_swap(db, tech_db, &net_ext, &incident, c1, c2, &mut eval);
                assert_eq!(cached_before, full_before, "before sum, pair ({c1}, {c2})");
                // The trial is applied here, so this is the full-scan after sum.
                let full_after: i64 = incident.iter().map(|&n| net_hpwl_dbu(db, tech_db, n)).sum();
                assert_eq!(cached_after, full_after, "after sum, pair ({c1}, {c2})");

                if cached_after < cached_before {
                    for &n in &incident {
                        net_ext[n] = build_net_extrema(db, tech_db, n);
                    }
                } else {
                    exchange_cells(db, c1, c2);
                }
            }
        }
        for (net, ext) in net_ext.iter().enumerate() {
            assert_eq!(
                cached_hpwl(ext),
                net_hpwl_dbu(db, tech_db, net),
                "final cache, net {net}"
            );
        }
        eval.fallbacks
    }

    #[test]
    fn test_cached_pass1_eval_matches_full_scan_on_random_designs() {
        let tech_db = pass1_tech();
        let mut total_fallbacks = 0;
        for seed in 1..=300u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut db = random_pass1_db(&mut rng);
            total_fallbacks += differential_sweep(&mut db, &tech_db);
        }
        assert!(
            total_fallbacks > 0,
            "random designs must exercise the full-scan fallback"
        );
    }

    #[test]
    fn test_cached_pass1_eval_falls_back_when_both_top2_pins_move() {
        // Net 0: A holds the two smallest x (0 and 10) and B holds 600. Swapping A and B moves
        // both top-2 pins, while C at x=1000 stays put. Only a full rescan of net 0 gives the
        // new minimum x=100. Trial positions are A at 100 (pins 100 and 110), B at 0.
        // Net 1 joins only A and B, so every pin on it moves and no rescan is needed.
        let tech_db = pass1_tech();
        let cells = [(0, 0, Orient::N), (100, 0, Orient::N), (1000, 0, Orient::N)];
        let pins = [
            (Some(0), Some(0), 0, 0),
            (Some(0), Some(0), 10, 0),
            (Some(1), Some(0), 500, 0),
            (Some(2), Some(0), 0, 0),
            (Some(0), Some(1), 0, 0),
            (Some(1), Some(1), 0, 0),
        ];
        let mut db = build_pass1_db(&cells, &pins, 2);
        let net_ext: Vec<NetExtrema> = (0..2)
            .map(|n| build_net_extrema(&db, &tech_db, n))
            .collect();
        let mut eval = Pass1Eval::default();

        let incident = incident_nets(&db, 0, 1);
        assert_eq!(incident, vec![0, 1]);

        let geom = PinGeomCache::new(&db, &tech_db);
        let cell_pin_nets = build_cell_pin_nets(&db, &geom, db.num_cells());
        let (x0, y0, o0) = (db.cell_x_dbu[0], db.cell_y_dbu[0], db.cell_orient[0]);
        let (x1, y1, o1) = (db.cell_x_dbu[1], db.cell_y_dbu[1], db.cell_orient[1]);
        let mut ro_eval = Pass1Eval::default();
        let (ro_before, ro_after) = evaluate_trial_swap_ro(
            &mut db,
            &geom,
            &net_ext,
            &cell_pin_nets[0],
            &cell_pin_nets[1],
            0,
            x0,
            y0,
            o0,
            1,
            x1,
            y1,
            o1,
            &mut ro_eval,
        );
        assert_eq!((ro_before, ro_after, ro_eval.fallbacks), (1100, 1000, 1));
        assert_eq!(
            db.cell_x_dbu[0], 0,
            "read-only evaluator must leave db unmutated"
        );

        let (cached_before, cached_after) =
            evaluate_trial_swap(&mut db, &tech_db, &net_ext, &incident, 0, 1, &mut eval);
        assert_eq!(cached_before, 1100);
        assert_eq!(cached_after, 1000);
        assert_eq!(eval.fallbacks, 1, "only net 0 needs a full rescan");

        // The trial is still applied, so the full scan sees the same positions as the cache.
        let full_after: i64 = incident
            .iter()
            .map(|&n| net_hpwl_dbu(&db, &tech_db, n))
            .sum();
        assert_eq!(full_after, 1000);
    }

    #[test]
    fn test_aes_place_and_legalize() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let floorplan_def_path = require(design_dir("aes").join("aes.floorplan.def"));

        let mut interner = StringInterner::new();
        let tech_db =
            load_tech_db(&tlef_path, &lef_path, &mut interner).expect("Failed to load TechDb");
        let mut design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        let gp_res = run_nesterov_placement(&design_db, &tech_db, &NesterovConfig::default());
        println!(
            "aes GP: converged={}, iterations={}, final_overflow={:.4}, final_hpwl_um={:.1}",
            gp_res.converged, gp_res.iterations, gp_res.final_overflow, gp_res.final_hpwl_um
        );
        assert!(
            gp_res.converged && gp_res.iterations < 300,
            "aes GP must converge in < 300 iterations"
        );

        let leg_res = legalize_placement(
            &mut design_db,
            &tech_db,
            &gp_res.cell_x_um,
            &gp_res.cell_y_um,
        )
        .expect("Legalization must succeed");
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        let ref_path = require(design_dir("aes").join("aes.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read aes.reference.json"))
                .expect("Parse aes.reference.json");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();
        let gap = (leg_res.final_hpwl_dbu - ref_hpwl_dbu) as f64 / ref_hpwl_dbu as f64;
        println!(
            "aes HPWL: reference={} dbu, ours={} dbu, gap={:+.2}%",
            ref_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            gap * 100.0
        );
        assert!(
            gap.abs() <= 0.05,
            "aes legalized HPWL gap {:.2}% exceeds 5%",
            gap * 100.0
        );

        let out_def_path = std::env::temp_dir().join("grus_aes_legalized_test.def");
        write_def(&design_db, &tech_db, &floorplan_def_path, &out_def_path)
            .expect("Failed to write legalized DEF");
        let reloaded_db =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
        verify_legality(&reloaded_db, &tech_db).expect("Reloaded DEF must pass verify_legality");
        assert_eq!(
            reloaded_db.compute_hpwl_dbu(&tech_db),
            leg_res.final_hpwl_dbu,
            "Reloaded DEF HPWL must match legalized HPWL"
        );
    }

    #[test]
    fn test_jpeg_place_and_legalize() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let floorplan_def_path = require(design_dir("jpeg").join("jpeg.floorplan.def"));

        let mut interner = StringInterner::new();
        let tech_db =
            load_tech_db(&tlef_path, &lef_path, &mut interner).expect("Failed to load TechDb");
        let mut design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        let gp_res = run_nesterov_placement(&design_db, &tech_db, &NesterovConfig::default());
        println!(
            "jpeg GP at target density 0.60: converged={}, iterations={}, final_overflow={:.4}, final_hpwl_um={:.1}",
            gp_res.converged, gp_res.iterations, gp_res.final_overflow, gp_res.final_hpwl_um
        );
        assert!(
            gp_res.converged && gp_res.iterations < 300,
            "jpeg GP must converge in < 300 iterations"
        );

        let leg_res = legalize_placement(
            &mut design_db,
            &tech_db,
            &gp_res.cell_x_um,
            &gp_res.cell_y_um,
        )
        .expect("Legalization must succeed");
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        let ref_path = require(design_dir("jpeg").join("jpeg.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read jpeg.reference.json"))
                .expect("Parse jpeg.reference.json");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();
        let gap = (leg_res.final_hpwl_dbu - ref_hpwl_dbu) as f64 / ref_hpwl_dbu as f64;
        println!(
            "jpeg HPWL: reference={} dbu, ours={} dbu, gap={:+.2}%",
            ref_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            gap * 100.0
        );
        assert!(
            gap.abs() <= 0.05,
            "jpeg legalized HPWL gap {:.2}% exceeds 5%",
            gap * 100.0
        );

        let out_def_path = std::env::temp_dir().join("grus_jpeg_legalized_test.def");
        write_def(&design_db, &tech_db, &floorplan_def_path, &out_def_path)
            .expect("Failed to write legalized DEF");
        let reloaded_db =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
        verify_legality(&reloaded_db, &tech_db).expect("Reloaded DEF must pass verify_legality");
        assert_eq!(
            reloaded_db.compute_hpwl_dbu(&tech_db),
            leg_res.final_hpwl_dbu,
            "Reloaded DEF HPWL must match legalized HPWL"
        );
    }

    #[test]
    fn test_ibex_place_and_legalize() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let floorplan_def_path = require(design_dir("ibex").join("ibex.floorplan.def"));

        let mut interner = StringInterner::new();
        let tech_db =
            load_tech_db(&tlef_path, &lef_path, &mut interner).expect("Failed to load TechDb");
        let mut design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        let gp_res = run_nesterov_placement(&design_db, &tech_db, &NesterovConfig::default());
        println!(
            "ibex GP at target density 0.60: converged={}, iterations={}, final_overflow={:.4}, final_hpwl_um={:.1}",
            gp_res.converged, gp_res.iterations, gp_res.final_overflow, gp_res.final_hpwl_um
        );
        assert!(
            gp_res.converged && gp_res.iterations < 300,
            "ibex GP must converge in < 300 iterations"
        );

        let leg_res = legalize_placement(
            &mut design_db,
            &tech_db,
            &gp_res.cell_x_um,
            &gp_res.cell_y_um,
        )
        .expect("Legalization must succeed");
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        let ref_path = require(design_dir("ibex").join("ibex.reference.json"));
        let ref_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ref_path).expect("read ibex.reference.json"))
                .expect("Parse ibex.reference.json");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();
        let gap = (leg_res.final_hpwl_dbu - ref_hpwl_dbu) as f64 / ref_hpwl_dbu as f64;
        println!(
            "ibex HPWL: reference={} dbu, ours={} dbu, gap={:+.2}%",
            ref_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            gap * 100.0
        );
        assert!(
            gap.abs() <= 0.05,
            "ibex legalized HPWL gap {:.2}% exceeds 5%",
            gap * 100.0
        );

        let out_def_path = std::env::temp_dir().join("grus_ibex_legalized_test.def");
        write_def(&design_db, &tech_db, &floorplan_def_path, &out_def_path)
            .expect("Failed to write legalized DEF");
        let reloaded_db =
            parse_def(&out_def_path, &tech_db, None).expect("Failed to re-parse written DEF");
        verify_legality(&reloaded_db, &tech_db).expect("Reloaded DEF must pass verify_legality");
        assert_eq!(
            reloaded_db.compute_hpwl_dbu(&tech_db),
            leg_res.final_hpwl_dbu,
            "Reloaded DEF HPWL must match legalized HPWL"
        );
    }

    #[test]
    fn test_picorv32_place_and_legalize() {
        let tlef_path = require(sky130_hd().join("techlef/sky130_fd_sc_hd__nom.tlef"));
        let lef_path = require(sky130_hd().join("lef/sky130_fd_sc_hd.lef"));
        let floorplan_def_path = require(design_dir("picorv32").join("picorv32.floorplan.def"));

        let mut interner = StringInterner::new();
        let tech_db =
            load_tech_db(&tlef_path, &lef_path, &mut interner).expect("Failed to load TechDb");
        let mut design_db =
            parse_def(&floorplan_def_path, &tech_db, None).expect("Failed to parse floorplan DEF");

        let gp_res = run_nesterov_placement(&design_db, &tech_db, &NesterovConfig::default());
        println!(
            "picorv32 GP: converged={}, iterations={}, final_overflow={:.4}, final_hpwl_um={:.1}",
            gp_res.converged, gp_res.iterations, gp_res.final_overflow, gp_res.final_hpwl_um
        );
        assert!(
            gp_res.converged && gp_res.iterations < 300,
            "picorv32 GP must converge in < 300 iterations"
        );

        let leg_res = legalize_placement(
            &mut design_db,
            &tech_db,
            &gp_res.cell_x_um,
            &gp_res.cell_y_um,
        )
        .expect("Legalization must succeed");
        verify_legality(&design_db, &tech_db).expect("verify_legality must pass");

        let ref_path = require(design_dir("picorv32").join("picorv32.reference.json"));
        let ref_json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&ref_path).expect("read picorv32.reference.json"),
        )
        .expect("Parse picorv32.reference.json");
        let ref_hpwl_dbu = ref_json["placed_hpwl_dbu"].as_i64().unwrap();
        let gap = (leg_res.final_hpwl_dbu - ref_hpwl_dbu) as f64 / ref_hpwl_dbu as f64;
        println!(
            "picorv32 HPWL: reference={} dbu, ours={} dbu, gap={:+.2}%",
            ref_hpwl_dbu,
            leg_res.final_hpwl_dbu,
            gap * 100.0
        );
        assert!(
            gap.abs() <= 0.05,
            "picorv32 legalized HPWL gap {:.2}% exceeds 5%",
            gap * 100.0
        );
    }

    #[test]
    fn test_trial_add_cell_x_matches_cluster_push() {
        let mut sr = SubRow {
            row_idx: 0,
            orig_x_dbu: 460,
            orig_y_dbu: 0,
            step_x_dbu: 460,
            orient: Orient::N,
            x_min_dbu: 460,
            x_max_dbu: 460 + 200 * 460,
            free_width: 200 * 460,
            clusters: Vec::new(),
            cells: Vec::new(),
        };

        // Pseudo-random deterministic stream of cell widths (multiples of site) and x_gp targets
        let mut rng = 0x9e37_79b9_7f4a_7c15u64;
        for c in 0..80usize {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let w_i = (((rng >> 16) % 5) as i32 + 1) * 460;
            if w_i > sr.free_width {
                continue;
            }
            // Bias some placements to collide and trigger multi-cluster cascading merges.
            let x_gp = sr.orig_x_dbu + ((((rng >> 32) % 90) as i32) * 460);
            let trial_x = trial_add_cell_x(w_i, x_gp, &sr);

            let sr_meta = SubRow {
                clusters: Vec::new(),
                cells: Vec::new(),
                ..sr
            };
            add_cell_to_clusters(&mut sr.clusters, c, w_i, x_gp, &sr_meta);
            sr.free_width -= w_i;
            let c_last = sr.clusters.last().unwrap();
            let actual_x = c_last.x_dbu + c_last.width_dbu - w_i;
            assert_eq!(
                trial_x, actual_x,
                "Mismatch at cell {} (x_gp={}, w={})",
                c, x_gp, w_i
            );
        }
    }
}
