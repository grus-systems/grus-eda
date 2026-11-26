# Island semantics — `NetRcTree`, `write_spef`, `spef_net_tree`

## Measured reference behavior (evidence: `tests/designs/gcd/gcd.guide.spef`, net `_132_`)

- `*CAP` lists **all** route nodes including island nodes (104 nodes; several
  with `0` caps). `*D_NET` = Σ node caps + pin-wire halves — unchanged
  whether a node is reachable or not.
- `*RES` lists the island's **internal** wire/via links as real resistors
  (e.g. `_132_:8 _132_:9 0.0093`, `_132_:9 _132_:10 0.00616071`), pin
  resistors landing on island nodes as normal resistors
  (`_378_:A2 _132_:2 0.0113491`), and self-loops `n n r` **only** for
  unpaired zero-length stub segments (`_132_:7 _132_:7 0.0093` mcon,
  `_132_:40 _132_:40 0.0045` via1, `_132_:37 _132_:37 0.00341` via2).
- There is **no** resistor joining a disconnected component to the driver's
  component. OpenSTA accepts such nets: unreachable pins get no wire delay
  and island caps do not load the driver's path (`docs/results.md`
  attribution, confirmed by the `guide_spef` golden endpoints).

## `NetRcTree` representation

Keep the BFS-ordered arrays; add:

- `dead: Vec<bool>` — parallel to `parent`, `false` for nodes of the
  driver's component, `true` for every node of a component the driver's BFS
  cannot reach. Built trees (`build_net_tree`, placement path) fill it with
  `false` — placement trees are always connected.
- `self_loops: Vec<(u32, f32)>` — `(node, kohm)` entries for unpaired
  zero-length stub segments so `write_spef` can emit the `n n r` resistors
  `RouteGraph::edge` currently drops silently. `RouteGraph` records them
  during segment ingestion (a via-segment with `l0 == l1`): collect
  `g.self_loops.push((a, r))` instead of skipping.
- Dead-component roots: `parent[v] == v as u32` and `edge_r_kohm[v] = 0.0`
  (a deliberate sentinel; node 0 keeps `parent[0] == 0` and is the only
  legitimate root). Island children keep real parents/edge R inside their
  component.

`guide_net_tree` BFS tail becomes: while unreached nodes remain, pick the
lowest-index unreached node as the component's dead root (deterministic),
BFS its component appending nodes with `dead = true`; **remove the
`SPEF_STUB_RES_KOHM` bridge** (`nearest-tree-node` link). The
`nearest_route_node`-restricted-to-`reached` fallback for pin-attach misses
stays (documented `EST-0026` mirror).

`spef_net_tree` (reader) applies the identical convention: nodes the
driver BFS cannot reach are appended as dead subtrees instead of making
`pin_node` bail (`"not connected to the driver"`). Side effect:
`spef_parasitics` then accepts OpenROAD's own `gcd.guide.spef` (the `_057_`
island-pin bail recorded in `docs/results.md` disappears — record the
change honestly).

## Engine semantics

- `elmore_ns`: pins whose `pin_node` is on a dead node return **0.0** —
  OpenSTA gives island pins no wire delay. Guard the accumulation loops
  with `if parent[v] != v` so dead roots don't self-accumulate; dead
  children may accumulate internally (values never read for reachable
  pins), or skip `dead[v]` entirely — pick the simplest correct form.
- `pi_model`: nothing flows from a dead subtree to node 0 (the dead root's
  parent is itself), so island wire caps and island pin caps are excluded
  from the driver's moments automatically once the bridge is gone.
- **Resolved — island sink pin cap loading.** `report_dcalc` probes on
  the guide SPEFs show OpenSTA's answer is not per-pin but per-net: when
  the net stays pi/elmore, dead pins' caps do not load the driver; when
  OpenSTA takes its **lumped fallback** the driver is looked up at the
  pin-capacitance sum of *all* pins and every pin gets zero wire delay.
  The lumped condition, as probed: a dead `*P` port always forces it;
  dead `*I` pins alone trigger it once the island carries roughly a
  third of the wire capacitance. `NetRcTree::lumped` implements
  `dead_port || dead_cap >= 0.32 * total` — the band is inconsistent
  between structurally identical nets, so this is a documented
  approximation, not a replica of OpenSTA's internal criterion.
  `dead_wire_cap_pf` is kept alongside `total_wire_cap_pf` so
  `net_load` charges only live wire cap on non-lumped nets.
- `total_wire_cap_pf` = Σ `wire_cap_pf` over **all** nodes including dead —
  per-net `*D_NET` and the e4-2 cap gate are unchanged by construction.

## `write_spef` emission

- `*CAP`: unchanged — every tree node (dead included) plus `0`-cap pin
  stubs, so `Σ*CAP == total_wire_cap_pf` still round-trips.
- `*RES`: emit `parent[v] → v` for every `v` where `parent[v] != v` —
  real resistors inside island components are preserved; dead roots emit
  nothing (the removed bridge). Append `(name, name, r)` self-loops from
  `self_loops` after the tree edges — the `n n 0.00x` pattern OpenROAD
  writes for unpaired stubs.
- `*D_NET`/`*CONN`: unchanged.

## Constraints kept

- `test_guide_parasitics_spef_round_trip` and the `sta --spef` gate re-read
  our own output: reader and builder share the same `dead` convention, so
  Elmore/cap parity holds by construction.
- `RouteGraph` must not silently drop self-loops anymore; everything else
  (caps, adjacency, `updateVias`) untouched.
