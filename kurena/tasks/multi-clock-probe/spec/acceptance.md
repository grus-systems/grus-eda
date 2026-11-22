# Acceptance

- [ ] `docs/results.md` records exactly where a two-clock SDC
      breaks today (parse error / silent overwrite / partial
      eval), with the failing command and behaviour.
- [ ] OpenSTA's two-clock rules are recorded: launch/capture
      pairing, cross-domain timing, per-clock
      uncertainty/latency, and the `set_min_delay` capture
      anchor on a second-domain endpoint — all from container
      runs.
- [ ] Either the gap is closed inside this ticket (with golden
      coverage), or a sequenced implementation plan with
      ticket-sized steps lands in the design docs /
      `docs/roadmap.md`.
- [ ] `cargo test --workspace` and clippy pass (unchanged code
      must not regress).
