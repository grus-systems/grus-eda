# E7: DRC — Reference Surfaces and Rule Taxonomy

E7 ticket 1 (`e7-drc-ref`): committed, script-generated DRC reference
surfaces on the routed DEFs of `gcd` and `picorv32`, produced by
`tests/designs/generate_drc_ref.sh` inside the pinned image
(`ghcr.io/librelane/librelane:3.0.16`). This ticket adds **no Rust
functionality and no version bump** — it bounds what a future
`grus-eda drc` checker can honestly claim by pinning three reference
surfaces (Magic full deck, OpenROAD DRT report, OpenROAD antenna check)
and by classifying every rule the Magic deck checks.

---

## 1. The three counting surfaces are not the same number

Magic's `drc` reports three different quantities; the golden records all
of them separately because they disagree by construction:

| Surface | Command | Meaning | gcd | picorv32 |
|---|---|---|---|---|
| Flat count | `drc count` | error regions in the top cell only (nothing inside subcells) | 0 | 0 |
| Total count | `drc count total` | **merged error regions, hierarchy-wide** — the headline `magic_total` | 305 | 8049 |
| listall bbox | `drc listall why` | per-rule `{why, bbox-list}` fragments | 426 | 11082 |

`drc check` runs asynchronously; the sequence is `drc check` →
`drc catchup` → count. **The 2026-11-22 P-DRC probe's "0 DRC errors on
`gcd.routed.def`" in `docs/results.md` was premature counting** — it ran
`drc count total` without `drc catchup`, so the still-running check had
recorded nothing. With `catchup` the real hierarchical total is 305.

`drc count total` merges overlapping/split error regions into one;
`drc listall why` enumerates every bbox fragment of every region — the
bbox sum is therefore larger (gcd 426 vs 305; picorv32 11082 vs 8049).
The JSON keeps both (`magic_total`, `magic_listall_bbox_total`,
`magic_count_matches_bbox_sum`) and never asserts their equality. A
future checker compares **per-rule lists** against `magic_per_rule`, not
against `magic_total`.

## 2. The "clean" baseline is not zero — name it honestly

