# Acceptance

- [ ] `--top` accepts the exact design name and the
      trailing `\`-segment; wrong names and
      non-final-segment collisions still bail with
      the loud error; help text updated; unit tests
      cover all four cases.
- [ ] Single-top auto-accept verified (either
      already worked — recorded — or implemented);
      the pre-change state is named in
      `docs/results.md`.
- [ ] `docs/sdc-examples.md` exists with the three
      topologies; every example is a parseable SDC
      traced to a committed fixture (or a new
      committed fixture a parse test reads).
- [ ] `cargo test --workspace`, `clippy -D warnings`
      pass; patch bump with rationale.
- [ ] `docs/status.md` E9 row updated.
