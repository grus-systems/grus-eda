# perf-legal-passes — algorithm

All changes live in `crates/grus-core/src/placement/legalize.rs`. No
public API or CLI change. `LegalizeResult` gains one telemetry field
(`refine_extrema_rebuilds`).

## 1. Shared extrema cache

`net_ext: Vec<NetExtrema>` (the existing type: `pin_count` plus, per
axis `a ∈ {x=0, y=1}`, the two smallest `lo[a][0..2]` and two largest
`hi[a][0..2]` pin coordinates, each tagged with its pin;
`UNUSED_EXTREMUM` placeholders when `pin_count < 2`) is built **once**
after Abacus positions are committed — inside the existing
`place.legal.pre` scope, next to `pin_geom`/`cell_pin_nets`, with the
same `par_iter` build pass 1 uses today (`n_nets >= 1024`). It is then
threaded by `&mut` through the 4-round loop:

- pass 1: `equal_width_swap_pass_fast` takes `net_ext: &mut
  [NetExtrema]` instead of building it internally;
- pass 2 / pass 3 read and update it in place;
- at the end of pass 3 the cache is still exact, so round `k+1`'s
  pass 1 starts from a valid cache — the per-round full-net rebuild
  (`build_net_extrema_fast` over all nets ×4) is deleted entirely.

`pin_x` (the flat per-pin x cache for the x-only sweeps) is unchanged:
rebuilt at each `place.legal.resync` and refreshed on accepts by
`refresh_cell_pin_x`. Pass 1 does not use `pin_x`.

## 2. Incremental update rule (the core mechanism)

After a move is **committed** to `design_db`, for every net `n`
incident to a moved cell, with `moved` = `(pin, new_x, new_y)` for
each pin of the moved cell(s) on `n` (passes 2/3 may pass `new_y` =
current `pin_pos` y — only `x` is consumed for them):

```
if ext.pin_count < 2: return
for each axis a in moved_axes:           # pass 1: {x, y}; passes 2/3: {x}
    if any moved pin p == ext.lo[a][s].pin or p == ext.hi[a][s].pin
       for s in {0, 1}:
        net_ext[n] = build_net_extrema_fast(db, geom, n)   # full rescan
        rebuilds += 1; return
for each axis a in moved_axes:
    for (p, v) in moved, v = new coordinate on axis a:     # deterministic order
        lo: if v < lo[a][0].val { lo[a][1] = lo[a][0]; lo[a][0] = (v, p) }
            else if v < lo[a][1].val { lo[a][1] = (v, p) }
        hi: if v > hi[a][0].val { hi[a][1] = hi[a][0]; hi[a][0] = (v, p) }
            else if v > hi[a][1].val { hi[a][1] = (v, p) }
```

Helper sketch:

```rust
/// Exact incremental `NetExtrema` maintenance after committed moves.
/// Returns true when the net's extrema were rebuilt by full rescan.
fn update_net_extrema(
    db: &DesignDb, geom: &PinGeomCache, net_ext: &mut Vec<NetExtrema>,
    net: usize, moved: &[(PinId, i32, i32)], move_y: bool,
) -> bool
```

### Exactness

Invariant: each axis's top-2 slots hold the true two smallest (resp.
largest) *values* among the net's pins, each tagged with a pin holding
that value.

- A pin not in a slot satisfies `x_old >= lo[1].val` (resp. `<= hi[1]`),
  so removing it cannot change the top-2 on that side. Inserting its
  new value with the strict-`<` rule restores the invariant — ties need
  no insert: an extremal *value* already present is covered by the
  existing slots.
- A pin in a slot forces the rescan branch — which produces the true
  top-2 by construction (same builder used everywhere today).
- Membership is checked on **pin identity** against the pre-update
  slots; the (stale) values are exactly the pre-move extrema, which is
  what the non-extremal argument requires.
- Pin tags may differ from a fresh build on multi-way value ties
  (which of the tied pins sits in the slots). Slot *values* are
  unaffected; a moved tied pin not in the slots is treated as
  non-extremal, which the argument above shows is still exact.
  Consequence: fallback/rebuild *frequency* can differ from a
  rebuild-everything scheme — never a computed value.

Worst case is the old behavior: every moved pin extremal → a rescan
per incident net per accept, i.e. today's pass-1 accept cost, while
evaluations stay cheap. No asymptotic regression.

### Evaluation helpers

