# Acceptance

- [ ] OpenSTA's `Clear`/`Preset` propagation behaviour is
      measured from the container and recorded verbatim —
      whether the async arc yields an arrival at `Q`, and how
      endpoints only reachable through it are reported.
- [ ] Falling recovery/removal coverage measured against the
      async cell variants that actually exist in
      `sky130_fd_sc_hd`.
- [ ] Our current behaviour on the same fixture recorded
      (divergence or agreement, verbatim).
- [ ] Either R6 gets a sequenced plan, or is closed as
      'already matching' — per the measurement.
- [ ] R3/R4 roadmap entries updated to DONE with the test
      evidence; no code regressions (`cargo test --workspace`
      + clippy pass); version bumped.
