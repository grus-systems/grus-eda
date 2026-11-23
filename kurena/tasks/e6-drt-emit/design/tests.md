# e6-drt-emit — Test design

All emit tests live in `crates/grus-core/tests/drt.rs` in the emit
section after `mod maze` (sibling scope, shared helpers `emit_leg`,
`self_drc`, `print_emit_census`, `emit_gate`, `tmp_def`,
`placed_def`, `geom_totals`). Emitted DEFs land in
`std::env::temp_dir()` per-process paths — generated artifacts,
never committed (the committed `<d>.drt.ours.routed.def` is
`e6-drt-e2e`'s artifact).

## Landed test list (WIP `dc8cdb4`)

- `test_emit_roundtrip_gcd` — the byte-stable gate: route →
  `apply_routes` → `write_routed_def(out1)` → `parse_def(out1)` →
  `net_geoms` equality → `write_routed_def(db2, out1 → out2)` →
  `out1`/`out2` byte-equal. Non-vacuity asserts on routed count,
  wire/via totals, empty `unrouted` on gcd.
- `test_emit_selfdrc_gcd` — the acceptance gate via `emit_gate`:
  write → re-parse → `assemble` → `run_all`, all 50 rules'
  `findings` empty; non-vacuity per-family `candidates > 0`;
  `run_all` double-run identical; census print via
  `print_emit_census`.
- `test_emit_selfdrc_picorv32` — same gate, **`#[ignore]`d** for
  wall clock (`route_nets` ≈ 135 min release). Runs detached for
  the `results.md` evidence.
- `test_emit_fail_proof` — arm A injected positive sub-bound gap →
  `Family::Spacing` finding; arm B `OccMode::Disabled` → findings
  ≥ 1. (Occupancy marks are still painted under `Disabled` —
  `premark_obs` is unconditional — but the `Enabled`-only verdict
  check never consults them, so collisions still reach the emitted
  text.)
- `test_emit_determinism` — two full gcd route+emit passes:
  `RouteDb` equality, report equality, `net_geoms` equality,
  emitted-byte equality.
- `test_emit_unrouted_fixture` — synthetic `Unattachable` net emits
  a conns-only record (no `ROUTED`/`NEW`) that re-parses.
- `test_emit_via_default_consistency` — every emitted `PdnVia`
  equals its pair's default candidate verbatim.

## Remaining test work (this revision)

1. **Hoist the byte-stable check into `emit_gate`** — today only
   the gcd roundtrip test does write → parse → write → identical.
   The acceptance words the roundtrip over *both* designs, and the
   marginal cost inside the picorv32 leg is one extra write+read
   (seconds vs the 135-min route). Add to `emit_gate` after
   `self_drc(out, …)`:

   ```rust
   let out2 = tmp_def(&format!("{name}-gate2"));
   write_routed_def(&db2, &tech, &out, &out2).expect("re-emit");
   assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&out2).unwrap(),
              "{name}: write → parse → write is not byte-stable");
   ```

   `test_emit_roundtrip_gcd` keeps its own explicit check (it also
   asserts `net_geoms` equality pre-write) — no dedup needed.

2. **CLI parse test** — `Drt` arm has no `Cli::try_parse_from`
   coverage yet (the original plan listed one). Add a short assert
   in `main.rs`'s `mod tests` matching the
   `cli_parsers_reject_out_of_range_and_nan` idiom:
   `grus-eda drt --tlef a --lef b --def c --guide g --out-def o`
   parses to `Commands::Drt` (plus `--self-drc`, `--occ disabled`,
   `--reference`, `--max-iters` binding). ~15 lines.

3. **Hard constraint — the `#[ignore]` boundary does not move.**
   `test_emit_selfdrc_picorv32` stays ignored, and no new
   picorv32-input leg may be added to the default set (incl.
   via a shared helper called from a non-ignored test). Grep-check
   before verify: `#[ignore]` must directly precede every test
   that reaches `emit_leg("picorv32"`/`emit_gate("picorv32"`.

## Existing tests that must stay green

- `test_maze_*` — the WIP's `maze.rs` changes (PDN/io-port premark,
  projected stubs) shift route digests; asserts are count-agnostic
  so they should hold, but a pinned-number failure is a finding to
  investigate, not silently re-pin.
- `test_via_defaults`, `test_routed_def_net_geoms_reparse`, the
  `tests/drc.rs` `run_all` control — emit touches neither the
  parser nor the via table.
- `test_maze_occupancy` — `Disabled` arm still paints-but-ignores
  marks, so collisions still occur; only digest/census difference
  is asserted.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
