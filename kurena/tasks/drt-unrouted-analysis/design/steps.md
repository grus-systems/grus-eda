# drt-unrouted-analysis — Steps (r3: finish, don't rebuild)

The r2 executor died on a model rate limit at `04b11ca kurena:
wip checkpoint` — after the picorv32 probe leg had already run
to completion. **Do not re-implement, do not re-probe blind.**
The leg's output survives in `/tmp/probe_picorv32.log`; the two
assert failures are stale-pin deltas, both anticipated by the
r2 decision rules or explained below. Remaining work: re-pin
the picorv32 test, one detached re-run to prove it green, the
docs amendment, verify lanes, commit.

## 0. State inventory (verified during r3 planning — do not re-run)

| item | state |
|---|---|
| Implementation (`probe.rs`, `graph.rs`, `maze.rs` openings, tests, fixtures) | complete, committed |
| `cargo test --workspace --profile verify` | **green on HEAD** (ran in r3 planning: 8 cli + 559 lib + 18 drc + 42 drt [4 ignored] + 2 irdrop, 0 failed) |
| `cargo clippy --workspace --all-targets -- -D warnings` | **clean on HEAD** (the r2 FAIL was `private-interfaces` on `EdgeProbe`; `04b11ca` restored `pub(crate)` — already fixed) |
| picorv32 probe leg | **ran once, completed in 3403.67 s** (`/tmp/probe_picorv32.log`); FAILED at `tests/drt.rs:2887` `rep.nets.len() == 2016` → left **1731**; the `geom_met3_bridge`/`unguided` asserts below it never evaluated |
| Measured decomposition | in the log header, transcribed below |
| `docs/results.md` entry | written — gcd complete, picorv32 paragraph says "pending / in flight" |
| `docs/e6-drt.md`, `docs/status.md` | written — cite the leg as in-flight / gcd-only numbers |

### Measured picorv32 decomposition (from the completed log)

```
picorv32: unrouted_nets=1731 unrouted_pins=3083 unguided_io_only=67 probe_ms=224517
geom_met3_bridge = 466 pins / 32 nets     neg_attach_stub     = 1230 / 1000
geom_pocket      =  73 / 30              neg_wave            = 1206 /  605
geom_disjoint    =   0 /  0              neg_wave_obs_frontier = 59 /  54
geom_no_in_region=   0 /  0              neg_wave_unattributed = 49 /  45
geom_obs_sealed  =   0 /  0              neg_attach_port     =    0 /   0
geom_off_track   =   0 /  0              residual_*          =    0 /   0
bridged_union_nets=142  geom_met3_bridge_pins=466  rescued_pins=546
region_escape ∩ u.failed = 73 (met3_bridge 10, pocket 1, stub 22, wave 40)
```

`assert_probe_coverage` passed (it precedes the failed assert):
all 3,083 pins classified, residual buckets 0, class counts sum
to `Σ u.failed`, containment `32 ⊆ 142` held.

### The two deltas, root-caused in r3 planning

