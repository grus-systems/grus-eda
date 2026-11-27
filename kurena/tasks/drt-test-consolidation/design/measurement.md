# Measurement — honest before/after evidence

The acceptance requires a `docs/results.md` dated entry with
before/after dev-profile wall time and per-run route counts, plus
a recorded red-check. Protocol below; do all "before" measurements
on the unmodified test file.

## Route-count measurement

`GRUS_DRT_TIMING=1` makes `route_nets_with_progress` emit
`drt: premark_obs …` exactly once per route execution
(`src/drt/maze.rs:4279`). Count executions dynamically:

```sh
GRUS_DRT_TIMING=1 cargo test -p grus-eda-core --test drt -- --nocapture 2>&1 \
  | rg -c 'drt: premark_obs'
```

This counts all route executions including the µs-cost synthetic
fixture runs — same suite before and after, so the delta is the
real saving; the static table in `callsites.md` (gcd: 21 → 9)
attributes the gcd share by call site. Report both numbers.

Note: the package is `grus-eda-core` — `goal.md`'s
`-p grus-core` is stale (so is the `--release -p grus-core` hint in
the `test_maze_census_picorv32` doc comment; leave the comment, it
is out of scope unless trivially fixable — fix it while touching
the file, it is a wrong command).

## Wall-time measurement

```sh
time cargo test -p grus-eda-core --test drt
```

Dev profile (no `--profile` flag = `test` profile, which inherits
`dev`). One warm run each side on the same host; record host,
rustc, and rep count in the entry (single rep is acceptable — say
so; the repo convention is "say what you ran"). Do **not** measure
with `--profile verify` — the acceptance pins dev-profile.

Caveat to record honestly: parallel test execution means wall time
is bounded by the longest test, not the sum — the "after" figure is
bounded below by `test_net_graph_rebuild_identity` +
`test_rayon_route_determinism` + one shared route + emit/DRC legs
serialized by their shared `leg`. The route-count table is the
sharper evidence; wall time is the acceptance's headline number.

## Assertion-count check

```sh
rg -c 'assert' crates/grus-eda-core/tests/drt.rs   # before vs after — must be equal
```

## Red-check protocol (prove the gate still fails)

1. After the refactor passes: perturb `GCD_ROUTE_DIGEST` (`^ 1`).
2. `cargo test -p grus-eda-core --test drt test_maze_census` → must
   FAIL at `gcd route digest drifted`. Record the failure line.
3. Restore the constant; rerun → PASS.
4. Log both outcomes in the results.md entry (AGENTS §7.7).

## `docs/results.md` entry template

Append at the end of the Verification Log, matching house style
(see the `drt-rayon` entry for format):

```markdown
### 2026-11-27 — drt-test-consolidation: shared `route_nets` legs across drt test assertions (v0.15.2)

**Machine context**: <hostname, chip, RAM, sw_vers, rustc — copy the
drt-rayon entry's block shape>.

`tests/drt.rs` executed <N_before> `route_nets` gcd runs per suite —
one per assertion group — on identical inputs. `OnceLock`-shared
`(fixture, config)` legs now feed every read-only consumer; the
executions kept fresh are the ones under test (determinism second
runs, forced `RAYON_NUM_THREADS` arms). Assertion count
`rg -c 'assert'` before/after: <n> → <n>.

| metric | before | after |
|---|---|---|
| `route_nets` executions (`GRUS_DRT_TIMING` `premark_obs` count) | <n> | <n> |
| gcd full routes (static call-site census) | 21 | 9 |
| `cargo test -p grus-eda-core --test drt` wall (dev profile) | <s> | <s> |
| assert count in `tests/drt.rs` | <n> | <n> |

**Prove-it-red** (AGENTS §7.7): `GCD_ROUTE_DIGEST ^ 1` →
`test_maze_census` FAILS at the digest assert (<failure line>);
restored, PASS.
```

Also bump `[workspace.package] version` `0.15.1 → 0.15.2` in the
same commit (AGENTS §5 patch bump for a `test:`-scoped refactor —
name the bump in the entry title as above, matching how prior
entries embed `vX.Y.Z`).

## Verify gate

```sh
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
```
