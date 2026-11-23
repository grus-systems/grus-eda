# e6-drt-via: layer-transition (via) edges

E6 ticket 4, depends on `e6-drt-graph`. Adds the via model: which
LEF/DEF via may connect two adjacent routing layers at a track
crossing, and which via the router picks.

## Scope

1. **Via candidate table** — for each adjacent routing-layer pair
   (l, l+1) enumerate the LEF `VIA` defs (from `TechDb` — already
   parsed with per-layer rects and the cut layer) whose (bottom,
   cut, top) layer triple matches, plus DEF `VIAS` entries that
   expand to the same triple. The table records each via's metal
   rects so later DRC-aware costs can price enclosure.
2. **Via edges in the graph** — a via edge exists at node
   (l, ix, iy) → (l+1, ix, iy) when (x,y) is a track crossing on
   BOTH layers (`x ∈ tracks_x(l) ∧ y ∈ tracks_y(l)` and same for
   l+1) — tracks on non-preferred axes are still physical grid
   lines the via can sit on.
3. **Via selection policy** — record the default choice per layer
   pair (the reference DEF's `vias_per_cut_layer` census in
   `<d>.drt.reference.json` shows which via ids TritonRoute picks —
   `M1M2_PR` etc.); prefer the smallest-area candidate or the
   reference's majority pick, and record the rule in docs. This is
   a policy statement, not yet a DRC-driven choice.
4. **Census** — per layer pair: number of candidate vias, number of
   graph nodes where a via edge exists on gcd/picorv32 regions; the
   reference `vias_per_cut_layer` names are joined so the choice
   policy is verified against the deck's own output.

## Non-goals

- Maze search (next ticket), DRC-aware via costing, via stacking
  beyond one cut layer (multi-hop transitions compose at the node).
- DEF `VIAS` FIXED viarule parameterization beyond the cut triple
  match (the generated-via expansion in `drc::geom` already handles
  the rect form — reuse it).

## Files

- `crates/grus-core/src/drt/via.rs` (new — candidate table + edges)
- `crates/grus-core/src/drt/graph.rs` (via-edge hookup)
- `crates/grus-core/tests/drt.rs` (via census gate)
- `docs/e6-drt.md` (via section)
- `docs/results.md` (dated entry)
