# e7-drc-geom — Implementation steps

Ordered so each step compiles and commits green. Suggested commit
split (one ticket, several focused commits per AGENTS §5).

## Step 1 — LEF/tech data retention

- `tech_db.rs`: `ViaRectLayer`, `ViaLayerRect`, `ViaDef.rects`,
  `CutLayer.width_dbu`; fix the stale `ViaDef` doc comment.
- `lef.rs`: `parse_via` collects per-group `Vec<RectDbu>` → `rects`;
  record `width_um` into `CutLayer`.
- Unit tests in `lef.rs` (rect retention, multi-cut group, cut width).
- **Commit**: `feat: Retain LEF VIA rects and CUT layer widths` +
  version bump `0.8.0 → 0.9.0`.

## Step 2 — DEF via geometry plumbing

- `design_db.rs`: `ViaGeom`, `GeneratedViaGeom`, `PdnVia.geom`.
- `def.rs`: `DefVia::Generated` gains `geom`; `add_def_via` retains
  `CUTSIZE` (required → bail absent), `CUTSPACING`, `ENCLOSURE`,
  `ROWCOL` per `def-grammar.md` defaults; `resolve_def_via` returns
  the `ViaGeom` fifth element and its messages say "net" not
  "SPECIALNETS net" (check pinned assertions first).
- `pdn.rs` call sites destructure/ignore `geom`.
- Unit test: generated-via params parsed (ROWCOL 1 5 →
  `cut_count == 5`, params present).
- **Commit**: `feat: Carry via shape geometry through DEF via resolution`.

## Step 3 — `NETS` routed-geometry parse

- `design_db.rs`: `NetGeom`, `net_geoms` + I7 invariant,
  `net_geom()`, `signal_wire_rects()`; `create_net` gains the
  `net_def_spellings`-style conditional `NetGeom::default()` push.
- `def.rs`: NETS loop → `for_each_record` + `add_net`; conn scan
  with the existing inline lookups moved verbatim, `+` dispatch,
  `parse_net_wire` per `def-grammar.md`; the deferred SoA/CSR build
  is untouched except assigning `db.net_geoms`.
- Unit tests per `tests.md` §def.rs.
- **Commit**: `feat: Retain routed geometry for signal nets in DEF`.

## Step 4 — `drc` module: shapes + index

- `lib.rs`: `pub mod drc;` (+ re-export convention per module list).
- `drc.rs`: `DrcLayer`, `ShapeSrc`, `ShapeRect`, `LayerShapes`,
  `DrcDesign`, `Violation`; `mod geom; mod width;`.
- `drc/geom.rs`: `assemble`, `expand_via`, `iter_overlapping`.
- Unit tests per `tests.md` §geom (generated-via arithmetic +
  orientation + source coverage fixture).
- **Commit**: `feat: Add drc geometry model with per-layer shape index`.

## Step 5 — Width family

- `drc/width.rs`: `WIDTH_RULES` table, `check_width`.
- Bound-parity + thin-shape unit tests.
- **Commit**: `feat: Add width-rule DRC family (LEF-bound)`.

## Step 6 — Integration tests + mutation + fail-proof

- `tests/drc.rs`: `load_design` helper + the five tests in
  `tests.md`.
- Run mutation fail-proof (break `<` → `<=`, capture red output,
  restore); keep the excerpts for `results.md`.
- **Commit**: `test: Add DRC integration tests and width mutation fixture`.

## Step 7 — Docs + verify

- `docs/e7-drc.md` additions, `docs/results.md` dated entry,
  `docs/status.md` row, `docs/roadmap.md` line per `docs.md`.
- `cargo test --workspace` + `cargo clippy --workspace --all-targets
  -- -D warnings` clean.
- **Commit**: `docs: Record e7-drc-geom verification and rule taxonomy status`.

## Done-when map

| Acceptance | Covered by |
|---|---|
| `parse_def` retains routed geometry, per-layer counts, deterministic re-derivation | Steps 3, 6 (`retains_routed_geometry`, `roundtrip_deterministic`) |
| Via resolution via `TechDb::via_defs`, loud unknown-name errors | Steps 2–3 (`resolve_def_via` shared path, unit tests) |
| All four sources + right layer + orientation | Steps 4, 6 (source-coverage + orientation fixtures) |
| Width 0 violations, `magic_per_rule` join | Steps 5–6 (`width_zero_violations` + name-parity) |
| Mutation fixture + fail-proof in results.md | Step 6 (`mutation_met1_below_width`, `<`→`<=` break) |
| W/FE orientation fixture | Step 4 (geom unit tests) |
| `cargo test --workspace` + clippy | Step 7 |
| `results.md` entry + `e7-drc.md` provenance + family table | Step 7 |

## Watch-items during implementation

- `resolve_def_via` error text rename — grep tests for pinned
  "SPECIALNETS net" substrings before editing.
- picorv32 `NETS` volume (~6.8k records) — `for_each_record`
  reuses one `words` buffer; keep it that way (allocation profile).
- `create_net` call sites (cts) — the I7 push must fire only when
  `net_geoms` is already parallel (`net_def_spellings` precedent).
- `MINI_DEF` conn-error tests may need message tweaks if the
  pair-scan text differs — keep `` `( component pin )` `` verbatim.
- If the width 0-gate trips on real shapes, use `ShapeSrc`/`net`
  fields to name the class in `results.md` before touching the rule
  set — never loosen a bound to pass.

## Files

- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/`
- `crates/grus-core/src/lib.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/`
- `Cargo.toml`
