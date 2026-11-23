# e6-drt-via — Documentation updates

All English, matching existing style.

## `docs/e6-drt.md`

New section **"6. Via model"** after §5:

- **Candidate table**: two sources (LEF `VIA` triple +
  `DesignDb::def_vias` retention — note the DEF `VIAS` map was
  parse-local before this ticket), adjacency match
  `(lower, upper) == (k, k+1)`, `expand_via`-at-origin rects for
  later DRC-aware costing, `unmatched`/`gaps` findings.
- **Selection policy**: the `(nonpref_metal, area_dbu2, order)` rank
  with the sky130 per-family tie structure (all non-`_C` variants
  equal-area; `PR` = both metals preferred-aligned) and the
  reference-census join: matched on via/via2/via3; mcon name-only
  divergence (`L1M1_PR` ≡ `L1M1_PR_MR` rect-for-rect, gate-asserted);
  via4 unobserved in both routed DEFs.
- **Via edges**: the shared-crossing rule + `ViaEdge`/`via_edges`/
  `via_edges_per_pair` API, geometric emission (no table coupling),
  ≤1 edge per coordinate per pair, multi-hop composition at the
  node.
- **The measured gap (must be stated, not implied)**: the
  shared-crossing table (`via-edges.md` §2 — met2–met3 = 0 on both
  designs by grid arithmetic) and the reference placement table
  (§3 — preferred-track anchors, 98%+ of non-mcon vias; mcon 87% +
  115 off-anchor pin-access drops). Say plainly: met2–met3
  transitions are unreachable under the landed node model — the
  maze ticket must relax node coordinates (union non-preferred
  grid, TritonRoute-style) or add off-node anchors; same deferred
  class as the io-port attachment fallback already recorded in §5.
- **§4 claim boundary**: replace the "Via edges / layer transitions"
  non-goal bullet with the landed state; new non-goals: DRC-aware
  via costing, via stacking beyond one cut layer (multi-hop
  composes at the node), DEF `VIAS` FIXED-viarule parameterization
  beyond the cut-triple match, and the preferred-anchor node model
  (deferred, named above).

## `docs/results.md`

Dated `### 2026-11-23 — e6-drt-via: via candidates + layer-transition
edges` entry:

- What landed (one paragraph): `drt::via` candidate table + policy,
  `DesignDb::def_vias` retention, `NetGraph::via_edges`.
- **Reference placement measurement table** (the §3 table: per via
  name — instances, strict-shared count, preferred-anchor count) —
  this is the evidence for the met2–met3 named gap; name it as
  computed from the committed `gcd.routed.def` + `TRACKS`, not a new
  container run.
- **Census tables**: per design the per-pair lines (candidates,
  via_edges, default, ref majority, ref used-set), met2–met3 = 0
  called out; total via edges both designs.
- **Findings**: gaps=0, unmatched=0 on both designs; the mcon
  name-only divergence + rect-equality assert.
- **Determinism** (double-run) and **fail-proofs** (fixture
  coverage-drop + gap-pair named finding).
- **Checks run**: exact commands; whether `picorv32.placed.def` was
  already present. Version bump `0.12.2 → 0.12.3` (feature, patch
  on 0.x). Commit policy per the run card (previous entries:
  "No commit per ticket instruction").

## `docs/status.md`

New `Last updated:` head paragraph: `v0.12.3` E6 ticket `e6-drt-via`
— candidate table + default picks joined to the reference census,
`via_edges` at shared crossings, the met2–met3 edge-domain tech gap
with the preferred-anchor measurement and the deferred node-model
fix named; census numbers per design.

## `docs/roadmap.md`

Progress line under E6 after the `e6-drt-graph` entry (~line 798):
ticket `e6-drt-via` — what the via model is, per-pair census
numbers, the met2–met3 gap and what remains (maze search next,
needs the anchor-model decision).

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
