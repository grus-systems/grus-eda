# e4-topology — Overview

Attack the last E4 e2e axis: **guide topology**. After `e4-guide-islands`
(v0.3.12) the parasitics axis is ~0 and the gate residual is purely `our
guides vs OpenROAD's guides` (`docs/results.md` 2026-11-22): gcd
`resp_val` port on a dead island (+250.8 ps = gcd max), picorv32
`_108xx_` detour islands (+627 ps; ours 1,754 multi-node detour islands
vs reference 604).

## Problem

Two emission-side connectivity gaps in `build_guides`, both verified
against the committed artifacts before writing this design:

1. **Dead `*P` ports.** Census of `gcd.guide.ours.spef` (union-find over
   non-self-loop `*RES`, driver-rooted): **12 dead `*P` pins** vs **7 in
   the reference** `gcd.guide.spef`. Mechanism, verified on `resp_val`:
   the port's DEF `+ LAYER` is `met3` (`conn` = met3), its GCell is
   covered by a `met1` strip only, so the `met3` access stub lands in
   `stub_extra` — an unpairable self-loop node that captures the pin's
   conn-layer lookup onto a dead island → OpenSTA lumped fallback →
   +250.8 ps. OpenROAD's own `gcd.guide` `resp_val` block covers the
   port cell on `met1`/`met2`/`met3` (stacked same-rect boxes that pair
   into via chains under `updateVias`), keeping the port on the
   connected component. Fix: `port-stubs.md`.

2. **Multi-node detour islands.** A bend/junction GCell covered by both
   an H (`met1`) run and a V (`met2`) run produces wire endpoints on
   `(C,met1)` and `(C,met2)` with **no resistor between them** — the two
   layer subtrees are disjoint components unless a pin stub chain at the
   cell bridges them (verified on `_012_`: an `li1→met1` stub chain plus
   a `met1` wire sit on a 4-node dead island because the run's junction
   back to `met2` has no via). OpenROAD emits the bend cell box on both
   layers (`_132_`: same rect on `met1`+`met2`+`met3`+`met4`), yielding
   consecutive zero-length segments that `updateVias` turns into a via.
   Fix: `bend-vias.md`.

Because the 2D route graph is connected, bridging every H∩V cell links
all strip components back to the driver — the multi-node island class
should collapse toward or below the reference's residual (whose 604
nets are real route-path features, not emission artifacts).

## Approach

1. **Census first** (`probe.md`): committed census tooling
   (`tests/designs/guide_islands.py`) gives the canonical dead-`*P` /
   dead-component / multi-node numbers on our emitted SPEFs, the
   reference SPEFs, and the OpenROAD read-back SPEFs — before and after.
2. **Bend via-stacks** (`bend-vias.md`): emit a consecutive
   `[met1 zls, met2 zls]` box pair at every non-pin GCell covered by
   both an H and a V strip of the net.
3. **Port rung stacks** (`port-stubs.md`): for a bound pin cell whose
   accesses reach at/above the covering strip's layer, emit a contiguous
   ascending zero-length stack anchored on the strip layer so every
   access-layer node chains to the wire node — replacing the dead
   `stub_extra` stubs for that case.
4. **Re-measure** (`steps.md`, `probe.md`): regenerate ours artifacts,
   step-9c goldens, decomposition table, `Tol` floors, and the docs.

## Non-goals

- No re-routing: route-path differences (different Steiner/detour
  choices vs OpenROAD's router) are the recorded inherent residual.
- No `met3`+ strips, no `pin`-geometry change to `pin_attach` — the fix
  is box emission only; `updateVias`/`boxToGlobalRouting` semantics stay
  the measured reference rule.
- No change to island semantics (`NetRcTree::dead`, lumped fallback) —
  they are the measured model and stay.

## Files

- `crates/grus-core/src/congestion/guide.rs`
- `crates/grus-core/src/sta/parasitics.rs` (census helper / test only)
- `crates/grus-core/src/sta/analysis.rs` (`Tol` floors + doc comments)
- `tests/designs/guide_islands.py` (new committed census script)
- `tests/designs/gcd/gcd.guide.ours.spef`
- `tests/designs/gcd/gcd.guide.sta.reference.json`
- `tests/designs/picorv32/` (gitignored artifacts regenerated)
- `docs/e4-guides.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml` (patch bump)
