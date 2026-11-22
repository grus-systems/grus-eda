# Acceptance

- [ ] Gate-function evaluator computes `P(f)` and
      `d(f)=Σ_i d_i·P(∂f/∂x_i)` under input independence for
      arbitrary `LibPin.function` expressions (incl. repeated
      literals); missing/unparseable functions fall back
      without panic.
- [ ] Combinational cells propagate in level order;
      sequential/latch outputs are never produced; gates with
      a `Default` input keep `Default` output (taint).
- [ ] Every `Propagated` net matches the golden
      `{density, prob}` at the recorded strict bound on
      `s_none`, `s_input`, `s_ports`, `s_ports_a2x`,
      `s_partial`, `s_full`; `EXPECTED_DIVERGENT` shrinks to
      exactly `u_df/Q` + the 11 `unknown` pins, asserted
      verbatim.
- [ ] Fail-proof recorded (propagation off → red; corrupted
      propagated golden → red; restored).
- [ ] `docs/results.md` entry: new group-column residuals,
      origin-count/`driven_nets_without_activity` movement,
      moved-golden explanation (fully-annotated fixtures
      byte-identical); roadmap N7 line updated.
- [ ] `cargo test --workspace` + clippy `-D warnings` +
      `cargo test --release -p grus-core sta` pass; version
      bumped in the same commit.
