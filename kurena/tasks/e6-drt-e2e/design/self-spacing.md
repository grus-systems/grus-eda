# e6-drt-e2e — same-net self-spacing (the measured residual class)

This file records the r3 root-cause measurement for the committed WIP
goldens' remaining `lef_expressible` findings (`met1.2=740`,
`met2.2=56` at `4c5e2cb`) and the fix it implies. **It corrects the
partner hypothesis the review memo carried**: the offending shapes are
not our paint vs subcell-internal or foreign pin metal — they are our
own same-net paint, facing itself at sub-spacing.

## Measured evidence (this planning cycle, in-container probes)

All probes ran inside the pinned image on the committed
`gcd.drt.ours.routed.def`; scripts live under
`tests/designs/gcd/runs/drt-ours/` (`who1.tcl`–`who5.tcl`,
`region.mag`, `gcd.whofix.def` — gitignored workdir, not committed).

1. **Site anatomy.** `drc why` + flattened `select save` on the
   met1.2 cluster at Magic-internal ~(5414..5443, 17144..17196)
   (inside `_365_` = `and2_2`): every `metal1` rect in the region is
   **ours** — `req_msg[9]`'s L1M1_PR plate (mcon at 26930,85850),
   its M1M2_PR plate (27370,85850), and the met1 wire joining them.
   The two plates are 440 DBU apart centre-to-centre; the plate
   overhangs (±140–160, wider than the 130-wide joining wire) face
   each other across a **135-DBU gap < s_met1=140** above/below the
   wire — a same-shape notch. No foreign metal exists at the site.
2. **No invisible internal metal.** `and2_2`/`a22o_2` `.mag` files
   paint metal1 only as rails + mcon teeth, all inside the VPWR/VGND
   pin-rect bands — already `ObsPin`-marked and spacing-enforced.
   The earlier "undeclared subcell-internal met1" theory is refuted
   for these cells: `drc why`'s region contains no subcell metal1
   outside those bands.
3. **Census.** 665/740 met1.2 tiles sit inside the plate-gap
   bounding boxes of **45 same-net L1M1↔M1M2 pairs** at centre
   distance ~300–450 (plate-merge window ≈ ≤300, spacing-clean ≥
   ~450). The remaining ~75 tiles are the same class at wire-end
   landings (via plate edge vs the same net's own wire side/end at
   15–135 gap). All 56 met2.2 tiles cluster at 15 wire-jog sites —
   a via's met2 plate overhang vs the same net's own met2 wire at a
   corner, gap ~90 < s_met2.
   Pair census: 95 L1M1↔M1M2 pairs at 45–60 DBU (plates overlap →
   merged → clean); 45 in the 300–450 notch window (all flagged);
   ~none beyond.
4. **Causality proven.** `gcd.whofix.def` moves req_msg[9]'s M1M2_PR
   from 27370 → 27830 (plate gap 135 → ~600): `drc why` on the box
   reports met1.2 **gone** — only the inherited `nwell.4` remains.
5. **Pin membership.** 39/45 danger pairs have the M1M2 outside the
   pin rect (the stub's escape via in hull whitespace); 6/45 have
   both plates inside the same pin rect — **per-rect confinement
   alone cannot clear these, nor the 15 met2 jog sites.**

## Why the occupancy model missed it

`Occupancy` marks committed paint as `Net(net)`; `hit_verdict` makes
own-net marks free — correct for connectivity (own shapes may touch
and overlap) but it never distinguishes "shapes that merge" from
"shapes disjoint at < s". Emit therefore committed plate-pair and
plate-vs-jog geometry whose *disjoint* edges sit inside the same
spacing window — exactly the class Magic's `spacing` rule checks
edge-to-edge regardless of net. `self_drc findings_total=0` shares
the same blind spot (recorded in `docs/e6-drt.md` §7 as scope).

## Fix — two rules, landed together

### Rule A — access-paint confinement on masked layers (the memo's rule)

On masked layers (`li1`, `met1`) inside a member hull, each emitted
piece of **pin-anchored access paint** is either inside the net's own
pin cover or keeps `≥ s` from every foreign mark — literally the
memo clause "⊆ own pin rect, or ≥ met1 spacing from foreign shapes
when no pin is being accessed", applied per rect:

- mcon plates and the stub run inside the pin rect: `⊆` cover —
  asserted (they already are by construction).
- The stub run outside the pin rect and the escape via's
  masked-layer plate: "paint when no pin is being accessed" —
  legal iff `≥ s` from foreign marks (today's host-open +
  occupancy rule — unchanged semantics, no connectivity cost).
- Transit paint (committed edges through a member hull not part of a
  pin attach): unchanged — host-open, `Obs`/`ObsPin`/foreign-`Net`
  marks enforce `w/2 + s`.

**Cover becomes union-exact** (fixes the L-shaped-pin miss this
cycle also measured): `rect_allowed`/`seg_allowed`'s covered-part
test is `part ⊆ ⋃ cover rects` (residual-empty after `rect_minus`
over all of the cell's cover rects), not `⊆ one rect`. A plate
spanning two joined same-pin sub-rects is then legal — the 65
"partial" delegated mcons are exactly that class and Magic-clean —
while a plate bridging disjoint pin shapes still fails (the
uncovered corner has no pin metal).

**Strict variant kept as escalation**: if post-fix measurement still
shows in-hull `met1.2` sites, tighten to the delegated model —
the whole mcon→met1→M1M2 stack `⊆` pin-cover union (the escape via
lands inside the pin; met2 transit carries onward). This is the
memo-literal reading; it is *not* the first rung because the
measured violation partner is same-net, and strict confinement buys
correctness nothing here while rejecting the 39 measured
outside-pin escapes onto `Unattachable`.

### Rule B — same-net self-spacing closure (the measured fix)

In `route_one_net`'s extraction (after `hlines`/`vlines`/`vplaced`
and the pin-attach block, before `NetOutcome::Routed`): per routing
layer, take the net's committed paint rects (wire `seg_rect`s,
default-candidate via plates, emitted stubs/fills) and find every
unordered pair **disjoint on one axis while overlapping on the
other at edge-gap < s_l** — the notch signature — and close it:

