# Acceptance — drt-pin-collision

- [ ] A committed wire overlapping a foreign pin shape either
      records a collision (triggering wire-side rip-up) or is
      forbidden in hard mode — mechanism chosen and rationale
      recorded.
- [ ] Synthetic fixture: cheap path across a foreign pin → clean
      route or named failure; proven red before the fix.
- [ ] Surviving pin overlap, if any, is a named residual.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/results.md` dated entry.
