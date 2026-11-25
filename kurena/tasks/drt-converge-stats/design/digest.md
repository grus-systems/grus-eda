# drt-converge-stats — Digest & test updates

## `route_digest` serialization (`maze.rs` ~3934-4028)

Append after `put(routed.iterations)` — end-of-stream, so every
existing field's byte position is untouched:

```rust
put(routed.iter_stats.len() as u32);
for s in &routed.iter_stats {
    put(s.attempted);
    put(s.routed);
    put(s.prov_collisions);
}
```

The digest is a `xxh3` over the canonical `RouteDb`; `iter_stats`
is a deterministic output of the run (same `BTree`-ordered,
fixed-order data as everything else serialized), so it belongs in
the digest by the field's own contract.

**Digest value changes on this commit** — expected, not a
divergence: no test pins a literal `0x…` (verified — the only
digest uses are print lines and equality asserts), and the
`*.drt.ours.reference.json` goldens carry no digest. The new gcd
value goes into the `docs/results.md` entry; historical dated
digest numbers stand as the record of their runs.

## Test updates (`crates/grus-core/tests/drt.rs`)

### `maze_census` (~1631-1665)

- Print the series: a second line after the census `println!`
  — `{name}: iter_progress={} iter_prov={}` with comma-joined
  `attempted`/`prov_collisions` (same spellings as the emit
  block, so the recorded line greps like the CLI's).
- Assert the `model.md` invariants on `r.iter_stats`:
  - `len() == iterations as usize`
  - `iter_stats[0].attempted == join.nets.len()` when non-empty
  - `attempted` non-increasing, `routed ≤ attempted` per entry
  - `last().prov_collisions == 0`
  - `last.attempted - last.routed == unrouted.len()`
  The picorv32 `#[ignore]`d census gets the same coverage for
  free — it calls `maze_census`.

### New test: `test_route_digest_covers_iter_stats`

Small, no routing — the fail-proof that the new field is actually
inside the digest (E0 rule 7: a test that cannot fail is no
gate). `RouteDb::default()` plus a hand-set `iter_stats`:

```rust
#[test]
fn test_route_digest_covers_iter_stats() {
    let a = RouteDb {
        iter_stats: vec![IterStat { attempted: 3, routed: 3, prov_collisions: 0 }],
        ..RouteDb::default()
    };
    let mut b = a.clone();
    b.iter_stats[0].prov_collisions = 1;
    assert_ne!(route_digest(&a), route_digest(&b));
}
```

Needs `IterStat` in the `use grus_core::drt::maze::{…}` list
(~1265) — `RouteDb` is already imported (1778).

### Unchanged tests

- `test_maze_determinism` — `assert_eq!(r1, r2)` already covers
  `iter_stats` via derived `PartialEq`; no edit needed.
- `test_maze_occupancy` — digests still differ (routes differ);
  the new field only widens coverage.
- The e2e golden gates — no golden carries digest or iteration
  stats; `cli_lines` joins are prefix-scoped (`output.md`).

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
