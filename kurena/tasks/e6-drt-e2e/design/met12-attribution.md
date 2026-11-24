# e6-drt-e2e — met1.2 residual attribution (r5, the memo's central item)

> **r7 note — unchanged at `5d1502b`** (alias refactor only; the X3
> fix and all-0 outcome stand).
>
> **r6 outcome — RESOLVED, X3 arm fired.** `who6.tcl` (site D) +
> `who7.tcl` (site E) ran in-container per §3: `drc why` names
> `met1.2`, the flattened tile dump shows only own-net met1 paint —
> the partner edge was **our own `RECT` fill stopped 20–30 DBU short
> of the facing band**. Root cause: `try_fill` ran the li1-strict
> `fin − cover_rects` blocker carve on `hull_host_open` met1, where
> `rect_allowed` already admits the whole member-hull interior — the
> pin-cover rect carved the uncovered band out of every member-hull
> fill. Fix = `if mask.host_open(l) { continue; }` in `try_fill`'s
> mask branch (`maze.rs`); li1 keeps the carve. All 7 sites
> re-measure clean (`met1.2` 32 → 0), the `_099_` `notch_residual`
> closed by the same arm, `routed` 187 → 188. §4's fix arm ran; the
> pin arm never did — `PINNED_LEF_RESIDUALS` is empty. The §1–§3
> record below stays as designed.

The post-closure measurement (run inside the pinned image at
`fba5f85`, 2026-11-24 20:07 — transcripts in the gitignored
`runs/drt-ours/` workdir; the step-E JSON it should have fed was
never persisted, so `check_drt_ours.sh gcd` reruns before commit)
reads `lef_expressible.ours = 0` on 49/50 rules and **`met1.2 = 32`
bbox fragments**. The memo asks for the class of those 32 — "same
mechanism residual or new site anatomy — check 2-3 bboxes". This file
carries the decomposition **already measured** plus the attribution
protocol for the remaining unknowns. No site is guessed.

## 1. Fragment → site decomposition (measured, no container needed)

`drc listall why` renders one bbox list per error; the checkpoint
run's log `tests/designs/gcd/runs/drt-ours/ours_magic.log`
(gitignored) holds all 32 met1.2 bboxes verbatim. Magic internal
units ×5 = DEF DBU (verified against the r3 `_365_` probe site).
Clustering the 32 bboxes gives **7 sites**:

| site | net | fragments | Magic bbox (×5 = DBU) | our paint at site |
|---|---|---|---|---|
| C | `_099_` | 4 | x∈[8266,8304] y∈[6848,6888] | **the named `notch_residual`** — a={41240,34170,41380,34510} b={41470,34380,41790,34640}; fragments land inside the residual's gap region |
| A | `resp_val` | 5 | x∈[1550,1603] y∈[9528,9537] | emitted `met1 ( 7876 47685 ) RECT ( 0 0 15 200 )` fill; wire `[7732,47770]-[8050,47770]` (shape yl 47700) |
| B | `_084_` | 5 | x∈[7975,8018] y∈[13311,13320] | emitted met1 `RECT` fill (same class) |
| D | `_131_` | 5 | x∈[9462,9515] y∈[5176,5185] | emitted `met1 ( 47436 25925 ) RECT ( 0 0 15 200 )` fill; wire shape bottom 25940; `M1M2_PR` at (47610,26010) |
| E | `_122_` | 5 | x∈[12103,12158] y∈[4088,4095] | emitted `met1 ( 60649 20475 ) RECT ( 0 0 9 210 )` fill; wire `[60350,20570]-[60802,20570]`; `M1M2_PR` at (60490,20570) |
| F | `_070_` | 5 | x∈[14783,14826] y∈[5176,5185] | emitted met1 `RECT` fill — **same y-band as D** (`y×5` identical) — repeated pattern, likely the same pin/site anatomy |
| G | `_066_` | 3 | x∈[15994,16024] y∈[6264,6271] | emitted met1 `RECT` fill (same class) |

Read: **1 site is the already-named `_099_` notch residual; 6 sites
are new anatomy** — every one adjacent to an emitted `RECT` fill on a
different net, all inside instance-hull territory on met1. D+F share
an identical fragment y-band (5176..5185 Magic), i.e. the same
geometry class repeats across cells.

## 2. What the DEF-side anatomy already rules out

- **Not a foreign committed wire/plate/RECT**: a ±500-DBU window scan
  of every net's met1 `wire`/`RECT`/`VIA` paint around site D finds
  only `_131_`'s own fill + wire — the partner edge is not another
  routed net's paint.
- **Not any geometry our model contains**: `self_drc findings_total=0`
  on this DEF. The subset checker's spacing family models euclidean
  corner gaps and pin/OBS/member-pin shapes — a partner inside our
  LEF+DEF model at < s would have been flagged (it caught `_007_`'s
  same-net pair pre-fix).
