# Acceptance — drt-unrouted-analysis

- [ ] Every pin in the final unrouted report carries exactly one
      root-cause class, geometric vs negotiation split with the
      named subclasses; counts sum to the pinned unrouted totals on
      gcd and picorv32.
- [ ] The classifier reproduces the validation anchors (met3-bridge
      residual, region_escape population, unguided io-port nets) or
      the results entry names the delta.
- [ ] Routed output unchanged: gcd and picorv32 route digests and DEF
      goldens byte-identical; no routing decision, cost, iteration,
      or emit behavior altered.
- [ ] Prove-it-red recorded: an injected known-class failure lands in
      the expected class, then the injection is reverted.
- [ ] `cargo test --workspace --profile verify` and clippy
      `-D warnings` pass.
- [ ] `docs/results.md` dated entry: decomposition table for both
      designs, measured probe cost, prioritized target list with fix
      directions; `docs/e6-drt.md` and `docs/status.md` in step.
