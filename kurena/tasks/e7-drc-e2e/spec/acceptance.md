# Acceptance — e7-drc-e2e

- [ ] `grus-eda drc` runs on `tests/designs/<d>/<d>.routed.def` for both
      designs and emits per-rule counts + a summary JSON; with
      `--reference` it prints per-rule deltas vs `magic_per_rule`.
- [ ] Gate test: every rule in `magic_rules_checked.classification`
      marked `lef_expressible` is implemented (name join complete) and
      reports the golden count (all zero today) on both designs.
- [ ] CLI output names the claim boundary (55/278 statements; magic_only
      out of scope) so a `0 violations` report cannot be misread as a
      full-deck clean result.
- [ ] met2 sanity-control mutation flags `met2.2` exactly as Magic did
      on the same input.
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/results.md` dated entry with per-rule table, runtime on
      both designs, and the claim-boundary statement; `docs/status.md`
      records E7's state honestly (subset checker complete / full deck
      out of scope).
