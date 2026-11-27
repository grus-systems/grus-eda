# tests — determinism pins and golden regeneration

All tests live in `crates/grus-eda-core/tests/drt.rs`; all fail
loudly on missing inputs (existing convention — no silent skips).
Thread control uses `std::env::set_var("RAYON_NUM_THREADS", …)` —
the private `drt_pool()` reads it at call time
(`parallel-model.md` §7), so the arms are honest in-process.

## 1. Build/emit serial-identity (the acceptance byte-identical bar)

**`test_rayon_graphs_serial_identity`** (new): `build_route_graphs`
on gcd under `RAYON_NUM_THREADS=1` then `=8` → full `RouteGraphs`
equality — `nets` plus every census field (`zero_coverage`,
`region_escape`, `escapes`, `total_*`, `via_edges_per_pair`,
`hull_masked`). Serial-baseline identity itself is pinned by the
debug-hash before/after record (`measurement.md` §2) and by every
pre-existing graph assertion staying green (`test_graph_census`,
`test_pin_attachment`, `test_graph_determinism`).

**`test_rayon_emit_serial_identity`** (new): on a *fixed* `RouteDb`
(any single `route_nets` run — thread count of the route is
irrelevant to the emit proof), `apply_routes` + `write_routed_def`
under 1 then 8 threads → byte-identical DEF text, identical
`db.net_geoms` (`PartialEq`), identical `EmitReport`. Decoupling the
emit proof from the route output is deliberate: the route digest
legitimately changes, so emit-vs-serial can only be proven on a
pinned input.

Existing guards that stay green unchanged:
`test_graph_determinism`, `test_emit_determinism`,
`test_route_nets_progress_hook` (hooked ≡ unhooked).

## 2. Route-loop determinism (the weaker-but-honest bar)

**`test_rayon_route_determinism`** (new, gcd):
- `RAYON_NUM_THREADS=1` → `route_nets` → digest `d1`, `collisions`
  `c1`, unrouted census `u1`
- `RAYON_NUM_THREADS=8` → digest `d2`, `c2`, `u2`
- `RAYON_NUM_THREADS=8` → digest `d3` (run-to-run arm)
- Assert `d1 == d2 == d3` (the stronger any-thread contract,
  `parallel-model.md` §4), `c1`/`c2` empty, `u1 == u2`, and
  `d1 != 0` (nonzero evidence). `test_maze_determinism` stays as the
  fixed-thread double-run pin.

**`GCD_ROUTE_DIGEST` re-pin**: the digest legitimately changes
(searches see the iteration-start snapshot, not serial-interleave
occupancy — `parallel-model.md` §2). Step order matters: the
baseline digest `0x8a4768ab15bd4bd5` is recorded in the results
entry *before* the change; the new constant is measured post-change,
pinned, and the results entry records old → new with the
snapshot-semantics explanation. `collisions=` must remain `0` — the
collision-free end state is part of the bar, not the digest value.

## 3. Compile-time sync assertion

In `maze.rs`'s `#[cfg(test)]` module:
`const _: fn() = || { fn need<T: Sync + Send>() {}; need::<Occupancy>(); need::<NetTopo>(); };`
plus `need::<HullMask>()` in `graph.rs` — a future `Cell`/`RefCell`
regression fails at compile time naming the type.

## 4. Golden chain regeneration (collision-free end state evidence)

The route output changes → the ours-side artifacts must be
regenerated, in order, per the committed scripts (E0 rule 1 —
run as written, `git diff` the outputs):

1. `tests/designs/generate_drt_ours.sh gcd` (host, release binary —
   self-DRC default-on, findings budget 0: **a nonzero findings
   total aborts the emit leg** — if the new route produces any
   self-DRC finding, that is a real regression or a named residual;
   it is never budgeted up silently).
2. `tests/designs/generate_drt_ours.sh gcd --verify` — the
   script's own second-run `cmp` (`OURS-DETERMINISTIC <sha>`) is the
   run-to-run determinism evidence at default thread count.
3. `tests/designs/check_drt_ours.sh gcd` (LibreLane container via
   Colima — Magic `drc(full)` on our DEF + placed baseline + the
   mutation control) → new `gcd.drt.ours.reference.json`.
   `PINNED_LEF_RESIDUALS` stays empty for gcd iff every
   `lef_expressible` row still reports `ours == placed`; any moved
   count is a named residual entry, never a silent re-pin.
4. `test_e2e_ours_def_deterministic_gcd` (byte-equal vs the new
   committed DEF) and `test_e2e_ours_reference_gcd` (the joined
   golden) go green on the regenerated artifacts.

picorv32 (`picorv32.drt.ours.routed.def` is gitignored; only its
`drt.ours.reference.json` is committed): regenerate via the same two
scripts iff the `RAYON_NUM_THREADS=8` route fits the ticket budget
(`measurement.md` §4 — the parallel route should cut the recorded
~135 min substantially). If regenerated, `PINNED_LEF_RESIDUALS`
picorv32 counts (`li.3` 19, `met1.2` 146) are re-measured — moved
counts get named new pins with the mechanism recorded; if not
regenerated, the stale picorv32 ours-artifact is a **named deferred
gap** in `results.md` (the committed golden describes geometry the
current code no longer emits).

## 5. Prove-it-red arms (AGENTS §7.7 — executed, restored, transcripts in results)

1. **Commit-order break**: rasterize `commit_route` *before* the
   `edge_collisions` prov scan → the route's own marks mask stale
   foreign hits → stale pairs commit silently → `overlap_census`
   reports real collisions → `test_rayon_route_determinism` red on
   `collisions.is_empty()`. Restore.
2. **Fold-order break**: collect per-net build partials via
   `collect::<BTreeMap>`-style unordered fold (or sort `out.nets`
   by `net.0`) → `test_rayon_graphs_serial_identity` /
   `test_graph_determinism` red. Restore.
3. **Digest pin**: bump `GCD_ROUTE_DIGEST` → `test_maze_census`
   red naming drift. Restore.

## 6. Verify commands (workflow.toml lanes)

`cargo test --workspace --profile verify` + `cargo clippy --workspace
--all-targets -- -D warnings` — both must pass at every commit.
`#[ignore]`d picorv32 legs run only under the explicit arm above.
