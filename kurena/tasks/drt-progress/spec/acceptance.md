# Acceptance — drt-progress

- [ ] `grus-eda drt` prints periodic progress to stderr during
      routing: nets done/total, unrouted so far, elapsed.
- [ ] stdout census block byte-identical to before (the contract
      consumers read).
- [ ] No stderr writes from library callers — hook is opt-in.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/results.md` dated entry with a sample progress line.