Neither routed DEF is Magic-clean under `drc style drc(full)`. All
clean-DECK violations sit inside `sky130_fd_sc_hd` standard-cell masters
(nwell geometry that OpenROAD's LEF-based DRT never sees):

| Rule | gcd bboxes | picorv32 bboxes |
|---|---|---|
| `All nwells must contain metal-connected N+ taps (nwell.4)` | 300 | 8084 |
| `N-well spacing < 1.27um (nwell.2a)` | 120 | 2756 |
| `N-well width < 0.84um (nwell.1)` | 6 | 242 |

Meanwhile the OpenROAD `-output_drc` DRT report is empty on both
(`drt_drc_count: 0`; gcd's is a byte-identical re-route of the input
DEF, `drt_report_source: regenerated_identical`, input sha
`d1571bd2eeddd947`). The two surfaces measure different decks — DRT
checks routing-layer geometry only; the Magic full deck also checks
diff/well/implant rules inside the cell masters. A `grus-eda drc`
claim must therefore name which deck it emulates.

Antenna: `check_antennas -verbose` → gcd **0 net / 0 pin** violations;
picorv32 **19 net / 19 pin** violations (`[INFO ANT-0002]`/`[INFO
ANT-0001]`, Tcl result `19`). `-verbose` prints one `Net:` block per
violating net — per-pin `Layer:` rows with `Partial/Cumulative area
ratio` vs `Required ratio` and a `(VIOLATED)` suffix on the failing
requirement (met1/met2 side-area 400.00 limits on picorv32);
`antenna_per_net` records each net's pins and violated layers.

## 3. Rule taxonomy — bounds the future checker's claim

`generate_drc_ref.sh` extracts every `why` string from the `drc` section
of `sky130A.tech` (lines ~4164–4961): **278 statements, 239 unique
names**. The classifier keys on the parenthesized Magic rule id
(`(met1.3)`, `(licon.1c + li.5)`, `(diff/tap.23)` — first id wins), not
prose keywords: a spacing/width/area statement on diffusion, taps, poly,
wells, FETs, or caps is not checkable from LEF+DEF, so the earlier
keyword-based split (195 "lef_expressible") was overstated and has been
corrected. Both denominators are recorded in the JSON
(`classification_unique` / `classification_statements`).

| Class | Unique (of 239) | Statements (of 278) | Meaning | Claim boundary |
|---|---|---|---|---|
| `lef_expressible` | 50 | 55 | rule id ∈ {li, mcon, via, via2–via4, met1–met5} with no device-layer reference in the text — pure routing/cut geometry | A LEF+DEF checker can assert equality with Magic on these — the E7 subset-checker scope |
| `magic_only` | 189 | 223 | every other id: diffusion/tap/poly (`diff/tap`, `psd`, `nsd`), wells (`nwell`, `dnwell`, `hvi`), contacts to device layers (`licon`), FET/varactor/cap (`denmos`, `depmos`, `varac`, `capm`, `cap2m`), photodiode/SONOS/RDL/pad constructs, angle rules (`x.*`), and routing ids whose text references MiM caps (`via3.* + via3.4`, `met4.3 - via3.4`) or device contacts (`li.5` poly/diff overlap) | Never claimable from LEF+DEF data |
| `unclassified` | 0 | 0 | every name in this deck carries a parenthesized id; a name without one would land here rather than be guessed | — |
| `antenna` | 0 | 0 | no `why` string in the `drc` section mentions antenna/ratio — antenna checks are a separate Magic surface, not part of `drc check` | Covered by `check_antennas`, not by `drc listall` |

Two sky130A.tech statements (`rect_only *mimcap`, `rect_only *mimcap2`)
carry an unclosed `"` — a PDK tech-file defect. The extractor joins
continuation lines and works per logical line so the defect cannot
desync quote pairing; the post-quote tail is taken as the intended name.

## 4. Sanity control — `add_spacing_probe_wire`

The script appends one `NEW` `met2` segment to a different signal net at
a 0.03 µm edge gap from an existing vertical wire (min spacing 0.14 µm)
on both designs. Result, recorded in `sanity_control`:

- gcd: `magic_total` 305 → **307** (+2 merged regions; +3 bbox
  fragments), `new_rules_vs_clean` = `["Metal2 spacing < 0.14um
  (met2.2)"]`; picorv32: 8049 → **8051**, same new rule.
- `violated_rules_in_checked_deck: true` — every observed `why` string
  template-matches a `drc`-section statement (`%d`/`%a` render as
  `.*?`).

The control proves the surface both detects a sub-minimum spacing
violation and names the violated rule.

## 5. Reproduction

```
tests/designs/generate_drc_ref.sh gcd      tests/designs/gcd/gcd.routed.def
tests/designs/generate_drc_ref.sh picorv32 tests/designs/picorv32/picorv32.routed.def
```

The script dispatches into the pinned image itself (docker or
`mise exec docker-cli@latest colima@latest lima@latest -- docker`).
picorv32's `picorv32.routed.def` is gitignored — run
`tests/designs/picorv32/generate_ref.sh` first (step 5b routes).

Byte-reproducibility: reran end-to-end on 2026-11-23 — both JSONs
bit-identical. Runtime stays out of the golden (per-phase seconds go to
the gitignored `runs/drc/runtime.json`; gcd ≈ 22 s wall incl. the 19 s
DRT regen, picorv32 ≈ 105 s). One named, deterministic divergence for a
fresh tree: picorv32's `drt_report_*` fields encode `existing` while
`picorv32.drc.rpt` (gitignored) is present; without it the script takes
the regen path and records `regenerated_*` instead — same count (0).

---

# E7 ticket 2 (`e7-drc-geom`): geometry model + width family

The LEF+DEF subset checker's geometry substrate and its first rule
family. `drc::geom` assembles every shape Magic sees on each
routing/cut layer; `drc::width` checks the 13 `lef_expressible` width
rules and joins `magic_per_rule` by rule-name template.

## 6. Shape sources and the DEF wire model

Per-layer `LayerShapes` (routing layers indexed by `LayerId`, cut
layers by `cut_layers` position) collect five source classes:

| `ShapeSrc` | Source | Width-checked? |
|---|---|---|
| `SignalWire` | `NETS` `ROUTED`/`FIXED`/`COVER`/`PLACED`/`NEW` wire segments + `RECT` stubs | yes |
| `SpecialWire` | `SPECIALNETS` wire segments | yes |
| `Via` | cut rect + every LEF `VIA`/`VIAS` metal rect, placed at the via point | cuts yes; metals no |
| `MacroPin` / `MacroObs` | placed component's macro `PIN`/`OBS` rects, orientation-transformed | no |
| `IoPort` | top-level `PINS` `+ LAYER` rects | yes |

Via resolution is the single `resolve_def_via` path SPECIALNETS already
uses — `def_vias` (DEF `VIAS` records) first, then `TechDb::via_defs`
(LEF `VIA`); unknown names error loudly. `+ VIARULE` generated vias
expand deterministically from retained `CUTSIZE`/`ROWCOL`/`CUTSPACING`/
`ENCLOSURE` (OpenDB symmetric-overhang reading, arithmetic-verified
against `via2_3_1600_480_1_5_320_320`).

**Wire expansion is square-capped.** A `pt pt` segment paints
`[min−half, max+half]` along its axis — this is what OpenROAD/DRT
actually emit: picorv32 carries sub-width jogs such as
`NEW met1 ( 390670 213350 ) ( * 213360 )` that only stay DRC-clean if
the endpoint extends half a width. Perpendicular (flush) expansion made
that jog a real 140×10 notch and produced ~90 false `met1.1` findings;
square caps reproduce Magic's 0 verdict exactly. `SPECIALNETS` keeps
the pre-existing perpendicular expansion — `pdn.rs` mesh and attach
semantics pin those rects — which can only under-report metal
(conservative for merge-relief); unifying is a recorded divergence.
`+ NONDEFAULTRULE name` applies the `NONDEFAULTRULES` `+ LAYER l
WIDTH w` override; `EXT`/`DESIGNRULEWIDTH`/`SUBNET` bail loudly.

## 7. Width family status

| Family | Status |
|---|---|
| `width` | **implemented** — `drc::width`, 13 rules, 0 violations on both routed DEFs |
| `spacing` | **implemented** — `drc::spacing`, 20 rules, 0 violations on both routed DEFs (§10–12) |
| `enclosure` | **implemented** — `drc::enclosure`, 9 rules, 0 violations on both routed DEFs (§13–14) |
| `area` | **implemented** — `drc::area`, 6 rules, 0 violations on both routed DEFs (§15) |
| `holes` | **implemented** — `drc::area`, 2 rules, 0 violations on both routed DEFs (§15) |
| `magic_only` statements | out of scope per §3 (nwell-in-master etc. — the 305/8049 `magic_total` counts live here) |

## 8. Rule bounds — LEF provenance

Bounds are the **LEF** layer `WIDTH` / via-cut `WIDTH`, never the Magic
techfile constant — where they diverge we check LEF, which is the
documented claim boundary. `rule_name` is the verbatim
`magic_rules_checked.names` template; the first `(id)` inside its
parens is the `magic_per_rule` join key. Magic `%d` re-derived from
`sky130A.tech` `drc` section (local PDK copy, pinned content):

| `rule` | layer | LEF bound (DBU) | `rule_name` (verbatim) | Magic `%d` | divergence |
|---|---|---|---|---|---|
| `li.c1` | li1 | 170 | `Core local interconnect width < %d (li.c1)` | 140 | Magic checks a looser core-li bound; we check LEF 170 |
| `li.1` | li1 | 170 | `Local interconnect width < %d (li.1)` | 170 | — |
| `li.7` | li1 | 170 | `Local interconnect width < %d (li.7)` | 290 | Magic's fat-li context bound; collapses to the same li1 bound here |
| `mcon.1` | mcon | 170 | `mcon.width < %d (mcon.1)` | 170 | — |
| `met1.1` | met1 | 140 | `Metal1 width < %d (met1.1)` | 140 | — |
| `met2.1` | met2 | 140 | `Metal2 width < %d (met2.1)` | 140 | — |
| `met3.1` | met3 | 300 | `Metal3 width < %d (met3.1)` | 300 | — |
| `met4.1` | met4 | 300 | `Metal4 width < %d (met4.1)` | 300 | — |
| `met5.1` | met5 | 1600 | `Metal5 width < %d (met5.1)` | 1600 | — |
| `via.1a` | via | 150 | `Via1 width < %d (via.1a + 2 * via.4a)` | 260 | Magic's bound is enclosure-augmented; we check the cut's LEF WIDTH |
| `via2.1a` | via2 | 200 | `via2 width < %d (via2.1a + 2 * via2.4)` | 280 | same |
| `via3.1` | via3 | 200 | `via3 width < %d (via3.1 + 2 * via3.4)` | 320 | same |
| `via4.1` | via4 | 800 | `via4 width < %d (via4.1 + 2 * via4.4)` | 1180 | same |

## 9. Check semantics vs Magic

Magic measures width on the merged per-layer polygon; we check each
`ShapeRect` (`min(w,h) < bound`, strict `<`) and then apply
**merge-relief**: a sub-bound wire rect is not reported iff drawn metal
on the same layer covers its entire perpendicular span and chains
outward until the union spans `bound` on the narrow axis — the narrow
boundary pair dissolves into the union (what Magic does to `RECT`
stubs and jog caps); a shorter reach is still a sub-bound corridor.
`MacroPin` rects count as drawn metal for relief (pin abstracts
understate GDS); `MacroObs` does not (not fabricated metal). Relief
sources are `SignalWire`/`SpecialWire`/`Via`/`IoPort`/`MacroPin`.

`li.c1`/`li.7` are statement-position rules in Magic (core-area and
resist-li contexts); our model carries no per-statement position, so
all three `li1` names share the LEF bound — a recorded collapse, fine
for the zero gate since every real li1 shape meets the tightest bound.

---

# E7 ticket 3 (`e7-drc-spacing`): spacing family + connectivity

`drc::spacing` checks the 20 `lef_expressible` spacing rules against
the same assembled `DrcDesign`. Bounds are parsed LEF
`SPACING`/`SPACINGTABLE` values retained per layer
(`RoutingLayer::{spacing_dbu, spacing_table}`,
`CutLayer::spacing_dbu`) — never the Magic techfile constants, which
for this deck happen to be identical numbers.

## 10. Spacing rules — LEF provenance

Non-plain `SPACING` forms (`... RANGE ...` — only commented-out lines
in this PDK) are consumed and ignored; `min_spacing_dbu()` prefers the
plain scalar, else the lowest-width `SPACINGTABLE` row (choosing by
width, not PRL — over-requires at worst). `wide_spacing()` returns the
top row only when the table has ≥2 rows; met5's single-row table
correctly yields no wide rules, matching the deck's absent `met5.*`
wide statements.

| `rule` | layer | LEF bound (DBU) | provenance |
|---|---|---|---|
| `li.3` / `li.c2` | li1 | 170 | `SPACINGTABLE WIDTH 0 0.17` |
| `mcon.2` | mcon | 190 | `SPACING 0.19` |
| `via.2 - 2 * via.4a` | via | 170 | `SPACING 0.17` |
| `via2.2 - 2 * via2.4` | via2 | 200 | `SPACING 0.2` |
| `via3.2 - 2 * via3.4` | via3 | 200 | `SPACING 0.2` |
| `via4.2 - 2 * via4.4` | via4 | 800 | `SPACING 0.8` |
| `met1.2` / `met2.2` | met1/2 | 140 | `SPACINGTABLE WIDTH 0 0.14` |
| `met1.3a`/`3b`, `met2.3a`/`3b` | met1/2 | 280 | `SPACINGTABLE WIDTH 3 0.28` (threshold 3 µm) |
| `met3.2` / `met4.2` | met3/4 | 300 | `SPACINGTABLE WIDTH 0 0.3` |
| `met3.3c`/`3d`, `met4.5a`/`5b` | met3/4 | 400 | `SPACINGTABLE WIDTH 3 0.4` (threshold 3 µm) |
| `met5.2` | met5 | 1600 | `SPACINGTABLE WIDTH 0 1.6` |

Deck-id irregularities are pinned, not "fixed": the met3 wide pair is
`3c`/`3d`, met4's is `5a`/`5b`, and each `*b`/`*d` name covers two
deck statements (`allmN` + `*obsmN` widespacing) — the checked-source
model has no separate obs plane. `rule_name` strings are verbatim
`magic_rules_checked.names` templates.

The table below joins every implemented spacing rule to its LEF bound
and to the Magic techfile `%d`, re-derived from `sky130A.tech` `drc`
section (`spacing`/`widespacing`/`cifspacing` statements, nm). Where
the two disagree the difference is named — it is a claim boundary,
not a parity bug.

| `rule` | layer | LEF bound (DBU) | `rule_name` (verbatim) | Magic `%d` | divergence |
|---|---|---|---|---|---|
| `li.3` | li1 | 170 | `Local interconnect spacing < %d (li.3)` | 170 | — |
| `li.c2` | li1 | 170 | `Core local interconnect spacing < %d (li.c2)` | 140 | Magic's core-li bound is *looser* (140); we check LEF 170 — same class as the `li.c1` width collapse |
| `mcon.2` | mcon | 190 | `mcon.spacing < %d (mcon.2)` | 190 | — |
| `met1.2` | met1 | 140 | `Metal1 spacing < %d (met1.2)` | 140 | — |
| `met2.2` | met2 | 140 | `Metal2 spacing < %d (met2.2)` | 140 | — |
| `met3.2` | met3 | 300 | `Metal3 spacing < %d (met3.2)` | 300 | — |
| `met4.2` | met4 | 300 | `Metal4 spacing < %d (met4.2)` | 300 | — |
| `met5.2` | met5 | 1600 | `Metal5 spacing < %d (met5.2)` | 1600 | — |
| `via.2 - 2 * via.4a` | via | 170 | `Via1 spacing < %d (via.2 - 2 * via.4a)` | 60 | Magic checks the fabricated cut plane (60); we check the router-level LEF `SPACING` — tighter, recorded |
| `via2.2 - 2 * via2.4` | via2 | 200 | `via2 spacing < %d (via2.2 - 2 * via2.4)` | 120 | same |
| `via3.2 - 2 * via3.4` | via3 | 200 | `via3 spacing < %d (via3.2 - 2 * via3.4)` | 80 | same |
| `via4.2 - 2 * via4.4` | via4 | 800 | `via4 spacing < %d (via4.2 - 2 * via4.4)` | 420 | same |
| `met1.3a` | met1 | 280 | `Spacing of metal1 features attached to and within 0.28um of large metal1 < %d (met1.3a)` | 280 | bound equal; wide trigger differs — ours `>3000` (LEF `WIDTH 3` breakpoint) vs Magic `widespacing` 3005 |
| `met1.3b` | met1 | 280 | `Metal1 > 3um spacing to unrelated m1 < %d (met1.3b)` | 280 | same trigger divergence |
| `met2.3a` | met2 | 280 | `Spacing of metal2 features attached to and within 0.28um of large metal2 < %d (met2.3a)` | 280 | same |
| `met2.3b` | met2 | 280 | `Metal2 > 3um spacing to unrelated m2 < %d (met2.3b)` | 280 | same |
| `met3.3c` | met3 | 400 | `Spacing of metal3 features attached to and within 0.40um of large metal3 < %d (met3.3c)` | 400 | same |
| `met3.3d` | met3 | 400 | `Metal3 > 3um spacing to unrelated m3 < %d (met3.3d)` | 400 | same |
| `met4.5a` | met4 | 400 | `Spacing of metal4 features attached to and within 0.40um of large metal4 < %d (met4.5a)` | 400 | same |
| `met4.5b` | met4 | 400 | `Metal4 > 3um spacing to unrelated m4 < %d (met4.5b)` | 400 | same |

## 11. Connectivity islands — what may flag

Magic merges same-net touching shapes before spacing evaluation; we
reproduce that with a union-find over the flat all-layer rect space:

- **Same layer**: same-net rects that touch or overlap merge
  (closed-box intersection — corner contact counts, it only ever adds
  connectivity). Same-net *separated* islands still check against each
  other.
- **Cross layer**: every rect emitted by one via instance shares a
  `ShapeRect.group` id and merges unconditionally — a cut array's
  cuts connect through the via's metals, not through cut-layer
  proximity. This is what chains `met1 wire ↔ via1 cut ↔ met2 wire`.

A pair in different islands flags iff its Euclidean gap is in
`(0, bound)` — integer-exact `gx² + gy² < bound²` over per-axis
separations (the golden ran `drc euclidean on`). Exact-bound passes;
distance-0 different-island pairs are exempt (`touching_ok` in every
deck spacing statement).

Pair enumeration is `LayerShapes::for_each_pair_within` — an
`xl`-order sweep with an `xh`-keyed active set, O((n + pairs) log n).
Per-rect `iter_overlapping` probes degrade to O(n²) here because a
die-spanning power rail carries the `max_xh` prefix past every probe
start; the sweep visits each candidate pair exactly once.

## 12. Wide-metal rules

`WideUnrelated` (`MetalN > 3um spacing to unrelated mN`) fires when
either side's `min(w,h)` exceeds the LEF `WIDTH` threshold (3 µm = the
`WIDTH 3` table breakpoint) and the pair is in different islands.