- **Not a disjoint-piece pair the closure scans**: `close_self_spacing`
  pairs disjoint pieces facing at < s over wires/plates/pin-shapes.
  The surviving sites' partners must therefore be one of:

| class | partner | why invisible to our scan | fix surface |
|---|---|---|---|
| **X1 — invisible subcell paint** | undeclared `.mag`-internal met1 (present only in Magic's cell view) | never enters LEF/OBS/`occ` | not fixable at emit level except by avoiding the neighborhood → named residual, or confinement escalation |
| **X2 — LEF-vs-mag pin discrepancy** | the member pin's real `.mag` met1 edge, tighter than the LEF `PORT` rect our `pin_shapes`/cover use | our model trusts the LEF rect (overlap ⇒ merged ⇒ no pair); Magic sees the true edge < s | same named-residual class — the pin IS our net's member, so this is a same-net notch in Magic's view |
| **X3 — within-island notch our pair-merge hides** | our own committed paint bridged to the prong through a third piece (plate/stub) — the facing edges stay < s where the bridge doesn't cover | the pair's two rects *are* in `pieces`, but if the pair-scan sees them as overlapping/merged (e.g. a 1-DBU corner touch or a piece-union artifact) `notch_axis` yields no signature — needs the probe to confirm the mechanism | closure scan extension (pair the unmerged pieces, or a gap-detection pass over the committed union) |
| **X4 — euclidean corner pair** | same-net paint disjoint on both axes at `√(gx²+gy²) < s` | `notch_axis` requires one-axis overlap | bounded closure extension — the signature is mechanical |

X3/X4 are bounded fixes inside `close_self_spacing`. X1/X2 are the
honest-residual classes — Magic sees geometry our emit substrate
cannot (undeclared cell internals / real pin contours), which is
exactly the residual the hull mask was built to bound and cannot
fully remove without route surgery.

## 3. Attribution protocol (in-container, the r3 method)

For each of the 6 new sites (minimum **2**: D — the D/F repeated
pattern — and E — different fragment shape family):

1. `magic -dnull -noconsole` on the committed DEF → `select` the
   fragment bbox → `drc why` names the rule and the interacting
   layers/shapes; `drc find`/`select save` + the flattened
   `gcd_flat.mag` tile dump shows **whose** paint each edge is
   (our net tiles vs subcell tiles vs pin tiles).
   Probe Tcl goes in the gitignored `runs/drt-ours/` dir
   (`who6.tcl`+ convention — r3's `who1`–`who5` are the template);
   results.md names the files.
2. Record per site: partner edge owner (`_131_` member pin / foreign
   subcell tile / own net tile), partner edge class (pin contour /
   undeclared paint / committed paint), and the gap measurement.
3. Classify into X1–X4. If a site matches no class, it is
   unexplained → keep probing before any pin (§5 discipline).

## 4. Decision rule (fix vs pin — never rebaseline)

- **Any X3/X4 site** → the closure missed a real class; extend the
  scan (bounded, ≤2 iterations), add the unit fixture reproducing the
  site pattern, re-emit, re-run `check_drt_ours.sh gcd`, regenerate
  the golden. The 20:07 317-measurement is then superseded —
  recorded in the results trajectory, not committed.
- **All sites X1/X2** → the residual is geometry-invisible to our
  model: **pin it**. `PINNED_LEF_RESIDUALS` gets
  `("met1.2", <post-fix measured>)` with every site named in the test
  comment and `docs/results.md` (site table above + probe results).
  The `_099_` cluster is the named `notch_residual` cross-check:
  Magic fragments at a census-named site are the mechanism working.
- **Mixed** → fix the fixable class first, then pin the rest per
  site. A pinned count must decompose to zero sites unexplained.

## 5. Residual contract (unchanged from `residuals.md` §5)

Every nonzero `lef_expressible.ours` maps to a named site; a count
with no named site is unexplained, not tolerated. The gate test
asserts `ours == placed` strictly on every unpinned rule and
`ours == pinned && placed == 0` on `met1.2` — a residual moving
either direction fails loudly and must be re-pinned with a named
reason (see `gate.md`).

## Files

- `crates/grus-core/src/drt/maze.rs` (only if X3/X4 lands — closure scan extension + fixtures)
- `crates/grus-core/tests/drt.rs` (`PINNED_LEF_RESIDUALS` entries + site comments)
- `tests/designs/gcd/gcd.drt.ours.routed.def` (only if a fix re-emits)
- `tests/designs/gcd/gcd.drt.ours.reference.json` (commit post-decision)
- `docs/results.md` (site table + probe record)
