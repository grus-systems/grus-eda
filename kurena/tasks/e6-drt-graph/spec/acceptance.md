# Acceptance — e6-drt-graph

- [ ] Both committed guides (`gcd.guide`, `picorv32.guide`) parse and
      join to `DesignDb` nets; missing/extra nets are named, not
      silently dropped.
- [ ] Per-(net,layer) regions are non-empty wherever the guide covers
      the layer; zero-coverage layers are named in the census.
- [ ] Every member pin's access points map to ≥1 in-region graph node
      on at least one layer, or the pin is counted in the named
      `region_escape` census with its count printed per design.
- [ ] Node/edge enumeration is deterministic (double-run assert with
      nonzero evidence — AGENTS §7.7) and sized O(guide boxes ×
      tracks), not O(die area).
- [ ] A synthetic fixture — a guide box shrunk to exclude a pin's
      access points — produces exactly that pin's `region_escape`;
      fail-proof (empty region) goes red and restores green.
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/e6-drt.md` documents the node/edge model and what the
      graph does NOT yet do (no vias, no costs); `docs/results.md`
      dated entry with node/edge census on both designs.
