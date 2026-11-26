# drt-progress — Tests & verification

## Hook test — `tests/drt.rs`

`test_route_nets_progress_hook`, placed next to `test_maze_cases`
(:1587) on the same `maze_db`/`maze_guide` 3-net fixture (real
`DesignDb`/`RouteGraphs`, not a toy — the join→route path the CLI
walks). Collect events into a `Vec<RouteProgress>`:

```rust
let mut events = Vec::new();
let hooked = route_nets_with_progress(
    &db, &tech, &tracks, &graphs, &vias, &cfg,
    Some(&mut |p: RouteProgress| events.push(p)),
);
```

Asserts:

1. **Coverage** — `events.len() == sum(iter_stats.attempted)`:
   one event per attempt, across all iterations.
2. **Ordering** — grouped per `iteration`, `done` runs
   `1..=queued` contiguously and the group's last event has
   `done == queued == iter_stats[it-1].attempted`.
3. **Unrouted semantics** — the final event of each iteration has
   `unrouted == attempted - routed` for that `IterStat` (the
   failure count, matching `IterStat`'s doc at `maze.rs:202-206`).
4. **Determinism under the hook** — `hooked == route_nets(&db,
   &tech, &tracks, &graphs, &vias, &cfg)` (`RouteDb: PartialEq`,
   `maze.rs:218`). The whole struct compares — the strongest
   statement that the sink perturbs nothing.
5. **Opt-in default** — the unhooked `route_nets` path is already
   exercised by every existing test; no assert needed beyond (4).

No wall-clock asserts — `elapsed` is observational; asserting
monotonicity or magnitude invites flake for zero coverage gain.

Optionally a second leg on the `route_nets` `OccMode::Disabled`
path is unnecessary — the hook is mode-agnostic by construction.

## stdout byte-identical — the contract check

`== drt emit:` is consumed by `check_drt_ours.sh:929-957`
(`ours_router.cli_lines` golden) and `recount_unrouted.py`. Prove
the block is unchanged by construction + measurement:

- Construction: no `println!` added; the only new output is
  `eprintln!` in the closure; `generate_drt_ours.sh:67` tees
  stdout only.
- Measurement: rerun `generate_drt_ours.sh gcd` (release build,
  `drt` on `gcd.placed.def` + committed `gcd.guide`), capturing
  stderr separately:
  `… 2> runs/drt-ours/gcd.progress.log | tee …drt.ours.log`.
  Progress lines appear in the stderr file; then run
  `check_drt_ours.sh` step E (or the `tests/drt.rs` leg that
  deserializes `gcd.drt.ours.reference.json`) — `cli_lines` must
  still match the committed golden, and `git diff` on
  `gcd.drt.ours.reference.json` shows only provenance fields
  (`emit_log` hash etc.), no content drift. If the golden
  legitimately regens, the diff must be empty of census lines.

Record in the `docs/results.md` entry: the exact command, the
stderr line count, one verbatim sample line, and the
`cli_lines`-unchanged outcome. The sample line is observational —
mark it as such (E0 lesson 9: say what ran; elapsed values are
not goldens).

## Library-quiet check

Acceptance requires no stderr writes from library callers. By
construction the engine gains zero `eprintln!` (the only existing
ones sit behind `GRUS_DRT_TIMING`, pre-existing). No test needed —
the hook test's unhooked leg plus a `rg 'eprintln' maze.rs` audit
in the commit body is the evidence.

## Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

plus the gcd CLI run above. `cargo test --workspace` alone proves
nothing about the CLI line (acceptance #1) — the gcd run is the
required leg; picorv32 stays deferred (the ~135 min leg is not
this ticket's bar; the sample line in `cli.md` is shape-only and
labeled so).
