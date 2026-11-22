# e7-drc-geom: DRC geometry model + width-rule family

E7 ticket 2 (first implementation ticket; `e7-drc-ref` already landed the
reference surfaces and taxonomy). Build the geometry substrate a LEF+DEF
DRC subset checker runs on, and land the first rule family — **width** —
verified against the committed Magic goldens.

## Scope

1. **Signal-net routed geometry.** `crates/grus-core/src/readers/def.rs`
   currently keeps only connectivity from the `NETS` section; `PdnNet`
   retains routed shapes only for `SPECIALNETS`. Parse each regular net's
   `ROUTED`/`FIXED`/`COVER`/`PLACED` wire segments (two-point, per-segment
   width in DBU, `TAPER`/`EXT` handling as the grammar requires) and
   `NEW <viaName>` instances into `DesignDb` (e.g. `RoutedShape`/`NetGeom`
   records — wire half-width-expanded rects + via instances, mirroring
   `PdnNet`'s proven model). Vias resolve through
   `TechDb::via_defs` exactly like `pdn.rs` — no name heuristics.
2. **Layer shape extraction.** New `drc` module assembles every shape
   Magic sees on each routing/cut layer: signal wire rects, special-net
   wire rects, `NEW`-style via rects (cut rect + each layer rect the LEF
   `VIA` defines, placed at the via point), and placed component
   `pin_obs_rects`/`obs_rects` transformed by the component's placement
   and orientation (the same transform `pin_pos_dbu` uses).
3. **Spatial index** per layer (the sorted-sweep structure `pdn.rs`
   already uses is fine to reuse or mirror).
4. **Width family.** Implement the `lef_expressible` width rules as
   per-rule checkers emitting violation rects with Magic-shaped rule
   names: `li.1`, `li.c1`, `li.7`, `mcon.1`, `met1.1`–`met5.1`,
   `via.1a`, `via2.1a`, `via3.1`, `via4.1` (id → threshold taken from the
   LEF layer `WIDTH`/via cut size — record in `docs/e7-drc.md` that the
   bound is LEF, not the Magic techfile constant).
5. **Rule-name parity.** Rule ids/names must match the golden
   `magic_rules_checked.classification` strings so results can join
   `magic_per_rule` 1:1 (emit Magic's `Metal1 width < %d (met1.1)`-style
   template or the extracted `(id)` — pick one and document it).

## Non-goals

- Spacing/enclosure/area/hole rules — later tickets.
- No CLI yet — `grus-eda drc` lands at `e7-drc-e2e`.
- The `magic_only` rules are out of scope by the recorded taxonomy.

## Files

- `crates/grus-core/src/readers/def.rs` (NETS routed-geometry parse)
- `crates/grus-core/src/design_db.rs` (routed-geometry storage)
- `crates/grus-core/src/drc.rs` + `crates/grus-core/src/drc/geom.rs`
  (layer shapes, spatial index, rule framework)
- `crates/grus-core/src/drc/width.rs` (width family)
- `tests/designs/` fixture DEFs only if needed for orientation coverage
- `docs/e7-drc.md` (bound provenance note + family status table)
- `docs/results.md` (dated comparison entry)
- `docs/status.md`, `docs/roadmap.md` (E7 progress line)
- `Cargo.toml`/`Cargo.lock` (version bump per surface)

## Conventions already pinned

- `magic_per_rule` on both designs carries only `nwell.*` counts — every
  `lef_expressible` rule is expected **zero** on both routed DEFs, so the
  honest gate is "0 violations everywhere" **plus** a mutation fixture
  that proves detection (a too-narrow wire must flag the right rule id).
- Wire `width == 0` PDN-style stub segments keep the documented
  treatment; per-segment min-dimension is the width-check semantics —
  record in the doc that Magic's merged-polygon edge-pair check can
  differ at junctions, and assert 0 anyway on the real designs (routers
  emit min-width-or-wider shapes).
