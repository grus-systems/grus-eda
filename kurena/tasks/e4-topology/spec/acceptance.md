# Acceptance

- [ ] `*P`-port-on-dead-island census on our emitted
      guide SPEFs: zero on both designs, or equal to the
      reference count with per-net evidence that the
      residual matches OpenROAD's own behavior; probe of
      how OpenROAD covers bterm cells recorded in
      docs/results.md.
- [ ] Detour-island census before/after with the
      dominant disconnected-component class named;
      emission-local reduction applied, or residual
      recorded as route-path divergence with per-net
      evidence (not emission artifacts).
- [ ] e2e decomposition re-measured
      (`test_<d>_guide_sta_decomposition` table
      updated): gcd topology max < 250.8 ps and
      picorv32 < 626.66 ps, or the remaining extreme
      re-attributed to a named mechanism; E4
      criterion-4 verdict (met / named residual)
      stated in docs/status.md.
- [ ] e4-1 topology gates in bounds after coverage
      changes (overlap share ≥ 0.70, box delta ±15%);
      e4-2 cap gate 100% on both designs; stub census
      not regressed beyond recorded bound.
- [ ] `cargo test`, `clippy -D warnings`, release sta
      regression pass; non-guide goldens
      byte-identical; fail-proof mutation recorded;
      version bumped.
