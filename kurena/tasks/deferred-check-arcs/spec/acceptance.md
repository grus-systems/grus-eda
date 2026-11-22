# Acceptance

- [ ] Lib census recorded: which cells carry each deferred
      arc type (non_seq/three_state/preserve), so fixture
      claims are grounded.
- [ ] Each arc type's OpenSTA behaviour measured with
      committed artifacts: evaluated as a check (which
      surface) or ignored.
- [ ] Whatever OpenSTA evaluates is implemented and
      golden-covered at STRICT; whatever it ignores stays
      counted with an explicit record.
- [ ] `cargo test --workspace` + clippy pass; existing goldens
      byte-identical; version bumped; docs updated.
