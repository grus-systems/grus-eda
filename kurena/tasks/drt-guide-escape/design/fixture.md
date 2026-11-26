# Fixture — pin outside its guide on a lower layer

`test_maze_guide_escape` in `tests/drt.rs` uses `MAZE_LEF` +
`ESC_LEF_WALL` (`MACRO WALL`, `SIZE 1.1 BY 5.0`, no pins/OBS — a
pure-hull cell) and `ESC_DEF_TAIL`. Tracks: `0..5000` step `1000`
on met1/met2; met1 is the `hull_masked` layer, met2 the first
unmasked.

## Layout

```
- u0 CELL + PLACED ( 900 900 )      A → rect (900,900)-(1100,1100) met1
- w0 WALL + PLACED ( 1400 0 )       hull x∈[1400,2500] × y∈[0,5000]
PINS: io1  rect (3900,900)-(4100,1100) met1 — contains (4000,1000)
Net n1 = io1 + u0/A.
```

Guides: `3000 0 5000 2000 met1` (arms A/B/F), `4000 0 5000 2000 met1`
(arms C/D/E). u0/A's only met1 crossing is (1000,1000), outside every
guide box → strict-escape at graph build in all arms.

The wall is the confinement device: met1 is hull-masked, so every
met1 pair inside `x∈[1400,2500]` fails `point_allowed` (foreign
hull, not own-cover) and every met1 stub east of the pin dies at
`seg_allowed` — the graph cannot cross the wall on met1, period.
met2 is unmasked: `transit_extend` plants u0's host-hull interior
pair `(1000,1000)` — a one-node island, since the guide covers no
met2 pair for its corridor to reach (`base.is_empty()` → hull pairs
only, no transit corridor).

## Why escape-off is `NoPath`, not `Unattachable` (contract fix)

The earlier design predicted `Unattachable`; the router's semantics
give `NoPath`, and the code asserts it. `Unattachable` requires
*every* attach rung empty: strict `pin_nodes` are empty here, but
`transit_extend` plants the met2 island node for the pin's host
hull **independent of the escape switch**, and the pin-anchored
`access_vias` rung (u0/A's met1 shape is a masked-layer instance
shape — eligible) lands `b` on that island node inside the pin's
own hull → `attachable` holds → the wave seeds from the island and
dies on a node with no edges → `NoPath`. `Unattachable` is reserved
for pins with no attach rung at all — e.g. an io port (no host hull
→ no transit island, and `access_vias` skips ports) whose
projection also finds nothing.

`region_escape` names u0/A in every arm either way — the flag is
the pre-halo graph-build statement, not the route outcome.

## Arms

| Arm | Config | Expected |
|-----|--------|----------|
| **A off** | `escape: None` | `escapes` empty; `region_escape` = {u0/A}; `pin_nodes` empty; `n1` unrouted, u0/A **`NoPath`** — bit-identical to the pre-escape build |
| **B halo** | `escape: Some(DEFAULT)` | `region_escape` still names u0/A; record `Halo`, `corridor_span = 0`, `pairs_added > 0`, token `halo`; `pin_nodes` non-empty; `n1` routed, both pins `connected` |

