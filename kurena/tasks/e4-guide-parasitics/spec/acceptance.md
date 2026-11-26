# Acceptance

- [ ] Guide boxes → `NetParasitics` (`NetRcTree` per net)
      reusing the SPEF-path types; `--spef-out`-style
      emission works and `sta --spef` can read it.
- [ ] OpenROAD's guide-SPEF shape probed and recorded
      (D_NET structure) before/alongside the model.
- [ ] `docs/results.md`: per-net cap tables for (a) and
      (b) on gcd + picorv32; fraction within
      `max(5%, 1 fF)`; worst offenders root-caused.
- [ ] Tests assert (a) against committed reference
      (fail-proof proven); `cargo test`,
      `clippy -D warnings`, release sta regression pass;
      existing goldens byte-identical; version bumped.
