# Acceptance

- [ ] `[get_clocks <name>]` inside `-from`/`-to` on exception
      commands resolves (no longer counted ignored); the
      measured matching rule (launch vs capture anchoring) is
      recorded in `docs/results.md` and implemented.
- [ ] `set_clock_groups -asynchronous -group cka -group ckb`
      drops exactly the measured endpoints (`t2_grp`: 8→5,
      cross-domain and cross-anchored I/O gone, same-domain
      untouched); other group modes either share the path or
      bail with a named error per the probe.
- [ ] `create_clock -waveform` parses and the edge lattice
      uses the waveform instants; a non-50% duty scenario
      matches OpenSTA at strict tolerance.
- [ ] All new behaviour golden-covered on `sta_clocks`;
      existing goldens byte-identical; `cargo test
      --workspace` + clippy pass; version bumped; docs updated.
