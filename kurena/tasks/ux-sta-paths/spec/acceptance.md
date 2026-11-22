# Acceptance

- [ ] `grus-eda sta --paths N` emits worst-N path
      traces per corner (startpoint→endpoint, ordered
      stages with pin names, per-stage delay and
      arrival, required, slack); `critical_paths`
      block present in `--json` output with the
      documented schema.
- [ ] Trace covers the modelled stage kinds and
      names anything it cannot trace instead of
      dropping it.
- [ ] OpenSTA `report_checks` golden captured in the
      pinned container on `gcd`; our path pin
      sequences and per-stage delays compared with a
      recorded bound (`max(5 ps, 1%)` or a named
      residual); golden + commands recorded in
      `docs/results.md`.
- [ ] Fail-proof red/restore recorded (corrupted
      predecessor selection → test red).
- [ ] `cargo test --workspace`, `clippy -D warnings`
      pass; version bumped (minor, new CLI/JSON
      surface).
- [ ] `docs/results.md` + `docs/status.md` E9 row
      updated.
