# e6-drt-via — Via candidate model

## 1. Sources and the triple match

Two sources, both already resolved to `(lower, cut, upper)`:

- **LEF `VIA`** — `tech.via_defs` (`lef.rs` `parse_via`): `lower`/
  `upper` are the min/max routing `LayerId` of the block's routing
  `LAYER` groups, `cut` the first cut-layer group name, `cut_count`
  the total cut `RECT` count, `rects` every `RECT` in group order.
- **DEF `VIAS`** — `db.def_vias` (new retention, `api.md`): each
  record resolved at parse to the same triple plus a `ViaGeom`
  (`Generated` params or `Lef(idx)` for `+ VIA <name>` aliases).
  Inline `+ RECT`/`+ POLYGON` records never resolve and store
  nothing.

A def is a candidate for pair `(k, k+1)` iff `lower.0 == k &&
upper.0 == k+1` — an adjacent routing-layer pair in LEF stack order.
A def whose routing span is wider (`upper - lower > 1`, a multi-cut
stack) or degenerate lands in `ViaTable::unmatched` with name and
span — named, not silently dropped (empty on sky130: all 25 LEF defs
and all 4 DEF records are adjacent single-cut stacks).

The pair's own `cut` label is the `cut_layers` entry with
`above.0 == k` (mcon→li1, via→met1, via2→met2, via3→met3, via4→met4;
`licon` sits below every routing layer and is never a pair cut).
Candidates on one pair can in principle name different cut layers —
each candidate records its own `cut`; the pair label is display-only.

## 2. Rect expansion

Every candidate carries `rects` — the def's painted shapes at the
origin — computed by constructing a `PdnVia` at `(0,0)` and calling
`drc::geom::expand_via`:

- `ViaGeom::Lef(i)` → `via_defs[i].rects` verbatim (`ViaRectLayer` →
  `DrcLayer`).
- `ViaGeom::Generated(g)` → the OpenDB enclosure/cut-array expansion
  (`aw/ah` from `+ CUTSIZE`/`+ ROWCOL`/`+ CUTSPACING`, symmetric
  `+ ENCLOSURE` metal overhangs), already golden-verified against the
  committed `via2_3_1600_480_1_5_320_320` record
  (`geom.rs` test).

`expand_via` returns `Result`; `via_table` propagates it (`Err` means
the retained store is corrupt — a `ViaGeom::Lef` index or `Generated`
cut the parse had already validated). `area_dbu2` is the sum over all
expanded rects (metals + cuts; within a family the cut is constant so
the ranking reads the same either way — total painted area is the
honest number).

## 3. Default pick per pair — the policy

`pairs[k].default` = argmin over `pairs[k].candidates` of
`(nonpref_metal, area_dbu2, order)`:

1. **`nonpref_metal`** — count of routing-layer rects whose long axis
   is *not* along that layer's preferred direction: a rect on a
   `Horizontal` layer with `height > width`, or on a `Vertical` layer
   with `width > height`, counts one; squares are neutral. This is
   what the techlef's `PR`/`PR_R`/`PR_M`/`PR_MR` naming means
   ("plus/minus" = which metals ride the preferred direction).
2. **`area_dbu2`** — smallest total painted area. Discards `_C`
   ("we really do not want to use it" — strictly larger on every
   family).
3. **`order`** — declaration order (LEF file order, then `def_vias`
   record order): deterministic, no name parsing.

This is the spec's "smallest-area or reference-majority" rule made
*computable*: pure area ties 4-ways inside every sky130 family, so
the preferred-direction count is the principled discriminator the
deck's own naming encodes. Result on sky130:

| pair | candidates | default | reference majority (both designs) | verdict |
|---|---|---|---|---|
| li1–met1 | 5 | `L1M1_PR` | `L1M1_PR_MR` (884 / 26,365) | **name-only divergence** — the two defs are rect-for-rect identical (gate asserts it), physically equivalent |
| met1–met2 | 6 (5 LEF + `via2_3_*`) | `M1M2_PR` | `M1M2_PR` (881 / 31,606) | matched |
| met2–met3 | 6 | `M2M3_PR` | `M2M3_PR` (28 / 1,784) | matched |
| met3–met4 | 6 | `M3M4_PR` | `M3M4_PR` (2 / 90) | matched |
| met4–met5 | 6 | `M4M5_PR` | unobserved (0 M4M5 in either routed DEF) | policy pick; no reference evidence |

A tie-breaking alternative — hardcoding `*_MR` on mcon — would be
fitting to the reference with no geometric basis; the honest name
divergence is documented instead (§7.5-style: the gate asserts both
defs' rect lists are equal, so any future deck change that
differentiates them trips the test, not silently).

DEF `+ VIARULE` generated vias score poorly by design: e.g.
`via2_3_1600_480` paints a 1×5 cut array under 1600×480 metals —
`nonpref_metal = 1` (the met2 rect is wider than tall) plus a huge
area — and the routed-DEF census shows the reference never picks
them for signal nets either (they are the PDN vias; signal routing
uses LEF names only — `vias_per_cut_layer` holds no `def_viarule:`
entries on either design).

## 4. Findings

- `gaps` — adjacent pairs with zero candidates. Acceptance requires
  ≥1 per sky130 pair; the gate asserts `gaps.is_empty()` and any
  entry prints as a named tech gap (`(lower_name, upper_name)`).
- `unmatched` — defs that fit no adjacent pair (name + span).
  Empty on sky130; the list exists so a future stacked via is
  *dropped loudly* rather than silently.
- Per-pair census print (test): `lower–upper  cut  candidates
  via_edges  default  ref_majority  ref_used` — the join to
  `vias_per_cut_layer` is evidence display + the pinned assertions
  from the table above.

## Files

- `crates/grus-core/src/drt/via.rs`
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/readers/def.rs`
