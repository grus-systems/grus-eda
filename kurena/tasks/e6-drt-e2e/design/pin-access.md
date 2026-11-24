# e6-drt-e2e r3 — pin access / unrouted recovery

> **r4 status — PARTIAL, remainder deferred.** A1 (union-cover
> probing) and the A3 `ACCESS_K` 3→8 widening landed at `b372d5e`;
> measured `unrouted` 73 → 81 (`NoPath` 57→45, `Unattachable` 33→57
> — the closure's notch rejections push pins out, exactly the
> interaction this file predicted). Per the r4 memo the remaining
> rungs are not in this revision: the 81 nets are the ticket's named
> connectivity residual (pinned in `gate.md`, listed in
> `docs/results.md`). The ladder below stays documented for the
> follow-up ticket.
>
> **r5 note**: the `fba5f85` residual fixes shifted the set once
> more — `unrouted=82` (`NoPath=44 Unattachable=58`). The pin is the
> golden's `ours_router.unrouted` list itself (`gate.md` §4 — a join,
> not a literal), so the residual moves with re-measurement.

The connectivity bill: `unrouted=73` on gcd at `4c5e2cb`
(`unrouted_kinds NoPath=57 Unattachable=33` — kinds overlap;
`unrouted_blockers l1Obs=4 l1Net=48 l2Net=1`;
`stub_outside_cover` = 111 pins). The memo's direction: recover via
the **off-track pin-anchored access model** — `maze::access_vias`
already exists (pin-rect-anchored via candidates with b-node
landings, used when no legal projected paint) — it is the model to
widen, not replace. Sequenced **after** the self-spacing fix
(unrouted wires emit no geometry, so spacing is measured clean
first).

## Failure decomposition (measured at 4c5e2cb)

- **`Unattachable` (33 nets / 111 pins)** — every projected access
  stub crossed an uncovered hull or every `access_vias` arc's
  via+stub hit a foreign mark (`Obs`/`ObsPin`/foreign `Net`/`Pin`).
  `stub_outside_cover` is this census: **one line per pin that got
  no legal paint** — the `Unattachable` diagnostic, not the
  "nonzero is a bug" invariant the r2 docs claimed (that claim
  predates the access model; the code comment is updated to match).
  Post-recovery target: **empty** — every connectable pin has an
  attach; any surviving entry names a genuinely unattachable pin and
  lands in the named residual list.
- **`NoPath` (57)** — attach sets exist and are graph-reachable but
  the wave dies against committed foreign `Net` marks on met1
  (`l1Net=48` of 53 attributed blockers) — in-hull met1 congestion
  around host-open channels, plus 4 `Obs` + 1 met2 `Net`.

## The existing model (`access_vias`, ~maze.rs:1278)

Per member pin shape on a masked layer: compute the position range
where the via's lower plate fits `⊆` the pin rect, probe
{edges, midpoint, in-range upper tracks}, keep up to `ACCESS_K=3`
candidates passing confinement (mask on both layers + all plates)
plus a full occupancy+stub scan; each candidate carries a landing
node `b` on an upper-layer line inside the rect's projection. At
emit the pin-attach block walks `topo.access[pi]` in order and
commits the first arc whose paint clears live occupancy.

Limits that starve it today: default via candidate only;
{edges,mid,in-range-track} probes only; `b` restricted to upper
lines inside the rect's own projection; per-sub-rect fit
(union-spanning plates rejected by the old per-rect cover test —
fixed by `hull-model.md`'s union-exact cover).

## Expansion ladder — evaluate in order, measure unrouted after each

Do not stack rungs blind: each lands only if the previous leaves
measured residual, each is independently revertable, none relaxes a
hard block.

### A1 — union-cover probing

With union-exact cover (`hull-model.md`), the plate-fit range in
`access_vias` is computed against the pin's merged component union
instead of a single sub-rect — plates spanning joined same-pin
sub-rects (the delegated 65-partial class) become candidates.
Cheapest rung, lands with the cover change itself.

### A2 — all via candidates, not just `default`

`vias.pairs[k].candidates` may hold several plate profiles; the
emitter still drops `default` at placement time (emitted = priced),
but the *admission* probes every candidate — a non-default plate
whose footprint clears the blocking mark is a legal attach the
default-only probe never sees. Deterministic order: candidate index,
then existing probe order.

### A3 — off-track probe raster + wider K

Probe positions beyond the upper tracks: the plate-fit range
rastered at a fine deterministic step (e.g. half-pitch, then DBU
where the range is smaller), candidates ordered by measured
clearance to the nearest foreign mark (existing `occ` scan — reuse,
don't add a new metric). `ACCESS_K` 3 → ~8. This is the memo's
"off-track" leg: a legal attach position need not sit on a track —
only the b-landing stub must reach an in-tree node legally.

### A4 — extended b-landing

`b` currently must be an in-tree node on an upper line inside the
rect's projection. Extend to index-adjacent upper lines (the
`relaxed_vias` L-corridor reach): the stub stays axis-aligned, the
landing fans out to the nearest clear node — recovers pockets whose
in-rect landings are committed by earlier nets (`l1Net` class).

### Residual protocol

Whatever stays unrouted after the landed rungs is **named**:
`unrouted` net list + `unrouted_kinds` + `unrouted_blockers` census
+ the `stub_outside_cover` pin list — all already emitted and kept
in the golden; the names go into `docs/results.md` verbatim. A
surviving `stub_outside_cover` entry must map to a named
`Unattachable` net — no free-floating diagnostics.

## Interaction with the self-spacing fix

Rule A confinement + Rule B closure (`self-spacing.md`) may push a
few more pins into `Unattachable` (rejected notch candidates) — that
is expected and absorbed here, not a regression to chase first. The
access work's own emits get the same closure pass for free (it runs
in extraction for every net).

## Guardrails (unchanged — do not do these to regain routed count)

- Do not widen `hull_masked_layer`, do not make met2+ masked, do not
  unmask li1, do not open foreign hulls.
- Do not soften `ObsPin(foreign)`/`Obs(_)` hard blocks or shrink
  `w/2`/`s` in scans.
- Do not emit geometry the confinement checks rejected — the
  expansion widens *candidates*, every one still passes the same
  scans before commit.

## Files

- `crates/grus-core/src/drt/maze.rs` (`access_vias` expansion,
  `ACCESS_K`, `stub_outside_cover` semantics/comment)
- `crates/grus-core/src/drt/graph.rs` (cover components feeding the
  fit ranges)
- `crates/grus-eda/src/main.rs` (census lines only if the log gains
  a new field — `unrouted_blockers`/`stub_outside_cover` are already
  emitted and kept)
- `tests/designs/check_drt_ours.sh` (keep-list only if new lines
  ship)
