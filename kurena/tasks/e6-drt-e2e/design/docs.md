# e6-drt-e2e — doc updates (r4)

All in `docs/`, written at the end of implementation with the final
measured numbers — not pre-written claims.

## `docs/results.md` — dated entry

Structure (E7-style, plus the fix trajectory):

- **Trajectory line**: `lef_expressible` violations on our DEF
  ~3025 (first contact) → 847 (`d8d0c6c` mask) → 514 (`4c5e2cb`
  confinement WIP) → final post-closure measurement. Each hop names
  what landed and what was measured; the b372d5e emit census
  (`routed=188 unrouted=81 notch_fills=…` dedup'd) is recorded
  verbatim.
- **Root-cause record** (evidence-attached, two layers):
  - the r3 measurement behind the 514: `met1.2=740`/`met2.2=56`
    decomposed into same-net classes (45 plate-pair sites ≈665
    tiles; ~75 wire-end tiles; 15 met2 jog sites); `drc why` +
    flattened-dump anatomy; `who5.tcl` move-via causality proof;
    `.mag` inspection refuting subcell-internal met1; census tables.
    Probe files named (`runs/drt-ours/who*.tcl`, `region.mag`,
    `gcd.whofix.def` — gitignored).
  - the r4 residual decomposition (`residuals.md`): duplicate fills
    on `_055_` (dedup evidence — identical `RECT` records verbatim);
    the `_055_` `met1.7` fill-induced hole with the DEF anatomy
    (fills sealing the wire-U's right side); the `_007_` `met1.2`
    post-closure site with the confirmed attribution (patch growth
    or via-candidate — whichever the probe showed); the `_099_`
    `notch_residual` site with its blocking class.
- **Gate table** (final): per-rule `lef_expressible` ours vs placed
  vs routed_ref — with `PINNED_LEF_RESIDUALS` entries named per
  rule + site; `magic_only` decomposition; `unclassified_fired`
  status; `sanity_control` result.
- **Comparison table**: `delta.vs_baseline`/`vs_delegated` —
  wirelength, vias per cut layer, antenna counts, signal nets
  routed; named outlier explanations (the wirelength delta is
  restated "on the N routed nets" with unrouted names listed, per
  the honest-delta convention).
- **Connectivity record**: `unrouted` final list (all 81 named) +
  `unrouted_kinds` (NoPath=45 Unattachable=57 — the 73→81 move is
  explained: notch-rejection pushed attach candidates out, as
  `pin-access.md` predicted) + `unrouted_blockers` census +
  `stub_outside_cover` final state; `notch_fills` (dedup'd) +
  `notch_residuals`/`hole_residuals` counts (the repair cost,
  recorded not hidden).
- **picorv32 deferral — explicit** (r4 memo): the leg is un-run;
  `picorv32.drt.ours.reference.json` is not committed; the ignored
  gate test carries the schema joins for when the leg runs.
- **What ran** (E0 rule 9): which script steps, which legs, which
  container digest; any partial reruns named.

## `docs/e6-drt.md` — e2e section + self-DRC scope correction

- e2e section: gate outcome on gcd, the fix narrative at summary
  level (hull mask → confinement → self-spacing closure → residual
  closure — mechanisms point at `kurena/tasks/e6-drt-e2e/design/`
  docs, results has the numbers); picorv32 deferral stated.
- **Self-DRC scope — corrected**: the §8 inventory needs precision,
  not a new claim. Measured at b372d5e: self-DRC's spacing family
  checks same-net *separated islands* (it caught `_007_`) and the
  area family checks enclosed holes (it caught `_055_`) — the blind
  spot is specifically **within-island same-net notches** (touching
  paint merges before the pair check — the `_099_` class), which the
  emit-side closure pass owns. Amend §8/`§7`'s scope sentence to
  exactly that; the claim boundary stays honest: self-DRC remains a
  fast pre-gate, not the deck.
- Emit-grammar note: `RECT` fill pieces in the NETS section — landed
  at b372d5e (self-spacing fills + void fills); the §8 statement
  list gains the `RECT` form.

## `docs/status.md` + `docs/roadmap.md`

E7-style done line scoped honestly: "native DRT under committed
guides — Magic `drc(full)` `lef_expressible` on gcd: clean except
N named residuals (per-rule, per-site in results.md) [or: clean];
81 unrouted nets named; deltas vs baseline/delegated in results.md;
picorv32 leg deferred". No "fully routed" claim — the line carries
the unrouted count and the residual count.

## Files

- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `docs/roadmap.md`
