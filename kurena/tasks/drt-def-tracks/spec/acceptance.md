# Acceptance — drt-def-tracks

- [ ] `write_def_full` output for a placed design contains TRACKS
      lines for every LEF routing layer with correct
      origin/step/layer, matching what the routed-DEF emitter would
      write for the same tech.
- [ ] GCELLGRID emitted; the chosen grid density recorded in the
      commit message with rationale.
- [ ] Round-trip: `parse_def` on our emitted file succeeds and the
      parsed track model equals the emitted values (test, not
      eyeballed).
- [ ] Determinism: identical input → byte-identical DEF.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/results.md` dated entry.