`WideAttached` (`features attached to and within 0.28/0.40um of large
metalN`) fires when a non-wide member of a wide rect's island sits
within `window` (Euclidean, inclusive — the names say "within") of the
wide member and meets a non-wide shape of another island at sub-bound
gap; `window` is the rule's bound (280/400). `MacroPin`/`MacroObs`
join islands (pin metal is connectivity) but never generate findings —
under-reports at worst vs Magic's GDS-plane checks.

---

# E7 ticket 4 (`e7-drc-enc-area`): enclosure + area + hole families

`drc::enclosure` checks the 9 `lef_expressible` cut-overlap rules;
`drc::union` + `drc::area` check the 6 min-area and 2 min-hole-area
rules over merged plane polygons. Bounds are the LEF `ENCLOSURE`
(directional pairs on cut layers), `AREA`, and `MINENCLOSEDAREA`
statements retained in `TechDb` — never the Magic techfile constants.

## 13. Enclosure rules — LEF provenance

`ENCLOSURE {BELOW|ABOVE} o1 o2` reads symmetrically: `min(o1,o2)`
required on both axes **and** `max(o1,o2)` on at least one — what the
deck's `surround` pairs encode and what OpenDB's generated-via
expansion produces. A per-axis pinning is *not* used: the deck proves
the symmetric reading and a directional cut would falsely flag a via
rotated the other way. The table joins each implemented rule to its
LEF pair and the Magic `%d`, re-derived from `sky130A.tech` `drc`
section (`surround` statements, nm) — where they diverge the
difference is a claim boundary.

