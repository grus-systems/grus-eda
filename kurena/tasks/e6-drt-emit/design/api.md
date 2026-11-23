# e6-drt-emit — API design

One new module `drt::emit` plus a `drt` subcommand in `grus-eda`.
**Landed at WIP `dc8cdb4`** — the signatures below are the shipped
ones (doc comments abridged); deltas vs the original plan are
flagged inline.

## `crates/grus-core/src/drt/emit.rs` (landed)

```rust
/// What apply_routes did — printed by the CLI and asserted in tests.
pub struct EmitReport {
    /// Nets whose NetRoute was applied (route_db.nets.len()).
    pub routed: usize,
    /// DEF spellings of guided nets in route_db.unrouted
    /// (picorv32's met3-bridge NoPath set).
    pub unrouted: Vec<String>,
    /// Σ |x2-x1| + |y2-y1| over all emitted WireSegs, DBU.
    /// Centerline length — matches `routed_def_parse`'s per-segment
    /// accounting, so the delta vs `total_wirelength_um` is honest.
    pub wire_length_dbu: u64,
    /// cut-layer name → placed-via count, grouped by
    /// `pairs[vp.lower].cut` resolved via tech.strings.
    /// Joins `vias_per_cut_layer` in `<d>.drt.reference.json`.
    pub vias_per_cut: BTreeMap<String, u64>,
}

/// Convert RouteDb results into db.net_geoms entries.
///
/// For every NetRoute in `route_db.nets` (guide-file order, each
/// carrying `nr.net: NetId`):
///   net_geoms[nr.net] = NetGeom {
///       wire_rects: nr.wires expanded ±w/2 (square caps),
///       vias:       nr.vias → PdnVia from
///                   pairs[nr.vias[i].lower.0].default,
///   }
/// and for every `route_db.unrouted` entry the net's NetGeom is set
/// to `NetGeom::default()` (a guided net with no route emits
/// conns-only — never stale geometry).
///
/// Guided nets absent from both lists would mean graphs/route_db
/// disagree — bail. Nets absent from `graphs.nets` are left
/// untouched (a placed DEF has no pre-existing signal geometry; if
/// a routed template is ever fed in, their records pass through as
/// foreign geometry).
/// `db.net_geoms` is resized to num_nets first — parse_def fills it
/// from DEF, but the field is only guaranteed populated on the
/// DEF-input path.
///
/// Errors (bail, not silent):
///   - a ViaPlace whose pair has `default == None`
///     (a via-table gap reached emit — the router already skips gap
///     transitions, so this means graphs/table disagree: a bug);
///   - `vp.lower != cand.lower` (table consistency check).
pub fn apply_routes(
    db: &mut DesignDb,
    tech: &TechDb,
    graphs: &RouteGraphs,
    route_db: &RouteDb,
    via_table: &ViaTable,
) -> Result<EmitReport>;

/// Splice a canonical NETS section into the template DEF.
///
/// Locates `NETS n ;` … `END NETS` in `template_def` exactly the way
/// `write_def` locates `COMPONENTS`, writes everything outside that
/// span verbatim, and regenerates the NETS records from
/// `db.net_geoms` per def-model.md. Errors if the template carries
/// no NETS section, or if any wire_rect is not square-cap-expressible
/// (see def-model.md §centerline).
pub fn write_routed_def(
    db: &DesignDb,
    tech: &TechDb,
    template_def: &Path,
    out_def: &Path,
) -> Result<()>;
```

`NetGeom` construction details:

```rust
// per WireSeg w on routing layer l, w2 = tech.routing_layers[l].width_dbu / 2:
RectDbu::new(w.x1.min(w.x2) - w2, w.y1.min(w.y2) - w2,
             w.x1.max(w.x2) + w2, w.y1.max(w.y2) + w2)

// per ViaPlace vp, cand = &via_table.candidates[
//     via_table.pairs[vp.lower.0].default.unwrap()]:
PdnVia {
    name:      cand.name,                 // db.strings sym (LEF or DEF spelling)
    lower:     cand.lower,                // == vp.lower
    upper:     cand.upper,
    cut:       cand.cut,
    cut_count: cand.cut_count,
    geom:      cand.geom.clone(),         // ViaGeom::Lef | ::Generated
    x_dbu:     vp.x,
    y_dbu:     vp.y,
}
```

