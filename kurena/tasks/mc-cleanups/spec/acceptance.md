# Acceptance

- [ ] Every code nit resolved (dead code removed or restated,
      empty clock list counted, pin-overwrite semantics
      documented, PROBE_ONLY asserts the real parse bail,
      post-wrap waveform validated, stale names/comments
      fixed).
- [ ] The three probe items (`-to clk2` warn/count,
      port-vs-clock namespace priority, `-clock`-less input
      under `-waveform`) are measured in the container and the
      measured behaviour implemented + recorded.
- [ ] All docs fixes applied (`timing-exceptions.md`,
      `results.md` command/count entries,
      `driven_nets_without_activity`, README fixture list,
      `t2_mdx` description, saif nits).
- [ ] `cargo test --workspace` + clippy pass; all goldens
      byte-identical; version bumped.
