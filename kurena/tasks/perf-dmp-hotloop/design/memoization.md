# perf-dmp-hotloop — M2: per-call exact-key memo of the `table` closure

## Counts verdict (2026-11-22, see [`overview.md`](overview.md))

- **M1 (run-scoped `driver_response` memo): rejected.** The pi-path
  exact-repeat share is `R = 0.0%` — 897,452 calls on jpeg `sta`,
  every key bitwise-unique. A run-scoped memo scores zero hits;
  the code below is kept only as the rejected design record.
- **M2 (per-call `table` memo): shipped.** Intra-call `table(c)`
  repeats are 21.6% of `table` evals on jpeg `sta`/`power` and
  21.8% on ibex — `table(total)` ×3, `table(total ± 5%)` ×2–3, and
  the final `lumped(ceff)` re-evaluate points the same call already
  read.

## M2 shape

`dmp.rs`:

```rust
pub fn memo_table<'a>(
    entries: &'a RefCell<Vec<(u64, f64, f64)>>,
    table: DriverTable<'a>,
) -> impl Fn(f64) -> (f64, f64) + 'a {
    move |c| {
        let bits = c.to_bits();
        let mut e = entries.borrow_mut();
        if let Some(&(_, d, s)) = e.iter().find(|e| e.0 == bits) {
            return (d, s);
        }
        let (d, s) = table(c);
        e.push((bits, d, s));
        (d, s)
    }
}
```

`driver_response`'s pi branch wraps its `table` closure in
`memo_table`; `solve_dmp` and `driver_waveform_slew` share the one
wrapped closure, so the dedupe covers `is_lumped_bailout` in both
plus `residual(hi)` and the `rd_of` slopes.

- **Key**: `c.to_bits()` — bitwise exact; `-0.0`/`NaN` keep
  distinct keys matching the pure function's own behavior. No
  quantization (spec bar).
- **Container**: `RefCell<Vec>` with linear scan, not `HashMap` —
  ~22 distinct `c` per call (24.9M evals / 897k calls), so a scan
  of ≤ ~40 `u64` compares beats a SipHash lookup that costs about
  as much as the `eval_2d` it saves.
- **Scope**: created per `driver_response` call — the counts show
  cross-call bitwise repeats are zero, so a wider scope buys
  nothing. No shared-mutable state crosses rayon threads (each
  call owns its `RefCell`; the memo is never aliased across
  threads).
- **Bit-identity**: `table` is a pure function of `c` (two
  `eval_2d` lookups of immutable `NldmTable`s at fixed `in_slew`);
  a hit returns the stored pair — not a recomputation — so every
  consumer sees bitwise the same values regardless of hit/miss
  pattern, thread, or `RAYON_NUM_THREADS`.

## Rejected: M1 — run-scoped exact-key memo of `driver_response`

`R = 0.0%` in the counts. The repeat pools the design hypothesised
(partition classes re-issuing identical queries, `slew_eval`
sharing eval's keys, `driver_input_slew` upstream reuse) do not
exist bitwise: `slew_max`/`slew_min` at a pin differ almost
everywhere, so `(net, pol, tables, in_slew.to_bits())` never
collides — 897k calls, 897k unique keys on jpeg. Kept for the
record:

<details>
<summary>M1 design (not shipped)</summary>

```rust
thread_local! {
    static DRV_MEMO: RefCell<HashMap<DrvKey, (f32, f32)>> = RefCell::new(HashMap::new());
}
static DRV_MEMO_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DrvKey {
    epoch: u64,
    net: u32,
    pol: u8,
    in_slew: u32,
    delay_table: usize,
    slew_table: usize,
}
```

`Context` would gain `memo_epoch` from `DRV_MEMO_EPOCH.fetch_add`;
`driver_response` would key → lookup → miss-compute → insert.
Bit-identical by construction, thread-local, ~100 B/entry. Dead
because the hit count is zero — the epoch/pointer-key design was
never exercised.

</details>

## Explicitly not candidates

- **Tolerance-keyed / quantized cache** — banned by spec: changes
  results.
- **Brent bracket tightening or wider closed-form coverage in
  `rc_crossing`** — returns a different f64 (the approximation,
  not the root, is what's stored); `cmp`-dirty `--json`. The
  measured 43.7% Brent share is recorded as residual instead.
- **Caching `solve_dmp`/`driver_waveform_slew` separately** —
  `driver_response`-level keys cover both (plus the lumped path)
  with strictly less code; moot anyway with `R = 0`.
- **M4 `eval_2d` hardening** — ~6% leaf share in `sample`, below
  the ~20% gate.
- **`Rc`/`Arc` table handles or interning** — pointer identity is
  sufficient and zero-cost.
