# e6-drt-e2e — residual decomposition + bounded closure (r4)

> **r10 status — unchanged.** Verify-bound revision only (r10
> memo — `verify-timing.md`); every residual vector still empty,
> `PINNED_LEF_RESIDUALS = []`.

> **r7 status — unchanged at `5d1502b`** (alias refactor only; every
> residual vector still empty, `PINNED_LEF_RESIDUALS = []`).
>
> **r6 status — FULLY CLOSED at `0d113ba`.** §4's `_099_` did **not**
> survive to a pin: the `met12-attribution.md` probes classified it
> X3 alongside the 6 fill sites — the same `try_fill` host-open carve
> bug had starved its residual-slab fill. Post-fix every residual
> vector is empty (`notch_residuals=0 hole_residuals=0
> area_residuals=0`, `self_drc findings_total=0`) and
> `PINNED_LEF_RESIDUALS` is `[]` — the §5 protocol remains the
> contract for any future count, exercised but unused.
>
> **r5 status — LANDED at `fba5f85`, partially superseded.** §1 dedup
> (`notch_fills` 296 → 221), §2 patch same-net guard, §3
> `close_voids` all shipped and measured in the 20:07 transcripts:
> `self_drc findings_total=0`, `hole_residuals=0` (the step-E JSON
> was never persisted — `steps.md` §B regenerates it).
> §4's `_099_` is confirmed as `notch_residuals=1` — **but the
> container measure surfaced 6 further `met1.2` sites the census did
> not predict** (28 of the 32 fragments): the residual decomposition
> continues in `met12-attribution.md`. This file stays as the §1–§4
> mechanism record.

The `b372d5e` emit already carries its own diagnostics: one
`notch_residual` line, two `self_drc` findings, and visibly
duplicated `RECT` fills in the committed DEF. This file decomposes
each into a named site + mechanism, the bounded fix it implies, and
the residual-pinning protocol for whatever survives. **No site is
guessed** — every entry below is evidenced in the committed DEF or
the emit log; the container measurement in `steps.md` confirms the
mapping before the golden commits.

## §1 — Duplicate fills (bug, fix unconditionally)

