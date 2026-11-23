# Acceptance — e6-drt-via

- [ ] Every adjacent routing-layer pair on sky130 has ≥1 via
      candidate (li1–met1, met1–met2, met2–met3, met3–met4,
      met4–met5); a pair with zero candidates is a named tech gap.
- [ ] Via edges exist only at real track crossings on both layers —
      a synthetic off-crossing node has no via edge (fixture test).
- [ ] The default via per layer pair is named and joined to the
      committed `<d>.drt.reference.json` `vias_per_cut_layer` census
      (the reference's pick is either matched or the divergence is
      documented).
- [ ] Census prints per-layer-pair candidate/edge counts on both
      designs; deterministic double-run with nonzero evidence.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/e6-drt.md` via section + `docs/results.md` dated entry.