`drt.rs` gains `pub mod emit;` next to `maze`/`via`/`graph`.

## `crates/grus-eda/src/main.rs`

```rust
Drt(DrtArgs),

#[derive(Args, Debug)]
struct DrtArgs {
    #[arg(long)] tlef: String,
    #[arg(long)] lef: String,
    #[arg(long)] def: String,     // placed DEF — also the emit template
    #[arg(long)] guide: String,   // OpenROAD-format guide file
    #[arg(long)] out_def: String, // routed DEF output path
    /// <d>.drt.reference.json — prints delegated/baseline deltas.
    #[arg(long)] reference: Option<String>,
    /// Re-parse out_def, assemble, run_all; print findings per rule.
    #[arg(long)] self_drc: bool,
    /// Fail-proof / debug knob — maps to MazeConfig.occ.
    #[arg(long, value_enum, default_value_t = OccArg::Enabled)]
    occ: OccArg,
    #[arg(long, default_value_t = 8)] max_iters: usize,
}
```

Run sequence (landed, mirrors `Route` + `Drc` arms):

1. `load_congestion_inputs(&tlef, &lef, &def, None, None)` →
   `(tech, db)` (`DrtArgs` fields are `PathBuf`).
2. `fs::read_to_string(&guide)` → `parse_guides` → `join_guides` →
   `build_route_graphs_ext(&db, &tech, &tracks, &join, 1)` — the
   one-spare-layer pin-pocket escape (`docs/e6-drt.md` §7).
3. `layer_tracks`, `via_table`, `route_nets(.., MazeConfig { occ,
   max_iters })` — landed fn names (not `ViaTable::new`).
4. `apply_routes` → `EmitReport`; `write_routed_def`.
5. Print block (informational, all lines labeled):

   ```
   == drt emit: <design> ==
   guided=<g> routed=<r> unrouted=<u> projected=<p> collisions=<c>
       iterations=<i> route_ms=<t>
     unrouted <name>            (one line per unrouted net)
   wirelength_um ours=<x> delegated=<y> delta=<±z> baseline=<b>
   via.<cut>  ours=<x> delegated=<y> delta=<±z> baseline=<b>
   self_drc findings=<n> [rule names + counts]      [--self-drc]
   ```

6. `--reference` loads the JSON and fills the delegated/baseline
   columns; absent → ours-only columns.
7. `--self-drc`: `parse_def_bytes` the emitted file → `assemble` →
   `run_all` → print every `RuleOutcome` with `!findings.is_empty()`
   as `rule=<id> findings=<n>` plus `findings_total=<n>`. The CLI
   never exits nonzero on findings — the gate is the test, the CLI
   reports.

No JSON `--report` output in this ticket: the committed
`.drt.reference.json` files are generated by
`tests/designs/generate_drt_ref.sh` inside the container; our census
is compared *against* them, not re-committed (the e2e ticket adds
`<d>.drt.ours.reference.json`).

## Reused, not new

- `guide_net_name(db, net)` — DEF-faithful net spelling in records.
- `escape_def_name` — via/net name emission.
- `resolve_def_via` — re-parse name resolution (unchanged).
- `drc::geom::assemble`, `drc::report::run_all` — the gate.
- `load_congestion_inputs`, `Cli`/`Args` derive patterns.

## Files

- `crates/grus-core/src/drt/emit.rs`
- `crates/grus-core/src/drt.rs`
- `crates/grus-eda/src/main.rs`
- `crates/grus-core/tests/drt.rs`
