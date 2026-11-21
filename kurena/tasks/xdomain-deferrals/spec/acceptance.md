# Acceptance

- [ ] Each of the four deferred corners has a committed
      OpenSTA probe artifact and a recorded outcome.
- [ ] Corners where OpenSTA produces a value are implemented
      and golden-covered at STRICT; corners where it also
      skips/errors stay named deferrals with the evidence.
- [ ] `docs/multi-clock.md` §2 reflects the resolution of all
      four items; `cargo test --workspace` + clippy pass;
      goldens byte-identical; version bumped.
