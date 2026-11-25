# drt-converge-stats — Output format & consumers

## Line grammar

Two new lines inside the `== drt emit:` block, printed at column 0
immediately after the `guided=…` summary line (`main.rs` ~2067-2083,
before `unrouted_kinds`):

```
guided=269 routed=188 unrouted=81 … iterations=8 route_ms=… …
iter_progress=269,91,83,81,81,81,81,88
iter_prov=0,7,3,1,0,0,0,0
```

- `iter_progress=` — `s.attempted` per entry: the bad-set size
  entering each iteration (spec's `iter_progress=6700,410,…`
  example). Entry count == `iterations` on the summary line —
  self-checking against the existing field.
- `iter_prov=` — `s.prov_collisions` per entry: the provisional
  collision count at each iteration's end. This is the spec's
  "(or still collides provisionally)" half of the motivation —
  `iter_progress` alone cannot distinguish persistent *failures*
  from persistent *collision ping-pong* (flat `attempted` +
  nonzero `prov` vs flat `attempted` + zero `prov`). One extra
  `println!`, same record.
- Comma-separated `u32`s, no spaces, no brackets. Empty
  `iter_stats` (no guided nets) prints a bare `iter_progress=` /
  `iter_prov=` — unconditional lines keep the block shape stable
  for parsers.
- `route_ms` stays non-deterministic wall clock; both new lines
  are fully deterministic functions of the route.

Reading: `attempted` tail still falling → convergence was
progressing when capped; flat tail → stuck. `iter_prov` tail
nonzero → collisions, not dead ends, dominated the endgame.

## Consumer 1: `check_drt_ours.sh` step-E keep-list (~line 934)

The `cli_lines` whitelist is prefix-based. Add two arms beside
`s.startswith("guided=")`:

```python
or s.startswith("iter_progress=") or s.startswith("iter_prov=")
```

Consequences (same shape as `drt-unrouted-census`'s `fail` arm):

- Committed goldens (`gcd`/`picorv32.drt.ours.reference.json`)
  unchanged — generation-time snapshots; the next regeneration
  simply carries the lines.
- No `ours_router` schema change — a `iter_progress` array is
  derivable from `cli_lines`; the JSON surface stays stable.
- Absolute-path guard `(?<![A-Za-z_.])/` cannot fire: digits,
  commas, `=` only.
- The `guided=` regex (`mg`, ~955-959) is unaffected — the new
  lines are separate, and `mg` does not anchor end-of-line.

## Consumer 2: `recount_unrouted.py`

No change. Its `cli_lines` walk matches `unrouted `/`fail
`/`stub_outside_cover `/`unrouted_kinds`/`unrouted_blockers`
prefixes; unknown prefixes are ignored by construction. Confirm
at implementation time (`recount_unrouted.py gcd` still exits 0).

## Consumer 3: `tests/drt.rs` gates

No golden-dependent assert touches these lines (`cli_lines`
joins are all prefix-scoped to other prefixes). The in-run
asserts are new and live in `maze_census` (`digest.md`).

## Consumer 4: `docs/e6-drt.md`

- §7 (Order and rip-up, ~556-570): one sentence — per-iteration
  `attempted`/`routed`/`prov_collisions` are recorded in
  `RouteDb.iter_stats`.
- §8 (CLI census, ~703-715): extend the printed-line
  enumeration with `iter_progress`/`iter_prov` and the
  hard-iteration `prov=0` note.

E0 lesson 8: doc updates ride in the same commit as the code.

## Files

- `crates/grus-eda/src/main.rs`
- `tests/designs/check_drt_ours.sh`
- `docs/e6-drt.md`
