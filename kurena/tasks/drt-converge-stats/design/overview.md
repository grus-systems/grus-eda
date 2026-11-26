# drt-converge-stats — Overview (r2)

E6 observability ticket. `route_nets` runs a capped rip-up loop
(`MAX_ITERS = 8`, last iteration hard — `maze.rs` ~3760-3847). When
the cap bites, `iterations=8 unrouted=N` alone cannot tell a run
that was still converging (bad-set shrinking to the end) from one
that flatlined at iteration 2 — the external review's "8 iters
wasn't enough" guess. This ticket records the per-iteration counts
the loop already computes, prints them as compact convergence
lines in the `== drt emit:` census, and folds them into
`route_digest` so they are deterministic, checked output.

## Revision r2 — review-memo reconciliation

The r1 driver timed out at a clean compile point; the memo says
"no design change needed — continue per design 7a82de1". This r2
keeps the design identical and re-baselines it onto what the wip
checkpoint ca053ac actually committed. The memo's "remaining"
list is already landed — verified line-by-line in the tree:

| Memo "remaining" item | Status at HEAD (ca053ac) |
|---|---|
| stats struct plumbed through `RouteDb` | `IterStat` `maze.rs:194-212`; `RouteDb.iter_stats` :227-231; loop collection :3755/:3763/:3768/:3776/:3809-3813; ctor literal :3899; `drt.rs:21` re-export |
| per-iteration counts in the emit census | `main.rs:2084-2104` — `iter_progress=` / `iter_prov=` after `guided=…`, before `unrouted_kinds` |
| `docs/results.md` dated entry w/ gcd numbers | landed — `iter_progress=269,94,87,81,81,81,81,81`, `iter_prov=25,6,0,0,0,0,0,0`, gcd emit-leg verbatim lines, digest `0xab23bcc1997e4484 → 0xa6d8b74a740a7489` |
| test pinning the census lines | `maze_census` invariant asserts `tests/drt.rs:1680-1716` + printed series :1641-1653; `test_route_digest_covers_iter_stats` :1821-1838 |

Plus, landed but not enumerated in the memo: `route_digest`
serialization (`maze.rs:4061-4067`), `check_drt_ours.sh` keep-list
(:935-936), `docs/e6-drt.md` §7/:566 + §8/:710-713,
`docs/status.md`, `Cargo.toml` 0.12.10 → 0.12.11.

**Digest ruling (memo):** "stats are a print/recording path, keep
route_db digest inputs unchanged unless the design says
otherwise." This design *does* say otherwise (`digest.md`):
`iter_stats` is a deterministic run output and is serialized —
appended end-of-stream after `put(routed.iterations)`, so every
pre-existing digest input's byte position is unchanged. Keep the
landed serialization; do not strip it, and do not touch any
existing digest input.

## Genuinely open at HEAD

1. **picorv32 iteration profile** — the
   `test_maze_census_picorv32 --ignored` leg (~135 min release,
   over the 7200 s per-invocation bound → detached) was in flight
   when the driver timed out; no process survives. Its
   `iter_progress`/`iter_prov` series still goes into the
   results.md entry, which currently marks it pending
   (`steps.md` §2). The acceptance's picorv32 box stays open
   until the series is recorded.
2. **Workspace verify** — `cargo test --workspace` + clippy
   `-D warnings` per `workflow.toml`. results.md records green
   from the r1 run; the memo asks for a fresh full verify.

## State the design is written against

- `e6-drt-e2e` (and the picorv32 leg) landed — the spec's "queue
  after `e6-drt-e2e`" note is satisfied.
- `RouteDb.iterations` stays — it is the census/digest scalar;
  `iter_stats` is the series behind it, not a replacement.
- `GRUS_DRT_TIMING` stderr already prints `iter {it} bad=…
  failed=… prov=…` — opt-in timing evidence, not a census
  record. This ticket is the *committed* equivalent: same
  numbers, in `RouteDb` and stdout.

## Boundary vs `drt-progress`

`drt-progress` (spec only, unlanded) adds a *periodic stderr*
progress line and promises "stdout census block byte-identical"
— that promise is about its own diff. This ticket deliberately
extends the stdout census; the two are complementary channels
(periodic vs final) and must not be merged per the spec. Whichever
lands second rebases over the other's output surface — no
mechanism overlap (`drt-progress` adds a hook/callback;
`iter_stats` is loop-internal accounting).

## Non-goals

- No routing-behavior change: `bad` computation, rip-up, history
  escalation, hard mode — all untouched (verified in the ca053ac
  diff: collection is additive, no reordering).
- No new output channel, no JSON schema change in
  `ours_router`, no golden regeneration (the series is derivable
  from `cli_lines` on next regen).
- No per-net iteration attribution (which nets were in each
  `bad` set) — counts only, per spec.
- No stdout-literal pin for the new lines: consistent with every
  other census line, coverage is invariant-level (`maze_census`
  asserts) + digest-level (`route_digest` fail-proof), not a
  string match on emit output.

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
