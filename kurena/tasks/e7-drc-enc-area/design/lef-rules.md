# e7-drc-enc-area — LEF `AREA` / `MINENCLOSEDAREA` / `ENCLOSURE` retention

`parse_layer` (`readers/lef.rs`) drops all three statements this
ticket needs via the `else → skip_to_semi` arm. The sky130 techlef
(`sky130_fd_sc_hd__nom.tlef`) carries them on `TYPE ROUTING` and
`TYPE CUT` layers:

```
LAYER mcon
  TYPE CUT ;
  WIDTH 0.17 ;                # Mcon 1
  ENCLOSURE BELOW 0 0 ;       # Mcon 4      <- li1 side, zero overhang
  ENCLOSURE ABOVE 0.03 0.06 ; # Met1 4 / Met1 5
END mcon
LAYER met1
  TYPE ROUTING ;
  WIDTH 0.14 ;                        # Met1 1
  AREA 0.083 ;                        # Met1 6
  MINENCLOSEDAREA 0.14 ;
END met1
LAYER via
  TYPE CUT ;
  ENCLOSURE BELOW 0.055 0.085 ; # Via 4a / Via 5a
  ENCLOSURE ABOVE 0.055 0.085 ; # Met2 4 / Met2 5
END via
```

The in-file `# Met1 4 / Met1 5` comments pin each overhang pair to the
sky130 rule ids the Magic deck names — provenance for the bound table
below.

## Grammar parsed

Inside `parse_layer`'s statement loop, three new arms:

- `AREA <f> ;` — minimum polygon area, µm². Recorded on routing
  layers only (`area_dbu2`); on other layer types skip the value.
- `MINENCLOSEDAREA <f> ;` — minimum hole area, µm². Routing layers
  only (`min_enclosed_area_dbu2`).
- `ENCLOSURE {BELOW|ABOVE} <f> <f> ;` — cut-layer overhang pair.
  Recorded on cut layers only (`enc_below_dbu`/`enc_above_dbu`); a
  non-`BELOW`/`ABOVE` first word falls back to `skip_to_semi`.

`ENCLOSURE` also appears inside `VIARULE ... GENERATE` sub-blocks —
those blocks are wholesale-skipped at top level (`count_skip` +
`skip_block`) and never reach `parse_layer`, so the two namespaces
cannot collide. `AREA` inside `MACRO` blocks is unreachable for the
same reason. Watch item: `ANTENNA*` keywords all share the `AREA`
substring only inside longer words — match on the exact token.

## `TechDb` model

```rust
// RoutingLayer gains:
/// LEF `AREA` — minimum merged-polygon area, DBU². 0 = undeclared.
#[serde(default)] pub area_dbu2: i64,
/// LEF `MINENCLOSEDAREA` — minimum hole area, DBU². 0 = undeclared.
#[serde(default)] pub min_enclosed_area_dbu2: i64,

// CutLayer gains:
/// LEF `ENCLOSURE BELOW o1 o2` — overhang pair the routing layer
/// below the cut must provide. [0,0] = undeclared / zero.
#[serde(default)] pub enc_below_dbu: [i32; 2],
/// LEF `ENCLOSURE ABOVE o1 o2` — same for the routing layer above.
#[serde(default)] pub enc_above_dbu: [i32; 2],
```

Conversion: µm → DBU via `(v * dbu).round()` (existing convention);
µm² → DBU² via `(v * dbu * dbu).round()` into `i64` (areas grow past
i32 on met5 polygons, so the area fields are `i64` even though every
bound here fits).

`side` resolution for checks: a cut layer's `above: LayerId` is the
routing layer directly *below* it in stack order — `ENCLOSURE BELOW`
applies to `routing_layers[cut.above]`, `ENCLOSURE ABOVE` to the next
routing layer. Rules resolve the metal plane by name instead (see
`enclosure.md`); a unit fixture asserts the name/side pairing matches
the stack arithmetic on the real techlef data loaded by `load()`.

## Overhang-pair reading

`ENCLOSURE o1 o2` is checked as `min(o1,o2)` on both axes **and**
`max(o1,o2)` on at least one axis — this is what the deck's own
statements encode (`mcon 0.03 0.06` → `surround 30 absence_illegal` +
`surround 60 directional`) and what OpenDB's generated-via expansion
produces (the fat axis gets the large overhang). A per-axis pinning
(o1 → x, o2 → y) is *not* used: the deck proves the symmetric reading
and a directional cut would falsely flag a via rotated the other way.

## Bound table — all 17 rules (for `docs/e7-drc.md`)

LEF values from `sky130_fd_sc_hd__nom.tlef` (DBU / DBU²); Magic `%d`
/`%a` re-derived from `sky130A.tech` `drc` section lines ~4600–4760
(`surround`/`area`/`cifmaxwidth` statements, nm / nm²). The doc-writing
step re-derives Magic values from the deck, not from this table.

### Enclosure (9 names)

