# Acceptance

- [ ] Committed script runs `check_power_grid` +
      `analyze_power_grid -voltage_file` on both
      `VPWR` and `VGND` on `gcd.placed.def` inside the
      pinned image
      (`ghcr.io/librelane/librelane:3.0.16`); re-running
      it byte-reproduces the committed artifacts.
- [ ] `gcd.irdrop.reference.json` committed with
      connectivity verdicts, total power, supply V,
      avg voltage, avg + worst drop/bounce, and
      analyzed counts per net — all values emitted by
      the Tcl/script itself, never hand-written.
- [ ] Both current-source conventions measured and in
      the golden (embedded estimate and
      `set_pdnsim_inst_power` fed by the same run's
      OpenSTA per-instance power); the convention the
      E8 engine will mirror is pinned with its
      measured delta recorded.
- [ ] Per-instance voltage CSV(s) committed (< 1 MB)
      or gitignored with size + SHA-256 prefix
      recorded.
- [ ] picorv32 artifacts included, or explicitly
      deferred with reason.
- [ ] `docs/results.md` entry: commands verbatim,
      measured values vs P-IRD scoping numbers,
      artifact sizes, SHA prefixes, rerun count.
- [ ] `cargo test`, `clippy -D warnings` pass;
      version bumped only if Cargo-visible behavior
      changes (reference-only ticket may skip the
      bump — state which).
