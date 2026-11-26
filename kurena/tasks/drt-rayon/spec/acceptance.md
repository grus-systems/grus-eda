# Acceptance — drt-rayon

- [ ] Phase-split timings recorded before choosing targets.
- [ ] build/emit byte-identical to serial under forced
      `RAYON_NUM_THREADS=1` and a high thread count; if the route
      loop is parallelized, its contract is deterministic under a
      fixed thread count + collision-free end state (spec §3 —
      serial-identity is explicitly NOT required there).
- [ ] No unsafe shared-occupancy mutation; any parallel route
      scheme names its invariant (or names it a non-goal).
- [ ] gcd route wall time improves; numbers in results.md.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
