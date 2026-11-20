# Goal

Third ticket of the multi-clock sequence (`docs/multi-clock.md`
§3): evaluate the cross-domain paths `mc-eval` currently defers
as `CrossDomain` skips, per the measured OpenSTA rules.

## Work (rules are measured — `docs/multi-clock.md` §1.2–§1.5)

1. **Setup pairing** (§1.2): for a launch domain ≠ capture
   domain check, enumerate (launch edge, capture edge) pairs —
   each launch edge with the smallest capture-domain edge
   strictly after it — over the period LCM window; the
   reported pair minimises `capture − launch`. Verified
   numbers: `cka` 2.0 / `ckb` 1.4 → Δ0.2 pairs at launch 4.0 /
   capture 4.2 (A→B) and 9.8→10.0 (B→A); equal-grid integer
   ratios still pick the min-gap pair (1.0→2.0, not 0→2.0).
2. **Hold pairs** stay on coincident edges (the fixtures only
   exercise edge 0 — park deeper cross-domain hold selection;
   keep it recorded as a parked item in `multi-clock.md`).
3. **Uncertainty** (§1.3): cross-domain checks use the capture
   clock's row. Add `-from`/`-to` inter-clock form to
   `set_clock_uncertainty`: per directed pair, **replaces**
   the capture clock's value for that direction only (not
   additive, not symmetric); hold side uses `-hold`.
4. **Delay checks on cross-domain endpoints** (§1.5 +
   `mc-eval` review finding): the `set_max_delay`/`set_min_delay`
   anchor is the capture domain's edge-0 arrival and needs no
   edge pairing — an endpoint covered on both sides should
   report instead of deferring. Un-skip exactly that case.
5. Golden coverage on `sta_clocks`: `t2_frac` and `t2_int`
   cross-domain endpoints (`u_fx`, `u_fy`, `d_in`→`u_fb0`),
   `t2_unc`/`t2_uncx` direction-specific uncertainty, and a
   `t2_md*` cross-domain delay-check scenario — all against
   the committed OpenSTA artifacts / regenerated reference,
   strict tolerances, one fail-proof mutation recorded.
6. Record in `docs/results.md`; tick the box in
   `docs/multi-clock.md` §3; version bump.

## Files

- `crates/grus-core/src/sta/analysis.rs` (edge-pair search,
  xdomain skip removal, delay-check anchor path)
- `crates/grus-core/src/sta/sdc.rs` (`-from`/`-to` uncertainty
  parse + storage)
- `tests/designs/sta_clocks/` (golden regen + any new sdc)
- `docs/multi-clock.md`, `docs/results.md`, `docs/roadmap.md`
- `Cargo.toml` / `Cargo.lock`
