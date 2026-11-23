# e6-drt-emit — Overview

E6 ticket — routed DEF emission + self-check. Converts the maze
router's `RouteDb` (per-net `WireSeg`/`ViaPlace` lists) into DEF
`NETS` geometry, adds the `grus-eda drt` CLI surface that replaces
the delegation path (placed DEF + guide → routed DEF), and gates
the result on our own 50-rule subset checker. Depends on
`e6-drt-maze` (`v0.12.4`); the full Magic-deck check on our DEF
belongs to `e6-drt-e2e` and is explicitly out of scope here.

## State at this revision (WIP `dc8cdb4`)

The implementation **landed and compiles clean** — this revision of
the design is a close-out plan, not a build plan:

- `crates/grus-core/src/drt/emit.rs` (313 lines) — `apply_routes`,
  `write_routed_def`, `EmitReport` exactly per api.md/def-model.md.
- `grus-eda drt` CLI — `DrtArgs` + run arm in `main.rs`
  (`--tlef --lef --def --guide --out-def`, `--reference`,
  `--self-drc`, `--occ`, `--max-iters`), census print block.
- `tests/drt.rs` emit suite (~420 lines) — `emit_leg`/`emit_gate`
  helpers + `test_emit_roundtrip_gcd`, `test_emit_selfdrc_gcd`,
  `test_emit_selfdrc_picorv32` (**`#[ignore]`d**),
  `test_emit_fail_proof` (both arms), `test_emit_determinism`,
  `test_emit_unrouted_fixture`, `test_emit_via_default_consistency`.
- `drt/maze.rs` emit-driven fixes (already in the same WIP):
  - `premark_obs` now rasterizes every PDN `SPECIALNETS` wire rect
    and via-stack metal as `Obs(u32::MAX)` — signal routes must
    hold the `w/2 + s` band off the power grid.
  - io-port rects mark `Pin(own_net)` like instance pins — priced
    band, never a hard block (the gcd `met3.2` `req_msg`/`resp_msg`
    spacing class was a real finding this fixes).
  - `route_one_net` paints the projected-pin access stub closing
    the gap between `project_attach`'s node and the pin rect.

**Not yet done** (the rejection memo's list, i.e. this design):

1. Run the emit gate end to end — gcd legs pass in-suite;
   `test_emit_selfdrc_picorv32` stays `#[ignore]`d and runs
   detached for evidence. **No picorv32 leg may enter the default
   test set** — the maze route alone is ~135 min release; a
   non-ignored picorv32 leg timed the last verify run out.
2. `docs/results.md` dated entry with measured emit census numbers
   + the self-DRC verdict on both designs.
3. `docs/e6-drt.md` §8 emission section + `docs/status.md` /
   `docs/roadmap.md` updates.
4. `cargo test --workspace` + `cargo clippy --workspace
   --all-targets -- -D warnings`, version bump `0.12.4 → 0.12.5`,
   commit.

## Pipeline (as landed)

```
placed DEF + <d>.guide
  → load_congestion_inputs (tlef/lef/def → TechDb + DesignDb)
  → parse_guides → join_guides → build_route_graphs_ext(…, 1)
  → via_table → route_nets(MazeConfig)                  [e6-drt-maze]
  → emit::apply_routes (RouteDb → db.net_geoms)         [landed]
  → emit::write_routed_def (NETS-section splice)        [landed]
  → parse_def(out) → drc::geom::assemble → run_all      [the gate]
  → census: wirelength + vias/cut vs .drt.reference     [informational]
```

## Decisions (unchanged from the previous design round)

1. **NETS-only template splice, not `write_def_full`** — every byte
   outside `NETS`…`END NETS` passes through verbatim; `VIAS`,
   `SPECIALNETS` PDN, `TRACKS`, `PINS` survive for the self-DRC
   picture. Implemented in `write_routed_def`/`find_section`.
2. **No width token on `+ ROUTED`** — a bare integer after the
   layer parses as a point coordinate in regular `NETS` records;
   width is `SPECIALNETS`-only. Deliberate deviation from the
   spec's illustrative grammar; TritonRoute's committed DEFs
   confirm.
3. **Square-cap expansion** `seg ± w/2` everywhere — identical to
   `parse_net_wire`'s 2-point expansion and `commit_route`'s
   occupancy rasterization, so the emitted DEF describes exactly
   the shapes the router priced, and re-parse is a fixed point.
4. **Via records name the `ViaTable` pair default** — emit never
   reselects; `vp.lower == cand.lower` asserted, `default == None`
   bails (a gap pair reaching emit is a bug, not data).
5. **Unrouted nets emit conns-only records** — named in
   `EmitReport.unrouted`; picorv32's residual `NoPath` set is the
   recorded 108-net class (43 met3-bridge + 65 met2-connected —
   see the drift note below).
6. **`drt` is its own subcommand**, flags mirroring `drc`.

## Number drift vs the pinned maze entry (record honestly)

The WIP's `maze.rs` changes alter the route input: occupancy now
covers PDN metal and io-port bands, and projected pins get real
access stubs. The `e6-drt-maze` results entry pinned gcd 269/269 /
picorv32 6,636+108 under the *old* marking. The emit legs re-run
`route_nets`, so the emit results entry records **fresh**
routed/unrouted/collision/projected counts and notes any delta vs
the pinned numbers with this cause — never back-edits the maze
entry. Test asserts are count-agnostic (`nets + unrouted ==
guided`, classification asserts, digest-difference fail-proof), so
no existing test should need re-pinning; if one does, that is a
finding to investigate, not silently update.

## Risks (still live for the close-out)

- **`run_all` red on gcd.** The maze-side fixes above were made for
  the known classes; the gate has not been run green end to end
  yet. Any residual finding is a real bug to fix in place or a
  named class with rule + count in `results.md` — the acceptance
  accepts "0 or named", never a loosened assert.
- **picorv32 wall clock.** ~135 min route + emit + assemble +
  `run_all` on a 28M-node design's output — run detached under
  `--release --ignored` early in the close-out, draft docs while
  it runs.
- **Verify timeout regression.** `cargo test --workspace` (the
  kurena verify) must contain only gcd-scale emit legs — gcd
  routes in ~2 s release / ~10 s dev, so ~7 gcd emit legs are
  fine; the picorv32 gate stays `#[ignore]`d.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
