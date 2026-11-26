# drt-converge-stats — Output format & consumers (r3)

## CLI census lines — unchanged (`main.rs:2084-2104`)

Two lines inside the `== drt emit:` block, at column 0 after the
`guided=…` summary line:

```
guided=269 routed=188 unrouted=81 … iterations=8 route_ms=… …
iter_progress=269,94,87,81,81,81,81,81
iter_prov=25,6,0,0,0,0,0,0
```

- `iter_progress=` — `s.attempted` per entry (bad-set size
  entering each iteration). Entry count == `iterations`.
- `iter_prov=` — `s.prov_collisions` per entry (provisional
  collisions at iteration end; distinguishes persistent failures
  from collision ping-pong on a flat `attempted` tail).
- Comma-separated `u32`s, no spaces; empty `iter_stats` prints a
  bare key. Both lines deterministic; `route_ms` stays wall
  clock.

**r3: no CLI output change.** The informational monotonicity
print the memo asks for is test-side only — no new tokens on the
emit surface, keep-list arms unchanged, goldens unaffected.

## Test-side print — `maze_census` (new this round)

After the existing `{name}: iter_progress=… iter_prov=…` println
(`tests/drt.rs:1641-1653`), one informational line:

```
{name}: iter_progress_monotonic={true|false}
```

computed as `r.iter_stats.windows(2).all(|w| w[0].attempted >=
w[1].attempted)`. This is the memo's "downgrade to an
informational print comparing the counts": the property is
empirical (`model.md`), so it is reported for the record — a
`false` someday is a datum about the design input, not a test
failure. The `iter_progress` series itself is already printed,
so the location of any increase is visible without extra
fields. Boolean (not a literal `true` pin) keeps it honest on
every input `maze_census` runs.

## Consumers — unchanged

- `check_drt_ours.sh` step-E keep-list (:935-936): prefix arms
  `iter_progress=`/`iter_prov=` — the new print does not reach
  `cli_lines` (it is a test-side `maze_census` line, not CLI
  output; even if it ever were, an unknown prefix is dropped by
  the whitelist).
- `recount_unrouted.py`: ignores unknown prefixes by
  construction.
- `tests/drt.rs` golden gates: no assert string-matches these
  lines; coverage stays invariant-level + digest-level.
- `docs/e6-drt.md` §8 (:709-713): enumerates the two CLI lines —
  gains only the punctuation fix (`steps.md` §3).

## Reading (unchanged)

`attempted` tail still falling → convergence was progressing
when capped; flat tail → stuck. `iter_prov` tail nonzero →
collision churn, not dead ends. A rising `attempted` step → a
kept net was re-queued as a collision party (`model.md`).

## Files

- `crates/grus-core/tests/drt.rs` (test-side print only)
- `docs/e6-drt.md` (§8 punctuation)

Reference-only (no edits): `crates/grus-eda/src/main.rs`,
`tests/designs/check_drt_ours.sh`.
