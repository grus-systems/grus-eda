# drt-converge-stats — Overview

E6 observability ticket. `route_nets` runs a capped rip-up loop
(`MAX_ITERS = 8`, last iteration hard — `maze.rs` ~3734-3814) but
records only the scalar `RouteDb.iterations`. When the cap bites,
the operator sees `iterations=8 unrouted=N` and cannot tell a run
that was still converging (bad-set shrinking to the end) from one
that flatlined at iteration 2 — the external review's "8 iters
wasn't enough" guess. This ticket records the per-iteration counts
the loop already computes, prints them as a compact convergence
line in the `== drt emit:` census, and folds them into
`route_digest` so they are deterministic, checked output.

## State at HEAD (v0.12.10, what is already landed)

- `e6-drt-e2e` (and the picorv32 leg) has landed — the spec's
  "queue after `e6-drt-e2e`" note is satisfied; `maze.rs`'s
  iteration loop is the state this design is written against.
- `RouteDb.iterations` — iterations actually run, ≤ `max_iters`
  (`maze.rs` ~204), already in the census line and the digest.
- The loop already maintains the inputs we need: `bad` (the
  per-iteration attempt queue), `prov` (deduped provisional
  collisions, soft iterations under `OccMode::Enabled`), and
  `failed`. Only the collection is missing.
- `GRUS_DRT_TIMING` stderr already prints `iter {it} bad=…
  failed=… prov=…` — opt-in timing evidence, not a census
  record. This ticket is the *committed* equivalent: same
  numbers, in `RouteDb` and stdout.

## What lands

1. `IterStat { attempted, routed, prov_collisions }` — one record
   per iteration, `RouteDb.iter_stats: Vec<IterStat>` (≤
   `max_iters` entries by construction; `model.md`).
2. Two census lines after `guided=…`:
   `iter_progress=<attempted…>` and `iter_prov=<prov…>` —
   comma-separated, one number per iteration (`output.md`).
3. `iter_stats` serialized into `route_digest`; digest-sensitivity
   and census-invariant asserts in `tests/drt.rs` (`digest.md`).
4. `check_drt_ours.sh` step-E keep-list arms for the two new
   lines; committed goldens unchanged (generation-time
   snapshots). `docs/e6-drt.md` §7/§8 in step (`output.md`).

## Boundary vs `drt-progress`

`drt-progress` (spec only, unlanded) adds a *periodic stderr*
progress line and promises "stdout census block byte-identical"
— that promise is about its own diff. This ticket deliberately
extends the stdout census; the two are complementary channels
(periodic vs final) and must not be merged per the spec. Whichever
lands second simply rebases over the other's output surface — no
mechanism overlap (`drt-progress` adds a hook/callback;
`iter_stats` is loop-internal accounting).

## Non-goals

- No routing-behavior change: `bad` computation, rip-up, history
  escalation, hard mode — all untouched.
- No new output channel, no JSON schema change in
  `ours_router`, no golden regeneration (the series is derivable
  from `cli_lines` on next regen).
- No per-net iteration attribution (which nets were in each
  `bad` set) — counts only, per spec.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `Cargo.toml`
