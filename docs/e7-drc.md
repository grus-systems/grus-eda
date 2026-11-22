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
| `spacing` | pending — the `LayerShapes::iter_overlapping` spatial index exists for it |
| `enclosure` | pending |
| `area` | pending |
| `holes` | pending |
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
outward past its narrow extent — the narrow boundary pair dissolves
into the union (what Magic does to `RECT` stubs and jog caps).
`MacroPin` rects count as drawn metal for relief (pin abstracts
understate GDS); `MacroObs` does not (not fabricated metal). Relief
sources are `SignalWire`/`SpecialWire`/`Via`/`IoPort`/`MacroPin`.

`li.c1`/`li.7` are statement-position rules in Magic (core-area and
resist-li contexts); our model carries no per-statement position, so
all three `li1` names share the LEF bound — a recorded collapse, fine
for the zero gate since every real li1 shape meets the tightest bound.
