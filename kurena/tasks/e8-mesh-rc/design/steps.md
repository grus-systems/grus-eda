# e8-mesh-rc — Implementation order

One commit's worth of work, but ordered so each step compiles and is
independently testable. Patch bump `0.7.0 → 0.7.1` rides in the
same commit (additive public API).

## Steps

1. **Module skeleton + fixture resistance.** `pdn.rs` gains `pub
   mod mesh;` + `pub use mesh::*;`; create `pdn/mesh.rs` with the
   type declarations (`api.md`) and empty builders; add `RESISTANCE
   RPERSQ <r> ;` to `met1`/`met2` in the `PDN_LEF` fixture. `cargo
   test -p grus-core --lib pdn` — existing 7 tests still green.
2. **Feature collection.** Major-axis helper, the five feature
   kinds per rect (ends, same-layer overlap spans via the shared
   sweep logic, via landing points, pin-attach intersections,
   source io-rect intersections). Build `rect → sorted feature
   intervals`. No mesh output yet — unit-assert interval sets on
   the fixture if convenient.
3. **Junction pieces → nodes → wire resistors.** Closed-interval
   merge per rect, `UnionFind` over pieces per layer under
   `rects_intersect`, run emission with center-to-center `L`,
   `R = res_per_sq × L / W`. `test_synthetic_mesh_shape` +
   `test_synthetic_wire_r_arithmetic` green.
4. **Via resistors + sinks + sources.** `via_res_ohm` pricing,
   endpoint node lookup (lowest index on multi-piece ambiguity),
   dangling via-end nodes; sink map from `graph.attachments`
   distinct pins (lowest-index candidate node); source nodes from
   io-rect intersections. `test_synthetic_via_r_from_techdb`,
   `test_synthetic_dangling_via`.
5. **`validate()` + `census()` + `check_mesh`/`meshes`.**
   Finite/positive-R scan, index resolution checks, source-
   reachability BFS. Fail-proof unit tests both variants.
6. **Real-design tests.** `test_gcd_mesh_census` (per-element sink
   set vs graph attachments, `csv_rows` from the reference JSON),
   `test_gcd_orient_fixture_mesh`, `test_picorv32_mesh_census`.
7. **Observed break + restore on the real test** (tests.md item
   3), then `docs/results.md` (per-net census both designs, what
   ran, runtime via `--nocapture`, fail-proof observation, explicit
   "no reference mesh exists — recorded census, not a gate") and
   the §E8 line in `docs/roadmap.md` (mesh lands; DC solve stays
   open). Bump `Cargo.toml` to `0.7.1`.
8. **Full verify**: `cargo test --workspace && cargo clippy
   --workspace --all-targets -- -D warnings`.

## Acceptance → step map

- `PdnMesh` for VPWR+VGND on gcd, census recorded → steps 3–6
- Resistance from `TechDb` (`RPERSQ`, `via_res_ohm_above` /
  parallel cuts, no duplicated table) → steps 3–4 (reads
  `routing_layers[].res_per_sq` + `TechDb::via_res_ohm` only)
- Via cut count from LEF `VIA` cut-RECT count; renamed-via proof →
  step 4 (`cut_count` already normalized by ticket 2; fixture via
  `stack_a` is the renamed-via test)
- Invariants (R finite > 0, no dangling sink/source, sink set ==
  ticket-2 attach set, picorv32 census recorded) → steps 5–6
- Fail-proof (zero `RPERSQ` / remove via edges → red → restore) →
  steps 5, 7
- `cargo test --workspace` + clippy clean + patch bump → steps 7–8
- `docs/results.md` per-net census + runtime → step 7

## Open decisions left to implementation

- Whether `mesh.rs`'s tests reuse `pdn.rs`'s private
  `mod tests` fixtures in place (visible to descendant modules) or
  the fixture constructors hoist to `pub(crate)` helpers — either
  satisfies the plan; prefer in-place reuse, hoist only if cfg
  scoping fights.
- Whether the same-layer overlap sweep is factored out of
  `try_build_graph` for reuse (it emits features *and* unions) or
  the mesh runs its own pass — either is fine; prefer factoring
  only if it stays readable, a second ~30-line sweep is cheaper
  than a coupled one.
- Sink candidate-node tie-break is fixed (lowest node index); the
  *order* of `sinks` itself should follow `graph.attachments`
  first-appearance order — deterministic, matches CSV row order
  spirit.
- Exact census print format for `docs/results.md` — implementation
  chooses; match the ticket-2 entry's table style.

## Files

- `crates/grus-core/src/pdn.rs`
- `crates/grus-core/src/pdn/mesh.rs` (new)
- `Cargo.toml`
- `docs/results.md`
- `docs/roadmap.md`
