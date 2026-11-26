# Goal

Attack the last E4 e2e axis: **guide topology**. After
`e4-guide-islands` the parasitics axis is ~0 and the gate
residual is purely `our guides vs OpenROAD's guides`
(docs/results.md 2026-11-22, v0.3.12): gcd `resp_val`
port on a dead island (+250.8 ps = gcd max), picorv32
`_108xx_` detour islands (+627 ps; ours 1,754 multi-node
detour islands vs reference 604).

Two bounded sub-problems — **emission-side connectivity,
not re-routing**:

1. **Port-connectivity invariant.** Our emitted guide
   must not leave a `*P` (top-level port / bterm) pin
   cell on a dead island. OpenROAD's `read_guides` keeps
   the port on the connected component; ours orphans it
   (gcd `resp_val`). Probe how OpenROAD covers port
   cells (access stub on `li1` + guaranteed `met1`
   reach, or strip coverage), then emit equivalent
   coverage so every bterm-access cell chains to the
   driver component through consecutive same-point
   pairable boxes.
2. **Detour-island reduction.** Census why picorv32
   emits ~3x more multi-node detour islands than the
   reference (1,754 vs 604 nets): which strip endpoints
   fail to chain back (met2 runs whose endpoint cells
   lack `met1` overlap? stub groups ordering?),
   name the dominant class, and extend coverage where
   the fix is emission-local. Route-path differences
   (different Steiner/detour choices vs OpenROAD's
   router) are out of scope — record them as the
   inherent residual.

## Done when

- `*P`-port-on-dead-island census in our emitted SPEFs:
  0 (or matched to the reference count with evidence).
- Detour-island population re-censused with the
  dominant class named; reduced where emission-local.
- e2e decomposition re-measured; gcd max below 250.8
  (resp_val extreme gone), picorv32 max below 626.66
  or the residual re-attributed to named route-path
  divergence with per-net evidence.
- e4-1 gates stay in bounds (overlap ≥ 0.70, box delta
  ±15%); e4-2 cap gate stays 100%.
- E4 criterion-4 verdict stated honestly: met, or named
  residual (topology = route-path divergence, not
  emission).

## Files

- `crates/grus-core/src/congestion/guide.rs`
  (`build_guides` coverage emission)
- `crates/grus-core/src/sta/parasitics.rs` (only if a
  connectivity probe lands there)
- `tests/designs/{gcd,picorv32}/` goldens regenerated
  via committed scripts if they move
- `docs/results.md`, `docs/e4-guides.md`,
  `docs/status.md`, `docs/roadmap.md` (per-ticket note)
- `Cargo.toml` (patch bump)
