# Acceptance

- [ ] CLI emits an OpenROAD-format guide file per net
      (P-GR format: net/paren/DBU-rect+layer), including
      pin-access stubs; deterministic box order.
- [ ] Committed OpenROAD `global_route -guide_file`
      references for gcd + picorv32 produced by a
      committed script (container, pinned image).
- [ ] `docs/results.md` entry: per-net overlap share and
      per-net/total box-count deltas on both designs vs
      the reference; the ≥0.70 / ≤15% criteria evaluated
      with actual numbers (and if below the provisional
      bound, the diff root cause is named, not waived).
- [ ] `read_guides` accepts our file in the container
      (and `estimate_parasitics -global_routing` run
      recorded if exercised).
- [ ] Tests assert the metrics against the committed
      reference; fail-proof proven; `cargo test`,
      `clippy -D warnings`, release sta regression pass;
      goldens otherwise byte-identical; version bumped.
