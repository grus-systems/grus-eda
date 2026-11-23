# e7-drc-spacing — LEF `SPACING`/`SPACINGTABLE` retention

`parse_layer` (`readers/lef.rs`) currently drops every spacing
statement via the `else → skip_to_semi` arm. The sky130 techlef
(`sky130_fd_sc_hd__nom.tlef`) carries the rules this family needs in
two forms:

```
LAYER mcon
  TYPE CUT ;
  WIDTH 0.17 ;                # Mcon 1
  SPACING 0.19 ;              # Mcon 2      <- plain scalar
END mcon

LAYER met2
  TYPE ROUTING ;
  WIDTH 0.14 ;                        # Met2 1
  # SPACING  0.14 ;                   # Met2 2   <- commented out
  # SPACING  0.28 RANGE 3.001 100 ;   # Met2 3b  <- commented out
  SPACINGTABLE
     PARALLELRUNLENGTH 0
     WIDTH 0 0.14
     WIDTH 3 0.28 ;                    <- the live rule
END met2
```

Observed per layer (`grep` of the techlef, 2026-11-23):

| layer | form | content | base spacing (DBU) | wide row |
|---|---|---|---|---|
| li1 | TABLE | `PRL 0` / `WIDTH 0 0.17` | 170 | — |
| mcon | plain | `SPACING 0.19` | 190 | — |
| via | plain | `SPACING 0.17` | 170 | — |
| via2 | plain | `SPACING 0.2` | 200 | — |
| via3 | plain | `SPACING 0.2` | 200 | — |
| via4 | plain | `SPACING 0.8` | 800 | — |
| met1 | TABLE | `WIDTH 0 0.14 / WIDTH 3 0.28` | 140 | (>3.0µm → 280) |
| met2 | TABLE | `WIDTH 0 0.14 / WIDTH 3 0.28` | 140 | (>3.0µm → 280) |
| met3 | TABLE | `WIDTH 0 0.3 / WIDTH 3 0.4` | 300 | (>3.0µm → 400) |
| met4 | TABLE | `WIDTH 0 0.3 / WIDTH 3 0.4` | 300 | (>3.0µm → 400) |
| met5 | TABLE | `WIDTH 0 1.6` | 1600 | — |

## Grammar parsed

Inside `parse_layer`'s statement loop:

- `SPACING <f> ;` — plain scalar. Other `SPACING` forms
  (`... RANGE lo hi`, `... x BY y`, `SAMENET`, `CENTERTOCENTER`,
  stacked `SPACING` variants) do not occur inside sky130 `LAYER`
  blocks; if a numeric first word cannot be read, fall back to
  `skip_to_semi` and record `spacing_um` unset — never bail on a
  LEF this file class already accepts.
- `SPACINGTABLE` — consumes until `;`. Expect `PARALLELRUNLENGTH
  <f>...` then one or more `WIDTH <w> <s>` pairs. Retain all WIDTH
  rows; retain the PRL values only to reject the empty/!`0` case
  loudly? — No: keep it permissive, parse whatever `WIDTH` pairs
  appear. PRL is recorded nowhere in `TechDb` (see divergence note).

`SPACING` statements inside `VIARULE`/`NONDEFAULTRULE` blocks are
unreachable — those blocks are wholesale-skipped before this code
runs (`VIA`-rule-level spacing is out of scope, recorded).

## `TechDb` model

```rust
/// One `SPACINGTABLE` `WIDTH w s` row: shapes wider than `width_dbu`
/// require `spacing_dbu` edge clearance (strict `>` — the commented
/// RANGE form reads 3.001µm, i.e. >3000 DBU).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpacingTableRow {
    pub width_dbu: i32,
    pub spacing_dbu: i32,
}

// RoutingLayer gains:
/// Plain `SPACING <v>` in DBU; 0 when the layer is table-driven.
#[serde(default)] pub spacing_dbu: i32,
/// `SPACINGTABLE` `WIDTH` rows in file order (ascending threshold in
/// sky130; helpers do not rely on order).
#[serde(default)] pub spacing_table: Vec<SpacingTableRow>,

// CutLayer gains:
#[serde(default)] pub spacing_dbu: i32,
```

Helpers on `RoutingLayer`:

```rust
/// Minimum same-layer spacing: plain SPACING when present, else the
/// smallest-width-threshold table row's spacing. 0 = no LEF spacing
/// data (rule skipped, mirroring `width_dbu == 0`).
pub fn min_spacing_dbu(&self) -> i32;

/// The wide-metal row — the `WIDTH` row with the largest threshold —
/// as `(width_threshold_dbu, spacing_dbu)`. `None` when the table has
/// a single row or is absent.
pub fn wide_spacing(&self) -> Option<(i32, i32)>;
```

