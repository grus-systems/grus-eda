# Acceptance — e6-drt-maze

- [ ] Synthetic cases in a small hand-built design: two-pin straight
      route on one track, forced via transition, detour around a
      blocked interval, and a provably-unroutable pin that lands in
      the named-failure report (not an infinite loop or a panic).
- [ ] On `gcd` and `picorv32` placed DEFs + their committed guides:
      every signal net connects all member pins — unrouted count is
      printed and expected 0, else each failure is named.
- [ ] Occupancy actually constrains: a fail-proof (skip occupancy)
      measurably changes routes or collisions — recorded in
      `docs/results.md`.
- [ ] Deterministic: two full runs produce identical route sets
      (compared on a hash or the full structure — nonzero evidence).
- [ ] Runtime is recorded per design; picorv32 must complete in a
      test (not a timeout — state the measured number).
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/e6-drt.md` documents search order, costs, rip-up rule;
      `docs/results.md` dated entry.
