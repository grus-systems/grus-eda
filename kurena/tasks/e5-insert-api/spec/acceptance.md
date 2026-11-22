# Acceptance

- [ ] `insert_cell`, `create_net`, `connect_pin` /
      `disconnect_pin`, `insert_buffer_on_net` exist on
      `DesignDb` with documented invariants (driver-first
      `net_pin_ids`, `pin_nets` back-pointer, CSR
      rebuild policy recorded).
- [ ] Test inserts buffers on gcd's `clk` net: cells +
      pins appear in SoA, sink pins move to new nets,
      driver stays on the original net; assert the
      invariants above on both directions (net→pins,
      pin→net).
- [ ] Round-trip test: mutate → legalize inserted cells
      → `write_def_full` → `parse_def` re-reads the same
      component/net/pin counts and memberships; DEF is
      valid (no writer workaround needed).
- [ ] Untouched nets/cells byte-identical through
      mutation + re-emit (HPWL and guide parasitics on
      a control net unchanged).
- [ ] Fail-proof: break the driver-first invariant (or
      drop a `pin_nets` update) → the invariant test
      goes red, recorded, then restored.
- [ ] `cargo test`, `clippy -D warnings` pass;
      `docs/results.md` + `docs/status.md` E5 row
      updated; version bumped (minor — new API).
