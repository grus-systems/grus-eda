# e6-drt-emit — Overview

E6 ticket — routed DEF emission + self-check. Converts the maze
router's `RouteDb` (per-net `WireSeg`/`ViaPlace` lists) into DEF
`NETS` geometry, adds the `grus-eda drt` CLI surface that replaces
the delegation path (placed DEF + guide → routed DEF), and gates
the result on our own 50-rule subset checker. Depends on
`e6-drt-maze` (landed at `4ff7ade`, `v0.12.4`); the full Magic-deck
check on our DEF belongs to `e6-drt-e2e` and is explicitly out of
scope here.

## Pipeline

```
placed DEF + <d>.guide
  → load_congestion_inputs (tlef/lef/def → TechDb + DesignDb)
  → parse_guides → join_guides → build_route_graphs
  → ViaTable::new → route_nets(MazeConfig)            [landed]
  → drt::emit::apply_routes (RouteDb → db.net_geoms)  [this ticket]
  → drt::emit::write_routed_def (NETS-section splice) [this ticket]
  → parse_def(out) → drc::geom::assemble → run_all    [the gate]
  → census: wirelength + vias/cut vs .drt.reference   [informational]
```

## Decisions

1. **NETS-only template splice, not `write_def_full`.**
   `write_def_full` emits header/DIEAREA/ROW/TRACKS/COMPONENTS/PINS/
   NETS but **no** `VIAS`, `SPECIALNETS`, `GCELLGRID`, `BLOCKAGES`,
   `NONDEFAULTRULES` — emitting a routed DEF through it would drop the
   PDN geometry and DEF-generated via definitions, corrupting the
   self-DRC picture. The writer instead mirrors `write_def`'s
   `COMPONENTS` splice: scan the template for `NETS ` … `END NETS`,
   pass every other byte through verbatim, regenerate the NETS records
   canonically. PDN stripes, `VIAS`, `TRACKS`, `PINS`, `FILLS` all
   survive untouched.

2. **`+ ROUTED <layer>` carries NO width token.** The spec's example
   grammar (`+ ROUTED <layer> <width>`) does not parse through our own
   reader: in regular `NETS` records a bare integer after the layer is
   consumed as a point coordinate (width is a `SPECIALNETS` construct
   only). TritonRoute's committed DEFs confirm — `+ ROUTED met2
   ( 10350 48450 ) ( * 53210 )`, no width. Emitting a width would make
   our own output un-parseable. This is a deliberate deviation from
   the spec's illustrative grammar — recorded here so the implementer
   does not "fix" it.

3. **Square-cap expansion, identical everywhere.** `apply_routes`
   expands each `WireSeg` into a `wire_rect` of `[min(x1,x2)−w/2,
   min(y1,y2)−w/2, max(x1,x2)+w/2, max(y1,y2)+w/2]` — the same formula
   `parse_net_wire` applies to a 2-point record and the same rect
   `commit_route` rasterizes into occupancy. The emitted DEF therefore
   describes exactly the shapes the router priced, and re-parsing
   yields exactly the rects `apply_routes` produced (fixed point for
   the byte-stability argument). `w/2` for zero-width records is
   `routing_layers[l].width_dbu / 2`.

4. **Via records use the ViaTable's chosen default.** For each
   `ViaPlace`, emit `pairs[vp.lower].default`'s candidate — name
   (`escape_def_name(resolve(cand.name))`), `lower`, `upper`, `cut`,
   `cut_count`, `geom` copied verbatim into the `PdnVia`. The emitter
   never reselects a via: the placed shape must be the via the router
   priced (`vp.lower == cand.lower` is asserted). On re-parse,
   `resolve_def_via` finds LEF candidates via `via_by_name` and
   DEF-generated candidates via the template's preserved `VIAS`
   section — both name spaces already verified by
   `test_via_defaults`' name checks.

5. **Unrouted nets emit connections only.** picorv32's 65 residual
   `NoPath` nets (all met3-bridge class) get their `( conn ) …
   + USE SIGNAL` record with no geometry; `EmitReport.unrouted`
   names them. Unguided nets (`GuideJoin.missing_signal`) likewise
   keep empty `net_geoms` → conns-only records. A guide block naming
   a net the design lacks already bails inside `resolve_guide_nets`
   at join time — emit never sees that condition.

6. **New `drt` subcommand.** `route` is the E4 global-router guide
   path; detailed routing gets its own surface (`drt`) rather than a
   flag on `route`, keeping both commands' contracts simple. Flags
   mirror `drc`'s conventions (`--tlef --lef --def` + `--guide
   --out-def`, optional `--reference`, `--self-drc`).

## Measured state at design time

- `route_nets` census (e6-drt-maze, dev profile): gcd `guided=269
  routed=269 pins=931 projected=17 unrouted=0 collisions=0 iters=8
  ms=9704`; picorv32 `guided=780` with 65 met3-bridge `NoPath`
  residuals (routed=715) at ~135 min release.
- Occupancy fail-proof (maze): Enabled census=0 vs Disabled
  census=6,429 on gcd — occupancy measurably constrains.
- Self-DRC bar is real: `tests/drc.rs` asserts `run_all` finds **0**
  findings across all 50 rules on both committed TritonRoute DEFs
  (`gcd.routed.def`, `picorv32.routed.def`). The same assertion
  applied to OUR emitted DEF is this ticket's gate.
- `test_via_defaults`: LEF/DEF-alias candidates share geometry, all
  5 adjacent pairs' defaults resolve in both name spaces.

## Risks

- **Self-DRC findings on our routes.** Maze occupancy is
  deliberately loose: `Pin` marks are soft, host-cell OBS is carved
  unconditionally, projected attaches land outside exact pin shapes,
  io-port metal is unmarked, and cut-layer spacing is unmodeled.
  Any of these can surface as spacing/enclosure findings. Each
  finding is either a real router bug to fix in place or a named
  class with rule + count in `results.md` — never a silenced assert.
- **picorv32 wall clock.** The gate leg reuses `route_nets` (~135
  min release). Tests follow the maze convention: census-scale
  picorv32 legs are `#[ignore]`d and run detached for the results
  evidence; the gate keeps a fast fixture + gcd leg.
- **Foreign-geometry re-emission.** `write_routed_def` re-emits
  `net_geoms` rects as 2-point centerlines — only valid for
  square-cap-expanded rects (our emit path). A net carrying
  RECT-stub or flush-cap geometry from a foreign routed DEF
  template cannot be expressed in the no-width grammar; the writer
  bails naming the net rather than silently distorting geometry.
- **Pin-connection count formatting.** Emitted conn list must
  include every `net_pin_ids` member (driver-first) — dropping a
  conn silently changes connectivity while DRC still passes.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
