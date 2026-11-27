# tests — determinism pins and the byte-identity gate

All in `crates/grus-eda-core/tests/drt.rs`; the existing
fail-loudly-on-missing-inputs convention holds (no silent skips).

## 1. Pins that must stay green (unchanged expectations)

- `test_maze_census` — `GCD_ROUTE_DIGEST = 0xef2b3ff70271a2e9`.
  None of the options are semantic; a drift is a bug.
- `test_maze_determinism` (+ ignored `test_maze_determinism_picorv32`).
- `test_rayon_graphs_serial_identity`, `test_rayon_route_determinism`
  (threads=1 ≡ threads=8 — any picked change must keep the
  ordered-commit contract), `test_rayon_emit_serial_identity`.
- `test_graph_determinism`, `test_emit_determinism`,
  `test_e2e_ours_def_deterministic_gcd`
  (byte-equality vs the committed `gcd.drt.ours.routed.def`),
  `test_e2e_ours_reference_gcd`.
- `test_maze_occupancy` (enabled/disabled digests differ — proof the
  occupancy path still feeds decisions).

## 2. New tests (conditional on the picked options)

- **`test_mem_census_sanity`** (lands with commit 1, unconditional):
  on the gcd fixture, `route_graphs_bytes(graphs)` and
  `net_topo_bytes(topo)` return nonzero, internally consistent sizes
  (`≥ Σ len × size_of` per field — capacity ≥ len). A census that
  reads zero would make the whole measurement worthless; assert the
  floor, not a literal (sizes legitimately move with later changes).
- **O2 only — `test_net_graph_rebuild_identity`**: per guided net on
  gcd, `build_net_graph(net, boxes, …)` equals `graphs.nets[i]` under
  `PartialEq` (nodes/edges/via_edges/pin_nodes + census partial).
  Threads=1 and threads=8 arms identical.
- **O3 only — no new test needed**: identical commit stream is a
  construction argument; `test_rayon_route_determinism` +
  `test_maze_census` pin it.
- **O1 only — no new test needed**: CSR equivalence is
  representation-only; the digest + DEF pins cover it (state this
  deliberately rather than adding a second-order test).

## 3. Byte-identity gate (the acceptance criterion)

- gcd: `test_e2e_ours_def_deterministic_gcd` vs the committed
  golden — the hard pin. Plus a `/usr/bin/time -l` rep's DEF `cmp`d
  against `/tmp/gcd.mem.base.def`.
- picorv32: `cmp` the post-change `--out-def` against
  `/tmp/picorv32.mem.base.def` (generated in step 0). The reference
  JSON + emit-log census lines (`guided`, `routed`, `unrouted`,
  `iter_progress`) must also match — `check_drt_ours.sh`-style, but
  `cmp` on the DEF is the acceptance statement.

## 4. Prove-it-red (AGENTS §7.7)

Per picked change, one deliberate break → named test red → restore:

- O1: drop the `relax` arcs from the CSR build → `test_maze_census`
  red on digest drift (different reachable graph).
- O2: make the per-attempt rebuild emit a stale/extra node (perturb
  `net_region` by one index) → `test_net_graph_rebuild_identity` red
  and `test_maze_census` red.
- O3: reverse one chunk's commit order → `test_rayon_route_determinism`
  or digest red.
- Commit 1: gate forced on without the env var → `drt: mem` lines
  appear → restore (proves the gate, not an always-on leak).

## 5. Checks

`cargo test --workspace` (and `--profile verify` for the route
re-execution tests) and `cargo clippy --workspace --all-targets --
-D warnings` after every commit — the `workflow.toml` verify pair.
picorv32 route runs are detached legs; record what actually ran
(E0 rule 9 — "say what you ran").
