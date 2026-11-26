# e8-e2e — acceptance

- [ ] gcd both nets: worst drop within `max(5%, 5 mV)` and every
      per-instance voltage within `max(5%, 1 mV)` of the committed
      `embedded` golden — or the miss is decomposed into
      power-estimator vs mesh/solver axes by re-solving with reference
      per-instance powers regenerated via `generate_irdrop_ref.sh`,
      and the residual named in `docs/results.md`
- [ ] picorv32 both nets measured and recorded; floor only with a
      named cause
- [ ] Row-set assertion: ours and reference CSVs cover the identical
      (Instance, Terminal) set before any value comparison
- [ ] VGND handled as bounce with sign convention written down;
      absolute-voltage comparison for both nets
- [ ] Top-5 deviation pins named per net (instance, terminal, coords,
      ΔV)
- [ ] `cargo test --workspace`, `cargo clippy --workspace
      --all-targets -- -D warnings` pass; version bump per the CLI/API
      surface that landed
- [ ] `docs/results.md` entry + `docs/status.md`/`docs/roadmap.md`
      updated; E8 stage marked done or the exact residual stated
