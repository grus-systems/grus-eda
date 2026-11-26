# drt-converge-stats — Steps (r2, continuation)

Observability ticket; no routing-behavior change. HEAD is wip
checkpoint ca053ac — the r1 driver timed out *after* committing
the full code surface, so this plan is evidence + verify only.
**Do not re-implement anything in §1** — it is a read-only
reconciliation of the memo's "remaining" list against the tree.

## 1. Reconcile the memo's remaining list (read-only, no edits)

Confirm each item matches its design spec — all landed at ca053ac:

- [ ] `IterStat` + `RouteDb.iter_stats` + loop collection +
      ctor — `maze.rs:194-231, 3755, 3763, 3768, 3776, 3809-3813,
      3899`; re-export `drt.rs:21` (`model.md`)
- [ ] Emit census lines `iter_progress=`/`iter_prov=` after
      `guided=…` — `main.rs:2084-2104` (`output.md`)
- [ ] Digest serialization appended after
      `put(routed.iterations)` — `maze.rs:4061-4067` (`digest.md`;
      memo ruling: keep it, touch no existing input)
- [ ] Census-pinning asserts + digest fail-proof —
      `tests/drt.rs:1641-1653, 1680-1716, 1821-1838`
- [ ] keep-list arms — `check_drt_ours.sh:935-936`
- [ ] `docs/e6-drt.md` §7/:562-569 + §8/:709-713,
      `docs/status.md`, `Cargo.toml` 0.12.11
- [ ] `docs/results.md` dated entry — gcd census + emit-leg
      verbatim + digest move + fail-proof + consumers all
      recorded; only the picorv32 arm is pending (§2)

If any item has drifted from the spec, fix the spec deviation —
do not redesign.

## 2. picorv32 census leg (the only open evidence)

The acceptance's "gcd/picorv32 iteration profile" needs the
picorv32 `iter_progress`/`iter_prov` series in `docs/results.md`.
~135 min release — over the 7200 s per-invocation bound, so run
detached and poll across commands:

```
# 1. build once so the cargo lock window stays short
cargo test --release -p grus-core --test drt --no-run
# 2. detached run, output to a log outside the repo
nohup cargo test --release -p grus-core --test drt \
    test_maze_census_picorv32 -- --ignored --nocapture \
    > /tmp/drt-pico-census.log 2>&1 &
# 3. poll `tail /tmp/drt-pico-census.log` between other steps for
#    `picorv32: iter_progress=… iter_prov=…`
```

Launch this **first** so it overlaps §3's verify. Inputs are in
place (`tests/designs/picorv32/` seeded by `worktree_seed`;
`load_routed_inputs("picorv32")` reads them). The test also runs
the landed `iter_stats` invariant asserts — a red leg is a real
failure, not a doc gap.

When the series lands, edit the results.md entry's pending
paragraph ("picorv32 leg … running detached"): splice the
verbatim `picorv32: iter_progress=… iter_prov=…` line plus a
one-sentence reading (prior record: `unrouted=2016` at the cap —
the series says whether that tail was still falling or flat).
Extend "What ran" with the leg command.

## 3. Workspace verify (per `workflow.toml`, memo's closing item)

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Dev-profile drt tests measured ~2196 s on an M4 host — fits the
7200 s bound. Run even though results.md records green from r1:
the memo asks for a fresh full verify on the final tree. If green,
the results.md "What ran" claim stands as written.

## 4. Commit

- If the picorv32 leg landed: one docs commit —
  `docs: Record picorv32 iter_progress series for
  drt-converge-stats` (English, agent trailer). No version bump:
  the feature bump 0.12.10 → 0.12.11 is already in ca053ac and a
  docs-only delta needs none.
- If the leg is still running when §3 finishes: leave the
  results.md pending paragraph as written, note in the card that
  the picorv32 acceptance box stays open (same convention as the
  deferred picorv32 emit legs). No commit — the tree already
  matches ca053ac.

## Contingencies

- **Leg fails or panics** — treat as a real defect: the failing
  assert's message names which invariant broke; fix only that,
  re-verify, re-run the leg.
- **Leg cannot complete in-session** — the pending paragraph is
  the documented fallback; flag the open acceptance box in the
  card rather than faking a profile.
- **Cargo lock contention** between the release leg and the dev
  workspace test — cargo waits on the lock; the `--no-run`
  pre-build keeps the window short. If it still stalls, run §3
  first and launch the leg after.
- **`iter_stats` empty on an input** — only `guided=0`; the
  unconditional lines print bare keys. Acceptable; note if it
  ever fires.

## Files

- `docs/results.md` (picorv32 series splice — the only expected
  edit this round)

Landed surface (no further edits expected, listed for the diff
boundary):

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `tests/designs/check_drt_ours.sh`
- `docs/e6-drt.md`
- `docs/status.md`
- `Cargo.toml`
