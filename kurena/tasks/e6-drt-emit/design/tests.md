# e6-drt-emit — Test design

All tests live in `crates/grus-core/tests/drt.rs`, reusing the file's
existing helpers (`load`, `load_guide`, `design_dir`, `require`,
`route_graphs` builder). New tests extend `mod maze` or a sibling
`mod emit` — prefer a sibling module to keep the maze module's scope
statement accurate.

Emitted DEFs land in `target/` temp paths (`std::env::temp_dir` or
`cargo`'s `OUT_DIR`-style scratch — use a per-test tempdir via
`std::fs` + process-unique name), never into `tests/designs/`: our
routed DEFs are generated artifacts, not committed goldens (the
committed `<d>.drt.ours.routed.def` arrives in `e6-drt-e2e` via its
own generation script).

## Test list

### `test_emit_roundtrip_gcd` — the byte-stable gate

1. Route gcd (`route_nets`, default `MazeConfig`), `apply_routes`,
   `write_routed_def` → `out1.def`.
2. `parse_def_bytes(out1)` → `db2` — must not error (grammar proof:
   layer names, via names, conn tokens all resolve).
3. `db2.net_geoms == db.net_geoms` on every guided net — rects and
   `PdnVia`s identical (the `PartialEq` assert already used by
   `test_routed_def_net_geoms_reparse` in `tests/drc.rs`).
4. `write_routed_def(&db2, …, out1 → out2)` → `out2` bytes ==
   `out1` bytes, compared with `std::fs::read` + `assert_eq!` — the
   acceptance's "write → parse → write → identical".
5. Non-vacuity: `routed == guided`, `wire_rects` total > 0, `vias`
   total > 0, `unrouted` empty on gcd.

### `test_emit_selfdrc_gcd` — the acceptance gate

`out1` → `parse` → `assemble` → `run_all` → assert all 50
`RuleOutcome.findings` empty; on failure the assert message dumps
`rule=id findings=n` per firing rule plus `{:?}` of each finding
(existing tests' idiom). Census print: every rule's `candidates`
count — assert > 0 on the families TritonRoute's DEF exercises
(width/spacing/enclosure/area). Double-run `run_all` → identical
report.

### `test_emit_selfdrc_picorv32` — `#[ignore]`

Same gate on picorv32. Ignored for wall clock (`route_nets` ≈ 135
min release + full-DEF DRC); run detached with `--nocapture` for the
`results.md` evidence. Unrouted nets carry their named class — the
assert allows the gate to pass *with* the 65 met3-bridge conns-only
records (they contribute no geometry) but never silently: print each
unrouted name.

### `test_emit_fail_proof` — deliberate red

Arm A (injected): post-`apply_routes`, find two different-net
wire_rects on one layer separated by ≥ the layer's spacing bound —
or simpler and fully deterministic: extend `net_geoms[i].wire_rects
[j].xh` by `bound` onto a known neighboring foreign rect — write →
parse → `run_all` → assert `findings ≥ 1` and at least one finding's
`rule` is that layer's spacing rule. Arm B (`occ: Disabled`):
route+emit on gcd → gate → assert `findings ≥ 1` (maze already
proves 6,429 track collisions; this arm proves they reach the
checker through our emitted text).

### `test_emit_census` — reference join (gcd + picorv32 route)

Print the census block per census.md; asserts: `vias_per_cut` keys
resolve to real cut layers, totals > 0, `drt.reference.json`
deserializes with `delegated`/`baseline` present and all five cut
names mapped. Deltas print signed — never asserted. gcd leg in the
default suite; picorv32 leg shares the `#[ignore]` gate test's
output (one `#[ignore]` test producing both gate + census, to avoid
paying `route_nets` twice).

### `test_emit_determinism`

Two full `route_nets` + emit passes on gcd → `RouteDb`s equal,
`out` bytes equal. picorv32 leg folded into the `#[ignore]` test
(same reasoning).

### `test_emit_unrouted_fixture` — synthetic

Small fixture (reuse `mod maze`'s fixture-builder style): one
routed net + one forced-`NoPath` net → emit → assert the unrouted
net's record contains its conns + `+ USE SIGNAL` and **no**
`ROUTED`/`NEW`, `EmitReport.unrouted` names it, and the written DEF
re-parses. Covers the picorv32 shape without picorv32's cost.

### `test_emit_via_default_consistency`

For every emitted `PdnVia`: `lower == pairs[..].lower`,
`geom` == the table candidate's `geom` (`PartialEq` on `ViaGeom`),
and `resolve_def_via`-equivalence: the emitted name resolves to the
same `(lower, upper, cut, cut_count)` on re-parse — asserts emit
never reselects a different via than routing priced.

### CLI smoke

`Cli::try_parse_from(&["grus-eda", "drt", "--tlef", …])` parses to
`Commands::Drt` with all fields bound (existing `test_parse_drc`/
`test_parse_route` idiom). End-to-end `Run` invocation is covered
by the core tests' equivalent sequence — no separate binary test
(the CLI arm is thin).

## Existing tests that must stay green

- `test_via_defaults` — emitted names are exactly the default names
  it census-prints.
- `test_routed_def_net_geoms_reparse`, the `run_all` control in
  `tests/drc.rs` — the emit path does not touch the parser.
- All `mod maze` tests — emit reads `RouteDb`/`ViaTable` read-only.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs` (parse test only)
