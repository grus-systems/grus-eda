# drt-unrouted-analysis — Steps (r2: continuation plan)

The r1 executor committed `ed0a8d7 kurena: wip checkpoint` and died
on a rate limit before verify. **Do not re-implement.** Worktree is
clean; `cargo check --tests` passes; `cargo test -p grus-eda-core
--test drt unrouted_probe` → 5 passed / 1 ignored (dev, verified in
r2 planning). Everything below assumes HEAD = `ed0a8d7`.

## 0. State inventory (verified during r2 planning — do not re-run)

| item | state |
|---|---|
| `probe.rs` (RootCause/ProbePin/ProbeNet/ProbeReport/probe_unrouted/attach_evidence) | implemented, matches `api.md`/`probe.md` |
| `graph.rs` `GuideUnion`/`guide_union`/`guide_union_connected` | moved verbatim from tests; `maze_census` call sites updated |
| `maze.rs` `premark_marks(io_pin_marks)` split + `pub(crate)` openings | landed; 4 dead widenings to revert (§2) |
| tests: 6 probe tests + `SEAL_LEF`/`maze_tech_seal`/`maze_db_custom` fixtures | written, 5/6 green dev |
| `docs/results.md` dated entry | written — gcd complete, **picorv32 pending**, "Checks run" unverified |
| `docs/e6-drt.md` "Root-cause probe" subsection | written |
| `docs/status.md` last-updated line | written — says "picorv32 leg in flight", needs final numbers |
| `Cargo.toml` 0.16.1 / `Cargo.lock` | bumped |

## 1. Launch the picorv32 probe leg FIRST (critical path)