- **Primary repair — fill.** Emit `RectDbu` covering the gap
  (pruned to the facing-edge overlap, grown ±1 into both prongs so
  the merge is unconditional). A fill is admitted only after the
  same scans every other committed shape passes:
  `occ.scan_rect` + `exempt_obs` with zero `Obs`/foreign `ObsPin`/
  `Net`/`Pin` hits, and the layer's mask verdict (`rect_allowed` —
  which for a gap inside own cover holds by construction). Iterate
  to fixpoint (each fill strictly merges two components; bounded).
- **Fallback — reject.** If a fill can't be legally placed (e.g. a
  notch on strict `li1` outside cover), drop the emitting element —
  the access-arc candidate or the offending stub — and take the pin's
  next access candidate; `stub_blocked`→`Unattachable` if all fail.
- **Foreign-freeness is structural**: a foreign mark inside the gap
  would sit `< s` from both prong edges and would already have failed
  the prongs' own admission scans — the re-scan is defence-in-depth,
  not the argument.

**Emission channel.** `NetRoute` gains
`fills: Vec<(LayerId, RectDbu)>` → `NetGeom` gets a `rects` list the
DEF writer emits as `metN ( x y ) RECT ( dx dy dx dy )` records
(the delegated reference already uses `RECT` pieces, e.g.
`gcd.routed.def`'s `met1 … RECT ( 0 -70 350 70 )`); `commit_route`
rasterizes fills like wires/plates — the "emitted DEF = priced
shapes" invariant is unchanged. Census: `notch_fills=<n>` appended
to the `guided=` census line so the golden records the repair count
(`scripts.md`).

Determinism: rect lists and pair iteration are `BTree`-ordered;
fills are inserted in scan order → byte-stable DEF.

## Expected outcome

All ~60 measured sites are one of the two sub-classes above, both
closed by Rule B with zero connectivity cost; Rule A is the
bounding invariant the memo directs and aligns the access geometry
with delegated's proven-clean model where it is free. Success =
`lef_expressible` ours == 0 on all 50 rules; any residual site gets
the same probe treatment (`drc why` + flattened dump) and the
strict-confinement escalation is the named next rung.

## Non-goals / guardrails

- No relaxing `ObsPin`/`Obs` hard blocks, `w/2 + s` scans, the hull
  mask, or `s_l` itself — the fix adds paint (fills) or drops
  candidates, never loosens a check.
- Fills are measured paint, not magic: every fill is scanned,
  confined, rasterized into occupancy, and counted
  (`notch_fills`) — no invisible geometry.
- No new Magic rules; the `lef_expressible` gate is unchanged.

## Files

- `crates/grus-core/src/drt/maze.rs` (extraction closure pass,
  `NetRoute.fills`, access-arc rejection path)
- `crates/grus-core/src/drt/graph.rs` (`PinCover` union components,
  `rect_allowed`/`seg_allowed` covered-part test)
- `crates/grus-core/src/drt/emit.rs` (`NetGeom.rects`,
  `RECT` statement emission)
- `crates/grus-eda/src/main.rs` (`notch_fills=` census field)
- `tests/designs/check_drt_ours.sh` (census regex + `ours_router`
  field)
