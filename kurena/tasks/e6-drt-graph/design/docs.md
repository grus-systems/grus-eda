# e6-drt-graph — Documentation updates

All English, matching existing style.

## `docs/e6-drt.md`

New section **"5. Guide-bounded track graph"** after §4 (renumber
the findings taxonomy if needed — or fold the new findings into §3):

- **Region**: per (net,layer) the guide-box union clipped to the
  grid = the set of covered `(ix, iy)` track-index pairs, enumerated
  per box by `partition_point` ranges. State that covered index
  pairs are index-contiguous per box and that disjoint boxes never
  spawn corner nodes.
- **Nodes/edges**: node per covered pair; edges between
  consecutive-index nodes on both axes, induced-subgraph semantics,
  the sub-pitch-sliver simplification recorded, no via edges (next
  ticket).
- **Access hookup**: the attachment rule — in-region node inside a
  pin candidate rect — **with the midline caveat documented**: APs
  carry `(track, midline)` coordinates, the literal
  `x ∈ tracks_x ∧ y ∈ tracks_y` reading escapes ~89% of gcd's pins
  (measured), so attachment is defined on the crossing the AP
  designates. This is exactly the kind of spec-vs-code divergence
  AGENTS §7.8 says to write down.
- **Findings taxonomy**: add `missing_signal`, `absent_expected`,
  `zero_coverage`, `region_escape` to §3's table; extend §4 claim
  boundary with the non-goals list from `graph-model.md` §6 (no
  vias, no costs, no DRC pruning, not a FlexGridGraph reproduction).

## `docs/results.md`

Dated `### 2026-11-2x — e6-drt-graph: guide-bounded track graph`
entry in the established shape:

- What landed (one paragraph).
- **Census tables**: per design — guide nets, joined, `missing_signal`,
  `absent_expected` names, `zero_coverage`, total nodes/edges,
  `region_escape` count **with the escape pin names** (gcd's three
  escapes individually explained — the GCell-boundary class).
- **Determinism** line (double-run identical, nonzero).
- **Fail-proofs**: the fixture's shrink/empty-region red→restore-green.
- **Checks run**: exact commands (`cargo test -p grus-core --test
  drt`, `cargo test --workspace`, clippy) and whether
  `picorv32.placed.def` was already present or regenerated.
- Version bump `0.12.1 → 0.12.2` and the ticket's commit policy
  (previous entry's convention: "No commit per ticket instruction" —
  match what the run card says).

## `docs/status.md`

Replace the `Last updated:` head with the new dated entry — same
dense one-paragraph style: E6 ticket `e6-drt-graph`, guide join +
region/node/edge model + access hookup, census numbers
(nodes/edges/escapes per design), fixture + fail-proof reference.

## `docs/roadmap.md`

Progress line under the E6 block (after the `e6-drt-tracks` entry at
line ~789): ticket `e6-drt-graph`, what the graph is, census numbers,
what remains (via model next).

## Files

- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
