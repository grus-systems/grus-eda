# Fixture — `test_maze_pin_collision` (`crates/grus-core/tests/drt.rs`)

Two-net synthetic case in the `test_maze_cases` style: inline LEF/DEF
(`MAZE_LEF`/`MAZE_DEF_HEAD` reused), `maze_guide`, `route_nets`. Two
arms — a clean detour and a named failure — matching acceptance's
"clean route or named failure; proven red before the fix".

## Geometry

Reuse `MAZE_LEF` (met1 HORIZONTAL, met2 VERTICAL, `mv12`,
CELL/CELLB/CELLC/BLK macros, tight `SIZE`s so hulls stay local) and
`MAZE_DEF_HEAD` (5×5 grid, 1000-DBU pitch both axes/layers). The DEF
tail is custom — write a small `maze_db_custom(tech, tail)` builder
or a `MAZE_PIN_TAIL` const alongside `MAZE_DEF_TAIL`:

```
COMPONENTS 2 ;
- u0 CELL  + PLACED ( 900 900 ) N ;    # pin A: met1 rect → abs (900..1100, 900..1100)
- u1 CELLB + PLACED ( 3900 2900 ) N ;  # pin B: met2 rect → abs (3900..4100, 2900..3100)
END COMPONENTS
PINS 2 ;
- io1 + NET n1 ... + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 4000 1000 ) N ;
- io2 + NET n2 ... + LAYER met1 ( -100 -100 ) ( 100 100 ) + PLACED ( 2500 1000 ) N ;
END PINS
NETS 2 ;
- n1 ( PIN io1 ) ( u0 A ) + USE SIGNAL ;
- n2 ( PIN io2 ) ( u1 B ) + USE SIGNAL ;
END NETS
```

`io2`'s met1 port rect covers x∈[2400,2600], y∈[900,1100]; its
`w/2 + s = 210` band is x∈[2190,2810], y∈[690,1310] — the whole
y=1000 met1 row between io1 (4000,1000) and u0/A (1000,1000) passes
inside it. `n2` (io2 + u1/B) routes freely (port is its own mark;
u1/B reaches met2 via `mv12`).

## Arm A — clean detour (router must avoid the port)

Guide `n1`: `0 0 5000 2000 met1` `0 0 5000 5000 met2` — met1 rows
y ∈ {0, 1000, 2000}; only y=1000 sits inside the port band, so an
off-axis jog to y=0 or y=2000 (or a met2 leg) is legal. Guide `n2`:
`0 0 5000 5000 met1 met2`.

Assertions:

- `r.unrouted` empty, both nets routed and fully connected.
- `r.pin_collisions` **empty**.
- No `n1` `WireSeg` enters the inflated port rect
  `RectDbu::new(2190, 690, 2810, 1310)` — the same emitted-geometry
  assert style as the OBS detour case (~L1511–1516).

## Arm B — only access crosses the port (named failure)

Guide `n1`: `0 800 5000 1200 met1` — covered met1 rows = {y=1000}
only, so every wave path between io1 and u0/A lies inside io2's band;
no met2 guide box. Guide `n2` as in arm A.

Post-fix assertions:

- `unrouted` contains `n1` exactly, `failed` names its unreachable
  member `NoPath`, and `blockers` holds `(0, 3)` — met1 layer index
  0, `Pin` kind — the postmortem attribution (arm B is what proves
  mechanism.md §4).
- `r.iter_stats` shows `prov_collisions > 0` on a pre-cap iteration —
  the record path fired before the hard-mode block (evidence the
  rip-up trigger exists, not just the verdict).
- `r.pin_collisions` empty (the overlap never survives to emit).
- `n2` routed.

## Red-proof protocol (AGENTS §7.7)

Implement the census + fixture test **before** the verdict/record
changes (steps.md step 1). On the unfixed tree:

- Arm B commits the silent overlap (only path, soft-priced, never
  blocked) → `pin_overlap_census` reports it → the
  `pin_collisions.is_empty()`/`unrouted` assertions fail **red**.
- Record the red output in `docs/results.md` as the before/after
  evidence pair.

Then land the fix; both arms go green without touching the
assertions. Do **not** soften the asserts if arm A's detour comes
out different than sketched — the check is emitted geometry, not
wire count.

## Coverage note (named gap)

The `project_attach`/`try_pin_paint` stub scans (mechanism §5) are
not independently exercised by arms A/B — constructing a pin whose
*only* projected stub crosses a foreign port is possible (off-track
member rect + band over the sole candidate node) but adds fixture
surface disproportionate to a three-line strictness fix already
mirroring the access-arc precedent. If the implementer lands a
compact variant, add it as arm C; otherwise the coverage gap is
named here and in `docs/results.md`.
