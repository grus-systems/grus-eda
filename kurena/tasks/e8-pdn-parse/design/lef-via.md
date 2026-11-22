# e8-pdn-parse — LEF `VIA` parsing

`readers/lef.rs` currently lumps `VIA` and `VIARULE` into the generic
skip (`Token::Word("VIA" | "VIARULE" | "NONDEFAULTRULE") =>
end_block` + skip-report entry). This ticket parses `VIA` blocks;
`VIARULE`/`NONDEFAULTRULE` stay skipped (their layer triples reach us
through DEF `+ LAYERS` anyway — `def.md`).

## Grammar accepted

```
VIA <name> [DEFAULT]
  LAYER <layerName> ;
  RECT x1 y1 x2 y2 ;          # one per line
  …
  LAYER <layerName> ;
  RECT … ;
  TOPOFSTACKONLY | FOREIGN <…> | PATTERN <…> | RESISTANCE <r> | PROPERTY …
END [name]
```

Observed in `sky130_fd_sc_hd__nom.tlef` (25 blocks): each is three
`LAYER`/`RECT` groups in order **cut, bottom, top**:

```
VIA L1M1_PR DEFAULT
    LAYER mcon ;
    RECT -0.032 -0.032 0.032 0.032 ;
    LAYER li1 ;
    RECT -0.065 -0.085 0.065 0.085 ;
    LAYER met1 ;
    RECT -0.085 -0.08 0.085 0.08 ;
END L1M1_PR
```

## Parse algorithm

- `VIA` enters `parse_via` instead of `skip_block` + `count_skip`;
  the statement still counts as consumed (the skip report loses its
  `VIA` entry entirely). `VIARULE`/`NONDEFAULTRULE` keep the existing
  skip arm.
- Loop tokens to `END` (optional trailing name ignored, same as the
  file's other `END` handling):
  - `LAYER <w>` opens a group `{layer_sym, rects: []}`; `<w>` must
    resolve through `tech_db.layer_by_name` **or**
    `tech_db.cut_layer_by_name` — neither → `bail!` (typo or
    unsupported masterslice layer).
  - `RECT x1 y1 x2 y2` appends to the open group (scaled by
    `units.dbu_per_micron`); `RECT` before any `LAYER` → `bail!`.
  - `TOPOFSTACKONLY`, `FOREIGN`, `PATTERN`, `RESISTANCE`, `PROPERTY`,
    `MASK`/`VENDORMODEL`/`VIARULE`-style extras → `skip_to_semi`.
  - `DEFAULT` after the via name → consumed, no effect.
- Group classification: routing groups (`LayerId`) vs cut groups
  (index into `cut_layers`). A `VIA` must have ≥ 2 routing groups and
  ≥ 1 cut group; fewer → `bail!` (a `VIA` we cannot map is worse than
  no `VIA`). A `VIA` with **zero** groups (empty block, e.g. the
  skip-report fixture's `VIA via_a END via_a`) is tolerated and
  records nothing.
- `lower` = the routing layer with the smallest `LayerId`; `upper` the
  largest; `cut` = first cut group's name; `cut_count` = total `RECT`
  count across cut groups (multi-cut vias place one `RECT` per cut).
- Emitted into `tech_db.via_defs` + `tech_db.via_by_name[name]`
  (parse-local insert, rebuilt in `rebuild_maps` — same pattern as
  `layer_by_name`). A second `VIA` redefining a name → `bail!`.

## `TechDb` schema additions

```rust
pub struct CutLayer {
    pub name: SymbolId,
    /// Routing layer directly below this cut — indexes `via_res_ohm_above`.
    pub above: LayerId,
}

pub struct ViaDef {
    pub name: SymbolId,
    pub lower: LayerId,
    pub upper: LayerId,
    /// Cut-layer name; resistance via `cut_layer_by_name → cut_layers[..].above`.
    pub cut: SymbolId,
    pub cut_count: u32,
}
```

New fields on `TechDb` (all `#[serde(default)]` — `TechDb` is never
persisted today, but the pattern is free and matches `layers`):

- `cut_layers: Vec<CutLayer>`
- `cut_layer_by_name: HashMap<SymbolId, u32>` — `#[serde(skip)]`,
  maintained during `parse_layer` and rebuilt in `rebuild_maps`.
- `via_defs: Vec<ViaDef>`
- `via_by_name: HashMap<SymbolId, u32>` — `#[serde(skip)]`, same
  lifecycle.

`parse_layer` learns `is_cut`: `TYPE CUT` records push `CutLayer
{name, above: last_routing}` (existing `is_cut` local) instead of only
tracking `cut_res_above`; `cut_res_above` keeps its role of collecting
the cut's `RESISTANCE RPERSQ`. The routing-layer counter must not
advance on CUT layers (today's `next` index already handles this).

Resistance for a `PdnVia`: `via_res_ohm_above[cut_layers[cut_idx]
.above.0] / cut_count` — per-instance parallel-cut division. The
existing vec stores only the *first* cut above each routing layer;
that is unchanged, and is the resistance `e8-mesh-rc` will consume.

## Correction vs. `e8-mesh-rc`'s spec line

`e8-mesh-rc` says "via cut counts come from the LEF VIA definition's
cut-layer RECT count". For DEF-generated vias the cut count lives in
`+ ROWCOL` (or defaults 1) — the LEF `VIA` rule named by `+ VIARULE`
carries no per-instance count. This ticket normalizes both into
`PdnVia::cut_count` at parse; `e8-mesh-rc` reads only that field.

## Why this exists even though gcd's vias are DEF-defined

The acceptance requires a renamed via to still resolve — the binding
must come from parsed layer triples, never from parsing the via name.
LEF `VIA` parsing covers the fixed-via path (`+ VIA <name>` records
and LEF-only via libraries) and gives `TechDb` a `via_by_name` table
`e8-mesh-rc` needs for resistance lookup. A mini-DEF test exercises
both resolution orders (`tests.md`).

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
