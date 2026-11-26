# e6-drt-graph — Test plan

All additions land in `crates/grus-core/tests/drt.rs` (the existing
E6 gate file — same `load`/`pin_label`/`require` helpers, fail-loud
on missing inputs). Real-design tests iterate `["gcd", "picorv32"]`;
`picorv32.placed.def` is gitignored — generate via
`tests/designs/picorv32/generate_ref.sh` if absent (container run,
mention it before running).

## Loading the guide

Extend `load` to also return the `GuideJoin`: read
`design_dir(name)/{name}.guide`, `parse_guides`, `join_guides`.
Missing file → `require` panic, no skip.

## `test_guide_join`

- `join.nets.len() == parsed.len()` — every block resolved (extra
  names would have bailed inside `resolve_guide_nets`; reaching the
  assert proves the join is total).
- `missing_signal.is_empty()` — names printed on failure (a ≥2-pin
  signal net with no guide block is a real gap).
- `absent_expected` printed by name — measured on gcd: `VGND`,
  `VPWR`. Assert the gcd set by name; picorv32's set is whatever the
  run names (power nets + port-only nets — record it).
- `parsed` order preserved into `join.nets`.

## `test_graph_census`

Per design: `build_route_graphs`, then

- `total_nodes > 0` and `total_edges > 0` — nonzero evidence.
- `zero_coverage.is_empty()` — every `(net,layer)` with ≥1 box has a
  non-empty region; entries print `(net, layer)` on failure.
- Sizing: per net, `nodes.len() <= Σ_box |covered ix| · |covered iy|`
  (the union bound — O(boxes × tracks), not die area).
- Every `NetGraph`'s nodes sorted `(layer, ix, iy)` and deduped —
  cheap structural assert.
- Print the census table for `docs/results.md`: guide nets, joined,
  missing, absent, zero-coverage, nodes, edges, escapes.

Measured reference points (planning-time independent model; the
implementation's own numbers go in results.md — if they differ, find
out why before pinning):

| design | guide nets | missing_signal | absent_expected | nodes | edges | zero_cov | escapes |
|---|---|---|---|---|---|---|---|
| gcd | 269 | 0 | 2 (`VGND`,`VPWR`) | ~756k | ~1.45M | 0 | 3 |
| picorv32 | ~6744 | 0 | TBD | TBD | TBD | TBD | TBD |

## `test_pin_attachment`

Per guided net, per member pin: assert `pin_nodes` nonempty **or**
the pin appears in `region_escape` — the partition, checked per
element (§7.5). Then:

- Print escape names (`cell/pin` or port) and count per design.
- Pin the measured gcd set by name: `{_313_/X, _301_/B1, _307_/B1}`
  — the planning probe's finding; all three are the guide-box-
  stops-at-GCell-boundary class. If the implementation names a
  different small set, verify the difference is a real model
  difference, update the pinned set, and record it in results.md —
  never force-fit to the probe.
- picorv32: assert the partition + print; pin the measured set after
  the first run (same convention), record in results.md.
- Escape pins must still appear in `pin_nodes` with an empty list —
  `pin_nodes` covers every member pin of every guided net.

## `test_graph_determinism` (§7.7)

`build_route_graphs` twice on the same inputs → identical `nodes`,
`edges`, `pin_nodes` per net on both designs; nonzero node/edge
totals asserted (a deterministic empty graph proves nothing).

## `test_region_escape_fixture` (synthetic, no real inputs)

LEF/DEF in the `OFFTRACK_*` style: `met1` tracks `X 0 DO 5 STEP
1000` and `Y` same; `u0/A` macro pin rect `[900,1100]²` centered on
crossing (1000,1000); io port `+ LAYER met1` rect `[2900,3100]²`
placed at (3000,3000); net `n1` = `( PIN io ) ( u0 A )`. Guide text
with two met1 boxes: `[0,0,2000,2000]` (covers u0/A) and
`[2500,2500,3500,3500]` (covers io).

1. **Control**: full guide → `region_escape` empty, both pins
   attach, nodes/edges nonzero.
2. **Shrunk box**: replace u0/A's box with `[500,200,1500,800]` —
   covers x-track 1000 but no y-track inside the pin rect →
   `region_escape == {u0/A}` exactly; io stays attached;
   `zero_coverage` stays empty (the io box still covers crossings).
3. **Empty region (fail-proof)**: shrink *both* boxes under the grid
   (e.g. `[500,200,1500,800]` twice) → every member pin escapes and
   `(n1, met1)` lands in `zero_coverage` (boxes exist, no covered
   pair) — census red on two named findings → restore the full
   guide → green. This is the acceptance's "empty region goes red
   and restores green".

## Workspace gates

`cargo test --workspace` and `cargo clippy --workspace --all-targets
-- -D warnings` — record what ran in `docs/results.md`.

## In-module unit tests (`graph.rs` `#[cfg(test)]`)

- region: single box → expected `(ix,iy)` set; two disjoint boxes →
  union without phantom corners; box off-grid → empty set.
- edges: an L-shaped pair set → exact edge list, `axis` tags right;
  a one-index gap in a row → no edge across it.
- join: parsed fixture with an unknown net name → `Err` naming it;
  a `<2`-pin net and a power-members net → `absent_expected`, not
  `missing_signal`.
