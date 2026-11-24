# e6-drt-emit — Structural census vs reference

Informational comparison against `<d>.drt.reference.json` — signed
deltas, never pass/fail. Topology differences vs TritonRoute are
expected (different router, same guides); the DRC bar is the only
gate.

## Quantities

- **`wire_length_dbu`** (`EmitReport`): Σ Manhattan length over
  emitted `WireSeg`s — min-area patch wires are ordinary `WireSeg`s
  and join this total like any other segment (the re-run's
  wirelength delta vs the committed baseline includes them — name
  that in the results delta). The same centerline accounting
  `routed_def_parse` applies to `routed.def` when it produces
  `total_wirelength_um` in the reference JSON. µm conversion:
  `db.dbu_per_micron` (the DEF `UNITS DISTANCE MICRONS` value,
  1000 on sky130hd) — state the divisor source next to the number.
- **`area_patches` / `area_residuals`** (`RouteDb`): the patch
  census — components brought to bound, and the named leftovers
  (net, layer, bbox, area, bound) the gate can still fire on.
- **`vias_per_cut`** (`EmitReport`): placed vias grouped by cut-layer
  name (`tech.strings` resolve of `pairs[vp.lower].cut`) — joins the
  reference's `vias_per_cut_layer`, which `routed_def_parse` builds by
  re-mapping emitted via names through `via_names_meaning`. Our key
  space is the cut layer directly — same buckets, no name lookup.
- **routed / unrouted / projected**: from `EmitReport` + `RouteDb` —
  context lines, not comparison targets (the reference has no
  per-net residual data).

## Reference keys

`<d>.drt.reference.json` → `variants`:

| ours | reference |
|---|---|
| `wire_length_dbu / dbu_per_micron` | `variants.delegated.wirelength_um`, `variants.baseline.wirelength_um` |
| `vias_per_cut[cut]` | `variants.delegated.vias_per_cut_layer[cut]` (re-keyed map via the `ref_via_census` helper pattern in `tests/drt.rs`), same for `baseline` |

`delegated` is the primary comparison — it routed under the identical
committed guides; `baseline` (TritonRoute self-guided) is context.

## Output shape

CLI print block (after the emit lines):

```
== drt emit: gcd ==
routed=269 unrouted=0 projected=17
wirelength_um ours=<x.xx> delegated=<y> delta=<±z> baseline=<b>
via.via    ours=<n> delegated=<n> delta=<±d>
via.via2   …
via.via3   …
via.via4   …
via.via5   …
```

- Signed deltas formatted `+`/`-` explicitly — "informational delta,
  sign recorded" (acceptance).
- Zero-count cut layers print as `0` — never elided (a missing cut
  layer vs a present-zero reads differently in the results table).
- `--reference` absent → `delegated`/`baseline`/`delta` columns print
  `-`.

The test-side census (`test_emit_census_*`) prints the same block via
`--nocapture` and asserts only the non-vacuity properties: totals
nonzero, every emitted cut layer is a real tech cut layer, and the
reference JSON deserializes with all five cut names present.
Per AGENTS §7.5, integer counts print exact — if ours and delegated
land within a couple vias of each other that is noted, not hidden.

## `results.md` table

The dated entry carries one table per design:

| metric | ours | delegated | baseline | delta vs delegated |
|---|---|---|---|---|
| wirelength (µm) | … | … | … | ±… |
| vias: via/via2/…/via5 | n/n/n/n/n | … | … | ±… |

plus the unrouted-net count + named classes (picorv32's residual —
108 nets = 43 met3-bridge + 65 met2-connected under the old
occupancy marking; cite the maze entry rather than re-enumerating
names, and re-record the fresh count the emit leg measures — see
overview.md's drift note) and the runtime line (emit wall clock is
trivial; total `route_nets` time re-measures in this leg — record
it verbatim).

## Files

- `crates/grus-core/src/drt/emit.rs` (EmitReport)
- `crates/grus-eda/src/main.rs` (print block + `--reference`)
- `crates/grus-core/tests/drt.rs` (census test)
- `docs/results.md`