Arm B geometry: halo `halo_lines = 2` on {met1, met2} = pin bbox
(900,900)-(1100,1100) ⊕ 2 index steps → `ix, iy ∈ {0..3}` per layer.
met1 keeps `ix ∈ {0,1,3}` (the `ix = 2` column dies inside w0's hull)
minus region pairs `(3, y∈{0,1,2})`; met2 keeps all 16 minus the
transit pair `(1,1)`. met1 `(3,3)` is adjacent to region `(3,2)` →
span-1 merge → `Halo`, span 0. The routed path goes met1 → via at
(1,1) → met2 across `ix = 2` (unmasked, over the wall) → via down at
`ix = 3` → met1 guide → io1. **The met2 halo strip at `ix = 2` is
the load-bearing escaped metal** — arm F exploits that.

| Arm | Config | Expected |
|-----|--------|----------|
| **C bound** | guide `4000 …`, `escape: Some({halo_lines: 0, corridor_lines: 0})` | record `CorridorBound`, `corridor_span = 3`, `pairs_added = 0`, token `corridor_bound(span=3)`; `pin_nodes` empty; u0/A `NoPath` |
| **D masked** | same guide, `Some({halo_lines: 0, corridor_lines: 8})` | corridor `(1,1)→(4,1)` on met1 crosses the wall → dies at `point_allowed` → `CorridorMasked`, `corridor_span = 3`, `pairs_added = 1` (the one legal halo pair `(1,1)` stays — an island attach), token `corridor_masked`; `pin_nodes` non-empty; u0/A `NoPath` |
| **E landing** | open fixture (`maze_db`, no wall), guide `4000 …`, `Some({halo_lines: 0, corridor_lines: 8})` | corridor lands on met1 → `Halo`, `corridor_span = 3`, token `halo(span=3)`, **`pairs_added = 3`**; `n1` routed |

Arm E's `pairs_added == 3` is the union-count proof: met1 halo
`{(1,1)}` + path `{(2,1),(3,1)}` = 3 distinct pairs; the met2 halo
`{(1,1)}` is already `region` (the transit island) so it is skipped
and adds nothing. The retired arithmetic `halo_total + extra.len()`
reported 4 — the corridor layer's halo counted twice.

| Arm | Config | Expected |
|-----|--------|----------|
| **F occupancy** | wall fixture + `ESC_DEF_TAIL_OCC` (below), `escape: Some(DEFAULT)` | `OccMode::Enabled` → `n1` unrouted, u0/A `NoPath`; `OccMode::Disabled` → `n1` routed, 2 connected |

Arm F is the occupancy contrast — the arm the first design specced,
the first implementation dropped silently, and the review told us to
restore or justify. It is restored, in a form sharper than the
original: **every met2 pair the band can hit is an escaped pair**
(non-escape met2 `region` is exactly the `(1,1)` transit island; the
guide covers no met2 at all), so the block is attributable to escape
metal by construction, not by argument.

The band must be a foreign **`Pin`** mark — `hit_verdict` blocks
`Obs` and foreign `ObsPin` *before* the `OccMode::Disabled` check
(`maze.rs` ~L850–863), so OBS rects and foreign instance pins
hard-block in both modes and cannot contrast; `Net` marks would
need a routed foreign net. An io port on a second net is the one
light-weight mark that works — this constraint is why arm F is a
`Pin` band and not a `BLK`-style OBS wall.

```
ESC_DEF_TAIL_OCC:
PINS 2 ;
- io1 + NET n1 ... met1 ... + PLACED ( 4000 1000 ) N ;
- io4 + NET n2 ... met2 ( -100 -3600 ) ( 100 3600 ) + PLACED ( 2000 1500 ) N ;
END PINS
NETS 2 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
- n2 ( PIN io4 ) + USE SIGNAL ;
END NETS
```

io4's absolute rect ≈ `x∈[1900,2100] × y∈[−2100,5100]` rasterizes
`Pin(n2)` on met2 line `x = 2000` (the only track inside the
`w/2 + s ≈ 210` perp band) covering every y-track, and on every
y-track line covering `x∈[1900,2100]` — so **every** met2 edge on
the `ix = 2` escape column or crossing the band horizontally hits
the mark; `ix = 1` and `ix = 3` stay clear (band edge-to-track ≥ 800
> 210). n2 is an unguided one-pin net — never routed, but
`rasterize_pin_marks` paints `Pin(n2)` regardless. (If a lone-port
net trips the DEF reader, add a second `io5` port on `n2` placed in
a corner — same contract.)

- Enabled: the soft `Pin` price lets early iterations commit a
  band-crossing route, `prov_pins` re-queues n1, rip-up escalates
  `hist`, no band-free path exists, and the hard last iteration
  returns `None` on every corridor-crossing edge → `NoPath`.
- Disabled: `hit_verdict` returns `Some(0)` for the `Pin` hits → the
  identical graph routes.
- Assert in both runs: the `escapes` record is `Halo` — the graph
  build is occupancy-independent; the check is route-time.

| Arm | Where | Expected |
|-----|-------|----------|
| **G no-anchor** | `test_region_escape_fixture`, both-boxes-under-grid arm | `escapes` = 2 × `NoAnchor`, `corridor_span = 0`, `pairs_added = 0`, token `no_anchor` |

REGION_LEF is single-layer met1 (masked — no unmasked layer exists,
so no transit either): the empty guide leaves `region[met1]`
entirely empty, the halo survives on met1, and the refusal probe
finds no non-halo pair on any escape layer → `NoAnchor` — the
explicit replacement for `CorridorBound` + `u32::MAX` (previously
rendered `corridor_bound(span=4294967295)`).

## Red proof (E0 §7)

1. Arm A is today's behavior — it ran green immediately as the
   baseline pin (`escape: None` bit-identical).
2. The `Some(DEFAULT)` arm ran **red** against the unfixed tree
   (`Unattachable`-era expectation; the mechanism commit turned it
   green) — output captured in `results.md`.
3. Arm F carries its own contrast: enabled `NoPath` vs disabled
   routed, one `route_nets` pair apart.
4. Break-it checks after green: (a) drop `point_allowed` on halo
   pairs → arm D red; (b) delete io4 → arm F's *enabled* run routes
   (the band, not the wall, was the blocker); (c) restore.
