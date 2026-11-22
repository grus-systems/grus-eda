# Acceptance

- [ ] `-input_ports`/`-pins`-style overrides resolvable via
      the API with origin `user`; `-activity`
      clock-normalization vs `-density` absolute handled at
      the call site (`s_p40` pinned); measured precedence
      (user < global/clock/constant, > input default)
      implemented.
- [ ] `s_input_ports` + `s_p40` per-pin `{d, P, source}`
      verbatim; all 12 sessions still match; fail-proof
      recorded for both new arms.
- [ ] gcd `report_power` re-comparison landed in
      `docs/results.md` — which columns moved toward
      OpenSTA, which still diverge, named residuals.
- [ ] N7/roadmap updated; per-pin/per-net label residuals
      and `unknown` `prob_one`/`when` note recorded.
- [ ] `cargo test --workspace` + clippy `-D warnings` +
      `cargo test --release -p grus-core sta` pass; version
      bumped in the same commit.