| `rule` | cut | side → metal | LEF overhangs (DBU) | `rule_name` (verbatim) | Magic stmt | divergence |
|---|---|---|---|---|---|---|
| `met1.4` | mcon | ABOVE → met1 | 30 / 60 → both-dir ≥ 30 | `Metal1 overlap of local interconnect contact < %d (met1.4)` | `surround mcon/m1 *met1 30 absence_illegal` | — |
| `met1.5` | mcon | ABOVE → met1 | → one-dir ≥ 60 | `Metal1 overlap of local interconnect contact < %d in one direction (met1.5)` | `surround mcon/m1 *met1 60 directional` | — |
| `via.5a` | via | BELOW → met1 | 55 / 85 → both ≥ 55 ∧ one ≥ 85 | `Metal1 overlap of Via1 < %d in one direction (via.5a - via.4a)` | `surround v1/m1 *m1,rm1 30 directional` | Magic checks ≥30 in one direction only; we check the LEF pair — tighter, recorded |
| `met2.5` | via | ABOVE → met2 | 55 / 85 → both ≥ 55 ∧ one ≥ 85 | `Metal2 overlap of Via1 < %d in one direction (met2.5 - met2.4)` | `surround v1/m2 *m2,rm2 30 directional` | same |
| `via2.4a` | via2 | BELOW → met2 | 40 / 85 → both ≥ 40 ∧ one ≥ 85 | `Metal2 overlap of via2 < %d in one direction (via2.4a - via2.4)` | `surround v2/m2 *m2,rm2 45 directional` | Magic 45 vs LEF 40/85 pair |
| `met3.4` | via2 | ABOVE → met3 | 65 / 65 → both ≥ 65 | `Metal3 overlap of via2 < %d (met3.4)` | `surround v2/m3 *m3,rm3 25 absence_illegal` | Magic 25 vs LEF 65 |
| `via3.5` | via3 | BELOW → met3 | 60 / 90 → both ≥ 60 ∧ one ≥ 90 | `Metal3 overlap of via3 in one direction < %d (via3.5 - via3.4)` | `surround v3/m3 *m3,rm3 30 directional` | Magic 30 vs LEF 60/90 |
| `met4.3` | via3 | ABOVE → met4 | 65 / 65 → both ≥ 65 | `Metal4 overlap of via3 < %d (met4.3 - via3.4)` | `surround v3/m4 *m4,rm4 5 absence_illegal` | Magic 5 vs LEF 65 |
| `met5.3` | via4 | ABOVE → met5 | 310 / 310 → both ≥ 310 | `Metal5 overlap of via4 < %d (met5.3 - via4.4)` | `surround v4/m5 *m5,rm5 120 absence_illegal` | Magic 120 vs LEF 310 |

Sides without a deck statement (not implemented — recorded):

- `mcon` BELOW → li1: LEF `0 0`, no rule.
- `via4` BELOW → met4 (`via4.4` = 190): no `surround v4/m4` statement;
  Magic folds the overhang into `via4.1` width (`1180 = 800 + 2·190`),
  which the width family already checks at LEF `WIDTH 800`. Same
  folding for `via.1a` (150+2·55=260), `via2.1a` (200+2·40=280),
  `via3.1` (200+2·60=320).

### Min-area (6 names)

| `rule` | layer | LEF `AREA` (DBU²) | `rule_name` (verbatim) | Magic `%a` | divergence |
|---|---|---|---|---|---|
| `li.6` | li1 | 56100 | `Local interconnect minimum area < %a (li.6)` | 56100 | — |
| `met1.6` | met1 | 83000 | `Metal1 minimum area < %a (met1.6)` | 83000 | — |
| `met2.6` | met2 | 67600 | `Metal2 minimum area < %a (met2.6)` | 67600 | — |
| `met3.6` | met3 | 240000 | `Metal3 minimum area < %a (met3.6)` | 240000 | — |
| `met4.4a` | met4 | 240000 | `Metal4 minimum area < %a (met4.4a)` | 240000 | — |
| `met5.4` | met5 | 4000000 | `Metal5 minimum area < %a (met5.4)` | 4000000 | — |

`li.6` appears twice in `names` (`allli,*obsli` and the `coreli`
variant) — the `names.contains` join is unaffected; one rule row,
same collapse class as the `li.c*` width/spacing rules.

### Hole area (2 names)

| `rule` | layer | LEF `MINENCLOSEDAREA` (DBU²) | `rule_name` (verbatim) | Magic | divergence |
|---|---|---|---|---|---|
| `met1.7` | met1 | 140000 | `Min area of metal1 holes > 0.14um^2 (met1.7)` | `cifmaxwidth m1_hole_empty 0` on a derived 0.14 µm² hole plane — the name pins the bound | — |
| `met2.7` | met2 | 140000 | `Min area of metal2 holes > 0.14um^2 (met2.7)` | same via `m2_hole_empty` | — |

Bound resolution: `min_enclosed_area_dbu2` when the LEF declares it;
both hole-rule layers declare 0.14 in this PDK. Where a LEF omits it,
fall back to the deck-pinned `140000` — a named constant in
`drc/area.rs` with a comment, not a guessed value (the `rule_name`
literal itself carries "0.14um^2"). The name reads `>`, so an
exact-bound hole passes: flag iff `hole_area < bound` (strict).

## Files

- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/tech_db.rs`
- `docs/e7-drc.md`
