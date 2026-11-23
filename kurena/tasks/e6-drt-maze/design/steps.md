# e6-drt-maze — Implementation steps (resume from `2c3c713`)

The checkpoint compiles with 2 warnings; synthetic cases pass;
**measured**: gcd census 203 s with 48 `NoPath` unrouted (47 not
met3-bridge → classification assert red), picorv32 unmeasured
(>21 CPU-min observed on the earlier run). Remaining work, in order:

## 0. Clear the two warnings

- Delete `hit_rect` and the `Hit::x_line` field (dead since the
  carve moved to candidate-vs-halo). Remove `x_line` from the three
  `Hit{..}` constructions (`scan_wire`, `scan_rect` ×2).
- `cargo check -p grus-core` → zero warnings.

## 1. Bounded backtrack (hardening — do before diagnosis)

In `NetRun::extend_once`, bound `while !in_tree[v]` at
`g.nodes.len()` steps; overflow ⇒ treat the net as failed this round
(the parent-chain invariant makes it unreachable in correct runs —
`search.md` §4). This converts the one theoretical hang into a named
`NoPath`, so the rest of the plan can trust "still running = slow".

## 2. Diagnose the 48 gcd `NoPath`s

Temporary instrumentation (removed before commit — same convention
as the deleted `debug_*` fns):

1. `OccMode::Disabled` control on gcd. Still fails ⇒ `Obs`/topology
   (observed NoPath pattern already implies this — soft-mode foreign
   marks only cost, they never block).
2. Blocker probe (`debug_pin_block` or equivalent): for each failed
   pin, scan its attach nodes' incident edges through `edge_cost`
   and tally which `Occupant` class returns `None` —
   `Obs(host)`-uncarved / `Obs(other cell)` / `Net(foreign)` /
   no-hits-at-all (⇒ node component split, including relaxed-via
   single-nearest misses).
3. Decide per `occupancy.md` §5: **C1** unconditional host carve
   (expected primary — deep-pin pattern matches), **C2** `Pin`
   occupant class / unmark pin shapes, **C3** relaxed-via
   next-nearest widening or spare-layer bump. Apply minimally; a
   fix that lands gets a matching in-module unit case (`tests.md`).
4. Re-run `test_maze_cases` (case 3's OBS detour must still detour)
   and the gcd census leg — target `unrouted = 0`; any residual is
   classified by name (`tests.md`).

## 3. Performance (`perf.md`)

Instrument (per-iteration `eprintln!`: `bad` count, routed/failed,
cumulative ms; top-5 nets by ms with nodes/edges/pins; `edge_cost`
and interval-compare counters), run gcd, then picorv32 under an
external timeout. Apply levers in order until the budget is met:

1. **L1** `cand` scratch buffer into `edge_cost` (per-relax alloc).
2. **L2** `NetTopo` split — build attach/`relaxed_vias`/CSR once per
   net, reuse across iterations (`api.md`).
3. **L3** bounded interval scans — `lo`-sorted per-line lists with a
   chunked `max_hi` skip index (`occupancy.md` §6).
4. Re-measure after each lever; keep the counters' numbers for
   `results.md` (before/after, honest record — AGENTS §2).

Budget: picorv32 census leg completes inside the debug test run
(working target single-digit minutes; `perf.md` §3); record the
measured `ms=`. L4 (`[profile.test] opt-level`) only if L1–L3 fail —
recorded decision, never silent.

## 4. Green the gates

- `cargo test -p grus-core --lib maze` — in-module units.
- `cargo test -p grus-core --test drt test_maze_cases` — synthetic.
- `cargo test -p grus-core --test drt test_maze_census -- --nocapture`
  — collect both designs' census lines for `results.md`.
- `test_maze_occupancy` + `test_maze_determinism` — fail-proof and
  double-run evidence.
- `cargo test --workspace`;
  `cargo clippy --workspace --all-targets -- -D warnings`.
- Remove the temporary instrumentation before the commit.

## 5. Docs + version (same commit)

- `docs/e6-drt.md` §7 per `docs.md` — must record the *final* carve/
  pin-mark rules, the spare layer, relaxed vias, `obs_rects_raw`,
  the rip-up rule, and the §4/§5 claim-boundary amendments.
- `docs/results.md` dated entry — measured census numbers, the
  diagnosis evidence chain, the perf before/after, checks run.
- `docs/status.md` head paragraph; `docs/roadmap.md` E6 line.
- `Cargo.toml` `[workspace.package] version 0.12.3 → 0.12.4`.

## Commit

One commit on top of the checkpoints:
`feat: Sequential maze router over the track graph`
(Conventional Commits, English, agent trailer). The two checkpoint
diffs plus this remainder are one unit of work; stage explicit
paths — never `git add -A`.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
