# drt-converge-stats — Digest & test updates (r3)

## `route_digest` — unchanged (`maze.rs:4061-4067`)

`iter_stats` stays serialized, appended after
`put(routed.iterations)` — end-of-stream, so no pre-existing
digest input's byte position is touched:

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

`iter_stats` is a deterministic run output; it belongs to the
digest's contract and to the acceptance item. The digest value
moved on the landing commit (gcd `0xab23bcc1997e4484` →
`0xa6d8b74a740a7489`); no test or golden pins a literal, so no
expectation file needs regenerating. **r3 changes nothing here**
— removing the monotonicity assert does not alter any recorded
value.

## Test updates — `crates/grus-core/tests/drt.rs`

### `maze_census` — corrected assert set (this round's change)

Prints (`:1641-1653` + new line, `output.md`):

```
{name}: iter_progress=<attempted,…> iter_prov=<prov,…>
{name}: iter_progress_monotonic={true|false}
```

Asserts — all structural, all unchanged (`:1686-1716` minus the
removed block):

- `iter_stats.len() == iterations as usize`
- `iter_stats[0].attempted == join.nets.len()` (non-empty)
- `last.prov_collisions == 0`
- `last.attempted - last.routed == unrouted.len()`
- `s.routed <= s.attempted` per entry

**Removed:** the `windows(2)` assert `w[0].attempted >=
w[1].attempted` (`:1708-1713`). Per the review memo and
`model.md`: `bad[i+1]` can gain kept nets re-queued as
provisional-collision parties, so attempted non-increase is
empirical, not a structural invariant — it must not gate the
test. It is now the informational `iter_progress_monotonic=`
print above. The stale comment at `:1680-1685` ("the re-queue is
a subset") is rewritten to match the mechanism.

The picorv32 `#[ignore]`d census inherits the same coverage.

### `test_route_digest_covers_iter_stats` — unchanged (`:1821-1838`)

Fail-proof that `iter_stats` reaches the digest (E0 rule 7):
hand-built `RouteDb`, flip one `prov_collisions`, digests must
differ.

### Unchanged tests

- `test_maze_determinism` — `RouteDb` `PartialEq` covers
  `iter_stats`.
- `test_maze_occupancy` — digests still differ on differing
  routes.
- e2e golden gates — no golden carries digest or iteration
  stats.

## Files

- `crates/grus-core/tests/drt.rs`

Reference-only (no edits): `crates/grus-core/src/drt/maze.rs`.