| `rule` | cut → metal | LEF overhangs (DBU) | `rule_name` (verbatim) | Magic `%d` | divergence |
|---|---|---|---|---|---|
| `met1.4` | mcon → met1 | 30 / 60 → both ≥ 30 | `Metal1 overlap of local interconnect contact < %d (met1.4)` | 30 | — |
| `met1.5` | mcon → met1 | → one ≥ 60 | `Metal1 overlap of local interconnect contact < %d in one direction (met1.5)` | 60 | — |
| `via.5a` | via → met1 | 55 / 85 → both ≥ 55 ∧ one ≥ 85 | `Metal1 overlap of Via1 < %d in one direction (via.5a - via.4a)` | 30 | Magic checks ≥30 one-directional only; we check the LEF pair — tighter, recorded |
| `met2.5` | via → met2 | 55 / 85 → same | `Metal2 overlap of Via1 < %d in one direction (met2.5 - met2.4)` | 30 | same |
| `via2.4a` | via2 → met2 | 40 / 85 → same | `Metal2 overlap of via2 < %d in one direction (via2.4a - via2.4)` | 45 | Magic 45 one-directional vs LEF 40/85 pair |
| `met3.4` | via2 → met3 | 65 / 65 → both ≥ 65 | `Metal3 overlap of via2 < %d (met3.4)` | 25 | Magic 25 vs LEF 65 |
| `via3.5` | via3 → met3 | 60 / 90 → same | `Metal3 overlap of via3 in one direction < %d (via3.5 - via3.4)` | 30 | Magic 30 vs LEF 60/90 |
| `met4.3` | via3 → met4 | 65 / 65 → both ≥ 65 | `Metal4 overlap of via3 < %d (met4.3 - via3.4)` | 5 | Magic 5 vs LEF 65 |
| `met5.3` | via4 → met5 | 310 / 310 → both ≥ 310 | `Metal5 overlap of via4 < %d (met5.3 - via4.4)` | 120 | Magic 120 vs LEF 310 |

