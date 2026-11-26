# e8-pdn-parse — acceptance

- [ ] `gcd.placed.def`: `pdn_nets` carries VPWR + VGND with per-layer
      wire rects, via stacks resolved to cut layers, and the pin-attach
      table; SPECIALNETS via names resolve through LEF `VIA` parsing
      (not name-pattern heuristics alone — a renamed via must still
      map to the right cut layer)
- [ ] Connectivity self-check per net: every instance power pin reaches
      the net's top-level PIN; `unreachable_pins` empty on gcd for both
      nets — matching `check_power_grid` result=1 in
      `gcd.irdrop.reference.json`
- [ ] Attached-terminal count per net equals the reference CSV row
      counts (`gcd.irdrop.voltage.vpwr.embedded.csv` +
      `gcd.irdrop.voltage.vgnd.embedded.csv` = 384 rows total over
      both; assert the per-net split matches each file's data rows)
- [ ] picorv32 census recorded in `docs/results.md` (rects/nodes/vias/
      attached pins per net, unreachable count); reference comparison
      where its golden carries one
- [ ] Fail-proof: with via edges (or one cut layer's links) removed,
      the connectivity check flags unreachable pins — test observed
      red, then restored
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets
      -- -D warnings` pass; version bump per repo convention (patch,
      additive DesignDb fields — breaking `special_wire_rects`
      signature change counts as minor if it happens)
- [ ] `docs/results.md` entry: what ran, census numbers, reference
      agreement, runtime