For sky130, `min_spacing_dbu` returns the `WIDTH 0` row's value on
table layers and `spacing_dbu` on plain layers; `wide_spacing`
returns `Some((3000, 280))` on met1/met2, `Some((3000, 400))` on
met3/met4, `None` elsewhere.

## Divergence record — PRL and trigger reading

- **PRL column ignored.** `PARALLELRUNLENGTH` rows carry a
  run-length dimension we do not model; spacing is selected by width
  alone. sky130 uses a single `0` (vacuous); on a LEF with real PRL
  thresholds the lookup over-requires (conservative — can only add
  findings, never hide one).
- **Trigger `>`, not `>=`.** `min(w,h) > width_threshold_dbu`
  qualifies — the commented `RANGE 3.001 100` form confirms the open
  lower bound; in integer DBU `> 3000` ⟺ `>= 3001`. Magic's
  `widespacing` statement uses 3005 — the docs table shows both.
- **`li1` plain-vs-table ambiguity does not arise**: li1 ships only
  the table. If a LEF carried both, plain `SPACING` wins for
  `min_spacing_dbu` (LEF precedence rule) — pinned by helper order.

## Magic `%d` vs LEF bound — the spacing table for `docs/e7-drc.md`

Values re-derived from `sky130A.tech` `drc` section lines ~4560–4760
(`spacing`/`widespacing`/`cifspacing` statements, units nm):

| `rule` | layer | LEF bound (DBU) | `rule_name` (verbatim) | Magic `%d` | divergence |
|---|---|---|---|---|---|
| `li.3` | li1 | 170 | `Local interconnect spacing < %d (li.3)` | 170 | — |
| `li.c2` | li1 | 170 | `Core local interconnect spacing < %d (li.c2)` | 140 | Magic's core-li bound is *looser*; we check LEF — same collapse class as `li.c1` |
| `mcon.2` | mcon | 190 | `mcon.spacing < %d (mcon.2)` | 190 | — |
| `met1.2` | met1 | 140 | `Metal1 spacing < %d (met1.2)` | 140 | — |
| `met2.2` | met2 | 140 | `Metal2 spacing < %d (met2.2)` | 140 | — |
| `met3.2` | met3 | 300 | `Metal3 spacing < %d (met3.2)` | 300 | — |
| `met4.2` | met4 | 300 | `Metal4 spacing < %d (met4.2)` | 300 | — |
| `met5.2` | met5 | 1600 | `Metal5 spacing < %d (met5.2)` | 1600 | — |
| `via.2` | via | 170 | `Via1 spacing < %d (via.2 - 2 * via.4a)` | 60 | Magic checks the fabricated cut plane (60); we check the router-level LEF `SPACING` — tighter, recorded |
| `via2.2` | via2 | 200 | `via2 spacing < %d (via2.2 - 2 * via2.4)` | 120 | same |
| `via3.2` | via3 | 200 | `via3 spacing < %d (via3.2 - 2 * via3.4)` | 80 | same |
| `via4.2` | via4 | 800 | `via4 spacing < %d (via4.2 - 2 * via4.4)` | 420 | same |
| `met1.3a` | met1 | 280 | `Spacing of metal1 features attached to and within 0.28um of large metal1 < %d (met1.3a)` | 280 | bound equal; our trigger `>3000` vs Magic `widespacing` 3005 |
| `met1.3b` | met1 | 280 | `Metal1 > 3um spacing to unrelated m1 < %d (met1.3b)` | 280 | same |
| `met2.3a` | met2 | 280 | `Spacing of metal2 features attached to and within 0.28um of large metal2 < %d (met2.3a)` | 280 | same |
| `met2.3b` | met2 | 280 | `Metal2 > 3um spacing to unrelated m2 < %d (met2.3b)` | 280 | same |
| `met3.3c` | met3 | 400 | `Spacing of metal3 features attached to and within 0.40um of large metal3 < %d (met3.3c)` | 400 | same |
| `met3.3d` | met3 | 400 | `Metal3 > 3um spacing to unrelated m3 < %d (met3.3d)` | 400 | same |
| `met4.5a` | met4 | 400 | `Spacing of metal4 features attached to and within 0.40um of large metal4 < %d (met4.5a)` | 400 | same |
| `met4.5b` | met4 | 400 | `Metal4 > 3um spacing to unrelated m4 < %d (met4.5b)` | 400 | same |

The doc-writing step re-derives Magic `%d` from `sky130A.tech`, not
from this table (per the e7-drc-geom docs.md constraint).

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `docs/e7-drc.md`
