# Acceptance

- [ ] Skipped top-level LEF statements counted by keyword in
      `LefSkipReport`; unknown words and unmodelled blocks both
      counted.
- [ ] `parse_lef`/`parse_lef_bytes` return `Result<LefSkipReport>`;
      `TechDb` unchanged; all call sites updated.
- [ ] `grus-eda` prints the one-line stderr summary only when the
      merged report is non-empty.
- [ ] Unit tests prove counting and the empty-report case.
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
