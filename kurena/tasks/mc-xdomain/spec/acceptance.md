# Acceptance

- [ ] Cross-domain setup endpoints report the measured
      min-`capture−launch` pair: `u_fx` launch 4.0/capture 4.2
      (arr 4.277469, req 4.085446) and `u_fy` 9.8/10.0
      (`t2_frac`); `t2_int` B→A picks 1.0→2.0 not edge 0.
      Matched against OpenSTA at strict tolerance.
- [ ] `set_clock_uncertainty -setup 0.30 -from cka -to ckb`
      replaces the capture row's value for that direction
      only (`u_fx/D` req 3.785446); other directions and the
      hold side unchanged (measured values in `multi-clock.md`
      §1.3).
- [ ] A delay-check-covered cross-domain endpoint reports
      with the capture-domain edge-0 anchor instead of
      deferring; unmatched cross-domain endpoints still defer
      as named `CrossDomain` skips (never fold).
- [ ] Hold stays on coincident edges; the parked deeper-
      selection item is recorded in `multi-clock.md`.
- [ ] Fail-proof mutation recorded; `cargo test --workspace` +
      clippy pass; single-clock goldens byte-identical;
      version bumped; docs updated.
