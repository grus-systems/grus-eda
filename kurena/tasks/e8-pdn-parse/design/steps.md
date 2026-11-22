# e8-pdn-parse — Implementation order

Small, ordered steps; each is independently testable. Version bump
ships in the commit that removes the `special_wire_rects` field
(breaking → minor, `0.6.0` → `0.7.0` per workspace semver rules).

## Steps

1. **`tech_db`: cut layers + `ViaDef` scaffolding.**
   `CutLayer`, `ViaDef`, `cut_layers`, `cut_layer_by_name`,
   `via_defs`, `via_by_name`, `via_res_ohm()`, `rebuild_maps`
   updates; `parse_layer` `is_cut` branch starts recording
   `CutLayer`. `cargo test -p grus-core` — existing tech tests must
   still pass (fields are additive).
2. **`lef.rs`: `parse_via`.** Grammar per `lef-via.md`; skip-report
   `VIA` entry disappears (update the fixture expectations in the
   same change). Tests: `parses_via_blocks`, `via_unknown_layer_fails`.
3. **`design_db`: `Pdn*` types + `pdn_nets`.** Add types and the
   `#[serde(default)]` field; keep `special_wire_rects` *temporarily*
   (removed in step 6) so def.rs can migrate incrementally.
4. **`def.rs`: `VIAS` section + `add_special_net` rewrite.** Per
   `def.md`: `DefVia` map, conn groups, wire/via statements, ordering
   assumption, merge rule. Tests: `parses_specialnets`,
   `via_layers_from_vias_section`, `renamed_via_still_resolves`,
   error-path tests. At this point `special_wire_rects` is still fed
   from the same code path — assert old flat count equals the union
   of new per-net rects before deleting.
5. **`pdn.rs` + `lib.rs`: graph, attach, census.** Union sweep, via
   edges, conn expansion, `orient_rect` lift into `design_db.rs`,
   `build_graph`/`check_net`/`census`. Tests: `gcd_attach_and_connect`
   (all golden asserts), `gcd_orient_fixture_attach`,
   `degenerate_via_point`, `dropped_via_edges_flag_unreachable`.
6. **Breaking swap: drop the `special_wire_rects` field,** add the
   accessor, repoint `gcell.rs`, bump workspace version `0.6.0 →
   0.7.0` in the same commit. Regression: `special_wire_rects_view`
   test + existing congestion tests.
7. **`picorv32_census` + red→green ritual.** Run the picorv32 test;
   perform the acceptance's "remove via edges, observe red, restore"
   on the *real* gcd test; note both in `docs/results.md`.
8. **Docs + full verify.** `docs/results.md` entry (what ran, census
   numbers, per-net reference agreement, cold picorv32 parse time,
   `cargo test` wall time — AGENTS.md results format); update the §E8
   line in `docs/roadmap.md` (SPECIALNETS via extraction + LEF VIA
   parsing land; resistor mesh/solve stay open); keep
   `docs/e0-readers.md` SPECIALNETS bullet in step (flat list →
   `pdn_nets`). Then:
   `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`.

## Acceptance → step map

- per-net rects + via stacks + attach table on gcd → steps 4–5
- `check_power_grid`-shaped connectivity, `unreachable_pins` empty →
  step 5 (`gcd_attach_and_connect`)
- attached counts == per-file CSV rows → step 5
- renamed-via resolution through defs → steps 2, 4
- picorv32 census in `docs/results.md` → steps 7–8
- fail-proof red→green → steps 5 (synthetic), 7 (real)
- workspace test + clippy + version bump → steps 6, 8
- `docs/results.md` entry → step 8

## Open decisions left to implementation

- Whether `PdnConn::Instance` name resolution in `pdn.rs` builds its
  own `cell name → CellId` map per call (10k-name scan is fine at this
  scale) or `DesignDb` grows a persistent `cell_by_name` index —
  pick the scan unless a consumer needs it elsewhere.
- Exact `RectDbu` normalization for two-point segments (already
  handled by existing rect construction — reuse verbatim).
- Whether `PdnNet` gets a `wire_rects_on(layer)` helper or pdn.rs
  buckets rects itself — an implementation detail, not API.

## Files

- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/pdn.rs` (new)
- `crates/grus-core/src/congestion/gcell.rs`
- `crates/grus-core/src/lib.rs`
- `Cargo.toml`
- `docs/results.md`
- `docs/roadmap.md`
- `docs/e0-readers.md`
