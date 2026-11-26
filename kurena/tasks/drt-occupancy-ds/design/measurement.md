# measurement — scan-share protocol

Two instruments, run before and after the swap: symbol attribution
(`sample`, the repo's recorded profiler) and a committed counter
census. Plus `route_ms` wall time — the acceptance figure.

## 1. `ScanStats` counters (committed, `GRUS_DRT_TIMING`-gated output)

`Occupancy` gains a private `stats: ScanStats` — `Cell<u64>` fields,
so `&self` scans can count without signature churn:

```rust
#[derive(Default)]
struct ScanStats {
    wire_calls: Cell<u64>,
    rect_calls: Cell<u64>,
    /// Lines iterated across both axes inside `scan_rect`.
    rect_lines_visited: Cell<u64>,
    /// `ivs` elements inspected across all scans (pre-predicate).
    ivs_visited: Cell<u64>,
    hits_pushed: Cell<u64>,
    marks_inserted: Cell<u64>,
    marks_removed: Cell<u64>,
}
```

Increment sites: `scan_wire` (calls, ivs inspected, hits),
`scan_rect` (calls, per-axis lines iterated, ivs, hits),
`rasterize` (marks), `unmark` (removals). Cost is a handful of
relaxed single-thread adds per query — below timer noise against
the BTreeMap work being measured, and stated as such (not claimed
zero). Counts are unconditional; **emission** rides the existing
`timing` gate in `route_nets_with_progress`:

- per iteration, appended inside the existing
  `eprintln!("drt: iter {it} …")` block (~L3959): cumulative totals.
  Per-iteration emission is what makes a killed picorv32 run still
  yield data — the summary survives on earlier `iter` lines.
- final line at route end: same totals + `ivs/wire_call`,
  `lines/rect_call`, `hits/scan` averages — the query-shape record
  the pick depends on (long thin wires vs small pin boxes →
  whether `scan_rect` windows cover few lines).

stderr-only under the env var: census `cli_lines`, `route_ms`,
stdout, and every golden are untouched; `route_digest` input is
unchanged.

## 2. Baseline run (before any Occupancy change; counters commit lands first)

Commands run in the task worktree, release binary:

```sh
cargo build --release -p grus-eda
GRUS=target/release/grus-eda
LIB=$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd
TLEF=$LIB/techlef/sky130_fd_sc_hd__nom.tlef
LEF=$LIB/lef/sky130_fd_sc_hd.lef
D=tests/designs/gcd

# ≥5 reps, min/median/max — the perf-profile rep policy.
for i in 1 2 3 4 5; do
  $GRUS drt --tlef $TLEF --lef $LEF --def $D/gcd.placed.def \
    --guide $D/gcd.guide --out-def /tmp/gcd.occ.$i.def \
    --reference $D/gcd.drt.reference.json --no-self-drc \
    | tee /tmp/gcd.occ.$i.log        # route_ms from the guided= line
done
# DEF sha per rep must equal the committed golden's recorded sha
# (27f7665922c6df0212788f27b73f562d078e5bebdd47446eb1f933a469122aee);
# `--no-self-drc` only skips the post-emit check, not the paint.

# Counter census + iteration timing:
GRUS_DRT_TIMING=1 $GRUS drt … 2>/tmp/gcd.timing.log

# Symbol attribution: run in background, sample the route window
# (gcd route_ms ≈ 12–13 s; sample 10 s mid-route).
$GRUS drt … & PID=$!; sleep 1; sample $PID 10 -file /tmp/gcd.occ.before.txt
```

- `route_ms` is the `guided=…route_ms=N` census field — route phase
  only, self-DRC excluded by the flag and by construction.
- `sample` output collapses to per-symbol counts: attribute
  `Occupancy::scan_wire`, `scan_rect`, `insert`/`rebuild_from`/
  `unmark`/`rasterize`, and BTreeMap internals
  (`core::…btree`, `BTreeMap::get`/`range`/`entry`) vs total route
  samples. If release inlining folds `scan_*` into `edge_cost`,
  name that — the counter census and the route_ms delta still
  quantify the class; do not manufacture a share.
- Digest/census baseline: `cargo test --release -p grus-eda-core
  --test drt test_maze_census -- --nocapture` captures
  `digest=0x…` (expected `0x8a4768ab15bd4bd5` per the
  `drt-guide-escape` record — **verify, don't trust the doc**) and
  `collisions=`.

## 3. After-swap run (identical commands)

Same rep loop, same `sample` window, same `GRUS_DRT_TIMING` census,
same `test_maze_census`. Record: `route_ms` min/median/max both
sides; attributable scan share both sides (naming inlined residue);
counter deltas (call counts must be **identical** — the query
stream is a router-output invariant; `ivs_visited` may differ
within noise only via early-exit equality, investigate if it
moves); `hits_pushed` identical.

## 4. picorv32 (optional arm — only if cheap)

A full picorv32 route is ~135 min (`test_maze_census_picorv32`
record). The spec allows carrying a counter on an existing run —
no such run is otherwise scheduled, so the honest cheap arm is:
start `GRUS_DRT_TIMING=1 $GRUS drt … picorv32.placed.def` in the
background, `sample` a 60–120 s window mid-run, let it finish
iteration 1 if it lands inside the window, then kill. Record
iteration-1 counters + the sample share labelled
"picorv32 partial (iter-1 window, killed)". If iteration 1 does not
complete inside the window, record the sample share alone and say
so. Do **not** let this arm block the ticket — unmeasured is a
named result (`goal.md` item 1, acceptance: "picorv32 figure named
if measured").

## 5. `docs/results.md` entry shape (feeds `docs.md`)

`### 2026-11-27 — drt-occupancy-ds: dense track-indexed occupancy (v0.14.1)`

1. Machine context (reuse the perf-profile block — host, `sw_vers`,
   `rustc --version`).
2. **Baseline table**: gcd `route_ms` median (min–max) ×5 reps;
   `sample` collapsed-stack share for the scan class; ScanStats
   totals + per-call averages (the query-shape answer).
3. **After table**: same rows; speedup stated as medians ratio.
4. **Identity evidence**: digest before/after (must be equal),
   `collisions=` count, DEF sha256 per rep, differential-test
   result (op count driven).
5. **picorv32 arm**: what ran or "not measured — deferred".
6. **What was run**: exact command list; prove-it-red transcript
   tail (`tests.md` §4).
