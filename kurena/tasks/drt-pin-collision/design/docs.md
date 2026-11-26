# Docs — `e6-drt.md`, `results.md`, `status.md`

## `docs/e6-drt.md` edits

The doc is stale in two ways: it still describes `Pin` marks as
covering "instance pins and io-port rects alike" (pre-`ObsPin`
split) and states `Pin` "never a block". Fix both in this change.

1. **Occupancy table** (~L515–523): the `Pin(foreign)` row changes
   from `soft — never a block` to
   `+(OCC_PENALTY + hist·HIST_STEP)` in soft iterations / **hard
   block** on the last; add an `ObsPin` row (own free, foreign hard
   block — already true in code).
2. **The prose after the table** ("`Pin` never hard-blocks because
   rip-up cannot move a pin — charging it as congestion churns the
   queue… a foreign wire landing on pin metal is an emit-time
   finding") — replace with the new contract: a foreign `Pin` hit is
   soft-priced and recorded as a *directed* provisional
   `Collision` (wire net → pin net) that re-queues only the wire;
   on the hard iteration it blocks, and a surviving overlap is
   named in `RouteDb.pin_collisions`. The rip-up-cannot-move-a-pin
   rationale stays as the reason for *wire-side* rip-up.
3. **`premark_obs` paragraph** (~L541): instance pins → `ObsPin`,
   io-port rects → `Pin` — the split already in code; document it.
4. **Order-and-rip-up paragraph** (~L556): re-queue = unrouted ∪
   both parties of net×net collisions ∪ **the wire side only** of
   wire×pin collisions.
5. **`overlap_census` paragraph** (~L577): add `pin_overlap_census`
   / `RouteDb.pin_collisions` (directed `a`=wire, `b`=pin-owning
   net) and its `Enabled` gating.
6. **§7 projection fallback** if it claims stubs admit on cover
   alone — align with the new occupancy admission.
7. Code-side doc comments (`maze.rs`): `Occupant::Pin` (~L268),
   `hit_verdict` (~L810), `project_attach` (~L726),
   `IterStat.prov_collisions` (~L207), `Collision` (directed use).

## `docs/results.md` dated entry skeleton

```
### <date> — drt-pin-collision: foreign io-port hits cost the wire (v0.12.13)

**Gap (re-baselined post-e2e)**: `ObsPin` already hard-blocks
instance pins; remaining gap was io-port `Pin` marks — soft-priced,
unrecorded, unblocked. gcd committed routes: `pin_overlap_census`
= <measured, expected 0> (met3.2 ours=0 corroborates) — gap latent,
not absent.

**Fixture red-proof** (`test_maze_pin_collision`, arm B,
0 800 5000 1200 met1 confined guide): pre-fix
`pin_collisions = 1`, net routes silently through the port band →
red. Post-fix: `unrouted = [n1]`, `blockers` attributes (met1, Pin),
`pin_collisions = 0` → green. Arm A (open guide): <detour
description>, `pin_collisions = 0`, no wire in the inflated port
rect.

**gcd census** (release, `test_maze_census --nocapture`):
`iter_progress=<…>` `iter_prov=<…>` `collisions=<…>`
`pin_collisions=0` `unrouted=<…>` `digest=<…>` vs baseline
`0xa6d8b74a740a7489` — <delta or unchanged; root cause if moved>.

**picorv32**: <ran `test_maze_census_picorv32 --ignored` or
deliberately skipped — say which>.

**Coverage gap**: `project_attach`/Proj-stub scans change
unexercised by a dedicated arm (fixture.md note) — admitted
strictness mirroring the access-arc path.

Suite: `cargo test --workspace` <pass>, clippy `-D warnings` <pass>.
```

## `docs/status.md`

Update the E6 row only if a pinned number moved (e.g. a new named
residual or a changed unrouted population). Otherwise leave — the
row's claim boundary text is unaffected.

## Golden files

`gcd.drt.ours.reference.json` / `picorv32.drt.ours.reference.json`
regeneration is **conditional** (`steps.md` §6): only if the emit
census (`iter_prov`, `cli_lines`) moves. The conditional
`pin_collisions` print keeps the census byte-identical when empty,
which is the expected outcome.
