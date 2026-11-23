# e6-drt-via — Implementation steps

Sized ~450–550 lines across reader/model + via.rs + graph delta +
tests + docs; if it grows past ~550, split into two commits
(1–2 model/table, then 3–5 edges/census/docs). Order:

1. **DEF `VIAS` retention** — `design_db.rs`: `DefViaDef` struct +
   `#[serde(default)] def_vias` field next to `PdnVia`/`ViaGeom`;
   `readers/def.rs`: at the `VIAS` dispatch, push resolved records
   per `api.md` (Generated → `ViaGeom::Generated`; Fixed → resolved
   `ViaGeom::Lef`; Inline → nothing). No behavior change on the
   net-resolution path — `DefViaMap` stays the resolver.
2. **`drt/via.rs`** — `ViaCandidate`/`UnmatchedVia`/`ViaGap`/
   `ViaPair`/`ViaTable` + `via_table(tech, db)`: candidate
   enumeration per `via-model.md` §1, `expand_via`-at-origin rects +
   `area_dbu2`, the `(nonpref_metal, area_dbu2, order)` ranking per
   §3. `drt.rs`: `pub mod via;` + doc line. In-module unit tests per
   `tests.md`.
3. **`drt/graph.rs` via edges** — `ViaEdge`, `NetGraph::via_edges`,
   `RouteGraphs::{total_via_edges, via_edges_per_pair}`; emission in
   `build_route_graphs` after the per-layer loop, per `via-edges.md`
   §1 (iterate `region` adjacent pairs present on both sides;
   `layer_base` + sorted-pairs `binary_search` lookups — same
   idioms `layer_edges`/`pin_nodes` already use). In-module unit
   test for shared/one-sided coverage.
4. **Integration tests** (`tests/drt.rs`) — `load_drt_ref` helper,
   `test_via_candidate_table`, `test_via_edge_census`,
   `test_via_edge_fixture` (incl. coverage-drop fail-proof and the
   off-crossing fixture the acceptance names), determinism fold-in.
   Run `-- --nocapture` to collect the printed census for
   `results.md`; pin per-pair edge counts after the first real run
   (met2–met3 `== 0` is already known — `via-edges.md` §2).
   `picorv32.placed.def` must exist (generate via
   `tests/designs/picorv32/generate_ref.sh` if absent — container
   run, mention before running).
5. **Docs + version** — `docs/e6-drt.md` §6 + claim-boundary
   update, `docs/results.md` dated entry with the **actual** printed
   numbers (never the planning probe's — if they differ, the real
   run wins and the difference is recorded), `docs/status.md` head,
   `docs/roadmap.md` progress line; `Cargo.toml` workspace version
   `0.12.2 → 0.12.3` in the same commit as the functional change.

## Verification

- `cargo test -p grus-core --test drt -- --nocapture` — the via
  census print (both designs) for `results.md`.
- `cargo test -p grus-core --lib via` / `--lib graph` — new unit
  tests.
- `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` — record
  outcomes in `results.md`.
- No container run needed: the reference evidence is the committed
  `gcd.routed.def` / `<d>.drt.reference.json` + the committed
  `TRACKS` — the §3 placement table is arithmetic on files already
  in the repo (state which script/reading produced it in
  `results.md`, per AGENTS §7.9).

## Files

- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/drt/via.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
