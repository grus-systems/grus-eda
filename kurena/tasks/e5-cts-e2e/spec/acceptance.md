# Acceptance

- [ ] Skew compared within `max(25 ps, 25%)` per
      design against the committed reference metrics;
      gcd expected inside (2.2 ps ours vs ≈0 ref),
      picorv32 evaluated with actual numbers and any
      outside-bound case root-caused.
- [ ] Max insertion delay compared within 15% per
      design (gcd ~0.304 vs ~0.314 ns; picorv32
      0.73–0.80 vs 0.757–0.793 ns) — measured, not
      extrapolated.
- [ ] E2e STA: per-endpoint arrival/required/slack on
      our CTS DEF vs `<d>.cts.sta.reference.json`
      evaluated within `max(5 ps, 1%)` on gcd and
      picorv32 floors; decomposition table updated for
      failures (tree-shape vs engine axis named).
- [ ] Honesty gate: buffer census + clock-net
      wirelength ours-vs-reference printed in
      `docs/results.md` (informational).
- [ ] Our CTS DEFs regenerated from committed
      commands; gcd's committed if < 1 MB.
- [ ] E5 row in `docs/status.md` states the verdict
      per criterion (met / named residual); `cargo
      test`, `clippy -D warnings` pass; version bumped
      with the bump level recorded.
