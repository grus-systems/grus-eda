# drt-converge-stats — Digest & test updates (r2)

**Status: landed at ca053ac.** Line numbers are HEAD.

## Memo ruling on digest discipline

The review memo says: "stats are a print/recording path, keep
route_db digest inputs unchanged **unless the design says
otherwise**." This design says otherwise — `iter_stats` stays in
the digest, per the field's own contract: `route_digest` is an
`xxh3` over the canonical `RouteDb`, and `iter_stats` is a
deterministic output of the run (same `BTree`-ordered,
fixed-order data as everything else serialized). The acceptance
item "stats are part of the deterministic digest" requires it.

The "unchanged" half of the ruling still binds: **no pre-existing
digest input may be touched** — no reordering, no re-scoping, no
removed fields. The landed serialization satisfies this by
construction (end-of-stream append, below).

## `route_digest` serialization — `maze.rs:4061-4067`

Appended after `put(routed.iterations)` — end-of-stream, so every
existing field's byte position is untouched:

```rust
put(routed.iterations);
put(routed.iter_stats.len() as u32);
for s in &routed.iter_stats {
    put(s.attempted);
    put(s.routed);
    put(s.prov_collisions);
}
xxh3_64(&buf)
```

**Digest value changed on the landing commit** — expected, not a
divergence: no test pins a literal `0x…` (verified — the only
digest uses are print lines and equality asserts), and the
`*.drt.ours.reference.json` goldens carry no digest. gcd moved
`0xab23bcc1997e4484` → `0xa6d8b74a740a7489`, recorded in the
`docs/results.md` entry; historical dated digest numbers stand
as the record of their runs.

## Test updates — `crates/grus-core/tests/drt.rs` (landed)

### `maze_census` — :1641-1653, :1680-1716

- Prints the series after the census `println!`:
  `{name}: iter_progress={} iter_prov={}` with comma-joined
  `attempted`/`prov_collisions` — same spellings as the emit
  block, so the recorded line greps like the CLI's.
- Asserts the `model.md` invariants on `r.iter_stats` (len ==
  iterations, `[0].attempted == guided`, non-increasing,
  `routed ≤ attempted`, last `prov_collisions == 0`,
  `last.attempted − last.routed == unrouted.len()`). The
  picorv32 `#[ignore]`d census gets the same coverage for free.

### `test_route_digest_covers_iter_stats` — :1821-1838

The fail-proof that the new field is actually inside the digest
(E0 rule 7: a test that cannot fail is no gate). `RouteDb::default()`
plus a hand-set `iter_stats`:

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

`IterStat` is in the `use grus_core::drt::maze::{…}` list (:1265).

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
