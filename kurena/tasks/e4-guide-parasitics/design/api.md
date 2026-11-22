# e4-guide-parasitics — API surface

## `congestion/guide.rs`

```rust
/// Resolves parsed guide net names to `NetId`s. Names match on the same
/// token `write_guides` emits (`guide_net_name`: raw DEF spelling when
/// `net_def_spellings` is populated, else `escape_guide_name`). A guide
/// net with no match in the design is an error, never a skip; nets absent
/// from the guide simply contribute no entry.
pub fn resolve_guide_nets(
    design_db: &DesignDb,
    parsed: &[(String, Vec<GuideBox>)],
) -> anyhow::Result<Vec<(NetId, Vec<GuideBox>)>>;
```

Builds a `name → NetId` map once over `design_db.num_nets()` (not a scan
per guide net), then maps `parsed` in file order.

## `sta/parasitics.rs`

```rust
/// Builds one `NetRcTree` per net from route-guide boxes — the E4.4
/// estimator (`estimate_parasitics -global_routing` analogue). `None` for
/// nets without a driver, with < 2 pins, or with no boxes; semantics in
/// `docs/e4-guides.md`. `grid` maps box rects back to covered GCells —
/// `GCellGrid::build` for parsed guides, `routed.grid` when the caller
/// routed internally.
pub fn estimate_guide_parasitics(
    design: &DesignDb,
    tech: &TechDb,
    grid: &GCellGrid,
    net_boxes: &[(NetId, Vec<GuideBox>)],
) -> NetParasitics;
```

- Same return type and invariants as the sibling builders:
  `wire_cap_pf` is wire-only (`PIN_CAP NONE`), `pin_node` is in
  `net_pins` order, `pd_check_failed_nets = 0` (the field is
  PD-tree-specific; guide nets have no check to fail).
- `net_boxes` carries only nets that have boxes; the returned `nets` vec
  is indexed by `NetId` over `num_nets()` regardless of input order.
- Errors are internal-invariant `bail!`s only (a pin unreachable from
  the driver after the documented fallback). File-boundary errors live
  in the caller (`parse_guides`, `resolve_guide_nets`).

`sta/mod.rs` re-exports `estimate_guide_parasitics` next to
`estimate_placement_parasitics`/`spef_parasitics`.

## `grus-eda` CLI (`main.rs`)

`ParasiticsMode` (shared by `sta`/`power`) gains `Guide`. `StaArgs` gains:

```rust
#[arg(
    long,
    requires = "parasitics",
    help = "Path to a route guide file (OpenROAD global_route -guide_file format); \
            with --parasitics guide. Default: derive guides internally via route_design"
)]
guide: Option<PathBuf>,
```

The sta mode match gains `(Some(ParasiticsMode::Guide), None) => "guide RC"`,
and `compute_parasitics` gains the `"guide RC"` arm:
  - `--guide <path>` → `GCellGrid::build(&design, &tech_db, None, None)`
    → `parse_guides` + `resolve_guide_nets` →
    `estimate_guide_parasitics`;
  - no `--guide` → `route_design(&design, &tech_db, None, None)` →
    `build_guides` → per-net boxes straight from `NetGuide.net` →
    `estimate_guide_parasitics` on `routed.grid`. This is the
    criterion-4 plumbing: one command runs our guide → our RC → our STA,
    and `route`'s `--gcell-tracks`/`--layer-adjust` stay off this path
    (grid defaults, the same settings the committed guides were compared
    under).
- `Some(ParasiticsMode::Guide), Some(_)` → the existing
  `unsupported --parasitics ... with --spef` bail.
- `--guide` without `--parasitics guide` → bail
  (`clap requires` covers `--guide` needing `--parasitics`; the mode
  match rejects `--parasitics guide` combinations it doesn't name —
  add an explicit bail for `--guide` present with any other mode so the
  flag can never be silently ignored).

`--spef-out` needs no change: it serializes the `NetParasitics` the arm
already computed, so `sta --parasitics guide --spef-out g.spef` emits the
guide RC model, and `sta --spef g.spef` reads it back through the
existing `spef_parasitics` path.

`power --parasitics guide` falls into the existing catch-all bail
("unsupported --parasitics Guide ...") — deliberate: the guide mode is a
STA surface for now, and a clear error beats a half-wired `PowerArgs`.
The report mode string is `"guide RC"` in the `STA (...)` line.
