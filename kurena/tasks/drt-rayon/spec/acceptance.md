# Acceptance — drt-rayon

- [ ] Phase-split timings recorded before choosing targets.
- [ ] Byte-identical routed geometry vs serial baseline — including
      under forced `RAYON_NUM_THREADS=1` and a high thread count.
- [ ] No unsafe shared-occupancy mutation; any parallel route
      scheme names its invariant (or names it a non-goal).
- [ ] gcd route wall time improves; numbers in results.md.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
