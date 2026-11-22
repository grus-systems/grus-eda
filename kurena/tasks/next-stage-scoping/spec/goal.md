# Goal

Scoping ticket (doc-only, no implementation): map the
remaining pipeline from where grus-eda is today to a
usable flow, define **measurable done-when criteria** per
candidate stage against the reference tool, order them by
dependency, and estimate effort. Deliverable is a design
doc + updated `docs/roadmap.md` — same discipline as the
activity-propagation measurement ticket (Branch P).

Context: E0–E3 + N7 power all pass; the repo covers
readers → placement → STA/power. Missing for a real flow:
CTS, global routing, detailed routing, DRC, IR drop.
What already exists that biases the order:

- E1 congestion already computes per-net RSMT
  decomposition, GCell usage maps, and 0/1-overflow L/Z
  pattern routing (`crates/grus-core/src/congestion/`) —
  the same machinery OpenROAD's `global_route` builds on.
- STA reads routed SPEF and reports slack — a routed
  result has a downstream consumer already.
- No CTS, no route-guide/GRoute output, no track-level
  work, no DRC.

## Work

1. **Inventory the gap**: per candidate stage
   (E4 global routing, CTS, E5 detailed routing, DRC,
   IR drop — naming the author's choice), what
   inputs/outputs it has, what repo code it can reuse
   (name modules), what OpenROAD/OpenSTA command
   produces its reference output.
2. **Done-when per stage**, measurable against the
   reference on the committed designs: e.g. global
   routing could be guide-coverage / GRoute-vs-OpenROAD
   overlap + routed-DEF parasitics agreement; CTS could
   be insertion-delay/skew bounds vs OpenROAD CTS
   reports; be concrete (metric, tolerance, fixture) —
   not prose goals.
3. **Dependency order + ticket sketch**: the recommended
   sequence with rough sizing (reuse level per stage is
   the main lever — E4 reuses the most code).
4. `docs/roadmap.md` gets the new stage list; the design
   doc lives under `kurena/tasks/next-stage-scoping/`.

## Files

- `kurena/tasks/next-stage-scoping/` (design docs)
- `docs/roadmap.md`
- `docs/results.md` only if any measurement is run to
  size a criterion (e.g. probing what OpenROAD emits for
  a stage — a probe is allowed and encouraged for
  done-when precision, but keep it small)
