# e6-drt-emit — Overview

E6 ticket — routed DEF emission + self-check. Converts the maze
router's `RouteDb` (per-net `WireSeg`/`ViaPlace` lists) into DEF
`NETS` geometry, adds the `grus-eda drt` CLI surface that replaces
the delegation path (placed DEF + guide → routed DEF), and gates
the result on our own 50-rule subset checker. Depends on
`e6-drt-maze` (`v0.12.4`); the full Magic-deck check on our DEF
belongs to `e6-drt-e2e` and is explicitly out of scope here.

## State at this revision (post-measurement fix)

The emission half **landed and is measured** (`dc8cdb4`…`38befa4`,
v0.12.5): `drt::emit`, `grus-eda drt`, the emit test suite, the
self-DRC gate, docs. The detached picorv32 gate came back **red
with a classified cause** — this revision is the fix plan:

- gcd: `run_all` **0 findings** on our emitted DEF (all 50 rules),
  byte-stable roundtrip, determinism, both fail-proof arms green.
- picorv32 (30,658 s detached leg): `routed=6,628`, `unrouted=116`,
  but **42 findings** — `li.6`×3 (28,900 < 56,100) and `met3.6`×39
  (190,500–237,000 < 240,000). Every finding is a min-area shortfall
  on a **stub-class component**: one via plus a short stub — the
  class `relaxed_vias` implied stubs and projected-pin access stubs
  create when the connected metal on that layer is just that piece.
  A router-side deficiency, not an emit defect — emit correctly
  surfaced it; the gate worked.

**This revision's scope** (the rejection memo's list):

1. **Post-route min-area patch** — per committed net, per layer,
   find each connected component under the layer's `AREA` bound and
   extend it along the preferred axis inside the net's guide region
   until the bound is met, respecting foreign occupancy —
   TritonRoute's min-area patch analog. Design in `minarea.md`;
   lands as a RouteDb-level pass inside `route_nets` while the
   final committed occupancy is still live.
2. **Synthetic fixture** pinning the patch — a unit fixture that
   produces a sub-min-area stub component and asserts the patch
   fills it, plus the occupancy-blocked residual arm. The ~8.5 h
   picorv32 re-run must not be the only gate.
3. **One detached re-run** of `test_emit_selfdrc_picorv32`; the
   results entry records the delta vs the committed 42-finding
   baseline.
4. Docs delta (`docs/e6-drt.md`, `docs/results.md`, `status.md`,
   `roadmap.md`), version `0.12.5 → 0.12.6` (bugfix → patch on
   0.x), `cargo test --workspace` + clippy `-D warnings`, commit.

## Pipeline (as landed; patch slot marked)

```
placed DEF + <d>.guide
  → load_congestion_inputs (tlef/lef/def → TechDb + DesignDb)
  → parse_guides → join_guides → build_route_graphs_ext(…, 1)
  → via_table → route_nets(MazeConfig)
      ├─ sequential maze iterations                    [e6-drt-maze]
      ├─ min-area patch pass (occupancy still live)    [this fix]
      └─ overlap_census                                 [e6-drt-maze]
  → emit::apply_routes (RouteDb → db.net_geoms)         [landed]
  → emit::write_routed_def (NETS-section splice)        [landed]
  → parse_def(out) → drc::geom::assemble → run_all      [the gate]
  → census: wirelength + vias/cut vs .drt.reference     [informational]
```

## Decisions

1. **NETS-only template splice, not `write_def_full`** — every byte
   outside `NETS`…`END NETS` passes through verbatim; `VIAS`,
   `SPECIALNETS` PDN, `TRACKS`, `PINS` survive for the self-DRC
   picture. (Unchanged.)
2. **No width token on `+ ROUTED`** — width is `SPECIALNETS`-only.
   (Unchanged.)
3. **Square-cap expansion** `seg ± w/2` everywhere. (Unchanged.)
4. **Via records name the `ViaTable` pair default.** (Unchanged.)
5. **Unrouted nets emit conns-only records.** (Unchanged —
   picorv32's fresh residual re-measures on the patch re-run; the
   last leg recorded 116 vs the maze entry's 108, cause named:
   the PDN/io-port premark.)
6. **`drt` is its own subcommand.** (Unchanged.)
7. **Min-area patch is a post-loop `route_nets` pass, not a
   per-iteration `route_one_net` step** — occupancy at loop exit is
   the exact final committed set: patches see real foreign metal
   (never transient soft-mode marks about to be ripped up), run
   once instead of once per attempt, and rasterize straight into
   `occ` so `overlap_census` — which rebuilds from `route.wires`
   anyway — measures the patched truth. `route_one_net` keeps its
   signature; nothing outside `maze.rs` sees a new type except the
   two `RouteDb` census fields.
8. **Patch extensions are occupancy-strict, mode-blind** — the
   patch adds optional metal; any `Obs` hit or foreign
   `Net`/`Pin` hit inside the `w/2 + s` band rejects the
   extension step. The only legal hits are the net's own marks —
   `Net(own)` from its already-committed route and `Pin(own)`
   premarks — which the patch must overlap anyway at the junction.
   This is stricter than `hit_verdict`'s soft pricing and
   deliberately ignores the host-cell OBS carve: a patch that
   cannot find clear metal is a named residual, never a
   negotiated collision.
9. **Extension domain = existing graph edges** — a patch segment
   spans consecutive covered `(ix,iy)` pairs on the component's
   layer; every step equals an in-plane `g.edges` span, so the
   patch stays inside the guide region to exactly the standard
   the router's own wires hold. Patch endpoints are always node
   coordinates — the on-track invariant survives.

## Risks

- **Residual sub-bound components** — a component with no
  occupancy-clear preferred-axis extension inside the region stays
  under bound and is recorded in `RouteDb.area_residuals` (net,
  layer, bbox, area, bound). The self-DRC gate then names it —
  the acceptance's "0 or named" path. Do not loosen the assert.
- **gcd drift** — gcd's gate is 0 today; if gcd has no sub-bound
  components the patch is a no-op and the `0x48593d441074229b`
  digest stands. Any digest/count shift on gcd is a finding to
  investigate, not silently re-pin.
- **picorv32 wall clock** — the re-run is ~2.5 h clean CPU (the
  last leg measured 30,658 s under ambient load). Launch detached
  early; the kurena verify (`cargo test --workspace`) never runs
  it — it stays `#[ignore]`d.
- **New findings classes** — the patch adds metal; a spacing
  regression would mean the occupancy-strict check is wrong —
  investigate, don't paper over.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-core/tests/drt.rs`
- `crates/grus-eda/src/main.rs`
- `docs/e6-drt.md`
- `docs/results.md`
- `docs/status.md`
- `docs/roadmap.md`
- `Cargo.toml`
