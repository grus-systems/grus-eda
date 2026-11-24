# e6-drt-e2e r2 — pin access under the hull mask

The connectivity bill the hull mask charges: `unrouted` went 0 → 67
on gcd at `d8d0c6c`. The memo's direction — tune the pin-access
exception (full pin-rect overlap landing) so unrouted falls back
toward 0, without relaxing the hull mask itself. This document fixes
the diagnosis procedure and the fix ladder.

## Measured failure shape (probed on the d8d0c6c release binary)

`DRT_DEBUG_NET=<net>` prints, per failed `extend_once`, each pin's
attach-set size, graph-reachability (`seen`), and adjacency.

- `req_msg[16]` pin#1 = `_300_/A2` (`sky130_fd_sc_hd__a22o_2`,
  li1-only port): attach=1 — a lone li1 node at (47150,85850) with
  `edges=0 via=0 relax=1`; its only arc is a relaxed `mcon` at
  (47150,85850) to a met1 node at (47090,85850). `seen=true` for the
  whole pocket, `connected=false` on every iteration → `NoPath`.
- `_043_` pin#2 = `_190_/B` (`sky130_fd_sc_hd__xor2_2`, met1-only
  port): attach=9 — met1 nodes along the `B` port bar, all with
  in-plane edges and mcon/M1M2 arcs, all `seen`, none connectable →
  `NoPath`.
- Name census of the 67: 16×`dpath.a_lt_b$in1`, 8×`in0`,
  16×`req_msg`, `req_rdy`/`req_val`/`resp_val`, `ctrl.state.out[1,2]`,
  21 numbered nets — bus-wide systematic failure, not congestion
  noise.
- Both sampled pins are **sink pins on cells with no met1 OBS**
  (`a22o_2`, `xor2_2` declare li1 OBS only). `Unattachable` (empty
  attach sets) was not observed in the samples — the attach-side
  cover works; the escape side is what dies.

## Root cause — demonstrated on `_300_/A2`, not guessed

