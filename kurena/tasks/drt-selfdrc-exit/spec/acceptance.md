# Acceptance — drt-selfdrc-exit

- [ ] `grus-eda drt` exits nonzero when self-DRC findings_total > 0.
- [ ] The routed-means-connectivity-only semantics are stated in
      `docs/e6-drt.md` (and the census line ordering makes
      findings visible, not buried).
- [ ] A test proves the nonzero-exit path (injected violation, not
      a mock).
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/results.md` dated entry.
