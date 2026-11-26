# e7-drc-geom — Overview

E7 ticket 2, first implementation ticket. `e7-drc-ref` landed the
reference surfaces (`generate_drc_ref.sh`, `*.drc.reference.json`,
`docs/e7-drc.md` taxonomy). This ticket builds the geometry substrate a
LEF+DEF DRC subset checker runs on, and lands the first rule family —
**width** — verified against the committed Magic goldens.

## What ships

1. **Signal-net routed geometry in `DesignDb`.** The `NETS` parser keeps
   only connectivity today (`def.rs` drops everything after `+` in the
   net record). It gains the same routed-shape retention `SPECIALNETS`
   already has: half-width-expanded wire rects + via instances, stored
   per net in a new `net_geoms` field.
2. **Via geometry end to end.** `ViaDef` currently keeps only
   `(lower, upper, cut, cut_count)`; LEF `VIA ... RECT` coordinates are
   validated then discarded, and DEF `VIAS` params (`CUTSIZE`,
   `CUTSPACING`, `ENCLOSURE`, `ROWCOL`) are number-validated then
   dropped. Both are retained so every via instance can emit its cut and
   metal layer rects.
3. **`drc` module.** `drc::geom` assembles every shape Magic sees on
   each routing/cut layer — signal wires, special wires, via-derived
   rects, placed macro pin/OBS rects, io-port rects — into a per-layer
   spatial index. `drc::width` implements the 13 width rules.
4. **Verification.** Zero-violation gates on `gcd.routed.def` and
   `picorv32.routed.def` joined to the goldens by rule name; a
   below-min-width mutation fixture; orientation and all-source
   synthetic fixtures; `docs/results.md` + `docs/e7-drc.md` updates.

## Key decisions

- **`NETS` parse mirrors `SPECIALNETS`, not a new grammar.** The section
  loop is refactored to `for_each_record` + `add_net` so the
  word-collected record model and `WireSink`/`resolve_def_via` helpers
  are shared. See `def-grammar.md`.
- **One via record type.** `PdnVia` (misleading name, kept to avoid a
  cross-crate rename) becomes the shared DEF via-instance record and
  gains `geom: ViaGeom` — `Lef(idx)` verbatim LEF rects or
  `Generated(params)` from the DEF `VIAS` record. `resolve_def_via`
  stays the single resolution function — the SPECIALNETS records
  `pdn.rs` consumes are built there, and `NETS` now goes through it
  too. See `model.md`.
- **Layer-addressed shape store.** `DrcDesign` holds `routing:
  Vec<LayerShapes>` indexed by `LayerId` and `cut: Vec<LayerShapes>`
  indexed by `cut_layers` index. Cut-layer shapes exist for the via
  width rules and the future spacing family. See `geom.md`.
- **Bound provenance is LEF, names are Magic.** Width thresholds come
  from the LEF layer `WIDTH` / via cut size, never from techfile
  constants. Rule `rule_name` strings are copied verbatim from the
  golden `magic_rules_checked.names` so tests join by string equality;
  the `(id)` inside the parens is the `magic_per_rule` join key. Where
  the LEF bound and the Magic techfile `%d` diverge (`li.c1`, `li.7`),
  the divergence is recorded in `docs/e7-drc.md`. See `width.md`.
- **Zero is scoped to implemented rules.** `magic_total` is 305/8049 —
  Magic-only cell-master nwell violations outside LEF+DEF scope. The
  gate is "0 violations on every implemented width rule", never a global
  zero claim.

## Baseline semantics (record, don't rediscover)

- gcd: `magic_total` 305, all merged `nwell.*` cell-master violations;
  DRT count 0. picorv32: `magic_total` 8049, same class.
- `drc listall why` bbox fragments ≠ `drc count total` merged regions —
  our violation list counts rects, not merged Magic regions; the join is
  on rule names and the zero/nonzero verdict, not fragment counts.
- Width-rule statement names with Magic's observed `%d` constants are in
  `width.md`; the LEF-bound column is what we compute.

## Non-goals (from the spec)

- Spacing/enclosure/area/hole rules — later tickets (the spatial index
  and `ShapeRect` source tagging exist to host them).
- No `grus-eda drc` CLI — `e7-drc-e2e`.
- `magic_only` rules — out of scope per the recorded taxonomy.
- No `NETS` connectivity changes: conn groups, driver flags,
  `net_def_spellings`, and `create_net` keep current behavior.

## Doc map

| Doc | Content |
|---|---|
| `def-grammar.md` | `NETS` routed-statement grammar as exercised by the committed DEFs, refactor plan, error behavior |
| `model.md` | `ViaDef`/`CutLayer`/`ViaGeom`/`PdnVia`/`NetGeom`/`net_geoms` data model and invariants |
| `geom.md` | `drc::geom` — sources, via expansion arithmetic, orientation, spatial index |
| `width.md` | Rule table (LEF bound vs Magic `%d`), `Violation`, name-parity join |
| `tests.md` | Unit + integration test inventory, fixtures, mutation + fail-proof |
| `docs.md` | `results.md` entry shape, `e7-drc.md` additions, status/roadmap, version bump |
| `steps.md` | Ordered implementation steps, commit split, done-when map |

## Files

- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/tech_db.rs`
- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/`
- `crates/grus-core/src/lib.rs`
- `crates/grus-core/tests/drc.rs`
- `docs/e7-drc.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
