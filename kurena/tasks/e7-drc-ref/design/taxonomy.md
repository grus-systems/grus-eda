# e7-drc-ref — Rule taxonomy and scope boundary

The deliverable that bounds all later E7 work: an enumeration of the
rule categories Magic actually checks on sky130 routed DEFs, each
classified as **LEF-expressible**, **antenna**, or **Magic-only** —
plus the honest-claim bound for any future `grus-eda drc`. E7 is a
subset checker at best: Magic's tech file is the canonical sky130 rule
deck and is only partially expressible in LEF.

## Where the enumeration comes from

`drc listall why` on a clean DEF prints nothing, so it cannot
enumerate the deck. Two sanctioned sources (per the spec):

1. **The tech file itself** — `/pdk/sky130A/libs.tech/magic/sky130A.tech`
   is public PDK data (same mount as the LEFs — reading it is
   allowed). Its `drc` section carries one statement per check
   (`spacing`, `edge`, `edge4way`, `width`, `area`, `maxwidth`,
   `overhang`, `agreement`, `rect_only`, `slots_only`, …) each with a
   quoted `why` string — those strings are exactly what `drc listall
   why` prints. Step E extracts every quoted string inside the
   `drc`…`end` section as `magic_rules_checked.names`.
2. **Observed names** — `drc listall why` on the broken DEF. The
   sanity control cross-checks its violated names against the
   extraction (`violated_rules_in_checked_deck`), which catches an
   incomplete extraction grammar.

The extraction grammar is pinned on the first run against the real
file (statement keywords and section delimiters read from the file,
not assumed); zero extracted names aborts the script.

## Classification

A small keyword classifier lives in the committed step-E python (code
— reviewable, rerun-stable), mapping each rule name to:

| category | meaning for `grus-eda drc` | typical name patterns |
|---|---|---|
| `lef_expressible` | checkable from LEF rules (`SPACING`, `WIDTH`, `ENCLOSURE`, `SPACINGTABLE`) + DEF geometry alone | "… spacing …", "… width …", "… enclosure …", "… overlap …" |
| `antenna` | charge-accumulation limits — needs LEF `ANTENNA*` + routing topology, not geometry | "antenna", "… ratio …" on gate/diffusion |
| `magic_only` | geometry/rule-deck checks LEF cannot express: corner rules, exact-overlap (`agreement`/`rect_only`), max area, surround/notch/hole constraints, CIF-only constructs | "… corner …", "… must overlap …", "max area", "… surround …", "… notch …", "… hole …" |
| `unclassified` | no pattern matched — recorded, never force-binned; `docs/e7-drc.md` resolves or names it | (catch-all) |

Classifier precedence: `antenn`/ratio keywords → `antenna`; spacing/
width/enclosure/overlap keywords → `lef_expressible`;
corner/exact-geometry/max-area/surround/notch/hole keywords →
`magic_only`; else `unclassified`. The category names and the
classifier's verdicts are reviewable in the JSON
(`magic_rules_checked.classification`).

## `docs/e7-drc.md` — authored content outline

New doc (the taxonomy's permanent home):

1. **The three surfaces** — what each checks and what each can see:
   Magic `drc(full)` (canonical geometric deck), OpenROAD
   `-output_drc` (TritonRoute's own checker, LEF-driven), OpenROAD
   `check_antennas` (charge rules). Note they are *different* decks —
   a 0 in one does not imply the other.
2. **Rule-category table** — count per category + representative
   verbatim names (from the golden's `magic_rules_checked`), e.g.
   "met1 spacing < 0.14um" → `lef_expressible`; keep the full map in
   the JSON, the doc carries the per-category summary + every
   `unclassified` name resolved or named.
3. **The bound** — what a future `grus-eda drc` may honestly claim:
   - Claimable: the `lef_expressible` subset on DEF-expressible
     geometry, where "match" means per-rule violation counts equal to
     Magic's `drc listall why` breakdown on identical DEFs (the
     scoping criterion — exact integer equality, AGENTS.md §7.5).
   - Not claimable: `antenna` (topology + charge model, different
     input class) and `magic_only` (rule-deck constructs outside LEF).
     A green `grus-eda drc` therefore means "no violation in the
     checked subset", never "DRC clean".
   - The three surfaces' blind spots: Magic `drc(full)` on a DEF read
     sees routing geometry + cell abstracts, not the transistor-level
     deck the tech file also encodes — record which checked rules can
     even fire on LEF/DEF-input geometry (names observed firing count
     as evidence; category counts are the claim bound).
4. **Evidence** — point to `gcd.drc.reference.json` +
   `picorv32.drc.reference.json` and the results.md entry; name the
   image + Magic version.
5. **Sanity control outcome** — mutation name, broken total, verbatim
   violated rule names.

The doc's numbers come from the committed JSONs (quote them); the
classification narrative is authored. If the first run surfaces
`unclassified` names or a `violated_rules_in_checked_deck: false`,
the doc resolves them explicitly — they are findings, not noise.
