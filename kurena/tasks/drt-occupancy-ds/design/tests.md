# tests — differential oracle + regression pin

Three layers of proof: (1) a randomized differential test against a
verbatim copy of the pre-change structure, (2) the existing
byte-exact DEF golden, (3) a `route_digest` literal pin for the
parts the DEF does not serialize.

## 1. `RefOccupancy` oracle (maze.rs `#[cfg(test)] mod tests`)

Copy the pre-change `Occupancy` **verbatim** — `BTreeMap<i32,
Line>` fields, `rasterize(tracks, …)`, `insert`, `rebuild_from`,
`unmark` on a copied `RefMark { x_axis, layer, line: i32, iv }`,
`scan_wire(axis, l, t: i32, c, d, out)`, `scan_rect(l, r, out)` —
renamed `RefOccupancy`, minus `stats`. Sharing `Line`, `Interval`,
`Occupant`, `Hit`, `SKIP_CHUNK`, and the `insert`/`rebuild_from`
helpers is fine (they're unchanged); the oracle diverges only in
the keying. If `insert`/`rebuild_from` gain a coupling to the new
fields during implementation, copy them too — the oracle must be
the old *semantics*, not the old spelling.

## 2. Differential harness

Seeded `xorshift64*` PRNG (~10 lines in the test module — no `rand`
dependency exists and none is added). One synthetic track set per
fixture, mixing:

- uniform grids (the `toy_tracks` shape) and **non-uniform** lists
  (merged-grid irregular spacing — stresses `partition_point`
  windows),
- a layer with an empty `tracks_x` or `tracks_y` (the `TrackGap`
  case — `scan_rect` on it must hit nothing; `scan_wire` is never
  reachable by contract since no index exists),
- several layers (`≥3`) so per-layer indexing is exercised.

Op stream (~5k ops, fixed seeds — at least two):

- `rasterize`: rects with mixed sizes — point-ish pin boxes
  (~140–700 DBU), long thin wires (aspect ≥20:1), rects covering
  zero lines inside the band on one or both axes (the
  "contributes nothing" arm), rects fully outside all track
  coordinates, rects straddling the outermost track. Random
  `Occupant` of all four classes, incl. `Obs(u32::MAX)`.
- `unmark`: pick a random live mark index; each structure consumes
  its own log entry (`Mark.line_idx` vs `RefMark.line`) — logs stay
  in lockstep because insertion order is identical.
- `scan_wire`: random (axis, layer, track index, `c ≤ d`) — include
  indices with no marks and spans `[c,d]` both inside and outside
  the marked extent.
- `scan_rect`: random rects incl. off-grid, degenerate (`xl==xh`),
  and oversized (covers every line).

Compare per query: sort each `Vec<Hit>` by `(line, iv.lo, iv.hi,
iv.perp_lo, iv.perp_hi, who-tag, lo, hi)` and assert equality —
assert the **multiset**, and separately assert raw `Vec` order is
also identical on ≥1 seed (slice order == BTreeMap order; if the
raw-order check ever fails that's a finding to record, not a
tolerance). On mismatch, print the op index + query so the failure
is reproducible.

Edge arms asserted explicitly outside the random stream:

- rect whose band covers no line on either axis → marks nothing;
  scans still return prior marks only.
- marks inserted, fully unmarked, reinserted — tombstone-free
  equivalence.
- same `(lo)` interval inserted twice (equal-`lo` stability) →
  scans agree including hit multiplicity.

## 3. `tests/drt.rs` regression pin

Inside `maze_census` for `name == "gcd"` (single route run —
no second `route_nets`), assert:

```rust
const GCD_ROUTE_DIGEST: u64 = 0x…;   // measured at the swap commit;
                                     // 0x8a4768ab15bd4bd5 at
                                     // drt-guide-escape HEAD — verify
assert_eq!(route_digest(&r), GCD_ROUTE_DIGEST, "{name}: route digest
moved — an occupancy-identical change must not move it; a real
routing change re-pins with a results.md entry naming the cause");
```

`route_digest` serializes `nets`/`unrouted`/`collisions`/
`pin_collisions`/`iter_stats` — the collision census rides inside.
Byte-exact emit is already pinned by
`test_e2e_ours_def_deterministic_gcd`; keep that test unmodified.

## 4. Prove-it-red (AGENTS §7.7) — recorded in the results entry

1. Corrupt the new `scan_rect` window (`a..b` → `a+1..b`) → the
   differential test must go red; restore.
2. Drop one `perp_*` conjunct in the new `scan_rect` filter →
   differential red (or, if the corpus misses it,
   `test_e2e_ours_def_deterministic_gcd` / the digest pin red —
   record which fired); restore.
3. `GCD_ROUTE_DIGEST` ±1 → pin red; restore.

## 5. Suite gates

`cargo test --workspace --profile verify` and `cargo clippy
--workspace --all-targets -- -D warnings` (`workflow.toml`). The
differential test is a unit test — it runs in the suite, no PDK
needed beyond what `maze.rs` tests already build (`toy_*` fixtures
and synthetic track lists only; do **not** hang it on the gcd PDK —
keep it input-free like the neighboring occupancy tests).
