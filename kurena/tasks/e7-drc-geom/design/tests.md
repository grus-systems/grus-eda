# e7-drc-geom — Test plan

Unit tests live in-module (`def.rs`, `lef.rs`, `drc/geom.rs`,
`drc/width.rs`); design-scale tests go in
`crates/grus-core/tests/drc.rs` (new) — there is no existing tests/
harness for routed DEFs, `pdn.rs` keeps its `load_design` helper
in-module and the new test file gets its own equivalent
(`test_support::{sky130_hd, design_dir, require}`).

## Unit tests

`readers/def.rs` (NETS — synthetic DEFs on the `MINI_LEF`/`mini_tech`
style fixture, extended with a LEF via + met2):

- `ROUTED` two-point + `NEW` chain (`*` continuation across
  statements) → expected `wire_rects` (half-width = layer `WIDTH`/2).
- `pt viaName` → `vias` record with LEF-resolved stack + `ViaGeom::Lef`.
- `pt RECT ( dx1 dy1 dx2 dy2 )` → exact rect at point + delta.
- `+ FIXED`/`+ COVER` accepted; `VIRTUAL` consumed silently.
- Errors: unknown via name (mentions net + line), via on unrelated
  layer, `RECT` before any point, one-point stub, `EXT` /
  `NONDEFAULTRULE` / `SUBNET` → loud bail each.
- `VIAS` record without `CUTSIZE` → loud bail (unsupported input, not
  silently defaulted); `CUTSIZE`-less records otherwise parse for
  `+ VIA`/`+ RECT` forms.
- Conn-group error text unchanged (existing pinned tests).
- `gcd.routed.def`/`picorv32.routed.def` parse: `net_geoms.len() ==
  num_nets()`, per-layer wire-rect + via-instance census printed for
  `results.md`; every net's vias resolved.
- Determinism: parse `gcd.routed.def` twice → `net_geoms` equal.

`readers/lef.rs`:

- `VIA` rects retained: fixture via → 3 group rects exact coords,
  `ViaRectLayer` discriminants right, `cut_count` = cut-rect count.
- Multi-RECT cut group (two parallel cuts) → both in `rects`.
- `CutLayer.width_dbu` = LEF `WIDTH` in DBU; layer without `WIDTH` → 0.

`drc/geom.rs`:

- `expand_via` generated: `via2_3_1600_480_1_5_320_320` params at
  origin → lower metal `(-800,-240,800,240)`, upper
  `(-770,-160,770,160)` (1430+2·55 × 150+2·85), 5 cut rects
  150×150 at pitch 320.
- `iter_overlapping` probe semantics (strict overlap).
- Orientation: synthetic DEF+LEF — macro with asymmetric OBS
  `(0,0)-(100,50)` + pin rect; cells at `N`, `W`, `FE` → exact
  transformed obs coords (W/FE cover the E0 swap class), layer kept.
- Source coverage: one fixture exercising SignalWire (NETS ROUTED),
  SpecialWire (SPECIALNETS ROUTED), Via (NETS `pt LEFvia`), MacroPin +
  MacroObs (placed cell), IoPort (PINS `+ LAYER`) — each asserted on
  the right `DrcLayer` with the right `src`.

`drc/width.rs`:

- `WIDTH_RULES` bound parity on the real tech: 170/170/170/170/140/
  140/300/300/1600/150/200/200/800 in table order.
- Boundary discrimination: a 140×*n* met1 rect passes
  (`measured == bound` → no violation) and a 139-wide rect fails —
  this is the assertion the `<`→`<=` fail-proof turns red.
- Synthetic thin shape → exactly one `Violation` with right
  `rule`/`rule_name`/`measured_dbu`.

## Integration tests — `crates/grus-core/tests/drc.rs`

Shared `load_design(design)` mirroring `pdn.rs`'s (tlef + lef +
`<d>.routed.def`).

1. **`retains_routed_geometry`** (both designs): `net_geoms` non-empty;
   per-layer counts recorded (assert every routing layer has ≥1 rect
   the design uses, vias per net > 0); printed census feeds
   `results.md`.
2. **`roundtrip_deterministic`** (gcd): parse twice → `net_geoms`
   equal; `assemble` twice → `DrcDesign` equal.
3. **`width_zero_violations`** (both designs): `check_width` empty;
   name-parity join per `width.md` §3 against
   `<d>.drc.reference.json` (ids ⊆ lef_expressible, rule_names ∈
   golden `names`, no `magic_per_rule` entry for the 13 ids).
4. **`mutation_met1_below_width`** (gcd): read `gcd.routed.def` text,
   replace exactly one `NEW met1 480` in SPECIALNETS with
   `NEW met1 100` (assert the replacement count is 1 — the file has 28
   `NEW met1 480` rails; regular-net wires carry no explicit width, so
   the practical probe point is a special wire's width field; the
   mechanism under test — expansion → `min(w,h) < bound` → violation —
   is identical). Parse → `check_width` returns exactly one
   `Violation` with `rule == "met1.1"`, `measured_dbu == 100`,
   `net == VPWR` or `VGND`. Control: unmutated file → 0.
5. **`orientation_obs`** — fixture from geom unit tests promoted or
   kept in-module (either is fine; in-module is cheaper since it needs
   `expand`/assemble internals).

## Fail-proof (recorded in `docs/results.md`, per AGENTS §7.7)

After the mutation test passes: flip the width comparison `<` to `<=`
in `drc/width.rs` → rerun the boundary unit test **and** test 4 →
both must go red (at-bound met1/li1 shapes now flag, so the unit
assertion fails and the integration count jumps to thousands ≠ 1) →
restore `<`, rerun green. Record both runs verbatim.

## Verification commands

```
cargo test -p grus-core drc
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

picorv32 inputs must exist (AGENTS §3: generate first if missing);
`test_support::require` panics loudly otherwise — no skips.

## Risk register

- **0-gate false positive**: per-rect check on merged-geometry cases
  (a sub-bound macro pin leg covered by a via metal) could flag where
  Magic's merged polygon passes. `ShapeSrc` tags let `results.md` name
  the source class; if it fires, revisit whether the source belongs in
  the width input set — don't silently drop it.
- **Decode/fill cells with narrow OBS**: sky130_fd_sc_hd has li1 OBS
  ≥170 and met1 pins ≥140 by construction, but this is asserted by the
  gate, not assumed.

## Files

- `crates/grus-core/tests/drc.rs`
- `crates/grus-core/src/readers/def.rs`
- `crates/grus-core/src/readers/lef.rs`
- `crates/grus-core/src/drc/geom.rs`
- `crates/grus-core/src/drc/width.rs`
- `docs/results.md`
