# low-sweep — code nits

All fixes are provably no-ops: comment/structure changes, or a borrow
split that keeps the exact call order and result.

## C1 — `sta/analysis.rs`: share the slew-state dispatch

`sta_pin_slews` (~L2297) and the test helper `assert_pin_slew_parity`
(~L10003) both open-code the same choice:

```rust
if uncovered_borrow(&prepared) {
    eval_state_full(&prepared)?   // test: .expect("full-state fallback")
} else {
    slew_eval(&prepared)          // wrapped in Ok / bare
}
```

Extract one private helper next to `eval_state_full`/`slew_eval`
(~L3403–3520):

```rust
/// `sta_pin_slews`'s state choice: the slew replay, or the full eval
/// tail when an uncovered `latch_borrow` edge makes a slew
/// arrival-coupled. Shared with the parity test, which compares the
/// returned state's `slew_min` field by field.
fn slew_eval_state(prepared: &StaPrepared) -> Result<Vec<PinTimingState>> {
    if uncovered_borrow(prepared) {
        eval_state_full(prepared)
    } else {
        Ok(slew_eval(prepared))
    }
}
```

Call sites become `let state = slew_eval_state(&prepared)?;` and
`let replay = slew_eval_state(&prepared).expect("slew state");`.
Placement: after `slew_eval`'s definition so the doc comment can
reference both arms. If the implementer finds a clearer name
(`slew_state` is free), pick it — the point is the single dispatch.

## C2 — `sta/dmp.rs`: split `memo_table`'s borrow across `table(c)`

Current (~L494–503) holds `entries.borrow_mut()` through the `table(c)`
call. Safe today (the wrapped `table` never reaches the `RefCell`) but
one re-entrant table away from a borrow panic. Split it:

```rust
move |c| {
    let bits = c.to_bits();
    // Lookup and insert borrow separately so a `table` that ever reaches
    // this RefCell re-entrantly cannot hit a held `borrow_mut`.
    if let Some(&(_, d, s)) = entries.borrow().iter().find(|e| e.0 == bits) {
        return (d, s);
    }
    let (d, s) = table(c);
    entries.borrow_mut().push((bits, d, s));
    (d, s)
}
```

Identical semantics — `entries` cannot grow between the miss and the
push because `table` does not receive `entries` (the comment now names
that as the invariant). `memo_table_hits_are_bitwise_exact` still
covers the behavior.

## C3 — `placement/legalize.rs`: three pass-2/3 nits

### (a) `new_y` placeholder contract — comment only

`update_net_extrema` (~L475) already documents "`move_y = false` marks
an x-only move (passes 2/3 never touch y)". Extend the sentence:
"`move_y = false` marks an x-only move (passes 2/3 never touch y) —
the tuples' `new_y` element is then unread, so those call sites push a
placeholder `0`." Applies to the pass-2 eval build (~L1447/1450), the
pass-2 accept rebuild (~L1479/1482), and the pass-3 rebuild (~L1593);
add one short note at the first pass-2 build site pointing at the
contract (later sites are self-evident from the same pattern).

### (b) gate `moved_buf` builds on `pin_count`

`update_net_extrema` early-returns on `ext.pin_count < 2`, but the three
accept loops build `moved_buf` (a `cell_pins_on_net` scan per cell) for
those nets anyway:

- pass 1, `for &n in &incident` (~L916)
- pass 2, `for &n in &incident_buf` (~L1476)
- pass 3, `for &net in incident` (~L1590)

Add `if net_ext[n].pin_count <= 1 { continue; }` (pass-3 var is `net`)
at the top of each loop, matching the `<= 1` phrasing the neighbouring
eval loops already use. Provably identical: the skipped iterations only
filled the buffer for a call that returned `false` (0 rebuilds).

### (c) pass-1 width recheck is structurally unreachable — `debug_assert`

At ~L887: `if cell_w_dbu[c2] != w1 { continue; }`. Candidates come from
`band[lo..hi]` where `lo`/`hi` are partition bounds keyed
`(w1, x1 ± SWAP_MAX_DX_DBU, …)` — every entry in the slice has width
exactly `w1` (`bands` stores `cell_w_dbu[c]`; widths are immutable, and
`band_remove`/`band_insert` re-key with the same width on accepts). The
check can never fire. Replace the guard with:

```rust
// Same width is guaranteed by the band key — the partition bounds
// select `(w1, x…)` entries only; the assert pins the invariant.
debug_assert!(cell_w_dbu[c2] == w1);
```

Release output identical (assert compiles out; the `continue` was dead).

## C4 — `bool_expr.rs`: `sensitivity` doc line on the op budget

`prob`'s body already carries the required comment (~L338–340): the
budget resets per call and one gate eval can spend
`(1 + n_inputs) * MAX_OPS`. Add one sentence to `sensitivity`'s doc
comment (~L348) so the bound is visible at the API, not only inside
`prob`: "`prob`'s `MAX_OPS` budget resets per call, so a single
`sensitivity` — and a gate's `(1 + n_inputs)` `prob`+`sensitivity`
sweep — has a per-call work bound, not a shared per-gate budget." No
code change. (`bool_expr.rs` sits just outside the spec's file list —
one doc line, flagged here.)
