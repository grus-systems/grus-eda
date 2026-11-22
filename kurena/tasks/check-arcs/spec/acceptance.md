# Acceptance

- [ ] The OpenSTA command that surfaces `min_pulse_width` checks
      is identified and recorded in `docs/results.md`, with the
      returned fields.
- [ ] A new fixture emits golden for pulse-width checks (ideal
      AND propagated clock; at least one real violation), generated
      by running the committed script inside the LibreLane
      container.
- [ ] `min_pulse_width` endpoints are evaluated and match the
      golden at STRICT tolerance; other deferred check arcs stay
      counted, not silently evaluated.
- [ ] `cargo test --workspace` and `cargo clippy --workspace
      --all-targets -- -D warnings` pass.
- [ ] `docs/roadmap.md` N3 deferred note updated; version bumped
      in the same commit.
