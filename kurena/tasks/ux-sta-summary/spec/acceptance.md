# Acceptance

- [ ] `domains` block in `sta --json`: one row per
      `create_clock` domain + an explicit
      unattributed row when needed; fields `clock`,
      `endpoints`, `wns_max_ns`, `tns_max_ns`,
      `wns_min_ns`, `tns_min_ns`,
      `setup_violations`, `hold_violations`.
- [ ] `slack_histogram` block: documented bucket
      bounds, counts summing to the endpoint total;
      per-domain values equal the same quantities
      recomputed from `endpoints` (asserted in a
      test).
- [ ] Two-domain fixture (`sta_clocks`
      `t2_selmc.sdc` or equivalent) shows two
      populated domain rows; stdout renders the
      table + histogram.
- [ ] Fail-proof red/restore recorded.
- [ ] `cargo test --workspace`, `clippy -D warnings`
      pass; version bump recorded with patch-vs-
      minor rationale.
- [ ] `docs/results.md` + `docs/status.md` E9 row
      updated.
