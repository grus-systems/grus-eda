# Acceptance — e6-drt-emit

- [ ] `grus-eda` routes `gcd.placed.def` and `picorv32.placed.def`
      under the committed guides and emits routed DEFs that re-parse
      through our own DEF reader (byte-stable roundtrip).
- [ ] `run_all` on OUR routed DEF reports **0 violations across all
      50 subset rules** on both designs — every violation is either
      fixed or explicitly named with its rule and count.
- [ ] Wire-length and per-cut-layer via counts print beside the
      reference `variants` numbers (informational delta, sign
      recorded).
- [ ] A fail-proof (emit a deliberately spacing-violating segment,
      or skip occupancy) makes the self-DRC gate red — recorded.
- [ ] `cargo test --workspace` and clippy `-D warnings` pass.
- [ ] `docs/e6-drt.md` + `docs/results.md` dated entry;
      `docs/status.md` honest line (self-DRC clean — the full Magic
      deck check on our DEF belongs to `e6-drt-e2e`).
