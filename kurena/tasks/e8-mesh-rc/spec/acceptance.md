# e8-mesh-rc — acceptance

- [ ] `PdnMesh` built for both VPWR and VGND on `gcd.placed.def`:
      node/edge census recorded (wire R count, via R count, sink count
      = ticket-2 attached terminals, source nodes from the net PIN)
- [ ] Resistance values come from `TechDb` (wire `RPERSQ`, via
      `via_res_ohm_above` / parallel cuts) — the same fields guide
      parasitics already validated against OpenROAD; no duplicated
      resistance table
- [ ] Via cut count taken from LEF VIA cut-layer RECT count; a
      renamed-via test proves name parsing is not load-bearing
- [ ] Invariants asserted: all R finite and > 0; no dangling
      sink/source; sink set == ticket-2 attach set; picorv32 census
      also recorded (scale check, no gate on counts)
- [ ] Fail-proof: zeroing one layer's `RPERSQ` (or removing via edges)
      makes the check red, then restore
- [ ] `cargo test --workspace`, `cargo clippy --workspace
      --all-targets -- -D warnings` pass; patch version bump
- [ ] `docs/results.md`: per-net census on both designs, runtime
