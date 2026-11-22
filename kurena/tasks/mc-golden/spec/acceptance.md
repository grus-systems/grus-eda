# Acceptance

- [ ] `two_clock.reference.json` (+ latch if separate) is
      emitted by the committed `probe.sh` run inside the
      container and the test deserializes it directly —
      missing file fails loudly.
- [ ] All 40 golden scenarios pass at STRICT tolerance;
      `PROBE_ONLY` forms keep parse-gate pins.
- [ ] `docs/path-delays.md` §5 has no open items left (item 1
      closed); `docs/multi-clock.md` and `docs/roadmap.md`
      reflect sequence completion; `docs/results.md` records
      the promotion.
- [ ] `cargo test --workspace` + clippy + `sta-regression`
      pass; version bumped.
