# Acceptance

- [ ] Unpaired guide-stub components emitted as dead
      islands matching OpenROAD's self-loop resistor
      semantics: no stub edge into the driver tree,
      island caps preserved on their nodes, per-net
      `*D_NET` cap unchanged — the e4-2 cap gate still
      passes 100% of nets on both designs.
- [ ] Island/stub population measured on our guides vs
      OpenROAD's (gcd 212/1,389 vs 152/325 baseline);
      dominant unpaired-stub class named and reduced in
      `build_guides`, or recorded with evidence if the
      residual is inherent to met1/met2 strips.
- [ ] e4-1 topology gates stay in bounds (overlap
      share ≥ 0.70, box delta ±15%) after any guide-emit
      change; e4-3 e2e re-measured with the three-axis
      decomposition updated — parasitics-axis median
      must drop materially (expectation: toward the
      engine band), remaining residual re-attributed.
- [ ] `docs/results.md` entry with before/after numbers
      and probe conditions; `docs/e4-guides.md` island
      semantics documented; E4 section status updated
      honestly (gate pass or named residual).
- [ ] `cargo test`, `clippy -D warnings`, release sta
      regression pass; non-guide goldens byte-identical;
      version bumped.
