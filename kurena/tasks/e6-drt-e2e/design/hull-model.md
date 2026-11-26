# e6-drt-e2e — instance-hull occupancy model (shipped v1 + r3 confinement)

> **r10 note — unchanged.** Verify-bound revision only (r10 memo —
> `verify-timing.md`); the mask model and the all-0 gate stand.

> **r7 note — unchanged at `5d1502b`** (alias refactor only; the
> mask model below stands, `lef_expressible` 0 on all 50 rules).
>
> **r4 status — LANDED.** The union-exact cover (`rect_covered` /
> `rect_minus` in `drc/union.rs`) shipped at `b372d5e` with the
> closure; no r4 changes here. This file stays as the model record.
>
> **r5 note**: the 20:07 transcripts read `lef_expressible` clean
> except `met1.2=32` — the mask's foreign-shape class is closed.
> Whether the 6 new sites are the mask's invisible-geometry edge
> (X1/X2) or an emit-side class is `met12-attribution.md`'s probe.
>
> **r6 outcome**: emit-side, not the mask. All 7 sites were X3 — the
> `try_fill` host-open carve bug truncated fills inside member hulls
> (`met12-attribution.md` r6). The mask model stands unchanged;
> `lef_expressible` is 0 on all 50 rules on the committed DEF.

The core fix, **landed at `d8d0c6c` and tightened at `4c5e2cb`**: make
the maze's occupancy model conservative w.r.t. subcell-internal
geometry it cannot see. Inside any instance bbox (including
decap/filler/tap cells) block all maze nodes on the masked layers for
every net, except inside the net's own-pin cover — or, on host-open
layers, anywhere the `Obs`/`ObsPin` marks leave room.

## What the mask actually bought (measured trajectory)

`lef_expressible` violations on the same emit went
**~3025 (7 rules + 2 unclassified) → 847 → 514** across the two
checkpoints — the mask model works. The remaining 514 are **not**
the foreign-shape class the model was built for; they are same-net
self-spacing notches in our own emitted paint (`self-spacing.md` —
measured partner, not guessed).

## Root-cause correction (r3 measurement)

The review memo carried the hypothesis "our mcon/met1 paint vs
subcell-internal met1 or adjacent pin metal". Probes inside the
pinned image (`runs/drt-ours/who*.tcl`, `region.mag`) refute it for
the measured sites:

- `and2_2`/`a22o_2` `.mag` metal1 = VPWR/VGND rails + mcon teeth
  only, all inside the rail pin-rect bands — `ObsPin`-marked, spacing
  enforced, never invisible. The flattened DRC site contains **only
  our own net's metal1**.
- The failing edges are our own access-stack plates facing each other
  at 135 < 140 (same-net notch). Moving the escape via clears the
  site — causality proven in-container.

The mask therefore stays exactly as shipped — the residual is an
*emit-time* geometry class, not an occupancy-model gap.

## Shipped model (as of `4c5e2cb`)

**Hull.** `HullMask` indexes every `{Placed, Fixed, Cover}` cell's
placed outline into y-banded x-interval lists; legality
`point_allowed` / `rect_allowed` / `seg_allowed`.

**Masked layers = {li1, met1}; host-open = {met1}** — inside a
foreign hull both are fully masked; inside a host hull (cell hosts a
member pin of the net) li1 needs the pin cover, met1's whole
interior is open at graph level and `Obs(cell)`/`ObsPin` marks carry
legality (the delegated routes run met1 access wires through exactly
those OBS-free bands).

**`PinCover`** — `cell → [RectDbu]`, the XY union of the net's member
pin shapes per host cell (not layer-keyed: the delegated routes drop
the whole mcon→met1→M1M2 access stack inside a pin rect's XY on
layers the pin does not declare). Raw LEF rects — the 4c5e2cb
checkpoint removed the `grow_to_track` corridor growth.

**Occupancy** — `premark_obs` clips each cell's OBS against its own
pin rects (`rect_minus`); residual slabs are `Obs(cell)` walls for
every net; `ObsPin` = instance-pin metal, foreign hard-block;
`exempt_obs` drops `Obs(c)` hits only where paint ⊆ own-pin cover.

**Confinement (r3 — union-exact)** — the covered-part test in
`rect_allowed`/`seg_allowed` is `part ⊆ ⋃ cover rects`
(residual-empty after `rect_minus` over the cell's cover rects),
not `⊆ one rect`: a plate spanning joined same-pin sub-rects is
legal (delegated's 65 "partial" mcons are that class, Magic-clean),
a plate bridging disjoint pin shapes still fails.

## r3 met1 rule — per-piece confinement + transit clause

Per the review memo, applied per emitted rect on masked layers
inside member hulls (`self-spacing.md` Rule A):

- paint inside a member hull must either sit inside own-pin
  cover-union **or** keep `≥ s` from every foreign mark — the
  host-open layer already enforces the second clause via occupancy;
  the first is asserted for pin-touch paint (mcon, pin-side stub);
- **no same-net sub-spacing disjointness** — `self-spacing.md` Rule B
  (the measured fix) closes the notches the memo's clause cannot see.

Escalation (only if residual `met1.2` sites survive): strict
delegated model — the whole access stack ⊆ pin-cover union, escape
via inside the pin, met2 transit onward.

## Residual classes (named, counted, printed)

- `hull_masked` — nodes removed per layer (census print, landed).
- `unrouted` + `unrouted_kinds` + `unrouted_blockers` — connectivity
  cost, per net, per `FailKind`, per blocking `Occupant` class.
- `stub_outside_cover` — the unattachable-pin census (pins that got
  no legal paint; `pin-access.md` r3 redefines it — it is the
  `Unattachable` diagnostic, not a bug).
- `notch_fills` — self-spacing repair count (new, `self-spacing.md`).
- `boundary_overlap` / `boundary_abut` — the two allowed
  unclassified Magic names; `ours` target 0.
- Any nonzero `lef_expressible` count post-fix is a gate failure to
  chase, **not** a residual.

## Falsification arms

- The RED first-contact and WIP measurements are the demonstrated
  nonzero paths (recorded in `docs/results.md`).
- The committed mutation DEF (step D) must flag ≥1 rule vs clean —
  trivially non-vacuous once the golden reads clean (every fired
  rule is new).
- `hull_masked > 0` gate assert — the mask fires.

## Files

- `crates/grus-core/src/drt/graph.rs` (`HullMask`, `PinCover` union,
  `transit_extend`, covered-part test)
- `crates/grus-core/src/drt/maze.rs` (`ObsPin`, `exempt_obs`,
  confinement admissions, closure pass)
