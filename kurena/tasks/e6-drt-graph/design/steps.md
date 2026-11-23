# e6-drt-graph — Implementation steps

One commit per the ticket convention (~300–450 lines across
`graph.rs` + test additions; if it grows past ~500, split: 1–2
model, then 3–5 census+docs). Order:

1. **`drt/graph.rs` core** — types (`RouteNode`, `RouteEdge`,
   `NetGraph`, `GuideJoin`, `ZeroCoverage`, `RegionEscape`,
   `RouteGraphs`), `join_guides` (wrap `resolve_guide_nets` +
   member-pin signal classification per `guide-join.md`), and
   `build_route_graphs` (per-net `BTreeMap<layer, BTreeSet<pair>>`
   regions → sorted nodes → consecutive-index edges on both axes →
   `pin_shapes`-driven `pin_nodes` + `region_escape`). In-module
   unit tests from `tests.md` (region sets, edge lists, join
   classification, unknown-name error).
2. **Visibility + re-export** — `access.rs`: `pin_shapes` →
   `pub(crate)` (no behavior change); `drt.rs`: `pub mod graph;`
   plus one doc line.
3. **Integration tests** (`tests/drt.rs`) — extend `load` with the
   guide join; add `test_guide_join`, `test_graph_census`,
   `test_pin_attachment`, `test_graph_determinism`,
   `test_region_escape_fixture` (control → shrink → empty-region
   fail-proof). Run `--nocapture` to collect the printed census for
   results.md. `picorv32.placed.def` must exist — generate via
   `tests/designs/picorv32/generate_ref.sh` if absent (container
   run, mention it before running; it was present at planning time).
4. **Docs** — `docs/e6-drt.md` graph section + taxonomy + claim
   boundary; dated `docs/results.md` entry with the **actual**
   printed numbers (not the planning probe's — if they differ,
   root-cause the difference first); `status.md` head line;
   `roadmap.md` E6 progress line.
5. **Version** — `Cargo.toml` `0.12.1 → 0.12.2` (feature, patch on
   0.x) + `Cargo.lock` in the functional commit.
6. **Verify** — `cargo test --workspace`;
   `cargo clippy --workspace --all-targets -- -D warnings`. Record
   commands + outcomes in `docs/results.md`.

## Sequencing notes

- The fixture test (step 3) needs no real designs — write it first
  if you want fast red/green iteration on the attachment rule.
- The pinned gcd escape set (`_313_/X`, `_301_/B1`, `_307_/B1`)
  comes from the planning probe's independent model — treat it as
  the expected answer, not gospel: if the real run names a
  different small set, verify per-pin which side is right before
  pinning, and say what happened in results.md.
- Keep `pin_nodes` covering *every* member pin (escapes get empty
  lists) — the partition assert depends on it.
- No `rayon`, no new crates.

## Files

- `crates/grus-core/src/drt/graph.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/src/drt/access.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
