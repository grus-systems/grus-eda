# e7-drc-spacing — Same-net merge semantics (connectivity islands)

The heart of the ticket — what Magic's `spacing ... touching_ok`
statements actually mean on our rect model, and why a plain
net-name comparison is wrong.

## Magic's model

Magic's spacing rules apply between two pieces of *different nodes*.
Node = electrically connected metal: touching or overlapping geometry
on the same layer, connected through vias across layers. Two
consequences:

1. Touching/abutting/overlapping shapes are one node — never flagged,
   regardless of net name. The `touching_ok` flag additionally
   exempts *different-node* pairs whose edges abut (distance 0) or
   corner-touch.
2. Two same-net shapes that are *not* connected (separated islands —
   a disconnected stub, a floating segment) are different nodes and
   get checked like any unrelated pair.

## The merge

Union-find over **all** `ShapeRect`s in the `DrcDesign` (checked or
not — merge membership is about connectivity, not checkability):

- **Same-layer merge**: rects `a`, `b` on the same layer merge iff
  `a.net == b.net != None` and they touch-or-overlap: `gx == 0 &&
  gy == 0` where `gx`/`gy` are the per-axis separations. Corner
  contact counts (Magic's point-contact connectivity); recorded as a
  choice — it only ever *adds* connectivity, so it can hide a finding
  but never invent one. If real-design evidence disagrees, tighten to
  `strict-overlap || edge-share` and record the rerun.
- **Instance merge**: every rect emitted by one `expand_via` call
  shares `ShapeRect.group != 0` — cut + lower/upper metals union
  unconditionally, which is what chains connectivity *across* layers
  (a met1 wire overlapping the via's met1 rect merges with the cut,
  which merges with the met2 rect, which merges with the met2 wire).
  Without it the 5 cuts of a `via2_3_1600_480_1_5_320_320` array
  (170 DBU gap) are five separate same-net islands and false-flag
  under the LEF `via2` bound 200.

`group: u32` is a new `ShapeRect` field — `0` for non-via shapes, a
dense `1..` counter over via instances in the existing
signal-then-special chain order. Public-struct literal break → minor
version bump (0.9.0 → 0.10.0), and `#[serde(default)]` keeps old
serialized values loading as ungrouped.

Build cost: one merge pass per layer — for each rect, an
`iter_overlapping` probe inflated by 1 DBU yields every
touch-or-overlap candidate (`gx < 1 && gy < 1` ⟺ touching, integer
coordinates); union when the net matches. Then one pass unioning
group members. O(n log n + candidates) per layer.

## Check predicate

A rect pair `(a, b)` on a rule's layer, `find(a) != find(b)`, flags
iff:

```
gx = max(0, max(a.xl - b.xh, b.xl - a.xh))   // per-axis separation
gy = max(0, max(a.yl - b.yh, b.yl - a.yh))
(gx > 0 || gy > 0)  &&  gx*gx + gy*gy < bound*bound   // i64
```

- **`0 <` — `touching_ok`.** Every spacing statement in the deck
  carries `touching_ok`: abutting or corner-touching different-island
  pairs are exempt, and so are overlapping pairs (a cross-net short
  merges into one Magic node — it is a connectivity defect, not a
  spacing violation; no `lef_expressible` rule covers it).
- **`< bound` — Euclidean, integer-exact.** The committed reference
  ran `drc euclidean on`; a diagonal corner gap of `sqrt(gx²+gy²)`
  counts, not `max(gx,gy)`. Squared compare avoids floats.
- **Candidate reach**: `iter_overlapping` on `a` inflated by `bound`
  yields all pairs with `gx < bound && gy < bound` — a strict
  superset of the Euclidean candidates (`euclid >= max(gx,gy)`), so
  no candidate is missed; the precise test filters the corner cases.
  Pair dedup by raw-pointer order (`(b as *const _) > (a as *const
  _)` — same `Vec`, no API change).

## Checked sources

| Layer class | Generates findings | In union-find |
|---|---|---|
| routing | `SignalWire`, `SpecialWire`, `Via` metals, `IoPort` | all sources, incl. `MacroPin`/`MacroObs` |
| cut | `Via` cuts | all (only `Via` exists there) |

`MacroPin`/`MacroObs` never *generate* findings — LEF abstracts are
not the fabricated GDS Magic measures (the width family's recorded
scope decision). `MacroPin` still joins the union-find: its net tag
is real connectivity (pin metal does connect the two same-net wires
landing on it); `MacroObs` carries `net: None` and merges with
nothing. Excluding them from findings is *under*-reporting vs Magic
(pin abstracts understate the real metal, so the measured gap is
larger than GDS truth) — conservative direction, recorded.

## What this does NOT merge

- **Different nets, even when abutting** — union requires equal net.
  A VPWR stripe touching a signal wire is two islands; the pair is
  exempt via `touching_ok` only at distance 0, not by merge.
- **Same-net separated islands** — deliberately *not* merged (that is
  the Magic node rule and an acceptance-tested behavior: `assert`
  with a fixture that a separated same-net pair at sub-bound distance
  flags).
- **Across layers without a via instance** — a met1 shape and a met2
  shape never union on coordinates alone (different layers, no shared
  plane); only the via's group id bridges layers.

## The `li.c2` collapse

Magic's `li.c2` is a core-area statement (`coreli,pc,ndc,... allli`)
with a *looser* bound (140) than `li.3` (170). Our model has no
core-vs-periphery statement position — both names check the li1 LEF
bound 170, same collapse as `li.c1`/`li.7` in width. Recorded; fine
for the zero gate since every real li1 pair meets the tighter bound
(and li1 has near-zero candidate shapes on routed DEFs anyway — see
`tests.md` risk register).

## Files

- `crates/grus-core/src/drc.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/drc/spacing.rs`
- `docs/e7-drc.md`
