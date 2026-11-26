# Docs edits — `docs/e6-drt.md` + `docs/results.md` + `docs/status.md`

The implementation commit already wrote the §5 mechanism paragraph,
the §8 `fail`-line format, and the `results.md`/`status.md` entries.
The revision corrects three claims and adds one token.

## `docs/e6-drt.md`

1. **§5 outcome sentence** — the paragraph ends "…the refused kinds
   add nothing for that pin, so `pin_nodes` stays empty →
   `Unattachable`." Wrong as a rule: empty `pin_nodes` yields
   `Unattachable` **only when every route-time attach rung is also
   empty** — a pin whose `transit_extend` island (planted for every
   member-pin host, escape switch or not) still lands an
   `access_vias` `b`-arc attaches and fails `NoPath` instead.
   Rewrite the tail as: refused kinds add no pairs; the pin's final
   `FailKind` follows the route-time attach ladder —
   `Unattachable` when every rung is empty, `NoPath` when a
   transit-island/projection/access landing survives (the fixture's
   escape-off arm is the latter).
2. **§5 outcome list** — add `no_anchor` (no non-halo region pair
   on any escape layer — the corridor had no target; replaces the
   `u32::MAX`-span `CorridorBound` case). Note the span probe is a
   bounded windowed enumeration (`cap = corridor_lines` for
   candidates, uncapped on the refusal path for the exact recorded
   gap), not a grid BFS.
3. **§5 occupancy sentence** — extend "(no exemption path)" with:
   proven by the fixture's `OccMode` enabled-vs-disabled arm on a
   foreign `Pin` band that geometrically covers only escaped pairs
   (`Obs`/`ObsPin` cannot contrast — they hard-block before the
   `Disabled` check).
4. **§8 `fail`-line token list** — add `escape=no_anchor`; keep
   `corridor_bound(span=N)` and state N is the measured gap
   (uncapped refusal probe, never a sentinel).

## `docs/results.md` — amend the 2026-11-26 entry

1. **Fixture line, corrected in place.** The committed entry says
   "escape off → `Unattachable` + `region_escape`" — the test
   asserts `NoPath` and the router semantics agree (fixture.md §
   "Why escape-off is NoPath"). Replace with `NoPath` +
   `region_escape` and one clause naming the transit-island
   `access_vias` landing; mark it as a review-driven correction.
2. **Append the revision paragraph:**

```
**Review revision** (same ticket): the contract fixes — escape-off
fails `NoPath` (transit-island access landing), not `Unattachable`;
`EscapeKind::NoAnchor` names the no-target case (was a `u32::MAX`
span); `pairs_added` counts distinct inserted pairs (was
double-counting the corridor layer's halo); `escape_span` is a
bounded windowed enumeration (was an `nx×ny` BFS + allocation per
escaped pin per layer — identical spans, bounded work).
Arm F occupancy contrast: <enabled NoPath / disabled routed
measured>; kind census re-print: gcd <…>, picorv32 <…> (deltas vs
{halo:7, halo_empty:10} / {halo:425, corridor_masked:9,
halo_empty:158} named by pin); graph census pins <moved/unchanged>;
routed DEF sha <unchanged | old→new>.
```

## `docs/status.md`

In the v0.13.0 blurb (unreleased, edit in place): the token list
becomes `halo`/`corridor_bound`/`corridor_masked`/`no_anchor`/
`halo_empty`; no other named number moves unless the re-measure says
so.
