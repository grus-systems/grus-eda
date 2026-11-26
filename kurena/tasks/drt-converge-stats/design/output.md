# drt-converge-stats — Output format & consumers (r2)

**Status: landed at ca053ac.** Line numbers are HEAD.

## Line grammar — `main.rs:2084-2104`

Two lines inside the `== drt emit:` block, printed at column 0
immediately after the `guided=…` summary line (`main.rs:2069-2083`),
before the `unrouted_kinds` computation:

```
guided=269 routed=188 unrouted=81 … iterations=8 route_ms=… …
iter_progress=269,94,87,81,81,81,81,81
iter_prov=25,6,0,0,0,0,0,0
```

- `iter_progress=` — `s.attempted` per entry: the bad-set size
  entering each iteration (spec's `iter_progress=6700,410,…`
  example). Entry count == `iterations` on the summary line —
  self-checking against the existing field (verified on the gcd
  emit leg: 8 entries == `iterations=8`).
- `iter_prov=` — `s.prov_collisions` per entry: the provisional
  collision count at each iteration's end. This is the spec's
  "(or still collides provisionally)" half of the motivation —
  `iter_progress` alone cannot distinguish persistent *failures*
  from persistent *collision ping-pong* (flat `attempted` +
  nonzero `prov` vs flat `attempted` + zero `prov`).
- Comma-separated `u32`s, no spaces, no brackets. Empty
  `iter_stats` (no guided nets) prints a bare `iter_progress=` /
  `iter_prov=` — unconditional lines keep the block shape stable
  for parsers.
- `route_ms` stays non-deterministic wall clock; both new lines
  are fully deterministic functions of the route.

Reading: `attempted` tail still falling → convergence was
progressing when capped; flat tail → stuck. `iter_prov` tail
nonzero → collisions, not dead ends, dominated the endgame. The
gcd record: `269,94,87,81,81,81,81,81` + prov 0 from iteration 3
— a stable dead-end residue, not a convergence shortfall
(`docs/results.md` 2026-11-25 entry).

**Pin discipline:** no stdout-literal pin for the new lines —
consistent with every other census line (`guided=`, `unrouted `,
…), none of which has a string-match gate. Coverage is
invariant-level (`maze_census` asserts the series, prints the
line in the same spellings) + digest-level
(`test_route_digest_covers_iter_stats`). This is the memo's "test
pinning the census lines" item as designed.

## Consumer 1: `check_drt_ours.sh` step-E keep-list — landed :935-936

The `cli_lines` whitelist is prefix-based; the two arms sit beside
`s.startswith("guided=")`:

```python
or s.startswith("iter_progress=") or s.startswith("iter_prov=")
```

Consequences (same shape as `drt-unrouted-census`'s `fail` arm):

- Committed goldens (`gcd`/`picorv32.drt.ours.reference.json`)
  unchanged — generation-time snapshots; the next regeneration
  simply carries the lines.
- No `ours_router` schema change — an `iter_progress` array is
  derivable from `cli_lines`; the JSON surface stays stable.
- Absolute-path guard `(?<![A-Za-z_.])/` cannot fire: digits,
  commas, `=` only.
- The `guided=` regex (`mg`, ~957-961) is unaffected — the new
  lines are separate, and `mg` does not anchor end-of-line.

## Consumer 2: `recount_unrouted.py`

No change. Its `cli_lines` walk matches `unrouted `/`fail
`/`stub_outside_cover `/`unrouted_kinds`/`unrouted_blockers`
prefixes; unknown prefixes are ignored by construction. Verified
in the r1 run: `recount_unrouted.py gcd` exits 0 (recorded in
`docs/results.md`).

## Consumer 3: `tests/drt.rs` gates

No golden-dependent assert touches these lines (`cli_lines`
joins are all prefix-scoped to other prefixes —
`tests/drt.rs:2497-2559` walks `notch_residual `/`hole_residual
`/`unrouted `/`stub_outside_cover `). The in-run asserts live in
`maze_census` (`model.md`).

## Consumer 4: `docs/e6-drt.md` — landed

- §7 (Order and rip-up, :562-569): the per-iteration
  `attempted`/`routed`/`prov_collisions` recording sentence.
- §8 (CLI census, :709-713): the `iter_progress`/`iter_prov`
  enumeration + the hard-iteration `prov=0` note.

## Files

- `crates/grus-eda/src/main.rs`
- `tests/designs/check_drt_ours.sh`
- `docs/e6-drt.md`
