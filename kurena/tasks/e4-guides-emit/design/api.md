# e4-guides-emit — API surface

## `congestion/route.rs`

`route_segments` returns the committed paths it already computes:

```rust
/// Output of the 3-pass router: the aggregate report plus the committed
/// GCell-edge path per input segment.
pub struct RouteOutput {
    pub report: CongestionReport,
    /// `segment_paths[i]` is the committed edge list of `segments[i]`;
    /// empty when the segment degenerated (zero length) or was never
    /// committed. Index-aligned with the `segments` argument.
    pub segment_paths: Vec<Vec<GridEdge>>,
}

pub fn route_segments(grid: &mut GCellGrid, segments: &[SteinerSegment]) -> RouteOutput;
```

The single caller inside `route_segments` is the report aggregation at the
end; it consumes `RouteOutput.report`. Existing test call sites destructure
`.report`.

`estimate_congestion` keeps its `(GCellGrid, CongestionReport)` signature
and delegates to the new entry point:

```rust
/// Everything `estimate_congestion` returns, plus the routed paths and the
/// net they belong to — the guide emitter's input.
pub struct RoutedDesign {
    pub grid: GCellGrid,
    pub report: CongestionReport,
    /// Flat segment list, index-aligned with `segment_paths`.
    pub segments: Vec<SteinerSegment>,
    /// CSR offsets into `segments`/`segment_paths`:
    /// net `NetId(n)` owns `net_seg_offsets[n]..net_seg_offsets[n+1]`;
    /// length `design_db.num_nets() + 1`.
    pub net_seg_offsets: Vec<u32>,
    pub segment_paths: Vec<Vec<GridEdge>>,
}

pub fn route_design(
    design_db: &DesignDb,
    tech_db: &TechDb,
    layer_adjustments: Option<&[f32]>,
    gcell_tracks_override: Option<i32>,
) -> RoutedDesign;
```

`estimate_congestion` becomes a thin call: `let r = route_design(..); (r.grid, r.report)`.

## `congestion/rsmt.rs`

```rust
/// Decomposes all nets into Steiner segments like
/// `decompose_design_nets`, but returns the per-net CSR mapping as well.
/// `net_seg_offsets[n]..net_seg_offsets[n+1]` slices `segments` for
/// `NetId(n)`; nets whose pins share one GCell contribute zero segments.
pub fn decompose_design_nets_indexed(
    design_db: &DesignDb,
    tech_db: &TechDb,
    grid: &GCellGrid,
) -> (Vec<SteinerSegment>, Vec<u32>);
```

`decompose_design_nets` delegates: `decompose_design_nets_indexed(..).0`.
The `n < 512` serial / `into_par_iter` split stays inside the indexed
variant so per-net ordering remains deterministic in both paths (collect
per net, then flatten — same as today).

## `design_db.rs` + `readers/def.rs`

```rust
// DesignDb pin SoA, indexed by PinId:
pub pin_io_layer: Vec<Option<LayerId>>, // DEF `+ LAYER` of primary I/O pins; None for instance pins

// DesignDb net SoA, indexed by NetId:
pub net_def_spellings: Vec<SymbolId>, // raw NETS token per net; empty for non-DEF sources
```

`add_pin` already parses `+ LAYER`; it currently keeps only the layer-rect
offset. It gains a `tech_db: &TechDb` parameter (its only call site in
`parse_def` already has one in scope), resolves the `+ LAYER` name through
`tech_db.layer_by_name`, and pushes `Some(LayerId)` for I/O pins, `None`
otherwise. Unknown layer name → `bail!` like the other parse errors.
`DesignDb` derives `Serialize`/`Deserialize`; the new fields bump any
cached encoding implicitly with the workspace version (no explicit
migration — caches are versioned rebuilds).

`net_def_spellings` records the NETS `- name` token before unescaping —
`read_guides` matches net names verbatim, and OpenDB stores the DEF
spelling (`ctrl.state.out\[1\]`, `req_msg[0]`), so emission must
reproduce it (measured; `escape_def_name` alone mismatches both
directions).

## `congestion/guide.rs` (new module)

```rust
/// One guide box: a DBU rect on a routing layer, as the
/// `xlo ylo xhi yhi layer` lines inside an OpenROAD `global_route
/// -guide_file` block carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuideBox {
    pub layer: LayerId,
    pub rect: RectDbu,
}

/// Guide boxes for one signal net, in deterministic emission order.
pub struct NetGuide {
    pub net: NetId,
    pub boxes: Vec<GuideBox>,
}

/// Builds per-net guides from a routed design. Semantics: `semantics.md`.
pub fn build_guides(
    design_db: &DesignDb,
    tech_db: &TechDb,
    routed: &RoutedDesign,
) -> Vec<NetGuide>;

/// Emits the P-GR text format: per net `name` / `(` / `xlo ylo xhi yhi
/// layer` / `)`. Deterministic: nets in `net_names` order, boxes in
/// `GuideBox` sort order ((layer, yl, xl, xh, yh)).
pub fn write_guides(
    out: &mut impl std::io::Write,
    design_db: &DesignDb,
    tech_db: &TechDb,
    guides: &[NetGuide],
) -> std::io::Result<()>;

/// Parses the same format back (comparison tests + round-trip checks).
/// Returns `(net_name, boxes)` in file order; unknown layer names and
/// malformed lines are errors, never skips.
pub fn parse_guides(
    text: &str,
    tech_db: &TechDb,
) -> anyhow::Result<Vec<(String, Vec<GuideBox>)>>;

/// `escape_guide_name` escapes `[`/`]` on top of the DEF-word escapes;
/// `guide_net_name` resolves the emitted spelling (raw DEF spelling when
/// `net_def_spellings` is populated, `escape_guide_name` fallback).
pub fn escape_guide_name(s: &str) -> String;
pub fn guide_net_name(design_db: &DesignDb, net: NetId) -> String;
```

`write_guides` sorts + dedups each net's boxes itself, so the
(layer, yl, xl, xh, yh) order is the file's invariant regardless of
caller order.

`mod.rs` gains `pub mod guide;` and the glob re-export picks it up.

## `grus-eda` CLI (`main.rs`)

```rust
/// E4: Global routing — committed edge paths emitted as an OpenROAD-format route guide
#[command(name = "route")]
Route(RouteArgs),

struct RouteArgs {
    #[arg(long, help = "Path to technology LEF (.tlef) file")]
    tlef: PathBuf,
    #[arg(long, help = "Path to standard cell LEF (.lef) file")]
    lef: PathBuf,
    #[arg(long, help = "Path to placed DEF (.def) file")]
    def: PathBuf,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..), help = "GCell grid track supply (default: from the DEF GCELLGRID)")]
    gcell_tracks: Option<u32>,
    #[arg(long, value_parser = parse_unit_interval, help = "Uniform layer adjustment factor (0.0 to 1.0)")]
    layer_adjust: Option<f32>,
    #[arg(long, help = "Path to output route guide file (OpenROAD global_route -guide_file format)")]
    guide_out: PathBuf,
}
```

The arm mirrors the `Congestion` arm (same load path, same
`layer_adjust`/`gcell_tracks` mapping — factor the shared load into a
small helper rather than duplicating), calls `route_design`, prints the
same congestion summary lines, then `build_guides` + `write_guides` and a
final `Guide: nets=<n> boxes=<m> wrote <path>` line. `--heatmap` is not
carried over.
