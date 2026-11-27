# steps — implementation order, commits, verification

Five commits. Order matters: the baseline phase split and digests
must be captured at HEAD (step 0), instrumentation lands while
output is still serial-identical (commit 1), and the route-loop
change is where goldens legitimately move (commit 3).

## Step 0 — baseline capture at HEAD (no code change)

1. `cargo build --release -p grus-eda`; run the `measurement.md` §2
   command list: one `GRUS_DRT_TIMING` run (phase split — but the
   build/emit legs are not yet instrumented, so capture what exists:
   `premark`/`topos`/per-iter/`patch`/`occ-scan totals`; `total` is
   timed by `route_ms` + CLI elapsed — record honestly), then
   `route_ms` ×5 reps.
2. `test_maze_census` → record `digest` (expect
   `0x8a4768ab15bd4bd5` — verify) and `collisions=`.
3. `sha256` of one rep DEF → expect `27f76659…` (committed golden).
4. `RouteGraphs` debug-hash baseline via a temporary `println!`
   (`measurement.md` §2) — recorded in the results entry, never
   committed as a pin.

## Step 1 — `feat:` instrumentation + Sync enablement + pool (commit 1)

`main.rs` (the 4 timing lines, `measurement.md` §1), `maze.rs`
(`Cell<u64>` → `AtomicU64` Relaxed + `search=`/`commit=` segments +
`threads=` token), new `drt/pool.rs` (`drt_pool()` +
`mod pool;` in `drt.rs`), `tests/drt.rs` (compile-time sync
assertion, `tests.md` §3). No parallel call sites yet — behavior is
serial-identical; `test_maze_census` digest must stay
`0x8a4768ab15bd4bd5`.

- `Cargo.toml` workspace version `0.14.2 → 0.14.3`.
- Commit: `feat: Add DRT phase timing and prepare occupancy for sharing`.

Verify before commit: `cargo test --workspace --profile verify`
green; digest identical. Re-run the §2 phase-split command → the
**baseline split table** now has every row; this is the
"phase-split timings recorded before choosing targets" acceptance
item — write it into `docs/results.md` at this commit or hold for
commit 5, but capture it here.

## Step 2 — `refactor:` parallel graph build (commit 2)

`graph.rs` only: per-net partial `NetBuild` + `map_init` scratch +
`pool.install` map over `join.nets` + serial fold
(`build-emit.md` §1); `escape_extend` signature kept, sink becomes
the per-net vec.

- `tests/drt.rs`: `test_rayon_graphs_serial_identity` (`tests.md` §1).
- Commit: `refactor: Build route graphs per-net in parallel`.

Verify: verify-lane green; new test green at both arms;
`test_graph_determinism`/`test_graph_census` unchanged;
`test_maze_census` digest still `…4bd5` (build order and content are
identical — a drift here means the fold is wrong, investigate before
proceeding).

## Step 3 — `feat:` parallel soft-iteration search + ordered commit (commit 3)

`maze.rs`: `topos` par-map; `NetOutcome::Routed` gains `used_edges`;
soft iterations split into `par_iter` search + serial commit with
the commit-time `edge_collisions` prov scan; hard iteration keeps
the serial interleave (`parallel-model.md` §3); progress sink moves
to the commit loop.

- `tests/drt.rs`: `test_rayon_route_determinism` (`tests.md` §2);
  `GCD_ROUTE_DIGEST` re-pinned to the measured post-change value.
- Golden chain (`tests.md` §4): `generate_drt_ours.sh gcd` +
  `--verify`, then `check_drt_ours.sh gcd` (container) → regenerated
  `gcd.drt.ours.routed.def` + `gcd.drt.ours.reference.json`
  committed in this commit; `PINNED_LEF_RESIDUALS` moves only as
  named residuals.
- Commit: `feat: Route nets by parallel search and ordered commit`.

Verify: verify-lane + clippy green; `test_e2e_ours_def_deterministic_gcd`
+ `test_e2e_ours_reference_gcd` green on regenerated goldens;
prove-it-red arms 1–3 (`tests.md` §5) executed and restored.

## Step 4 — `feat:` parallel emit assembly (commit 4)

`emit.rs`: `apply_routes` per-net `NetGeom` map + serial
`db.net_geoms`/report fold; `nets_section` per-net record map +
ordered concat (`build-emit.md` §2–3).

- `tests/drt.rs`: `test_rayon_emit_serial_identity` (`tests.md` §1).
- Commit: `feat: Assemble routed net geometry in parallel`.

Verify: verify-lane green; byte-identical DEF at 1 vs 8 threads on a
fixed `RouteDb`; `test_e2e_ours_def_deterministic_gcd` still green
(the route output is unchanged by this commit — emitted bytes must
not move; if they do, the emit fold is wrong).

## Step 5 — after-measurement + `docs:` entry (commit 5)

- `measurement.md` §3 verbatim: `RAYON_NUM_THREADS={1,8}` × 5 reps →
  wall table; phase-split after-table; picorv32 arm per §4 (or
  named deferred).
- picorv32 ours-artifact regen if the arm ran (`tests.md` §4).
- `docs/results.md` dated entry per §5 skeleton;
  `docs/e6-drt.md` edits per `docs.md`; `docs/status.md`
  last-updated line.
- Commit: `docs: Record rayon phase split and route speedup`.

## Verification checklist (maps to acceptance)

- [ ] Phase-split timings recorded before choosing targets (commit 1
      capture, results entry).
- [ ] build/emit byte-identical to serial under forced
      `RAYON_NUM_THREADS=1` and `=8` (`test_rayon_graphs_serial_identity`,
      `test_rayon_emit_serial_identity`, debug-hash baseline);
      route loop pinned deterministic at fixed **and** varying
      thread count (`test_rayon_route_determinism`) +
      collision-free end state (`collisions=0`, regenerated
      e2e goldens green).
- [ ] No unsafe shared-occupancy mutation — invariant named:
      snapshot-search / ordered-commit (`parallel-model.md` §1);
      `&Occupancy` shared immutably, commits serial in `bad` order.
- [ ] gcd route wall time improves — both medians in results entry.
- [ ] `cargo test --workspace --profile verify` + clippy
      `-D warnings` green at every commit.

## Files

- `crates/grus-eda-core/src/drt/graph.rs`
- `crates/grus-eda-core/src/drt/maze.rs`
- `crates/grus-eda-core/src/drt/emit.rs`
- `crates/grus-eda-core/src/drt/pool.rs`
- `crates/grus-eda-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-eda-core/tests/drt.rs`
- `tests/designs/gcd/gcd.drt.ours.routed.def`
- `tests/designs/gcd/gcd.drt.ours.reference.json`
- `Cargo.toml`
- `docs/results.md`
- `docs/e6-drt.md`
- `docs/status.md`