1. **`unrouted_nets` 1731 vs pinned 2016 — stale pin, not a
   bug.** The 2016 is the v0.12.9 census (`docs/results.md`
   ~10228). Two routed-output-changing tickets since never
   re-ran the deferred picorv32 leg: `drt-guide-escape`
   (v0.13.0 — 434 of 592 picorv32 escapes re-attached
   graph-side) and `drt-rayon` (v0.14.3 — "deterministic at
   any thread count but no longer serial-identical"). Set-diff
   of the log's unrouted names vs the committed golden
   `picorv32.drt.ours.reference.json` `ours_router.unrouted`
   (run in r3 planning): **970 rescued, 685 newly unrouted**
   — negotiation churn, direction consistent with the two
   tickets. `route_db.unrouted` *is* the route's own report;
   the probe mirrors it 1:1 (`assert_probe_coverage`), so the
   spec's "sum to the pinned totals" is satisfied against the
   live census — the delta vs the stale pin is a results-entry
   naming, not a classifier question.
2. **`geom_met3_bridge` nets 32 vs pinned 43 — the flagged
   class-vs-union split plus a moved basis.** The spec's 43
   is the union census of the *108-net* v0.12.4 era
   (`docs/results.md` ~9368); the 2016-era recount measured
   **138** bridged unions (~9925); today's union census over
   the 1731 is **142**. Of those, **32 nets** carry
   `geom_met3_bridge` pins (466 pins); the other bridged
   nets' 546 failed pins are probe-rescued → negotiation —
   that subtraction is the classifier working as designed.
   Pin the union census (142) and the class count (32)
   separately; the results entry names the 43→138→142→32
   lineage.

## 1. Preserve the evidence, then re-pin `test_unrouted_probe_picorv32`

```
cp /tmp/probe_picorv32.log /tmp/probe_picorv32.r1.log
```

Edit `crates/grus-eda-core/tests/drt.rs` (~2867-2901) — replace
the stale-pin asserts with the measured values, pinning the
union census and class count separately per the r2 rule:

- `rep.nets.len() == 1731`
- `rep.nets.iter().map(|n| n.pins.len()).sum::<usize>() == 3083`
- `rep.nets.iter().filter(|n| n.union == GuideUnion::Bridged)
  .count() == 142` — the union-census pin
- `rep.net_counts()["geom_met3_bridge"] == 32` — the class pin
- full `class_counts` pin: `neg_attach_stub 1230`, `neg_wave
  1206`, `geom_met3_bridge 466`, `geom_pocket 73`,
  `neg_wave_obs_frontier 59`, `neg_wave_unattributed 49`, all
  other classes 0 (the `assert_probe_coverage` residual-0
  asserts already cover the residuals; pin them anyway —
  a named class at 0 is evidence)
- `rep.unguided` name-equality + `== 67` — unchanged; note in
  the results entry this is the first run that actually
  evaluates it (the r1 run panicked above it)
- Update the stale doc comments: the `~135 min` runtime note →
  measured ~57 min total on this host (route leg ~53 min +
  probe ~3.7 min); the "2016 final-unrouted nets" comment →
  the current census with the stale-pin pointer to
  `docs/results.md`.

`cargo test -p grus-eda-core --test drt` (dev, non-ignored
tests) must stay green after the edit — the ignored leg does
not run there.

## 2. Relaunch the leg detached — prove the re-pin green

Measured 3403.67 s ≈ 57 min (route ~53 min + probe ~3.7 min)
— inside `timeout_sec = 7200`, but run detached anyway and
poll between §3's doc work:

```
nohup cargo test --release -p grus-eda-core --test drt \
  test_unrouted_probe_picorv32 -- --ignored --nocapture \
  > /tmp/probe_picorv32.log 2>&1 &
```

The route is digest-deterministic (`test_maze_determinism_
picorv32` pins it) and the probe is order-deterministic —
the re-run must reproduce the §0 table **exactly**. If any
count differs, stop: that is a nondeterminism finding, not a
re-pin. If the process dies mid-run, relaunch identically;
do not mark the leg done from a partial log.

## 3. Docs amendment (do while §2 runs)

`docs/results.md` — replace the "pending / in flight" picorv32
paragraph with the amendment:

- **Decomposition table** — the §0 table (all 13 rows,
  pins + distinct-nets columns), geometric (539 pins: 466
  met3_bridge + 73 pocket) vs negotiation (2,544 pins) split
  named.
- **Anchor block** — the delta lineage, named not smoothed:
  - unrouted total: 1731 live vs spec's pinned 2016 — the pin
    is the v0.12.9 census; `drt-guide-escape` (v0.13.0) and
    `drt-rayon` (v0.14.3) changed routing with the picorv32
    leg deferred; set-diff vs the committed
    `ours_router.unrouted` golden: 970 rescued / 685 newly
    unrouted (reproduce: `python3` json+regex over the log —
    record the one-liner in the entry per E0 rule 1). Name the
    corollary: `picorv32.drt.ours.reference.json`'s
    `unrouted`/`unrouted_kinds`/`unrouted_blockers` fields are
    the same stale v0.12.9 pin — regenerating the picorv32
    golden stays the deferred work named at those tickets,
    **not** this ticket (the spec forbids emit/route changes).
  - met3_bridge: spec anchor 43 = union census of the 108-net
    v0.12.4 era → 138 unions over 2016 (v0.12.9 recount) →
    **142** unions over the live 1731 → **32 nets / 466 pins**
    class count after the probe-rescue split (546 rescued
    pins). Containment 32 ⊆ 142 asserted.
  - region_escape: 73 flagged failed pins (10 met3_bridge /
    1 pocket / 22 stub / 40 wave) — vs the graph-side 592
    population; the rest were rescued out of `u.failed`.
  - unguided: **67, name-equal to the golden — reproduced.**
- **Probe cost** — `probe_ms=224517` (~3.7 min) against the
  leg's route time (~3,179 s ≈ 53 min; total test wall
  3403.67 s) — ~7 % of one route leg; cost model restated
  (one `premark` + one attempt per unrouted net, 1,731 nets,
  `drt_pool` parallel).
