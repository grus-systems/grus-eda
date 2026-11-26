# e6-drt-maze — Implementation steps (resume from `4ff7ade`)

HEAD `4ff7ade` holds the landed router, `docs/e6-drt.md` §7, the
clippy fixes, and `v0.12.4`; the tree is clean. Remaining work is
measure → document → verify → commit, in that order. Start the
long measurement first — it is the critical path.

## 0. Collect the runtime evidence (`measurement.md`)

Kick off detached, poll the log:

```
GRUS_DRT_TIMING=1 cargo test --release -p grus-core --test drt \
    test_maze_census -- --nocapture 2>&1 | tee /tmp/maze-census.log
```

Capture both designs' census lines verbatim (`ms=`, `digest=`),
every `unrouted …` / `met3_bridge=` line, and the timing phase
lines. picorv32 is the acceptance number — 30+ CPU-min in release
was the earlier figure; record whatever the run measures.

## 1. Write the docs (`docs.md`)

- `docs/results.md` — dated `e6-drt-maze` entry with the census
  table, fail-proof/determinism/fixture evidence, the runtime
  before/after record, checks run, version bump. Numbers verbatim
  from step 0 + the measured gcd/occupancy figures; state the
  profile next to each `ms=`.
- `docs/status.md` — new `Last updated:` head, demoting
  `e6-drt-via` to `Previous:`.
- `docs/roadmap.md` — the `v0.12.4` progress bullet after the
  `e6-drt-via` line in the E6 block.
- `docs/e6-drt.md` — spot-check §7 anchors only; fix drift if
  found (code wins, note the delta in the commit body).

## 2. Verify

```
cargo test --workspace        # dev profile — detached, tee to a log
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo test --workspace` includes all 3 picorv32 `route_nets`
runs — run detached and let it finish; record the real wall-clock
time (and the dev-profile `ms=` it prints) in `results.md` before
committing. If a number in the entry is still pending when the
docs are first drafted, fill it in from the gate log — do not
commit placeholder text.

## 3. Commit

One commit with the doc delta (plus any contingency code fixes).
Stage explicit paths — never `git add -A`.

```
feat: Sequential maze router over the track graph
```

Conventional Commits, English; body lists what shipped (router
shape, census numbers, named residuals) with the agent trailer
(`Co-Authored-By`). Version `0.12.4` is already in the tree from
the checkpoint — no second bump.

## Contingencies

- **picorv32 `NoPath` outside the met3-bridge class**: name the
  cause per net and extend the classifier in
  `test_maze_census` — never weaken the assert.
- **Workspace gate impractically long in dev profile**: record
  the measured envelope in `results.md` and surface it on the
  card; do not `#[ignore]` the legs or add a `[profile.test]`
  override.
- **A verify red in existing tests**: fix in place (small,
  focused), rerun the gate, note the fix in the commit body.

## Files

- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `docs/e6-drt.md` (drift fixes only)
- `crates/grus-core/src/drt/maze.rs` (contingency only)
- `crates/grus-core/tests/drt.rs` (contingency only)