The pocket's single relaxed `mcon` arc at (47150,85850) is hull-legal
(the position and both stubs sit inside the net's own-pin cover) but
`edge_cost`-dead: `Occupancy::scan_rect` expands the mcon's li1 plate
(±85 DBU, `L1M1_PR`) by `s_li1` = 170 DBU → x∈[46895,47405], which
overlaps sibling pin `A1`'s li1 `ObsPin` rect (placed xh=46920) by
~25 DBU. `ObsPin(foreign)` is a hard block in every iteration mode —
the arc can never be taken.

The compounding fact: `relaxed_vias` emits **exactly one** confined
candidate per node (nearest-first `find_map` over a ≤4×3 probe
space). With the li1 region inside cover collapsed to a singleton,
the pocket has exactly one via position — take it or `NoPath`. Yet
legal positions exist: an `mcon` whose x-centre clears `A1`'s rect
plus `s` (e.g. x ≥ 47430 inside the same `A2` rect, xh=47495), or the
same x with a different `vy` whose plate clears in y.

The general class the bus-wide names imply (confirm per pocket with
the step-0 diagnostic before fixing): **single-candidate escape arcs
whose via footprint fails `w/2 + s` against sibling `ObsPin` marks** —
a real same-net-vs-net clearance the mask made binding (pre-mask the
router escaped through interior geometry it now must avoid).

Honesty note: `_043_`'s met1-pocket blocker is *not yet named* — the
sampled arcs read occupancy-clean on inspection; the diagnostic must
print the blocking `Occupant` before any fix ships. Do not generalize
the `_300_` class to all 67.

## Step 0 — diagnostic instrumentation (lands first)

- `RouteDb`/`UnroutedNet` already carries per-pin `(PinId, FailKind)`
  — surface it: `unrouted_kinds NoPath=<n> Unattachable=<n>` appended
  to the emit census line set (own line, so `check_drt_ours.sh`'s
  `^\s+unrouted (\S+)$` regex and the `unrouted ` keep-list prefix
  are unaffected; add `unrouted_kinds ` to the keep-list + a golden
  field).
- Extend `DRT_DEBUG_NET`: for each `NoPath` pin with a graph-reachable
  attach set, re-run `edge_cost` on each boundary arc into the pocket
  and print the first blocking `Hit` — `Occupant` variant, layer,
  interval — naming the blocker class per pocket (stderr only, no
  census/determinism impact).
- Optional deeper cut if the blocker needs it: a
  `pin_debug <cell>/<pin>` line listing each pin's cover rects, the
  surviving in-cover nodes per masked layer, and each emitted relax
  candidate's confinement verdict.

## Fix ladder — evaluate in order, measure unrouted after each rung

Do not stack rungs blind: each lands only if the previous leaves
measured residual, and each is independently revertable.

### P1 — bounded multi-candidate relaxed vias

`relaxed_vias` currently keeps the *first* confined `(vx,vy,b)`
candidate per node. Change to collecting up to **K = 3** confined
candidates as separate `RelaxedVia` arcs (the existing
`nearest_idx`/`nearest_k` probes already enumerate the space — take
the first K confined, not the first K probed). Emission order =
probe order → deterministic; `edge_cost` is unchanged and prices all
of them. The wave then escapes through whichever position's
footprint + stubs clear `ObsPin`/spacing — `_300_`'s case is exactly
"a different candidate clears, the generator never emitted it".

Cost: relax arcs grow ≤ K×; per-arc `edge_cost` work is proportional
to arcs actually traversed. No mask, no occupancy-model, no
confinement change — this only widens a candidate set the search
already knows how to price.

### P2 — cover-scoped masked-layer region extension

For pockets whose surviving in-cover node set is a graph island with
`edges=0` (the `_300_` singleton): apply the `transit_extend`
mechanics to *masked* layers too, scoped to `cover ∩ hull` instead
of `hull`: on each masked routing layer, insert every track-grid
point inside each cover rect of the net's host cells into `region`
(the pin's legal landing area by construction), plus the same
index-adjacent L-corridor to the nearest pre-existing pair.

Effect: pins stop depending on the guide's masked-layer coverage
inside their own hull — `pin_nodes`/attach candidates and relax-via
hang nodes both widen, and in-cover in-plane edges appear (an
isolated li1 island becomes a small mesh with several via x-positions
for P1 to choose among).

The mask's legality rule is untouched — added nodes sit inside the
net's own-pin cover, which is exactly the exemption the mask already
grants. Grown-cover regions beyond the LEF port rect are the recorded
looseness the container check adjudicates; if the deck fires
partial-overlap/abut on them, that is a DRC finding to chase (shrink
growth), never silently kept.

### P3 — sibling-clearance-aware access (only if P1+P2 leave NoPath)

Foreign `ObsPin` geometry is static — pins never move under rip-up.
If a pocket still dies with "every confined candidate fails `ObsPin`
spacing", compute per `(pin, masked layer)` the dead subwindows
(sibling `ObsPin` rect + `s` dilation ∩ cover) and:

- skip dead positions during candidate enumeration (P1's probe list
  filters through it — equivalent outcome with fewer dead arcs);
- prefer the clearance side when `grow_to_track` breaks a distance
  tie.

This is a *feasibility filter* on own-pin cover — still strictly
inside the mask's exemption, no hard-block softened.

### Residual protocol

Whatever remains unrouted after the landed rungs is **named**: the
per-pin `FailKind` + the step-0 blocking-`Occupant` attribution per
pocket (`access_obspin`, `access_pocket`, boundary-`Obs`, etc.), the
net list verbatim into `ours_router.unrouted` and `docs/results.md`.
`stub_outside_cover` stays a bug, not a residual.

## Guardrails (memo — do not do these to regain routed count)

- Do not widen `hull_masked_layer` beyond {li1, met1}, do not make
  met2+ masked, do not unmask li1.
- Do not open foreign hulls, do not soften `ObsPin(foreign)` or
  `Obs(u32::MAX)` hard blocks, do not shrink `w/2`/`s` in scans.
- Do not emit geometry the confinement checks rejected (no
  `seg_allowed` bypass, no stub override).

## Files

- `crates/grus-core/src/drt/graph.rs` (`relaxed_vias` multi-candidate,
  cover-scoped region extension, `pin_cover` tie-break if P3)
- `crates/grus-core/src/drt/maze.rs` (`relaxed_vias` caller,
  `NetTopo` relax capacity, `UnroutedNet` kinds census, debug hook)
- `crates/grus-eda/src/main.rs` (`unrouted_kinds` print)
- `tests/designs/check_drt_ours.sh` (keep-list + `ours_router` field
  for `unrouted_kinds`)