- **Prioritized target list** — `neg_attach_stub` 1,230 pins
  → extraction-stage negotiation (rip-up/ordering +
  `try_pin_paint` re-entry); `neg_wave` 1,206 → wave-side
  ordering/history pricing; `geom_met3_bridge` 466 pins /
  32 nets → the met2–met3 transition domain — the dominant
  true-geometry class; `geom_pocket` 73 → pocket walls;
  `neg_wave_obs_frontier` 59 + `neg_wave_unattributed` 49 →
  the two evidence-gap subclasses, named with their meanings
  (freed-capacity vs uninstrumented divergence).
- **Checks run** — correct to what r3 actually ran (the
  verify/clippy lanes re-run per §4; the leg invocation +
  both run records: r1 3403.67 s FAILED on stale pins, r3
  re-run result).

`docs/e6-drt.md` — the "Measured decompositions" tail gains the
picorv32 headline; check §533's "met3-bridge residual class (43
picorv32 nets)" is presented as the historical v0.12.4 count —
if it reads as current, fix it in step (E0 rule 8).

`docs/status.md` — replace "picorv32 leg in flight — results.md
names it" with the measured headline (1731 nets / 3083 pins;
2,544 negotiation vs 539 geometric; met3_bridge anchor split
named).

## 4. Verify lanes + commit

Re-run on the final tree (both were green in r3 planning —
the re-pin is a test-code change, so re-run anyway):

```
cargo test --workspace --profile verify
cargo clippy --workspace --all-targets -- -D warnings
```

When §2 lands green and §3 is written: stage the touched paths
(`tests/drt.rs`, `docs/results.md`, `docs/e6-drt.md`,
`docs/status.md`, design dir) and commit per the pipeline
convention (`kurena: implement drt-unrouted-analysis`; the wip
checkpoints stay in history).

## Contingencies

- **Re-run diverges from §0** → nondeterminism finding; do not
  re-pin silently. `test_maze_determinism_picorv32 --ignored`
  is the instrument — record whatever it shows.
- **Leg dies again mid-run** → relaunch detached (§2); the r1
  log is already preserved, nothing else blocks on it until §3.
- **Residual bucket nonzero on the re-run** → a real finding;
  name the nets in results.md, never fold away.
- **`unguided` assert fails on first real evaluation** → the
  67 count printed but the name-join never ran in r1; if it
  fails, diff `rep.unguided` names vs the golden before
  touching anything — a name-set difference is a finding.

## Files

- `crates/grus-eda-core/tests/drt.rs`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
- `kurena/tasks/drt-unrouted-analysis/design/`
