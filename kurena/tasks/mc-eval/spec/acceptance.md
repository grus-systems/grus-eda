# Acceptance

- [ ] A two-clock SDC no longer bails at the gate; both
      domains' clock nets are seeded from their own ports
      (propagated and ideal parity), and clk2 is never a data
      input.
- [ ] Same-domain paths time correctly on both domains,
      verified against the committed `sta_clocks` OpenSTA
      artifacts under strict tolerances (per-endpoint, both
      periods distinct); at least one fail-proof mutation
      shown red then restored.
- [ ] Cross-domain launch/capture pairs are not evaluated:
      they bail or are counted with a named diagnostic (no
      silent fold); the deferral is recorded.
- [ ] MCP multipliers, latch-borrow windows, and I/O anchors
      use the correct per-domain period/latency row.
- [ ] Existing single-clock goldens byte-identical;
      `cargo test --workspace` + clippy pass; version bumped;
      `docs/results.md`/`multi-clock.md` updated (still-open:
      xdomain pairing, `-waveform`, `set_clock_groups`,
      inter-clock uncertainty).
