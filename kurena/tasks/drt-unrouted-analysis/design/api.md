# drt-unrouted-analysis — Code surface (r2: as-built)

Implemented in `ed0a8d7`. As-built deltas vs the r1 text:
internal `enum ProbeState {Connected, Failed(FailKind),
MissingPin}` and `struct AttachEvidence`/`fn attach_evidence`
carry the classification inputs; `GuideUnion::GuideAbsent` folds
into `GeomDisjoint` at classification (distinctness kept on
`ProbeNet.union`); `RootCause::ALL` (13) drives the zero-kept
`class_counts`/`net_counts`. The §2 widening list shrinks to
what `probe.rs` actually imports — see the as-built table.

## 1. New module: `crates/grus-eda-core/src/drt/probe.rs` (landed)

`pub mod probe;` in `src/drt.rs` (tests are an external crate —
the entry point must be `pub`; everything it touches in
`maze.rs` is `pub(crate)`).

```rust
/// The mutually-exclusive root-cause class of one finally
/// unrouted pin (taxonomy.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootCause {
    // geometric — fails with foreign Net/Pin marks absent
    GeomOffTrack,
    GeomNoInRegion,
    GeomObsSealed,
    GeomMet3Bridge,
    GeomPocket,
    GeomDisjoint,
    // negotiation — routes with foreign Net/Pin marks absent
    NegAttachStub,
    NegAttachPort,
    NegWave,
    NegWaveObsFrontier,
    NegWaveUnattributed,
    // residual named classes — defect signals, never bucketed
    ResidualMissingPin,
    ResidualAttachInconsistent,
}

impl RootCause {
    pub fn is_negotiation(&self) -> bool;
    pub fn is_geometric(&self) -> bool;
    /// Stable token for the decomposition table / test prints.
    pub fn name(&self) -> &'static str;
}

/// One classified `u.failed` entry, `u.failed` order.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbePin {
    pub pin: PinId,
    pub real_kind: FailKind,
    pub cause: RootCause,
    /// `region_escape` provenance flag (orthogonal to `cause`).
    pub region_escape: bool,
    /// Real `Unattachable` whose probe state is `NoPath` —
    /// foreign marks did block the real attach, but the pin
    /// would not have routed anyway.
    pub probe_attached: bool,
    /// Per-pin wave-blocker evidence when `u.blockers` pairs
    /// 1:1 with the `NoPath` subsequence (taxonomy.md §4);
    /// `None` means "net-level set only".
    pub wave_evidence: Option<(u8, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeNet {
    pub net: NetId,
    /// The probe re-attempt's outcome for the whole net.
    pub probe_routed: bool,
    /// Guide-union class: `Bridged` (`!conn(≤2) && conn(all)`),
    /// `Met2Connected`, `Disjoint`, `GuideAbsent`(defensive).
    pub union: GuideUnion,
    /// Probe-side wave-death evidence (`u2.blockers`,
    /// `(layer, kind)` — kinds ∈ {Obs, ObsPin} only).
    pub probe_blockers: Vec<(u8, u8)>,
    pub pins: Vec<ProbePin>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeReport {
    /// `route_db.unrouted` order.
    pub nets: Vec<ProbeNet>,
    /// Io-pin-only nets absent from the guide — the `unguided`
    /// golden class (probe.md §5).
    pub unguided: Vec<NetId>,
    pub elapsed: std::time::Duration,
}

impl ProbeReport {
    /// class → pin count, all 13 classes present (zero kept).
    pub fn class_counts(&self) -> BTreeMap<&'static str, usize>;
    /// class → distinct-net count.
    pub fn net_counts(&self) -> BTreeMap<&'static str, usize>;
}

/// The root-cause post-pass over a finished `RouteDb`. Rebuilds
/// a foreign-`Net`/`Pin`-free occupancy (`premark` minus io-port
/// `Pin` marks), re-attempts each unrouted net's attach + search
/// against it, and classifies every `u.failed` pin. `opts` must
/// match the options the route ran under (the same graphs are
/// rebuilt). `threads` sizes the probe's rayon pool; `0` =
/// rayon default.
pub fn probe_unrouted(
    db: &DesignDb,
    tech: &TechDb,
    tracks: &[LayerTracks],
    join: &GuideJoin,
    opts: GraphOpts,
    vias: &ViaTable,
    route_db: &RouteDb,
    threads: usize,
) -> ProbeReport;
```

