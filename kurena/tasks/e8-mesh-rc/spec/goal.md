# e8-mesh-rc — PDN resistor network from extracted geometry

E8 ticket 3 (after `e8-pdn-parse`). Turns the per-net PDN geometry
(wire rects, via stacks, pin attachments, source pin) into the resistor
network the DC solver consumes.

## Scope

1. Node extraction: same-layer wire rects that touch/overlap merge into
   one node. Each node carries its layer, bounding box(es), and net.
2. Wire resistance between adjacent node centers (or intersection
   points): `R = RPERSQ × L / W` using the tech-LEF per-layer value
   already in `TechDb` (`res_per_sq`/`r_per_sq` field used by guide
   parasitics — reuse the same source, no second table).
3. Via resistance between stacked nodes: `via_res_ohm_above[cut_index]`
   divided by the parallel cut count of the via. Cut count comes from
   the LEF `VIA` definition's RECT count on the cut layer (parsed in
   ticket 2) — not from name parsing; record name-parsed count only as
   a sanity cross-check.
4. Current-source nodes: every attached instance power pin injects
   `I = P_inst / V_supply` at its attach node (per-instance power
   values are wired in by `e8-solve`; this ticket defines the node and
   the attach map).
5. Voltage-source nodes: the special net's top-level `PIN` (e.g.
   `PIN VPWR`) is the ideal source — PDNSim's default regime places the
   supply at the net's source locations; our mirror is the top-level
   PIN geometry intersecting the PDN. Record the source-node set
   verbatim; if the PIN rect touches multiple disjoint nodes they are
   all source nodes (same ideal voltage).
6. Output `PdnMesh`: `nodes` (layer, bbox, net, role:
   wire/source/current-sink), `resistors` (node pair, R_ohms, kind:
   wire/via), `sinks` (instance pin → node), `sources` (node list).
   Plus a census print per net.

## Sanity gates

- Every resistor R is finite and > 0.
- Every sink/source resolves to exactly one node; dangling sinks are
  listed (must be empty on gcd per ticket-2 connectivity).
- Mesh node count ≤ a recorded bound (PDNSim granularity is unknown;
  ours is per-distinct-rect-run — record actual counts, no tuning).
- `GND` net mesh exists identically (bounce analysis mirrors drop).

## Files

- `crates/grus-core/src/pdn.rs` or `src/pdn/mesh.rs` (build `PdnMesh`
  from ticket-2 `PdnNet`s + `TechDb` resistances)
- `crates/grus-core/src/design_db.rs` (mesh storage or builder input)
- `docs/results.md` entry (node/resistor/sink/source census per net,
  gcd + picorv32)