```rust
/// Extremum over the net's pins not owned by `excl1`/`excl2`
/// (pass the same cell twice for single-cell exclusion), or None
/// when both top-2 pins are excluded — caller rescans.
fn surviving_extremum_owner(top: &[Extremum; 2], excl1: u32, excl2: u32) -> Option<i32>

/// Full-scan lo/hi x-extrema over net pins not owned by either cell.
/// Fallback for `surviving_extremum_owner` == None.
fn net_other_x_extrema(db: &DesignDb, pin_x: &[i32], net: usize,
                       excl1: u32, excl2: u32) -> (i32, i32)   // (i32::MAX, i32::MIN) when empty
```

Exclusion is by owner cell id only: an IO pin's owner is `u32::MAX`
(`pin_cells == None`), so `u32::MAX` must never be passed as an
exclusion — pass `excl2 = excl1` for the single-cell cases.
`net_other_x_extrema` filters via `db.pin_cells[pin] == Some(CellId)`
comparisons, which `None` (IO) never matches.

(The existing `surviving_extremum_pos` checks moved-pin *membership*;
for x-only moves "moved" ≡ "owned by the moving cell(s)", so owner
checks are equivalent and simpler — every pin of a moved cell moves.)

## 3. Pass 1 — equal-width swaps (`equal_width_swap_pass_fast`)

Changes:

1. `net_ext` arrives built; the per-round build block is removed.
2. Band index rekeyed: `BandEntry = (w, x, mi, cell)` sorted, so the
   candidate scan per cell is one `partition_point` pair per band over
   `[ (w1, x1−25_000) .. (w1, x1+25_000+) ]` — same-width entries only.
   `band_remove`/`band_insert` binary-search the same tuple on accept.
   The collected set is identical (the in-loop width recheck already
   guaranteed it); order is identical (`candidates.sort_unstable()` by
   `mi` then visit).
3. Accept path: after `exchange_cells`, instead of
   `net_ext[n] = build_net_extrema_fast(...)` per incident net,
   re-enumerate the moved pins — for each incident net `n` (the
   existing `merge_sorted_nets(&cell_nets[c1], &cell_nets[c2])` merge),
   slice each cell's pin group out of `cell_pin_nets[c]` (sorted by
   `(net, pin)` → `partition_point`), compute `(pin, px, py)` via
   `pin_pos_at_cell` at the post-swap positions/orients, and call
   `update_net_extrema(.., move_y = true)`.
4. `evaluate_trial_swap_ro`, `Pass1Eval`, `pass1_eval_fallbacks`, the
   `mi2 > i` / live-recheck / break-on-accept structure: unchanged.

Result: pass-1 output is bit-identical by construction — same
candidates, same evaluated values, same accepts.

## 4. Pass 2 — adjacent sub-row swaps

The `merge_sorted_nets` incident merge, the exact-fit gate
(`new_xa + wa > xb + wb → skip`), `sr.cells.swap` and `pin_x` refresh
are unchanged. Only the per-net evaluation changes.

For each incident net `n` (skip `pin_count <= 1`, as today):

- `before` contribution = `ext.hi[0][0].val − ext.lo[0][0].val`
  (identical to the old `max_b − min_b`).
- moved pins = the `ca`/`cb` groups on `n` in `cell_pin_nets` (each
  typically 1–2 pins), with `new_x = pin_x[pin] + dx{None => a,b}`.
- if `moved.len() < ext.pin_count`: `lo = surviving_extremum_owner(
  ext.lo[0], ca, cb)`, `hi = surviving_extremum_owner(ext.hi[0], ca,
  cb)`; if either is `None` → `net_other_x_extrema` rescan of `n`
  (fallback — rare); then fold `min/max` over the moved pins' `new_x`.
  Else (`moved.len() == pin_count`): extrema over moved `new_x` alone.

`hpwl_x_after < hpwl_x_before` → commit `cell_x_dbu`, then per incident
net `update_net_extrema(.., move_y = false)` with moved =
`(pin, pin_x[pin]+dx, _)`, then `refresh_cell_pin_x` for `ca`,`cb`,
then `sr.cells.swap(j, j+1)` — same order as today.

Result: every evaluated value equals the old full-scan value →
bit-identical accepts and output.

## 5. Pass 3 — single-cell slide

Per cell `c` in `sr.cells` order; `left_limit`/`right_limit` and the
`right_limit <= left_limit → skip` and `incident.is_empty() → skip`
gates unchanged.

### Span construction (replaces the full pin scans)

For each `net` in `cell_nets[c]` (skip `pin_count <= 1`, equivalently
`net_pins` of len ≤ 1 as today):