Sides without a deck statement are not implemented (recorded): mcon
BELOW → li1 is LEF `0 0`; via4 BELOW → met4 (`via4.4` = 190) is folded
into Magic's `via4.1` width composite (1180 = 800 + 2·190), which the
width family already checks at LEF `WIDTH 800` — same folding for
`via.1a`, `via2.1a`, `via3.1`.

## 14. Enclosure measurement

For each `ShapeSrc::Via` cut rect (the only cut-layer source):

- **Candidates** — every *same-net* rect on the rule's metal layer
  within `bmax` of the cut (`iter_overlapping` on the probe). Net
  filtering is the model: the via's own metals carry the same net tag
  and foreign metal reaching the cut would be a short anyway.
- **Side overhangs** — split the cut's perpendicular span into
  elementary sub-intervals at candidate rect edges; inside a
  sub-interval the covering union is constant so the overhang is
  exact: `cut_edge − union_edge`, negative when the cut edge
  protrudes past all same-net metal. `enc_x = min(left, right)`,
  `enc_y = min(bottom, top)`.
- **Clauses** — `Both`: flag iff `enc_x < b ‖ enc_y < b` at
  `min(pair)`; `One`: flag iff both axes `< max(pair)`; `Full`
  evaluates `Both` at `min` first then `One` at `max` — one violation
  per (rule, cut) recording the failing clause. Exact-bound passes
  (`enc == b` satisfies `>=`); the real generated vias sit exactly at
  the LEF bound in places, which is what the `>`-flip fail-proof
  exercises.

