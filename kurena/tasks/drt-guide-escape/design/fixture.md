# Fixture — pin outside its guide on a lower layer

Synthetic `MAZE_LEF`/`MAZE_DEF_HEAD` fixture (tests/drt.rs ~L1300):
tracks 0..5000 step 1000 on met1/met2; met1 is `hull_masked_layer` +
`hull_host_open_layer`, met2 is the first unmasked layer.

## Layout

```
- u0 CELL + PLACED ( 0 0 )          A → abs rect (0,0)-(200,200) met1
- u1 CELL + PLACED ( 3900 3900 )    A → abs rect (3900,3900)-(4100,4100) met1
PINS:
- io1  rect (3900,3900)-(4100,4100) met1   — contains (4000,4000)
- io2  rects (2900,0)-(3100,3000)  met1+met2 — the foreign band (arm C)
```

Net `n1` = `io1 + u1/A + u0/A`; guide =
`4000 0 5000 5000 met1` + `4000 0 5000 5000 met2`.

- `u1/A` and `io1` strict-attach at `(4000,4000)` — the "live" side.
- `u0/A`'s rect covers only pair `(0,0)`; every region pair sits at
  x ≥ 4000 → strict-escape → today's `region_escape` +
  `Unattachable`.

Net `n2` = `io2` plus a second pin placed wherever the impl needs it
(a lone port is enough if `premark_obs` marks unguided ports — verify
at impl; else add `u2` so `n2` is a real guided net or just live in
occupancy).

## Arms (all inside `test_maze_guide_escape`)

| Arm | Config | Expected |
|-----|--------|----------|
| **A red** | `escape: None` | `n1` unrouted; `u0/A` `Unattachable`; `region_escape` names the pin; `escapes` empty |
| **A green** | `escape: Some(DEFAULT)` | `n1` routed; `u0/A` ∈ `connected`; `escapes` record `Halo`, `span = 2`, `pairs_added > 0` |

Geometry: `u0/A` halo = {0,1000,2000}² on met1+met2 (pin bbox
(0,0)-(200,200) ⊕ 2 index steps); nearest region pair (4000,y) ⇒
`span = 2` ⇒ corridor adds `(3000, y)` on the argmin layer ⇒ halo
merges ⇒ strict attach at `(0,0)` ⇒ wave completes.

| Arm | Config | Expected |
|-----|--------|----------|
| **B bound** | `escape: Some({halo_lines: 2, corridor_lines: 1})` | `n1` unrouted; `u0/A` `Unattachable`; record `CorridorBound`, `corridor_span = 2` |

Proves the bound is a real gate, not decoration: same geometry,
bound 1 < span 2.

| Arm | Config | Expected |
|-----|--------|----------|
| **C occupancy** | `escape: Some(DEFAULT)` + `io2` band | `n1` unrouted; `u0/A` `NoPath` (halo attaches at `(0,0)` but every lane x∈{2900..3100} y∈{0..3000} carries the foreign `Pin` mark) |

The contrast run — same fixture, `OccMode::Disabled` — routes `n1`:
corridor/halo metal crosses the band unpriced. Enabled-vs-disabled
difference = escape metal is occupancy-checked (acceptance #2),
mirroring `test_maze_occupancy`'s fail-proof idiom. Band spans both
corridor-candidate layers so the argmin tie-break can't slip around
it.

| Arm | Config | Expected |
|-----|--------|----------|
| **D masked** (optional) | `escape: Some(DEFAULT)`, guide = **met1 box only**, + `u3 BLK` obs cell covering x∈[2500,3500] y∈[0,2000] | met2 `region` empty ⇒ `span = ∞`; met1 corridor's `(3000,y)` dies inside the BLK hull at `point_allowed` ⇒ record `CorridorMasked`; `u0/A` attaches to the halo island but `NoPath` |

If impl shows `CorridorMasked` unreachable by construction (the
first-unmasked corridor always survives its mask check), drop arm D
and either drop the variant or document why it's retained — a named
record that can never fire is dead code (E0 §4).

## Red proof (E0 §7)

1. Land the fixture + `GraphOpts` plumbing with `escape: None`
   asserting the red row (it is today's behavior — runs green
   immediately as a *baseline* pin).
2. Run the `Some(DEFAULT)` arm against the unfixed tree → expected
   red (`Unattachable` where the test wants routed). Capture output
   for `results.md`.
3. Land `escape_extend` → all arms green, no assertion edits.
4. Bonus break-it check: after green, temporarily raise
   `ESCAPE_HALO`/`CORRIDOR` or drop `point_allowed` filtering → an
   arm must go red (arm D red if the filter is dropped); restore.
