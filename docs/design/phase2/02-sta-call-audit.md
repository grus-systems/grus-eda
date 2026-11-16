# STA call audit: driver_response composition and level scheduling

## Goal

After ticket 01 lands (or independently, it is read-only measurement plus at
most small changes), account for the 448,726 `driver_response` calls on jpeg:
which arc timing types, polarity pairs and slew variants produce them, and
whether any calls are provably identical (same `net`, `dst_pol`, table pair,
`in_slew` bits) and can be deduplicated exactly. Also re-check the
`level.len() < 32` sequential threshold now that per-call cost is known.

## Files
- crates/grus-core/src/sta/analysis.rs
- crates/grus-core/src/sta/dmp.rs (measurement only if needed)

## Interface

No public API change.

## Acceptance

- A short write-up appended to `docs/results.md` (or `docs/design/phase2/`)
  with: calls per arc type, per polarity pair, per slew variant (max-only /
  min-only / both); count of exact-duplicate keys if a hash probe is added
  temporarily; level width histogram.
- Any dedup implemented must be exact (full `f32`/`f64` key, no quantization)
  and locked by the existing STA tests.
- `cargo test --release -p grus-core sta::` green; `cargo clippy` clean.

## Notes

- Prior reasoning says exact repeats are rare (each call site's `in_slew`
  differs); confirm with the probe before writing any cache. If the duplicate
  rate is under ~1%, do not implement a cache — record the number and stop.
- The `< 32` sequential threshold: with ticket 01 the per-pin cost drops; if
  narrow levels still dominate wall time, test `par_iter` unconditionally on
  jpeg and compare. Keep whichever is faster; results must be identical
  (the eval is already pure w.r.t. `state` within a level).
- Temporary instrumentation must be removed before commit.
