# Acceptance

- [ ] `grus-eda cts` runs on `gcd.placed.def` and
      `picorv32.placed.def`, emits a DEF that
      `parse_def` re-reads and `verify_legality`
      accepts; clock net(s) decomposed into
      buffer-driven segments with depth and buffer
      census in the TritonCTS league (gcd ~depth 2 /
      single-digit buffers, not per-sink).
- [ ] Report JSON emitted per run with buffer counts
      per type, sink count, depth, clock-net HPWL —
      schema mirroring `<d>.cts.reference.json` fields
      where practical; committed companion recorded in
      `docs/results.md` next to the TritonCTS census.
- [ ] Propagated-clock STA executes on our CTS DEF in
      `cargo test`; gcd clock-pin arrival spread and
      insertion delay recorded as informational
      assertions (strict golden comparison is the next
      ticket — stated, not claimed).
- [ ] Mutated/unmutated net boundary respected:
      non-clock components and nets byte-identical in
      the emitted DEF except the clock nets (control
      net HPWL unchanged).
- [ ] `cargo test`, `clippy -D warnings` pass; fail-proof
      mutation on the tree constructor recorded;
      `docs/e5-cts.md` + status/roadmap updated;
      version bumped (minor).
