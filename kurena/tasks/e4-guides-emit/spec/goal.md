# Goal

E4 ticket 1 (authoritative semantics:
`kurena/tasks/next-stage-scoping/design/`): emit an
OpenROAD-format route-guide file from the routes
`congestion/route.rs` already computes and discards, then
measure the topology criteria (done-when 1–2) on gcd +
picorv32. Guide parasitics (3) and STA e2e (4) are later
tickets — this one lands the writer + comparison harness.

## Work

- **Guide emission**: CLI surface (`grus-eda route` or a
  flag — pick the existing command shape) writing
  `net` / `(` / `xlo ylo xhi yhi layer` / `)` blocks per
  net in the P-GR-probed format (DBU rects + layer names,
  `li1`/`met1` pin-access stubs included), from the
  committed 2D edge paths under the
  `set_routing_layers -signal met1-met5` convention
  (layer assignment for edge paths + pin stubs per
  inventory.md's "Missing" list).
- **Reference**: extend the committed generate script
  (or add one) so OpenROAD `global_route -guide_file`
  emits the guide files for `gcd.placed.def` and
  `picorv32.placed.def` — committed artifacts, the same
  container/pinning as other goldens.
- **Measure** done-when 1–2 on both designs:
  per-net guide-box overlap share (≥0.70 provisional per
  done-when.md) and guide wirelength (total box count
  within 15% — **per-net compared**, not only totals,
  AGENTS.md §7.5). Numbers land in `docs/results.md`.
- **Round-trip proof**: `read_guides` on our emitted file
  inside the container must succeed (and
  `estimate_parasitics -global_routing` on it ideally —
  record which was run); errors are a fail, not a warning.
- **Tests**: deterministic emit (stable box order);
  a test deserializing the committed OpenROAD guide files
  and asserting the metrics — fails loudly, E0-discipline.

## Files

- `crates/grus-core/src/congestion/{route,gcell}.rs` +
  new guide writer module (and a small guide reader if
  the test needs to parse reference boxes)
- `crates/grus-eda/src/main.rs` (CLI)
- `tests/designs/{gcd,picorv32}/` (guide reference
  artifacts + generate scripts)
- `docs/results.md`, `docs/roadmap.md`, `docs/e4-*.md`
  if a stage doc is started
- `Cargo.toml` (patch/minor bump — new feature)
