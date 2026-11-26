# drt-converge-stats — Overview (r3)

E6 observability ticket. `route_nets` runs a capped rip-up loop
(`MAX_ITERS = 8`, last iteration hard — `maze.rs` ~3760-3847).
`RouteDb.iter_stats` records one `IterStat{attempted, routed,
prov_collisions}` per iteration, the `== drt emit:` census prints
them as `iter_progress=`/`iter_prov=` lines, and `route_digest`
serializes the vector. All of it landed at 08141b1 with
gcd/picorv32 evidence in `docs/results.md`.

## Revision r3 — review-memo reconciliation

The memo rejects one claim in the semantics, not the feature:

**`attempted` non-increase is empirical, not structural.** After
each soft iteration `bad[i+1] = failed[i] ∪ parties(prov[i])`
(`maze.rs:3819-3828`). `parties(prov[i])` resolves through
`net_idx` to *any* guided net — a provisional `Collision`'s
foreign party can be a *kept* net whose `Net(id)` marks still sit
in `occ` from an earlier iteration and which is not a member of
`bad[i]`. A re-queued kept net lands in `bad[i+1]` from outside
`bad[i]`, so `bad[i+1] ⊆ bad[i]` is false as a semantic fact and
`attempted` may rise. It did not rise on either recorded design
(gcd `269,94,87,81,…`; picorv32 `6744,2557,…,2326`) — that makes
it an empirical observation to report, not an invariant to assert
(E0 lessons 4–5, 8: docs and gates must match code semantics).

### Delta this round (corrections only, no behavior change)

1. `tests/drt.rs` `maze_census` — remove the
   `w[0].attempted >= w[1].attempted` `windows(2)` assert
   (:1708-1713); downgrade to an informational print comparing
   the counts (`iter_progress_monotonic=`); fix the stale comment
   (:1680-1685) claiming "the re-queue is a subset". All other
   `iter_stats` asserts stay — they are structural (`model.md`).
   `maze.rs` comments were audited: the `// bad = unrouted ∪
   both parties…` note (:3818) states the construction and never
   claims ⊆ — no source edit.
2. `docs/results.md` — entry header `2026-11-25` → `2026-11-26`
   (the picorv32 splice ran 2026-11-26); annotate the gcd legs
   inline as run 2026-11-25 so the header change loses no
   provenance; the two sentences that list asserted invariants
   (:10204-10208, :10237-10241) drop "non-increasing" from the
   asserted set and restate it as observed-only.
3. `docs/e6-drt.md` — §8 (:709-713) fix the `—,` em-dash+comma
   join; §7 (:560-562) gains one clause naming that a collision
   party may be a kept net, so `bad[i+1]` need not ⊆ `bad[i]`.

### Unchanged (verified at HEAD 08141b1 — do not touch)

- `IterStat` fields, `RouteDb.iter_stats`, all collection sites —
  `maze.rs:194-231, 3755, 3763, 3768, 3776, 3809-3813, 3899`;
  re-export `drt.rs:21`.
- CLI census lines `iter_progress=`/`iter_prov=` —
  `main.rs:2084-2104`. Byte-identical; no new tokens. The
  informational monotonicity print is test-side only
  (`output.md`).
- `route_digest` serialization — `maze.rs:4061-4067` (end-of-stream
  append after `put(routed.iterations)`).
- `check_drt_ours.sh` step-E keep-list — :935-936.
- `test_route_digest_covers_iter_stats` fail-proof —
  `tests/drt.rs:1821-1838`.
- `docs/status.md`, `Cargo.toml` 0.12.11.

## Boundary vs `drt-progress`

`drt-progress` (spec only, unlanded) adds a *periodic stderr*
progress line. This ticket's lines are the final census record in
stdout — complementary channels, do not merge per the spec.

## Non-goals

- No routing-behavior change: `bad` computation, rip-up, history
  escalation, hard mode — all untouched. Removing the assert does
  not change what the loop computes; it stops gating on a
  property the mechanism does not guarantee.
- No new output channel, no CLI output change, no golden
  regeneration, no `ours_router` schema change.
- No per-net iteration attribution — counts only, per spec.

## Files

- `crates/grus-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
