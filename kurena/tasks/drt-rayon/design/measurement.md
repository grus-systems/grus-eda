# measurement — phase split and wall-time protocol

Two measurement moments, mirroring the `drt-occupancy-ds` protocol:
**step 0** captures the baseline phase split at HEAD (the split
*names the targets* — acceptance item 1), and the after-run captures
the same rows at `RAYON_NUM_THREADS=8` plus the identity evidence.

## 1. Instrumentation being added

`GRUS_DRT_TIMING` already emits inside `route_nets`: `premark_obs`,
`topos`, per-iteration census, `min-area patch`, `occ-scan totals`.
Missing legs, added in `crates/grus-eda/src/main.rs` `Commands::Drt`
arm under the same env gate (stderr only, zero when unset):

- `drt: build_graphs {:?}` around `build_route_graphs_ext`
- `drt: emit_apply {:?}` around `apply_routes` + `preemit_check`
- `drt: emit_write {:?}` around `write_routed_def` + the self-DRC
  re-parse/`run_all` leg
- `drt: total {:?}` at the arm end

Inside `route_nets_with_progress` the per-iteration line gains
`search=`/`commit=` segments (`parallel-model.md` §5) so the parallel
split is measurable, and a `threads=N` token records the pool size
each run used.

## 2. Baseline run (at HEAD, before any parallel change)

```sh
cargo build --release -p grus-eda
GRUS=target/release/grus-eda
LIB=$HOME/.ciel/sky130A/libs.ref/sky130_fd_sc_hd
TLEF=$LIB/techlef/sky130_fd_sc_hd__nom.tlef
LEF=$LIB/lef/sky130_fd_sc_hd.lef
D=tests/designs/gcd

# Phase split — one timed run is enough for the split table
# (the split decides targets; wall-time reps come next).
GRUS_DRT_TIMING=1 $GRUS drt --tlef $TLEF --lef $LEF \
  --def $D/gcd.placed.def --guide $D/gcd.guide \
  --out-def /tmp/gcd.rayon.base.def \
  --reference $D/gcd.drt.reference.json \
  2>/tmp/gcd.rayon.base.timing.log | tee /tmp/gcd.rayon.base.log

# Wall time ≥5 reps (route_ms from the guided= census field);
# --no-self-drc per the occupancy-ds precedent.
for i in 1 2 3 4 5; do
  $GRUS drt --tlef $TLEF --lef $LEF --def $D/gcd.placed.def \
    --guide $D/gcd.guide --out-def /tmp/gcd.rayon.base.$i.def \
    --reference $D/gcd.drt.reference.json --no-self-drc \
    | tee /tmp/gcd.rayon.base.$i.log
done

# Baselines the parallel change must reproduce or re-pin:
cargo test --release -p grus-eda-core --test drt \
  test_maze_census -- --nocapture   # digest expect 0x8a4768ab15bd4bd5
shasum -a 256 /tmp/gcd.rayon.base.1.def   # expect 27f76659… (golden sha)
```

Also record a `RouteGraphs` serial-baseline fingerprint for the
build-identity claim: a `test_graph_census`-style debug hash
(`xxh3_64(format!("{graphs:?}"))` — temporary `println!` in the test,
value recorded in the results entry, not committed as a pin).

## 3. After run (identical commands, pool-forced thread counts)

Same rep loop ×2 arms — `RAYON_NUM_THREADS=1` and
`RAYON_NUM_THREADS=8` — recording `route_ms`, `build_graphs`,
`emit_*`, `total` per arm, min/median/max on `route_ms` and `total`.
The `threads=` token in each timing line is cross-checked against the
env var actually set (guards a miswired pool).

The split table drives the honest claim: if `route_nets` is < ~80 %
of `total` at baseline (it measured ~12 s of a ~12 s leg at
occupancy-ds — expect it dominant), that is stated; if emit/build
turn out nontrivial, the entry says which of them the parallel map
actually bought.

## 4. picorv32 arm (optional — only if cheap)

The gcd e2e golden chain must regenerate regardless (`tests.md` §4);
picorv32's is the expensive leg. The honest cheap arm: one
`GRUS_DRT_TIMING=1 RAYON_NUM_THREADS=8` picorv32 `drt` invocation run
to completion in the background while the rest of the ticket lands —
it doubles as the regen input for `generate_drt_ours.sh picorv32`
(`tests.md` §4). If it does not finish inside the ticket's budget,
record iteration-1 counters + the partial label as occupancy-ds did,
and mark the ours-artifact regen deferred-named.

## 5. `docs/results.md` entry shape

`### 2026-11-27 — drt-rayon: parallel route graph build, maze search, emit (v0.14.3)`

1. Machine context (reuse the occupancy-ds block).
2. **Phase-split table (baseline)**: `build_graphs`, `premark`,
   `topos`, rip-up total (search/commit segments), `min-area patch`,
   `emit_apply`, `emit_write`, `total` — names the targeted phases.
3. **Wall table**: `route_ms` and `total_ms` medians (min–max) ×5
   reps — baseline vs `threads=1` vs `threads=8`; speedup as median
   ratio. The acceptance figure is gcd route wall time.
4. **Identity evidence**: `route_digest` old → new (the expected
   snapshot-semantics change — *why* it changed, not a tolerance);
   threads=1 vs threads=8 digests **equal**; `collisions=` count;
   DEF sha256 per arm vs regenerated golden; build-graphs debug hash
   identical at both thread counts and vs the serial-baseline value.
5. **Scheme record**: the named invariant (snapshot-search /
   ordered-commit), the hard-iteration serial note, what stays
   serial (min-area patch, censuses, `run_all`).
6. **picorv32 arm**: what ran or "not measured — deferred".
7. **What was run**: exact command list; prove-it-red transcript
   tails (`tests.md` §5).
