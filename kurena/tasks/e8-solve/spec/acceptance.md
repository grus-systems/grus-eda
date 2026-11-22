# e8-solve — acceptance

- [ ] gcd: both nets solved; `gcd.irdrop.voltage.ours.{vpwr,vgnd}.csv`
      emitted in reference column order with exactly the golden CSV
      row set (same instance-terminal pairs, deterministic order)
- [ ] Scalar summary recorded per net: worst/avg drop (VPWR),
      worst/avg bounce (VGND), total injected current vs golden
      `total_power_w`/`V_supply` (delta recorded, expected nonzero
      since our estimator ≠ OpenSTA's — quantify, don't hide)
- [ ] Solver residual `‖GV−I‖∞` printed and small enough that node
      voltages carry ≥ 6 significant digits; method + crate choice
      justified in commit
- [ ] Source clamp verified: all source-node voltages exactly the
      golden `supply_voltage_v` per net
- [ ] picorv32 solved too (both nets; runtime + peak RSS recorded —
      no strict bound, but the number goes in `docs/results.md`)
- [ ] Sanity: with all instance powers zeroed the map is uniformly
      `V_supply` (drop = 0) — unit test, no reference needed
- [ ] `cargo test --workspace`, `cargo clippy --workspace
      --all-targets -- -D warnings` pass; minor bump (new CLI surface
      or new public API)
- [ ] `docs/results.md`: commands, deltas vs golden scalars, residual,
      runtime
