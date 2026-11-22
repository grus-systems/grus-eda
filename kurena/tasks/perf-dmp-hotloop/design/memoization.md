# perf-dmp-hotloop — M1: run-scoped exact-key memo of `driver_response`

## Shape

```rust
// analysis.rs — module scope, next to Context
thread_local! {
    static DRV_MEMO: RefCell<HashMap<DrvKey, (f32, f32)>> = RefCell::new(HashMap::new());
}
static DRV_MEMO_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DrvKey {
    epoch: u64,
    net: u32,
    pol: u8,
    in_slew: u32,       // in_slew.to_bits()
    delay_table: usize, // `delay_table as *const NldmTable as usize`
    slew_table: usize,
}
```

`Context` gains `memo_epoch: u64`, filled at every construction
site with `DRV_MEMO_EPOCH.fetch_add(1, Ordering::Relaxed)` — one
site in `prepare_sta` plus the `Context { .. }` literals in tests
(~7, all mechanical). `driver_response` becomes:

1. build `key` (cheap: two pointer casts + `to_bits`);
2. `DRV_MEMO.with(|m| m.borrow().get(&key).copied())` → hit:
   return the stored `(delay, slew)` pair;
3. miss: run the **unchanged** existing body, `insert`, return.

No signature change; all five call sites benefit
(`propagate_slew`, `propagate_clock`, `eval_sta` incl. every
partition class, `slew_eval`, `driver_input_slew`).

## Why the key is complete

Within one `Context` lifetime the result depends only on:

- `pi_models[net][pol]` / `net_wire_loads[net]` — built once in
  `prepare_sta`, immutable afterwards (covered by `net`+`pol`);
- `parasitics`, `lib` pin caps, `sdc` output loads — immutable
  inputs to the run (reached through `net`/`pol` and the table
  pointers);
- `delay_table`/`slew_table` contents — keyed by pointer identity;
  `NldmTable`s live in `lib` for the run's duration, so pointer ⇒
  contents;
- `in_slew` — keyed bitwise via `to_bits()`; `-0.0`/`NaN` are
  distinct keys, matching the function's own behavior on them.

The `epoch` field makes the cache scoped exactly to one `Context`:
a second `prepare_sta` (dual-corner hold run, `sta_pin_slews`
after `run_sta`, sequential tests) gets a fresh epoch, so a
recycled `NldmTable` heap address can never alias a stale entry.
Without it, correctness would rest on allocator addresses not
being reused — rejected.

## Bit-identity argument

`driver_response` is a pure function of its key within an epoch
(no mutable input: the walks mutate `state`, never the tables,
pi models, or loads it reads). A hit returns the exact `f32` pair
the miss path computed — not a recomputation, not an
approximation — so every downstream consumer sees bitwise the
same values regardless of hit/miss pattern, thread, or thread
count. `RAYON_NUM_THREADS` 1/4/10 equivalence holds by
construction and is verified empirically
([`verification.md`](verification.md)).

## Threading

`thread_local!` per rayon worker — `timing.rs`'s `PHASES` uses the
same pattern. Each worker caches only what it computed; a key
queried on two workers is computed twice and stored twice —
**identical bitwise result either way**, no shared-mutable state
crosses threads (spec: "thread-local or per-level-scoped — no
cross-thread shared cache that serializes"). `Mutex`/`DashMap`/
`map_init` rejected: the first serializes, the last forfeits
cross-level and cross-walk hits (the largest repeat pool: eval
classes and `driver_input_slew` recursion live across levels).

Memory: ≈ 100 B/entry (40 B key + 8 B value + map overhead) per
worker. A jpeg run's unique-key count lands in the counts run;
expected ≤ a few ×10⁵ entries → tens of MB worst case, acceptable
on the measurement machine. If the counts show a much larger
unique set, add a soft insert cap — record the decision.

## M2 — per-call `table` memo (conditional companion)

Inside `driver_response`, replace the `table` closure with a
`c.to_bits()`-keyed memo (`HashMap<u64, (f64, f64)>` local to the
call — single-threaded, no sync). Dedupes `is_lumped_bailout`'s
`table(total)` + `delay_slope(total)` evaluated by both
`solve_dmp` and `driver_waveform_slew`, `residual(hi)`, and the
`rd_of` slope re-evaluation — all bitwise-identical since
`table(c)` is pure. Small win (`eval_2d` is cheap); implement only
if [`counts.md`](counts.md) shows the intra-call repeat share or
`eval_2d` site cost warrants it.

## Explicitly not candidates

- **Tolerance-keyed / quantized cache** — banned by spec: changes
  results.
- **Brent bracket tightening or wider closed-form coverage in
  `rc_crossing`** — returns a different f64 (the approximation,
  not the root, is what's stored); `cmp`-dirty `--json`. The
  bisection share is recorded as residual instead.
- **Caching `solve_dmp`/`driver_waveform_slew` separately** —
  `driver_response`-level keys cover both (plus the lumped path)
  with strictly less code.
- **`Rc`/`Arc` table handles or interning** — pointer identity is
  sufficient and zero-cost.
