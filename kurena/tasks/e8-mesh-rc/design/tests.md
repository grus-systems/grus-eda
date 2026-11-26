# e8-mesh-rc — Test plan

Tests live in `pdn/mesh.rs`'s `#[cfg(test)]` module, reusing
`pdn.rs`'s test fixtures (`pdn_design()`, `load_design()`) via
`use super::tests`-style sharing or by hoisting the fixture
constructors — implementation detail; the fixture LEF gains
`RESISTANCE RPERSQ` lines on `met1`/`met2` (api.md). Design-file
tests fail loudly when inputs are missing (`require()`).

## Synthetic fixture — `pdn_design()`

met1 rail `(0,-50)-(2000,50)` (major X, W=100), met2 stripe
`(950,0)-(1050,2000)` (major Y, W=100), one `stack_a` via at
`(1000,0)` (cut `via12`, `RESISTANCE 15.0`, `cut_count 1`), pins
u1 `[0,200]` / u2 `[500,700]` on the rail, PIN io rect on the
stripe `[1750,1850]` — all derived from the existing fixture,
values asserted exactly:

- **`test_synthetic_mesh_shape`**: `nodes == 7` — met1 pieces
  `[0,200]` / `[500,700]` / `{1000}` / `{2000}` (4), met2 pieces
  `{0}` / `[1750,1850]` / `{2000}` (3). `wire_resistors == 5`
  (three met1 runs L = 500/400/1000, two met2 runs L = 1800/200),
  `via_resistors == 1`, `sinks == 2`, `sources == 1`,
  `validate().is_empty()`.
- **`test_synthetic_wire_r_arithmetic`**: each wire `r_ohms ==
  res_per_sq_metK * L / 100` for the five runs — per-element
  comparison (lesson 5: per element, not only totals), including
  the center-to-center `L` derivation, not just "all R > 0".
- **`test_synthetic_via_r_from_techdb`**: the single via resistor
  equals `15.0 / cut_count`, endpoints = the met1 `{1000}` node
  and the met2 `{0}` node. The fixture's via is named `stack_a` —
  a non-descriptive name resolving purely through the LEF `VIA`
  block — **this is the acceptance's renamed-via proof** for the
  mesh path (name parsing is not load-bearing). A second arm may
  re-spell the via name in the DEF to nonsense and assert the same
  R, matching ticket-2's `renamed_via_still_resolves` spirit.
- **`test_synthetic_dangling_via`**: move the via point off the
  met1 rail (`y_dbu = 1000`, as
  `test_failproof_via_edges_carry_connectivity` does — it stays on
  the stripe) → a `dangling` node exists on the met1 side and
  `validate()` reports it as `UnreachableNode`.

## Real designs

- **`test_gcd_mesh_census`**: `load_design("gcd", "gcd.placed.def")`;
  for VPWR + VGND assert: `sinks == 384` (== the reference embedded
  CSV row count read from `gcd.irdrop.reference.json`
  `conventions.embedded.csv_rows` — deserialize, don't hand-type),
  `sources >= 1`, `via_resistors == 46` (== `net.vias.len()`),
  `validate().is_empty()`, sink `PinId` set **equals** the graph's
  distinct `attachments` pin set (the acceptance's "sink set ==
  ticket-2 attach set"), all `r_ohms` finite and `> 0`. Print the
  full `MeshCensus` with `--nocapture` for `docs/results.md`.
- **`test_gcd_orient_fixture_mesh`**: run `build_mesh` on
  `gcd.orient.def`; assert every sink resolves to a valid node and
  violations contain no `DanglingSink` — same bounded claim as
  ticket-2's orient test (attach verdicts partition, no golden
  counts).
- **`test_picorv32_mesh_census`** (`require`-gated): build both
  nets, assert `sinks == 10,205` (from the reference JSON),
  `validate().is_empty()`, record the census + wall-clock for
  `docs/results.md` (scale check, no gate on counts).

## Fail-proof ritual (acceptance-mandated)

Unit-level, plus one observed break on the real test:

1. `test_failproof_zero_res_per_sq`: clone the fixture `TechDb`,
   set `routing_layers[met1].res_per_sq = 0.0` → `validate()`
   returns `NonPositiveResistance` entries for every met1 run.
   Restore → green.
2. `test_failproof_removed_vias`: clone the fixture `PdnNet`,
   `vias.clear()` → the met2 source side is stranded; `validate()`
   reports `UnreachableNode`s covering the source node (and with
   sources+geometry the sinks' side inverts on gcd — either way
   non-empty violations).
3. **Observed break on the real test**: during development, zero
   `met1`'s `res_per_sq` in `load_tech_db`'s output path (or drop
   via resistors in `build_mesh`) once, run
   `test_gcd_mesh_census`, observe it red, restore. Record the
   observation in `docs/results.md` (project rule 7 — prove the
   test can fail; the ticket-2 entry is the format precedent).

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p grus-core --lib pdn -- --nocapture   # census prints
```

No reference-tool run is needed — there is no PDNSim mesh surface
to compare against (overview.md); the numbers are a *recorded
census*, and `docs/results.md` says that explicitly.

## Files

- `crates/grus-core/src/pdn.rs` (fixture LEF `RESISTANCE` lines)
- `crates/grus-core/src/pdn/mesh.rs` (all mesh tests)
- `tests/designs/` (read-only: `gcd.placed.def`, `gcd.orient.def`,
  `picorv32.placed.def`, `*.irdrop.reference.json`)
- `docs/results.md`
