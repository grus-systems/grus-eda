# e6-drt-maze — Runtime evidence collection

The `results.md` entry needs measured numbers, not estimates
(AGENTS §2). Everything below is captured from test output —
`test_maze_census` already prints the headline line per design
(`guided routed pins_connected projected unrouted collisions
iterations ms digest`), and `GRUS_DRT_TIMING=1` adds the
phase/iteration breakdown to stderr.

## 1. Census run (the acceptance number)

```
GRUS_DRT_TIMING=1 cargo test --release -p grus-core --test drt \
    test_maze_census -- --nocapture 2>&1 | tee /tmp/maze-census.log
```

- **`--release` deliberately**: it bounds wall clock and matches
  the earlier 30+ CPU-minute picorv32 measurement the reviewer
  cited. The results entry must state the profile explicitly
  ("release test profile") next to every `ms=` number.
- The dev-profile number, if the workspace gate (step 3)
  completes, is recorded alongside — say which profile produced
  which figure.
- Run detached (`nohup … &` or a background shell) and poll the
  log; the picorv32 leg may exceed any per-command timeout.

Capture verbatim:

- the `== gcd maze census ==` / `== picorv32 maze census ==`
  lines with all counters and `ms=`/`digest=`;
- every `unrouted <net> pins=[...]` line — residual failures are
  named per-pin in `results.md` (expected 0; any `NoPath` net must
  carry its `met3_bridge=true/false` classifier line);
- the `GRUS_DRT_TIMING` phase lines (`premark_obs`, `topos`,
  per-iteration `bad=/failed=/prov=` + ms) — they document
  convergence and name the dominant phase for picorv32.

## 2. Fail-proof + determinism numbers

Already measured, re-confirmed by the gate run — no separate
rerun needed unless the gate fails:

- `test_maze_occupancy` (gcd): Enabled `digest=0xfbb3073487860974`
  census=0 vs Disabled `digest=0x349f82f53a0c7143` census=6,429.
- `test_maze_determinism`: identical `RouteDb` + nonzero digest
  on two full runs per design; record picorv32's digest from the
  census/determinism output.
- `test_maze_cases`: the four synthetic cases — straight route,
  forced via, OBS detour, named `NoPath` + `Unattachable`
  failures (no hang, no panic — the harness completing is the
  evidence).

## 3. Workspace gate (the verify run)

```
cargo test --workspace              # dev profile — the gate
cargo clippy --workspace --all-targets -- -D warnings
```

- Run `cargo test --workspace` detached, `tee` to a log, and poll.
  The `drt` binary spends 3 picorv32 `route_nets` runs
  (`test_maze_census` ×1, `test_maze_determinism` ×2 sequential)
  plus the 28.4 M-node graph build per test; `test_maze_census`
  and `test_maze_determinism` run on parallel test threads, so
  wall clock ≈ the determinism test's two sequential runs, not the
  sum.
- Record the suite's actual wall-clock time in `results.md`
  ("checks run"), and the dev-profile picorv32 `ms=` if it
  printed.
- **If the dev-profile run proves impractically long**: that is a
  recorded fact for `results.md` and the card — not a reason to
  `#[ignore]`, shrink `max_iters`, or add a `[profile.test]`
  override. Escalate the measured envelope to the reviewer.

## Runtime record for `results.md`

| run | before (`2c3c713`) | after (`4ff7ade`) |
|---|---|---|
| gcd census leg | 203 s, 48 `NoPath` | **9,704 ms**, 269/269, 0 unrouted, 0 collisions (dev profile) |
| picorv32 census leg | >21 CPU-min (killed) | **measured `ms=` — record verbatim, state profile** |

The diagnosis line: the 48-net gcd `NoPath` residual resolved as
the unconditional host-cell carve (approach edges blocked outside
the old pin-halo) plus the `Pin` occupancy class (pin metal must
never hard-block) — evidence: post-fix `unrouted=0`.

## Files

- `docs/results.md`
