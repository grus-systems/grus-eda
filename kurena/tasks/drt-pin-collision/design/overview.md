# drt-pin-collision — overview

## Problem (external review S1)

A committed wire may cross a foreign pin shape with no router-side
consequence: it is priced but never recorded, never ripped up for it,
and never forbidden even on the hard iteration.

## Re-baseline verdict (post-`e6-drt-e2e` dev, measured by reading
`maze.rs` at this worktree's HEAD)

`e6-drt-e2e` landed **most** of this ticket's scope already:

- `Occupant::ObsPin(m)` exists — instance-pin metal inside a cell hull
  is free to its own net and a **hard block to every other net** in
  `hit_verdict` (`maze.rs` ~L827). `premark_obs` marks instance pins
  `ObsPin` and clips their host OBS against the pin rects.
- Every emit-side admission site already treats `Pin(m ≠ own)` as
  blocking: `access_vias`' `confined` (~L1508), `try_pin_paint`'s
  access-arc scan (~L2770), `fill_admitted`/`fill_blocked` (~L3187),
  the void-closure `solid` scan (~L3101), and the min-area patch walk
  (~L3537). The comment at ~L2747 states the rule: "emit never gets a
  soft pass".
- The `blockers` postmortem already encodes `Pin` as `kind = 3`
  (~L2326) and the emit census prints `unrouted_blockers`.

The **remaining gap is io-port rects only** — `Occupant::Pin(m)` now
marks bterm metal outside every hull (`premark_obs` ~L673:

`Some(n) if db.pin_cells[p].is_some() => Occupant::ObsPin(n.0)` /
`Some(n) => Occupant::Pin(n.0)`). On those marks:

1. `hit_verdict` (~L844): `Pin(m)` pools into the soft `foreign`
   price set but never sets `blocked` — soft-priced in every
   iteration including `hard`.
2. `edge_collisions` (~L1017): records `Occupant::Net` hits only —
   a wire-over-port overlap never enters `prov`, so the wire is
   never re-queued for it.
3. `try_pin_paint`'s `AttachElem::Proj` stub (~L2700–2743): admission
   checks `mask.seg_allowed` only — no occupancy scan. The projected
   stub is committed metal and is the cheapest overlap to produce
   (the spec's `project_attach` point: `project_attach` ~L730
   likewise admits on cover alone).
4. `overlap_census` (~L3920): rasterizes only committed `Net` marks —
   `Net × Pin` pairs are unreportable, so `RouteDb.collisions` is
   structurally blind to the class.
5. Postmortem arm (~L2297): the `or_else` blocker search covers
   `hard && Net(m ≠ own)` only — a hard-mode `Pin` block would not
   attribute `kind = 3`.

Baseline evidence that the gap is real but currently latent on gcd:
`gcd.drt.ours.reference.json` shows `lef_expressible` 0/50 rules and
`collisions=0` — today's committed routes happen not to land in a
foreign port band, but nothing forbids it. The `met3.2`
`req_msg`/`resp_msg` class was exactly this finding before `Pin`
marks existed at all.

## Mechanism decision

**Mirror the `Net` contract for `Pin`: record the hit as a directed
provisional collision in soft iterations (rip-up re-queues the wire
side only), and hard-block it on the last iteration.** Plus strict
occupancy admission for the two attach-stub generators that never
scanned, and a `pin_overlap_census` that names any survivor in
`RouteDb.pin_collisions`.

Rejected alternatives:

- **Always-hard-block (`ObsPin`-style).** An io port lives in the open
  routing region, not inside a hull; its `w/2 + s` band is a wall with
  real area. Hard-blocking from iteration 1 removes the priced escape
  valve and grows `unrouted` before rip-up can spend iterations
  finding a detour. The `Net` two-phase is the proven pattern: price,
  record, escalate, block last.
- **Record-only (never block).** That is today's bug — a surviving
  overlap emits silently.
- **Hard-block the rect only, not the band.** Magic measures
  *spacing*, not overlap — the band verdict is the correct unit and
  matches every other mark.

Deadlock-avoidance is inherited from the `Net` mechanism and
tightened by the wire-only re-queue — `deadlock.md`.

## Honesty contract

- Any committed `Net × Pin` overlap that survives to emit is named
  in `RouteDb.pin_collisions` (directed: `a` = wire's net, `b` =
  port's owning net) — expected empty; an entry is a named residual,
  never silent.
- A net whose only access crosses a foreign port band fails named:
  `UnroutedNet` (`NoPath`) with `blockers` kind 3, or
  `stub_outside_cover` for attach-stub exhaustion. Never a hang —
  `deadlock.md`.
- The fixture is proven red on the unfixed verdict before the fix
  lands — `fixture.md` / `steps.md`.

## Files

- `crates/grus-core/src/drt/maze.rs` — verdict, collision record,
  re-queue, census, digest, stub admission.
- `crates/grus-core/tests/drt.rs` — `test_maze_pin_collision`
  fixture + `test_route_digest_covers_pin_collisions`.
- `crates/grus-eda/src/main.rs` — conditional `pin_collisions`
  census print (nonzero only; zero is byte-identical output).
- `docs/e6-drt.md` — occupancy table + `premark_obs`/rip-up/census
  paragraphs (`Pin(foreign)` row and the stale "never a block"
  prose).
- `docs/results.md` — dated entry.
- `docs/status.md` — E6 row only if the fix moves a named number.
- `Cargo.toml` — `workspace.package` patch bump (0.12.12 → 0.12.13).
- `tests/designs/check_drt_ours.sh` / `tests/designs/gcd/*.reference.json`
  — only if `iter_prov` or the census line moves on gcd (contingency,
  `steps.md` §6).
