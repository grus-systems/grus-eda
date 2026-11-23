# Acceptance — e6-drt-tracks

- [ ] `sky130_fd_sc_hd` LEF `TRACKS` records parse into per-layer,
      per-axis DBU coordinate lists; gcd/picorv32 load without error.
- [ ] Every signal-net member pin (instance pins + io `BTERM`s) has
      ≥1 access point on at least one pin layer on **both** designs;
      the census prints per-layer counts proving real coverage.
- [ ] Access-point output is deterministic (double-run assert,
      nonzero evidence per AGENTS §7.7 — not two empty vecs).
- [ ] A synthetic fixture (pin rect shifted off every track) produces
      a named off-track finding; a fail-proof (e.g. dropping the
      preferred-axis tracks) turns the census red and restores green.
- [ ] `cargo test --workspace` and
      `cargo clippy --workspace --all-targets -- -D warnings` pass.
- [ ] `docs/e6-drt.md` documents the model, the preferred-direction
      source, and what access points do NOT yet mean (no DRC prune,
      no TA choice); `docs/results.md` dated entry with the census.
