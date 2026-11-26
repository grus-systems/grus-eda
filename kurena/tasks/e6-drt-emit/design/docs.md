# e6-drt-emit — Documentation updates (post-rejection revision)

All English, existing style. The `v0.12.5` dated entry already
records the landed emit + the measured 42-finding picorv32 gate —
**do not rewrite it**; this revision appends the patch outcome as a
follow-on block in the same entry (the record keeps the failure and
the fix in one place, same convention as the maze entry's digest
addendum).

## `docs/e6-drt.md`

§7 "Maze search" gains a **post-extraction min-area patch**
paragraph, in the doc's claim-boundary voice:

- What it fixes: the stub-class component (one via + short stub —
  relaxed-via implied stubs and projected-pin access stubs) that
  lands under a layer's `AREA` bound; cite the measured picorv32
  classes (`li.6`×3, `met3.6`×39) with a pointer to `results.md`.
- The model: per-layer member rects (wires `±w/2`, via-metal of the
  pair default, member-pin shapes) merged by the checker's own
  `union::components`/`union_area`; conservative direction
  (`area_ours ≤ area_checker`, foreign metal can only help).
- The extension: preferred-axis node-walk, one graph-edge span per
  step, occupancy-strict (own `Net`/`Pin` hits only — no `Obs`, no
  foreign marks, no host-carve), inside the guide region by
  construction; endpoints are node coords, on-track.
- The residual contract: `RouteDb.area_residuals` names every
  component it cannot fix; a `*.6` gate finding without a residual
  is a model divergence, i.e. a bug — never a tolerance.

§8 keeps its emission content; add one line that patch wires are
ordinary `WireSeg`s — emit, roundtrip, and the census see them
identically. The §4 claim-boundary paragraph stays honest: subset
gate on our DEF, Magic deck is `e6-drt-e2e`.

## `docs/results.md`

Append to the `### 2026-11-24 — e6-drt-emit` entry (do not open a
new dated section — same ticket, same version line `v0.12.6` noted
inline):

- **Min-area patch** (one dense paragraph): the measured class, the
  fix (post-loop `route_nets` pass, preferred-axis in-region
  occupancy-strict extension), the `area_patches`/`area_residuals`
  census from both logs.
- **Fixture evidence**: the five `drt::maze::tests` fixtures and
  what each pins (fill, pin-merge, occupancy-blocked residual,
  region-bound residual, through-`route_nets` hook).
- **gcd legs**: `findings_total=0` re-confirmed under the patch,
  `area_patches`/`area_residuals` measured values, digest line
  (`0x48593d441074229b` or the explained new digest with cause).
- **picorv32 re-run** (`/tmp/emit-picorv32-patch.log`): fresh
  routed/unrouted/wirelength/via census **vs the committed
  baseline** — `li.6`×3 + `met3.6`×39 = 42 findings → the new
  `findings_total`, any residual findings each mapped to their
  `AreaResidual` record (net, layer, area, bound), wall clock vs
  the 30,658 s baseline. If the leg is still running at commit
  time, the block says exactly that (command + start time) — the
  previous entry's in-flight convention.
- **Checks run**: the exact commands (`drt::maze::tests`, the
  release emit suite, workspace test, clippy) and the
  `0.12.5 → 0.12.6` bump.

## `docs/status.md`

Update the head `Last updated:` paragraph's emit clause: the
self-DRC gate line becomes "0 findings on gcd; picorv32 re-run post
min-area patch — `<result>`" with the measured number (or "detached
re-run in flight"), plus one clause on the patch pass itself.
Everything else stands.

## `docs/roadmap.md`

The existing `e6-drt-emit` progress bullet gains one line: min-area
patch landed (stub-class fix) + the picorv32 gate delta headline.

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
