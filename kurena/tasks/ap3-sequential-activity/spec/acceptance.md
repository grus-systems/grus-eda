# Acceptance

- [ ] `LibCell` stores the sequential data pin(s) (parsed
      from `ff.next_state`/`latch.data_in`); uncovered forms
      (scan mux, set/reset, complex expressions) are a named
      residual with a sky130 census in `docs/results.md`.
- [ ] Sequential outputs copy the resolved data-net
      `{P, d}` (origin `Propagated` per the golden label);
      multi-output `Q`/`QN` inversion rule recorded.
- [ ] Resolution runs to a fixed point (resolved Qs re-enable
      combinational propagation); leftover `Default` nets
      become `unknown` `{0,0}` and taint downstream.
- [ ] `EXPECTED_DIVERGENT` == `{}`; all 12 mba sessions match
      per-pin `{d, P, source}` verbatim at strict bound;
      sequential columns `u_df/Q`/`qd` follow `a` incl. the
      a2x density doubling; group columns asserted at the
      recorded bound.
- [ ] Fail-proof recorded (sequential pass off → red;
      corrupted sequential golden → red; restored).
- [ ] results.md entry + roadmap N7 update; moved goldens
      enumerated (fully-annotated fixtures byte-identical).
- [ ] `cargo test --workspace` + clippy `-D warnings` +
      `cargo test --release -p grus-core sta` pass; version
      bumped in the same commit.
