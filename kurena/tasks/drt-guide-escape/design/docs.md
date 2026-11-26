# Docs edits — `docs/e6-drt.md` + `docs/results.md`

## `docs/e6-drt.md`

1. **§5 escape-classes paragraph (~L269–293)** — after the
   "Router implication" paragraph, add the mechanism paragraph:
   `escape_extend` stage (between `transit_extend` and node
   emission), the contiguous `pin_layer..=first_unmasked` halo rule,
   the bounded L-corridor + `EscapeCfg` constants with their
   rationale, and the pre-halo `region_escape` semantics. Update the
   closing sentence — "the remaining recorded gap" now includes the
   bounded-escape mechanism and its named refusal records.
2. **§7 attach ladder** — one paragraph after the `access_vias`/
   projection description noting that halo pairs enter `region`
   pre-emission, so strict/proj/access/`relaxed_vias` all see escape
   nodes with no maze-side code, and occupancy checks apply
   identically (no exemption path exists).
3. **§8 census format (~L752)** — extend the `fail` line doc:
   `    fail <inst>/<pin> <FailKind>[ region_escape][ escape=<token>]`
   with the five token shapes (`reporting.md` §1), and one sentence
   on `RouteGraphs.escapes` as the graph-side census.
4. **§4 claim boundary** — if the routed-vs-clean claim text still
   describes escapes as unrescueable, amend to "bounded halo rescue
   or named refusal".

## `docs/results.md` — dated entry template

```
## 2026-XX-XX — drt-guide-escape (bounded pin halo)

Re-baseline (post-e6-drt-e2e, worktree HEAD): gcd unrouted=81
(NoPath=46 Unattachable=52), 17 graph-build escapes pinned by name,
2 final `fail ... region_escape` lines (_134_/_301_/B1,
_138_/_307_/B1). Per-pin gap probe: <step-0 finding — which layer
lacked nodes>.

Mechanism: `escape_extend` graph-build pass — halo
(pin_bbox ⊕ ESCAPE_HALO=2 idx steps on pin_layer..=first_unmasked,
point_allowed-filtered) + bounded L-corridor (ESCAPE_CORRIDOR=8 idx
steps). EscapeCfg switch; region_escape keeps pre-halo semantics.

Post-run (gcd, release):
- escapes records: Halo=<n> CorridorBound=<n> CorridorMasked=<n>
  HaloEmpty=<n>; pairs added=<n>; corridor spans max=<n>
- unrouted=<n> (delta vs 81: <names of recovered/new-failed nets>)
- projected=<n> (delta vs baseline), stub_outside_cover=<n> (delta
  vs 142)
- digest 0x<old> → 0x<new>; routed DEF sha <old> → <new> (regenerated
  host-side via generate_drt_ours.sh)
- graph census re-pins: nodes <a>→<b>, edges <a>→<b>, via_edges
  <a>→<b>
- fixture: red (escape off → Unattachable+region_escape) → green;
  arm C enabled/disabled occupancy contrast <result>
- self-DRC on gcd emit: findings_total=<n> (<named residual if >0>)
- picorv32: graph-level escape census <counts>; emit leg deferred
  <decision>
- runtime: <route wall delta if measurable>
- known staleness: gcd.drt.ours.reference.json cli_lines predate the
  `escape=` suffix — internal-join gate stays green; regeneration
  is a container leg, deferred/approved <state>.
```

## `docs/status.md`

Only the E6 row, and only if `unrouted`/routed or a named residual
number actually moved.