- `(c_lo, c_hi)` = min/max of `pin_x` over `c`'s own group on `net` in
  `cell_pin_nets[c]` — `c` always has ≥ 1 pin on an incident net, so
  `c_lo <= c_hi` always (same push condition as today).
- `(o_lo, o_hi)` over the *other* pins:
  `surviving_extremum_owner(ext.lo[0], c, c)` and the mirror on
  `ext.hi[0]`; if either is `None`:
  - if `c`'s group on `net` has `pin_count` entries (all pins are
    `c`'s) → `(i32::MAX, i32::MIN)` directly (constant contribution,
    same as today);
  - else `net_other_x_extrema(db, pin_x, net, c, c)` rescan.
- push `(o_lo, o_hi, c_lo, c_hi)` to `net_spans_buf` — same tuple,
  same `eval_x_span` closure as today.

### Candidates: median → exact site-grid argmin

`f(dx) = Σ_n (o_hi.max(c_hi+dx) − o_lo.min(c_lo+dx))` is a sum of
convex piecewise-linear terms. Per net there are exactly two kinks,
at `dx = o_lo − c_lo` and `dx = o_hi − c_hi` (where each term's slope
changes); nets with `o_lo > o_hi` contribute a constant and are
skipped for kinks. `f`'s minimizer region endpoints are kinks of `f`,
hence in this set.

Candidates:

```
left_limit, right_limit,
for each kink target t = orig_x + (o_lo − c_lo | o_hi − c_hi):
    snap_down(t).clamp(left_limit, right_limit),
    snap_up(t).clamp(left_limit, right_limit)
```

then `sort_unstable` + `dedup`, evaluate each with `eval_x_span`,
keep the best under the existing strict-`<` rule over `eval_x_span(0)`.

Exactness of the site-grid argmin: if the minimizer region `[d1, d2]`
contains no site, the optimal site is `snap_down` or `snap_up` of
`d1` (equivalently `d2`); if it contains sites, `snap_up(d1)` is the
least site `>= d1` and lies inside. Either way the pair
`{snap_down, snap_up}` at every kink plus the two limits covers the
site-grid optimum exactly — proven by the
`test_pass3_candidates_hit_site_argmin` brute-force test
([`verification.md`](verification.md)), not by this paragraph alone.

Dominance: the old candidate set (limits + snapped median of all
other-pin x) sampled `f` at 3 points; the new set contains the true
site minimizer, so per cell the committed `best_span` is ≤ the old
choice's — a weakly better local move for the same objective. (This
is the one deliberate semantic change: move order may differ from
v0.3.4, allowed by the spec's equivalence bar. Global HPWL is gated
by the before/after table in [`measurement.md`](measurement.md).)

On accept: `cell_x_dbu[c] = best_x`; per incident net with
`pin_count >= 2`, `update_net_extrema` with moved = `c`'s pins on the
net at `pin_x + dx` (`move_y = false`); `refresh_cell_pin_x`.
`other_xs_buf` is deleted (no half-kept state).

## 6. Plumbing summary

- `legalize_placement`: build `net_ext` inside `place.legal.pre` (next
  to `cell_pin_nets`); declare `let mut net_ext` before the round loop;
  pass `&mut net_ext` to pass 1; pass 2/3 update it in place.
- `equal_width_swap_pass_fast` signature gains `net_ext: &mut
  [NetExtrema]`; the `#[cfg(test)]` `equal_width_swap_pass` wrapper
  keeps its signature and builds the cache internally.
- `LegalizeResult` gains `refine_extrema_rebuilds: usize` (sum of
  `update_net_extrema` rescan returns over the whole run). F5 set the
  precedent for this kind of telemetry (`pass1_eval_fallbacks`); the
  `place` CLI line is unchanged.
- `#[cfg(test)]` paths (`equal_width_swap_pass`,
  `evaluate_trial_swap`, `net_hpwl_after_swap`, `differential_sweep`,
  fixtures) stay working — the differential sweep remains the
  value-exactness gate for pass 1's evaluator.

## 7. What does not change

Round count (4), all thresholds (`SWAP_MAX_DX_DBU = 25_000`,
`max_dy = 4 site heights`, `max_y_dist = 10` in Abacus), the strict-`<`
accept rule, visit orders (movable order / `sr.cells` order),
resync/`pin_x` rebuild, `verify_legality`, and every phase key.
The passes remain serial; the only `par_iter` is the one-time
extrema build (deterministic: independent per-net map + ordered
collect).
