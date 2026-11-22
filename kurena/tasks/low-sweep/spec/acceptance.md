# Acceptance

- [ ] Every listed item either fixed or skipped with a
      one-line recorded reason in the design notes.
- [ ] No behavior changes: all goldens byte-identical;
      `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --release -p grus-core sta` pass.
- [ ] LibSeq parser unit tests added (incl. expression
      rejection).
- [ ] Version bumped (patch) in the same commit.
