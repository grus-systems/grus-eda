# drt-converge-stats — Data model & collection (r2)

**Status: landed at ca053ac.** This file is now the semantics
contract the landed code and asserts implement — keep it in
step with `maze.rs` (E0 lesson 8). Line numbers are HEAD.

## `IterStat` — `maze.rs:194-212`

One record per iteration of the `route_nets` rip-up loop
(`maze.rs` ~3760-3847), exported through `crate::drt::maze` and
re-exported in `crate::drt` (`drt.rs:21`):

```rust
/// One iteration of the `route_nets` rip-up loop — the convergence
/// record behind the census `iter_progress`/`iter_prov` lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IterStat {
    /// Nets queued into the iteration — `bad.len()` at entry;
    /// entry 0 is always `graphs.nets.len()` (every net attempts
    /// once before any rip-up exists).
    pub attempted: u32,
    /// Nets that committed a route this iteration — counts
    /// attempts, not distinct nets (a re-queued net that routes
    /// again counts again). `attempted - routed` is the
    /// iteration's failure count.
    pub routed: u32,
    /// Deduped provisional `Collision`s recorded by this
    /// iteration's committed routes — the re-queue trigger. Always
    /// 0 on the hard (last) iteration, where foreign hits block
    /// instead of reporting, and on every iteration under
    /// `OccMode::Disabled`, where recording is skipped.
    pub prov_collisions: u32,
}
```

`RouteDb.iter_stats` — `maze.rs:227-231`, after `iterations`:

```rust
/// Per-iteration convergence record — `iter_stats[i]` describes
/// iteration `i + 1` (1-based like `iterations`); `len() ==
/// iterations` and ≤ `max_iters` by loop construction.
pub iter_stats: Vec<IterStat>,
```

`iterations` stays: it is already the census/digest scalar;
`iter_stats` is the series behind it, not a replacement.

## Collection points in `route_nets` — landed

Additive only, no reordering — declaration and constructor
outside the `while it < cap && !bad.is_empty()` loop, three
touches inside its body:

1. `maze.rs:3755` — `let mut iter_stats: Vec<IterStat> =
   Vec::new();` beside `unrouted`/`iterations`.
2. `maze.rs:3763` — `let attempted = bad.len() as u32;` after
   `it += 1; iterations = it;`.
3. `maze.rs:3768` — `let mut routed = 0u32;` beside
   `prov`/`failed`; `maze.rs:3776` — `routed += 1;` inside the
   `NetOutcome::Routed` arm.
4. `maze.rs:3809-3813` — `iter_stats.push(IterStat { attempted,
   routed, prov_collisions: prov.len() as u32 });` **after** the
   per-net `for` closes and **before** `if hard { … break; }`.
   Push position matters: the hard iteration must be recorded,
   and `prov` is final for the iteration at that point (all
   `prov.extend(hits)` calls happen inside the routed arm).
5. `maze.rs:3899` — `iter_stats` in the `RouteDb` constructor
   literal (the only construction site; `#[derive(Default)]`
   covers the rest).

## Semantics and edge cases

- **Entry 0** = `bad = (0..n)` — every guided net; `attempted[0]
  == graphs.nets.len()` is the anchor invariant.
- **Soft iteration i**: `routed` = commits; `attempted - routed`
  = failures that re-queue; `prov_collisions` = the deduped
  collision census whose *parties* also re-queue. `bad[i+1] =
  failed[i] ∪ parties(prov[i]) ⊆ bad[i]`, so `attempted` is
  non-increasing across the series.
- **Hard iteration** (`it == cap`): `prov` is never populated
  (`if !hard && cfg.occ == OccMode::Enabled`), so
  `prov_collisions = 0` — honest, not missing; hard-mode failures
  are the `unrouted` census instead. `routed` still counts
  hard-mode commits.
- **Early convergence** (`bad` empties before `cap`): the last
  recorded entry is a soft iteration with `prov_collisions == 0`
  and `failed == 0` — same `prov == 0` last-entry property as a
  capped run.
- **`OccMode::Disabled`**: `prov_collisions` is 0 on every entry
  (recording skipped); `attempted`/`routed` still describe the
  failure-only retry loop.
- **`n == 0` / `bad` empty at entry**: loop never runs → empty
  `iter_stats`, `iterations == 0`. Print emits empty RHS.
- **Re-attempted nets**: `routed` counts commits per attempt, so
  `sum(routed) ≥ nets.len()` is possible; it is not a union count.

## Invariants asserted (`tests/drt.rs:1680-1716`, `maze_census`)

- `iter_stats.len() == iterations as usize`
- `iter_stats[0].attempted == join.nets.len()` (non-empty case)
- `attempted` non-increasing (`windows(2)`); `routed ≤ attempted`
  per entry
- `iter_stats.last().prov_collisions == 0` (always — see above)
- `last.attempted - last.routed == unrouted.len()` (hard-cap:
  the failures become `unrouted`; early-converge: both sides 0)

The picorv32 `#[ignore]`d census gets the same coverage for free
— it calls `maze_census`. `test_maze_determinism`'s `RouteDb`
equality covers the field via derived `PartialEq`.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
