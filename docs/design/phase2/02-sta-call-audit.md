# STA call audit: driver_response composition and level scheduling

## Goal

After ticket 01 lands (or independently, it is read-only measurement plus at
most small changes), account for the 448,726 `driver_response` calls on jpeg:
which call sites produce them (`eval_launch`, comb arcs with
`slew_min == slew_max`, comb arcs with `s_min != s_max`), and whether any
calls are provably identical (same `net`, `dst_pol`, table pair, `in_slew`
bits) and can be deduplicated exactly. Also audit level widths across designs
and re-check the `level.len() < 32` sequential threshold now that per-call
cost is known.

## Files
- crates/grus-core/src/sta/analysis.rs
- crates/grus-core/src/sta/dmp.rs (measurement only if needed)

## Interface

No public API change.

## Acceptance

- A short write-up appended to `docs/results.md` (or `docs/design/phase2/`)
  with: calls per call site and per polarity pair; count of exact-duplicate
  keys if a hash probe is added temporarily; level width histogram per design
  (at least gcd, aes, jpeg).
- Any dedup implemented must be exact (full `f32`/`f64` key, no quantization)
  and locked by the existing STA tests.
- `cargo test --release -p grus-core sta::` green;
  `cargo clippy --all-targets -- -D warnings` clean.
- Temporary instrumentation must be removed before commit.

## Notes

- Prior reasoning says exact repeats are rare (each call site's `in_slew`
  differs); confirm with the probe before writing any cache. If the duplicate
  rate is under ~1%, do not implement a cache — record the number and stop.
- jpeg has 33 levels, all `>= 32` pins, so the `< 32` branch never runs there;
  unconditional `par_iter` on jpeg is a no-op experiment. Narrow levels live
  in the smaller designs — if the histogram shows levels `< 32` dominating
  wall time on gcd/aes/ibex after ticket 01, test raising (not removing) the
  threshold on those designs and compare. Keep whichever is faster; results
  must be identical (the eval is already pure w.r.t. `state` within a level).
  Note rayon dispatch overhead grows in relative importance as per-pin cost
  drops, so measure rather than assume.
