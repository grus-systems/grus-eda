# e4-guides-emit — Overview

E4 ticket 1 of the sketch in
`kurena/tasks/next-stage-scoping/design/order.md`: emit an OpenROAD-format
route-guide file from the 2D edge paths `congestion/route.rs` already commits
and discards, then measure done-when 1–2 (per-net guide-box overlap ≥ 0.70,
total guide-box count within 15% per-net compared) on `gcd.placed.def` and
`picorv32.placed.def` against committed `global_route -guide_file` references.
Guide parasitics (criterion 3) and STA e2e (criterion 4) are later tickets;
this one lands the writer, the references, and the comparison harness.

## Key decisions

- **CLI shape: new `grus-eda route` subcommand**, not a flag on `congestion`.
  The command table maps one subcommand per stage (`check-e0`, `congestion`
  E1, `place` E2, `sta` E3, `power` N7); E4 is a new stage, and `route`'s
  output artifact is the guide file, so a sibling subcommand keeps E1's
  estimator surface untouched. `route` runs the identical
  grid → decompose → route pipeline and additionally writes
  `--guide-out PATH` in the P-GR format.
- **Router return**: `route_segments` stops discarding `segment_paths`; it
  returns them next to the `CongestionReport`. The flat `segments`
  ordering is preserved so paths stay index-aligned.
- **Net mapping**: `decompose_design_nets` keeps its flat signature; a new
  indexed variant returns `(segments, net_seg_offsets)` and the flat one
  delegates to it (shared parallel path, no duplicated logic, no shim).
- **New module** `crates/grus-core/src/congestion/guide.rs` holds the guide
  model (`GuideBox`, `NetGuide`), the emitter (`write_guides`), and a small
  `parse_guides` reader the comparison test uses on the committed OpenROAD
  `.guide` files. Emission semantics are specced in `semantics.md` and must
  land in repo docs as `docs/e4-guides.md` (AGENTS.md §7.6: the rule in
  `docs/` first, then the code).
- **Layer assignment is a documented convention, not a 3D router**: H edges
  → `met1`, V edges → `met2` (the lowest preferred-direction signal layers
  under `set_routing_layers -signal met1-met5`), corner GCells land on both
  layers naturally via strip overlap, and pins get access stubs on their
  port layer + `met1`. Clock nets are not banded (`met3-met5` clock band is
  not modeled — no SDC is loaded); recorded as an approximation.
- **I/O pin layers**: the DEF `PINS` `+ LAYER` name is parsed today and
  dropped (`readers/def.rs` `add_pin`); a new
  `DesignDb.pin_io_layer: Vec<Option<LayerId>>` carries it so I/O stubs land
  on `met2`/`met3` like the reference's.
- **References**: `gcd.guide` and `picorv32.guide` committed under
  `tests/designs/<d>/`, emitted by a new step appended to each
  `generate_ref.sh` in the pinned image with the same layer adjustments our
  grid applies (`met1 0.30`, `met2-met5 0.20`).
- **`picorv32.guide` size**: at ~6.8k signal nets it will exceed the
  `AGENTS.md` 1 MB golden guideline (≈1.5–2 MB). The acceptance requires a
  committed reference, and the geometry is what the overlap test asserts
  against, so it commits anyway; the deviation is recorded in
  `docs/results.md`.

## Files

Implementation is expected to touch:

- `crates/grus-core/src/congestion/mod.rs`
- `crates/grus-core/src/congestion/route.rs`
- `crates/grus-core/src/congestion/rsmt.rs`
- `crates/grus-core/src/congestion/guide.rs` (new)
- `crates/grus-core/src/design_db.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-eda/src/main.rs`
- `tests/designs/gcd/generate_ref.sh`
- `tests/designs/gcd/gcd.guide` (new, generated)
- `tests/designs/gcd/check_guide_readback.sh` (new)
- `tests/designs/picorv32/generate_ref.sh`
- `tests/designs/picorv32/picorv32.guide` (new, generated)
- `tests/designs/README.md`
- `docs/e4-guides.md` (new)
- `docs/results.md`
- `docs/roadmap.md`
- `docs/status.md`
- `Cargo.toml` (`[workspace.package] version` `0.3.8` → `0.3.9`; `Cargo.lock`
  refresh is incidental)