`MacroObs` never encloses (blockage abstract, not metal); a cut with
no same-net metal reaching its edge gets a negative overhang —
`absence_illegal` semantics.

## 15. Min-area and hole-area rules

`drc::union` supplies the shared substrate: `components` merges member
rects (`SignalWire`/`SpecialWire`/`Via`/`IoPort`/`MacroPin` —
`MacroObs` is never a member) on closed touch **regardless of net**,
matching Magic's merged-plane model (a two-net touch is a short, not
something the area check splits over); `union_area` is an x-sweep
union measure; `enclosed_voids` extracts free-space components inside
a component's bbox that never touch its boundary — the boundary test
is what separates exterior notches from holes, and a void pinched to
a point still counts as enclosed.

| `rule` | layer | LEF bound (DBU²) | `rule_name` (verbatim) | Magic | divergence |
|---|---|---|---|---|---|
| `li.6` | li1 | 56100 | `Local interconnect minimum area < %a (li.6)` | 56100 | — |
| `met1.6` | met1 | 83000 | `Metal1 minimum area < %a (met1.6)` | 83000 | — |
| `met2.6` | met2 | 67600 | `Metal2 minimum area < %a (met2.6)` | 67600 | — |
| `met3.6` | met3 | 240000 | `Metal3 minimum area < %a (met3.6)` | 240000 | — |
| `met4.4a` | met4 | 240000 | `Metal4 minimum area < %a (met4.4a)` | 240000 | — |
| `met5.4` | met5 | 4000000 | `Metal5 minimum area < %a (met5.4)` | 4000000 | — |
| `met1.7` | met1 | 140000 | `Min area of metal1 holes > 0.14um^2 (met1.7)` | derived hole plane | — |
| `met2.7` | met2 | 140000 | `Min area of metal2 holes > 0.14um^2 (met2.7)` | same | — |

Evaluation gates (recorded, same class as width's finding filter): a
component is checked only when it holds ≥1 drawn member
(`SignalWire`/`SpecialWire`/`Via`/`IoPort`) — `MacroPin`-only islands
are pin abstracts that understate the real GDS, so flagging them can
only over-report. `MacroPin` metal still merges and still counts in
area/coverage. `li.6` appears twice in `names` (`allli,*obsli` and the
`coreli` variant) — one rule row, same collapse class as `li.c*`.

Hole bounds resolve `MINENCLOSEDAREA` first; a LEF that omits it falls
back to the deck-pinned `140000` (the `rule_name` literal itself
carries "0.14um^2"). A void touching any `MacroObs` rect (closed
intersection) is discarded — real content hides behind the obs
abstract, and discarding the whole void rather than subtracting the
obs rect is the conservative direction. Both areas compare strict `<`
on exact integers — no tolerance.