The `#[ignore]`d leg re-routes picorv32 (`leg_for` → full
`route_nets`, ~135 min release per the test's own comment) then
probes 2,016 unrouted nets. That exceeds `timeout_sec = 7200` for
any single blocking call — **run it detached and poll**:

```
nohup cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture \
  > /tmp/probe_picorv32.log 2>&1 &
```

Poll `tail /tmp/probe_picorv32.log` between the §2–§3 work; do not
block on it. If the process dies with the session, relaunch
identically — the log preserves partial output. If a stale run is
already alive (`pgrep -f test_unrouted_probe_picorv32`), do not
launch a second one; reap its log instead.

## 2. Cleanup: revert dead `pub(crate)` widenings in `maze.rs`

The checkpoint widened more than the probe uses. `probe.rs`
actually needs (imports + field access): `Occupancy`+`new`,
`premark_marks`, `NetTopo::new`+`cover`, `NetAdj::new`,
`route_one_net`, `NetOutcome`, `scan_stub`, `fill_blocked`,
`covered_range`, `Hit` (+`Interval`+`Occupant` via `Hit.iv`
field-visibility), `UnroutedNet`/`FailKind`/`RouteDb`/`OccMode`
(already `pub`). Widenings with **no** user outside `maze.rs` —
revert to private:

- `enum EdgeProbe` → `enum`
- `fn node_xy` → `fn`
- `Occupancy::scan_wire`, `Occupancy::scan_rect` → `fn`

`hit_bbox` stayed private (api.md's "if needed" — not needed).
Confirm no other module/test references them before reverting
(`rg` the names crate-wide); then `cargo check -p grus-eda-core
--tests` must stay clean. No behavior change — this is hygiene,
not a fix.

## 3. Verify lanes (the r1 run never happened)

In order, on this tree:

```
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
```

- The committed `results.md` "Checks run" block claims verify
  green "8 cli + 559 lib + 18 drc + 42 drt + 2 irdrop, 0 failed
  (4 `#[ignore]`d)" — **confirm or correct it against the real
  run** (E0 lesson 9: say what you ran; the r1 memo records that
  no verify lane ran). If the numbers differ, amend the block.
- Lane 1 covers the output-unchanged acceptance:
  `test_maze_census` (`GCD_ROUTE_DIGEST`), the e2e DEF
  byte-compare pins, `test_net_graph_rebuild_identity`.
- Optional while it runs — re-confirm the recorded arm-D red
  record (cheap, keeps the record honest on this tree): swap
  `RootCause::GeomOffTrack` → `RootCause::GeomPocket` in the
  probe-`Unattachable` rescan arm of `probe.rs`, run
  `cargo test -p grus-eda-core --test drt
  test_unrouted_probe_geom_disjoint`, confirm it fails at the
  `GeomOffTrack` assert (the record cites `tests/drt.rs:2821`,
  `left: GeomPocket, right: GeomOffTrack` — verified consistent
  with the current file), revert, re-run green. Amend the record
  only if the observed failure differs.

## 4. When the picorv32 leg lands — transcribe + reconcile

`probe_report_lines` prints the table source; `assert_probe_coverage`
enforces completeness + containment; the test's own asserts check
`nets == 2016`, `net_counts()["geom_met3_bridge"] == 43`, and
name-equality of `unguided` (67) against the golden. Handle by
outcome:

- **Green** → transcribe: full 13-row table (pins + nets), the
  anchor block (bridged-union nets vs `geom_met3_bridge` vs
  rescued; `region_escape ∩ u.failed` per-class split; `unguided`
  67), `probe_ms` vs `route_ms`, picorv32 prioritized target
  list. Land as the "amendment" the pending entry already
  promises; update `status.md` "in flight" → measured.
- **`geom_met3_bridge` assert fails** → the pinned 43 is the
  *union* census, not the class count (a probe-rescued bridge is
  negotiation by definition — gcd's 2 bridged nets were both
  rescued). Check the printed `bridged_union_nets=N`: if `N == 43`
  the union census reproduces and the class split is a *finding*
  — re-pin the assert on the union count
  (`rep.nets.iter().filter(|n| n.union ==
  GuideUnion::Bridged).count() == 43`), let `geom_met3_bridge`
  be measured/printed, and name the split in the results entry
  (the spec's "or names the delta" arm). If `N != 43`, the leg's
  census itself moved — investigate against `maze_census`'s
  `n_bridge` on this leg before touching the assert.
- **`nets != 2016` or `unguided != 67`** → the leg diverged from
  the pinned census — investigate the route, not the probe.
- **Residual bucket nonzero** → a real finding: name the nets in
  the results entry; do not fold them away.

## 5. Docs amendment + commit

- `docs/results.md`: picorv32 table + anchors + `probe_ms` +
  target list into the pending amendment; correct "Checks run"
  to what §3 actually ran (the picorv32 leg invocation line is
  already recorded).
- `docs/status.md`: replace "picorv32 leg in flight — results.md
  names it" with the measured decomposition headline.
- `docs/e6-drt.md`: the "Measured decompositions" line gains the
  picorv32 headline if it still says gcd-only.
- Commit remaining work as the implement-stage commit (pipeline
  convention `kurena: implement drt-unrouted-analysis`; body per
  the r1 draft — probe post-pass, premark split, shared
  classifier, test pins, output-unchanged evidence). The wip
  checkpoint stays as-is in history.

## Contingencies

- **picorv32 leg is killed again mid-run** — relaunch detached;
  nothing else depends on it until §4. Do not mark the leg done
  from a partial log.
- **Probe outlives the session repeatedly** — acceptable last
  resort per spec: the results entry may record the picorv32
  table as pending *only* if a completed run is genuinely
  infeasible this session — but the acceptance lists picorv32
  counts, so exhaust the detached-relaunch path first.
- **Verify surfaces a real failure** — fix in place, re-run the
  lane, record both runs.
- **`probe_attached` arms dominate an attach class** — expected
  small; the flag exists precisely so the results story stays
  honest.

## Files

- `crates/grus-eda-core/src/drt/maze.rs` (dead-widening reverts only)
- `crates/grus-eda-core/src/drt/probe.rs` (only if §4/§3 forces a fix)
- `crates/grus-eda-core/tests/drt.rs` (only the §4 met3_bridge assert re-pin)
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
