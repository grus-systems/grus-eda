# Acceptance

- [ ] `ResolvedActivity` + `ActivityOrigin` replace the
      `Option<SignalActivity>` slots; `net_activity()`/
      `net_prob_one()` read through them; representation
      choice (f64 rate vs synthesized counts) recorded.
- [ ] Fill seeds in the measured order — constant (incl.
      constant-through-gate nets), clock `{2/period, 0.5}`,
      global override, annotation, input `{0.5, 0.1·f_clk}` —
      and unannotated leftovers take the recorded interim
      rule. Per-origin counts in `PowerDiagnostics`.
- [ ] `grus-eda power` runs with no `--vcd`/`--saif`;
      `duration_s` semantics defined and recorded.
- [ ] `s_global` + `s_global_density` golden: every pin
      `{density, prob, source}` + group columns at strict
      bound. `s_none` + `s_input`: seeded-class pins strict,
      propagated/unknown pins covered by an explicit
      expected-divergence table.
- [ ] Moved existing goldens (if any) each explained by
      named newly-defaulted nets in `docs/results.md`;
      fully-annotated fixtures byte-identical.
- [ ] Fail-proof recorded (fill off → red; corrupted golden
      → red; restored).
- [ ] `cargo test --workspace` + clippy `-D warnings` +
      `cargo test --release -p grus-core sta` pass; version
      bumped in the same commit.