`GuideUnion` is a small `enum { Bridged, Met2Connected,
Disjoint, GuideAbsent }` — the `guide_union_connected`
classifier's value form.

## 2. `maze.rs` — `pub(crate)` openings (landed; 4 dead ones to revert)

Widening only; no behaviour change. As-built, `probe.rs` needs:

| item | why |
|---|---|
| `Occupancy` (struct) + `Occupancy::new` | build the probe occupancy |
| `premark_obs` → `premark_marks(db, tech, occ, io_pin_marks)` | `premark_obs` stays as the `true` wrapper; `rasterize_pin_marks` gains `io_pin_marks` (`pin_overlap_census` call site passes `true`) |
| `NetTopo::new` + `NetTopo.cover` field, `NetAdj::new` | probe-time attach/adjacency rebuild; `ptopo.cover` feeds `attach_evidence` |
| `route_one_net`, `NetOutcome` | the re-attempt itself |
| `scan_stub`, `fill_blocked`, `Hit` | the attach-evidence rescan (`Hit.iv` is `pub(crate)`, so `Interval`/`Occupant` widened for field-type visibility — needed even though the rescan reads them only through `fill_blocked`) |
| `covered_range` | rescan helper |

Widened but **unused** — revert to private (`steps.md` §2):
`EdgeProbe`, `node_xy`, `Occupancy::scan_wire`,
`Occupancy::scan_rect`. `hit_bbox` stayed private.

No new `pub` items on `maze.rs`; no signature change to any
`pub` item. `premark_obs` keeps its name and call sites.

## 3. `graph.rs` — shared union classifier

Move `guide_union_connected` from `tests/drt.rs` (~2313) into
`graph.rs` as `pub fn guide_union_connected(boxes: &[GuideBox],
max_layer: u8) -> bool`, plus the `GuideUnion` wrapper
(`guide_union(boxes) -> GuideUnion`). The test's local copy is
deleted and call sites updated — one implementation, shared by
the pinned classifier and the probe, so the met3-bridge anchor
compares like with like.

## 4. What is *not* touched

- `route_nets` / `route_nets_with_progress` / `route_prepared` /
  the iteration loop / commit / extraction / emit — the probe
  is a separate entry point; routed output unchanged by
  construction. Verified anyway: `GCD_ROUTE_DIGEST` pin +
  `test_e2e_ours_def_deterministic_gcd` byte-compare re-run.
- `route_digest` — `RouteDb` gains no fields (the report is a
  separate `ProbeReport`, never stored on `RouteDb`), so the
  digest is unchanged by construction.
- `main.rs`, `check_drt_ours.sh`, `recount_unrouted.py` — out
  of `## Files` scope; the probe is invoked by tests only.

## 5. Invariants the implementation must hold

- Classification is a **total function**: every `(pin, kind)` in
  every `u.failed` yields exactly one `ProbePin`; a fallthrough
  is `residual_attach_inconsistent`/`residual_missing_pin`,
  never a panic or a dropped row.
- `report.nets.len() == route_db.unrouted.len()`, same order.
- The probe occupancy is built once per call and never
  mutated by probes (no commits — `route_one_net` takes
  `&Occupancy`; the borrow checker enforces the read-only
  claim).
- `ProbeReport` contains no references → `Send`/`Sync`-safe,
  collectible in `unrouted` order under `par_iter`.
