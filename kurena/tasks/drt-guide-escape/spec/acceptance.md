# Acceptance — drt-guide-escape

- [ ] A pin outside its guide region (lower layer) is routed via
      an escape mechanism, or fails with the escape attempt named.
- [ ] Escaped geometry is occupancy-checked like normal metal.
- [ ] Fixture proven red before the mechanism, green (or named)
      after.
- [ ] gcd unrouted delta recorded in `docs/results.md`.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
