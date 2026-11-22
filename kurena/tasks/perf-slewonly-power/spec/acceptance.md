# Acceptance

- [ ] `power` no longer runs `eval_sta` walk or endpoint
      collection for pin slews; slew-only path reuses the
      shared prep/seed code (no duplicated seeding).
- [ ] Parity proven by test: slew-only `PinSlew` ==
      full-run `PinSlew` on committed designs; all power
      goldens byte-identical.
- [ ] `cargo test --workspace`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --release -p grus-core sta` pass.
- [ ] `docs/results.md` before/after `power` table from
      committed `measure.sh` (REPS=5, same machine):
      achieved speedup and named residual.
- [ ] Phase keys still meaningful (`power.slews` envelope
      shrinks; record any moved keys).
- [ ] Version bumped (patch) in the same commit.