`_055_`'s emitted NETS record contains `met1 ( 74652 42399 )
RECT ( 0 0 98 61 )` **three times verbatim**, `( 74652 42459 )
RECT ( 0 0 98 97 )` four times, `( 74652 42555 ) RECT ( 0 0 145 46 )`
twice. `close_self_spacing`'s `seen` set dedups *prong pairs*, not
*fill rects*: three pieces pairwise facing the same gap produce the
same pruned fill once per pair (`(p,q)`, `(p,r)`, `(q,r)` →
identical rect each).

- **Fix**: dedupe `fills` before commit — `fills.sort_unstable()` +
  `dedup()` (or a `BTreeSet` accumulator). Overlapping-but-distinct
  fills stay; only byte-identical rects collapse. Geometry is
  unchanged (identical rects merge in Magic), `notch_fills` drops to
  the true count (296 → less), determinism preserved (sort order is
  canonical).
- **Test**: emit-side assert or a unit fixture — a three-piece
  collinear notch yields one fill rect, not three.

## §2 — `_007_` met1.2: post-closure paint (patch hypothesis — verify, then guard)

Self-DRC finding: `met1.2 [70450,55460,70770,55720] x
[70820,55520,71300,55660] gap=(50,0) bound=140 nets=_007_,_007_` —
a *separated-islands* pair (self-DRC's island model flags it, so the
two rects do not touch anywhere on met1). DEF anatomy: rect 1 is the
`M1M2_PR` via plate at (70610,55590); rect 2 is the wire
`(70890,55590)–(71230,55590)` ± w/2. Both are tracked paint classes
— a pair like this cannot pass `close_self_spacing` unseen, so the
offending geometry post-dates the closure:

- **Primary suspect — `patch_net_min_area`** (`area_patches=1`, runs
  per net inside `route_nets` *after* `route_one_net` returns):
  `patch_component` extends a sub-bound component along in-plane
  edge spans, scanning `occ` for *foreign* marks only — a growth
  that stops `< s` short of another own-net component creates
  exactly this notch. Verify first: instrument (temporary print or
  debug assert) which net the single patch fires on, or match the
  grown span to a guide edge — confirm `_007_` before fixing.
- **Secondary suspect — non-default via candidates**: the closure
  scans `pair.default` plate rects only; if an emitted via ever uses
  another candidate its real plate is unscanned. Check whether
  `relaxed_vias`/`access_vias` can commit a non-`default` candidate
  index into `vplaced`; if yes, scan the *placed* candidate's rects
  instead.
- **Fix (confirmed patch case)**: `patch_component` admission gains
  the same-net disjointness rule — a candidate growth is legal iff
  it merges into every own-net component it approaches (touch ⇒
  merge ⇒ free) or keeps `≥ s` from each. The components are already
  re-derived per patch iteration (`union::components` +
  `comp_rects`); the check is a per-candidate gap measurement, no
  new substrate.
- **Residual arm**: if the site survives (e.g. it is not the patch),
  it is a named site under the §5 protocol, not a silent count.

## §3 — `_055_` met1.7: fill-induced enclosed hole (fix: void-closure)

Self-DRC finding: `met1.7 [74360,42400,74652,42600] area=54680
bound=140000 nets=_055_` — `MINENCLOSEDAREA`. The committed DEF
anatomy names the mechanism: three met1 wires form a U
(`(73270..74750,42330)`, `(74290,42330..42670)`,
`(74290..74797,42670)`); the emitted `RECT` fills at `xl=74652`
seal the right side, enclosing exactly the flagged void. The notch
fills are individually legal — the *enclosed void they create* is
the violation the piece-pairwise scan cannot see.

- **Fix — void-closure pass**, inside `close_self_spacing` after the
  notch fixpoint, per layer:
  1. `solid` = the layer's committed pieces + admitted fills (+ member
     pin shapes, already in `pieces`); `window` = component bbox —
     reuse `union::enclosed_voids` (the E7 checker's own hole model,
     verified 0-finding on both designs) over each connected
     component of the layer's own paint.
  2. Each hole with `area_dbu2 < min_enclosed_area_dbu2` → emit
     covering fills: the hole's free-rect decomposition
     (`Hole.rects`) becomes same-net paint iff **every** covering
     rect passes the existing admission (`fill_admitted` — mask +
     `occ.scan_rect` + `exempt_obs`); all-or-nothing per void.
     A void containing an `Obs` mark fails admission the same way
     the checker's OBS-discard treats it — no special case.
  3. Iterate to fixpoint bounded like the notch loop (a void-fill
     strictly merges into its enclosing walls — it cannot create a
     new disjoint `< s` pair, and a fully-covered void cannot split).
- **Residual arm**: an unfillable under-bound void lands in a new
  `route_db.hole_residuals` (`{net, layer, bbox, area_dbu2,
  bound_dbu2}`) — printed `  hole_residual <net> <layer> area=…
  bound=… region=…` (the `area_residual` line convention), parsed
  into the golden, named in `docs/results.md`.
- **Note**: only *fills and committed paint* bound the void — no
  pin/OBS involvement needed for detection; if a void's wall is a
  member pin shape the pass still sees it (pin shapes are in
  `pieces`).

## §4 — `_099_` met1: named undroppable notch (residual, evidence-pinned)

`  notch_residual _099_ met1 a={41240,34170,41380,34510}
b={41470,34380,41790,34640}` — gap 90 × y-overlap 130. Both prongs
are `Tree`/`PinShape` paint (nothing droppable), and `try_fill`'s
shrink-retry found **no** residual slab touching both prongs —
foreign geometry or a foreign hull occupies the gap. Magic's
net-agnostic spacing will flag this site regardless of what sits
between the prongs.

- This is the memo's "small named residuals" case. It is *already*
  named by our own census — the honest outcome is a pinned residual,
  not silent acceptance: `gate.md`'s `PINNED_LEF_RESIDUALS` records
  `met1.2 → <measured count>` with this site named in the test
  comment, and `docs/results.md` carries the site bbox, the blocking
  class (from `drc why`/occupancy probe), and the reason reroute is
  out of scope.
- Before pinning, one cheap check: does the residual survive the §2
  patch guard and §1 dedup (a rerouted/changed local picture could
  dissolve it)? Re-emit once after the §1–§3 fixes; if the residual
  line persists unchanged, pin it.

## §5 — Residual protocol (the gate's honesty contract)

1. Every nonzero `lef_expressible.ours` count in the refreshed
   golden maps to a **named site** — a `notch_residual` /
   `hole_residual` / `area_residual` census line, or a `drc why`
   probe site written into `docs/results.md`. A count with no named
   site is unexplained: probe it (`drc why` + flattened tile dump,
   the r3 method), root-cause it, then fix-or-name. **Never
   rebaseline.**
2. `ours_router.notch_residuals` / `ours_router.hole_residuals`
   counts land in the golden (`golden.md`) so the machine-checked
   contract pins our *own* residual diagnostics against the same DEF
   Magic reads — the residual claim is evidence-backed, not just a
   tolerated number.
3. `PINNED_LEF_RESIDUALS` in the gate test is the reviewed list:
   `(rule, ours_count, site)` per entry; every non-pinned rule
   asserts `ours == placed` strictly (`gate.md`).
4. `self_drc findings_total` and Magic's `lef_expressible` need not
   agree site-for-site (self-DRC merges touching paint into islands
   — within-island notches like `_099_` are its declared blind
   spot); the decomposition explains each difference explicitly.

## Expected outcome after §1–§3

`met1.7 → 0` (void closed or named), `met2.2 → 0` (all 15 jog sites
were in-closure notches), `met1.2 →` the `_099_` site (a few bbox
fragments) plus whatever the §2 probe attributes — then the
container measurement either confirms this map or adds sites, which
enter the §5 loop. The `notch_fills` count is recorded, never a
target — dedup makes it honest, not small.

## Files

- `crates/grus-core/src/drt/maze.rs` (fill dedup, void-closure in
  `close_self_spacing`, `hole_residuals` on `RouteDb`, patch
  same-net guard in `patch_component`, via-candidate scan if §2's
  secondary suspect confirms)
- `crates/grus-eda/src/main.rs` (`hole_residual` print + census
  tokens — `scripts.md`)
- `tests/designs/check_drt_ours.sh` (census regex + `ours_router`
  fields + keep-list — `scripts.md`)
- `crates/grus-core/tests/drt.rs` (residual pins — `gate.md`)
