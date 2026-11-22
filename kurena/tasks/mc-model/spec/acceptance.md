# Acceptance

- [ ] `create_clock` accepts two design clocks; the clock
      table stores both domains' name/port/period/latencies/
      uncertainties/transition/propagated.
- [ ] `set_clock_latency 0.5 [get_clocks {a b}]`, per-name
      `set_clock_uncertainty`, virtual clocks, and non-equal
      virtual periods parse into the right rows; still-
      unsupported forms (`-from`/`-to`, `-waveform`,
      `-early`/`-late` if deferred) bail with named errors,
      not silent acceptance.
- [ ] `sta` on a two-clock SDC fails loudly (no silent fold of
      domain 2 into domain 1, no clk2-as-data-input); the
      failing command and message are recorded in
      `docs/results.md`.
- [ ] Uncertainty on a virtual clock lands on that row (the
      probe's silent wrong-target bug is structurally gone,
      with a regression test).
- [ ] All existing golden tests byte-identical; new
      parse/model tests; `cargo test --workspace` + clippy
      pass; version bumped; docs updated.
