# drt-unrouted-analysis — Probe mechanism

The probe answers, per finally-unrouted pin: "does the pin
attach and route when foreign `Net`/`Pin` marks are absent?"
It is a post-pass over `route_db.unrouted` — never wired into
`route_nets`, so the routed output is unchanged by
construction.

## 1. Probe occupancy

One shared `Occupancy` for the whole pass:

```rust
let mut occ = Occupancy::new(tech, tracks);
premark_obs(db, tech, &mut occ);   // minus the io-port `Pin` arm
```

Implementation: `premark_obs` refactors into
`premark_marks(db, tech, occ, io_pin_marks: bool)`; the existing
`premark_obs` stays as the `io_pin_marks: true` wrapper, and
`rasterize_pin_marks` gains the same flag (its second caller,
`pin_overlap_census` ~4883, passes `true`). With
`io_pin_marks: false` the `Some(n) if pin_cells[p].is_none() =>
Occupant::Pin` arm of `rasterize_pin_marks` (~841-842) is
skipped; `Obs`, `ObsPin`, and net-less `Obs(u32::MAX)` marks are
unchanged.

Equivalence argument for dropping *all* `Pin` marks (the spec
says "foreign `Net`/`Pin` absent"): a fresh occupancy carries
no `Net` marks at all, and an own `Pin` mark is free to its own
net under every verdict (`hit_verdict` ~993, `fill_blocked`
~3725-3728) — absent is also free. One shared occupancy
therefore serves every probed net; per-net occupancies would
cost one rebuild each for zero semantic difference.

### Why `OccMode::Disabled` is not the probe

`Disabled` gates only `hit_verdict`'s `Net`/`Pin` branch
(~986-988). The pin-attach paint checks — `try_pin_paint`'s
projected-stub and access-via scans (~3237, ~3288-3311),
`close_self_spacing`, `patch_net_min_area`, and
`project_attach`'s `fill_blocked` refusal — block on foreign
marks mode-blind. Under `Disabled` a foreign-`Net`-sealed stub
still refuses; the probe must physically not have the mark.

## 2. Per-net pipeline

For each `u` in `route_db.unrouted` (probed in report order):

```rust
let &(net, ref boxes) = join.nets[idx[&u.net]];
let nb = build_net_graph(db, tech, tracks, &hull, net, boxes, opts, scratch);
let ptopo = NetTopo::new(db, tech, tracks, &nb.graph, vias, &occ, &hull);
let padj  = NetAdj::new(&nb.graph, &ptopo, tech, tracks, vias, &hull);
let out   = route_one_net(db, tech, tracks, &nb.graph, &ptopo, &padj,
                        vias, &occ, &hull, /*hist*/ 0, /*hard*/ false,
                        OccMode::Enabled);
```

Notes:

- `mk_graph` equivalence — the real pass rebuilds graphs via the
  same `build_net_graph` with the same `opts` (`LOADED_OPTS` =
  `spare_above: 1, escape: DEFAULT`); the probe reuses the
  caller's `opts` so the searched graph is the one the router
  saw.
- `nb.region_escape`/`nb.escapes` come free per net and feed the
  `no_in_region` evidence + the `region_escape` flag on each
  pin row.
- `hard=false, mode=Enabled` — in the probe occupancy no `Net`/
  `Pin` marks exist, so `hit_verdict`'s `blocked` can never
  set; `Obs`/`foreign ObsPin` block in every mode (the
  geometric walls). `hist=0` — negotiation pricing is moot with
  no negotiable marks.
- Full `route_one_net` (search + extraction closure) is the
  honest probe: a pin reached by the wave but with no legal
  paint lands in `stub_blocked` → `geom_obs_sealed`, not
  "connected". The closure cost is proportional to the probe
  outcome; if measured `probe_ms` is unacceptable on picorv32,
  a `skip-closure` variant is follow-up, not v1.
- Probe outcome for the net is recorded (`ProbeNet.probe_routed`)
  plus `u2.blockers` when the probe net fails — the
  fixed-wall `(layer, kind)` evidence behind `geom_*` classes.

### Per-pin probe state

For each `(pin, real_kind)` in `u.failed`:

- `out == Routed(_)` → `Connected` (every member pin routes).
- `out == Failed{unrouted: u2, stub_blocked}` →
  - `pin ∈ u2.connected` → `Connected`;
  - `pin ∈ u2.failed` → `ProbeFailed(u2 kind)` — `Unattachable`
    or `NoPath`;
  - `pin ∈ stub_blocked` → also recorded (`u2.failed` already
    marks it `Unattachable`; the flag feeds `geom_obs_sealed`);
  - `pin` absent from `g.pin_nodes` → `residual_missing_pin`
    (defensive; `join` guarantees members — a hit means a
    graph/join bug, named not hidden).

## 3. Attach evidence rescan

`geom` subclasses need the *why* behind a probe-`Unattachable`.
`attach_evidence(g, nb, pin)` replays `project_attach`'s
enumeration (~884-929) read-only against the probe occupancy
and counts, per pin shape:

1. shapes with **any covered track** —
   `covered_range(&tracks[l].tracks_x|tracks_y, r)` non-empty on
   either axis for any `(l, r) ∈ pin_shapes(pin)`;
2. **candidate nodes** — in-region nodes in the same ±1-line
   window and covered-line rule `project_attach` uses
   (`mask.seg_allowed`-filtered);
3. **refused candidates** — candidates whose `scan_stub` hits a
   `fill_blocked(own)` mark (`Obs`/`ObsPin` only exist in the
   probe occupancy, so every refusal is a fixed wall).

Decision: zero covered tracks on all shapes → `geom_off_track`;
tracks but zero candidates → `geom_no_in_region` (guide coverage
gap — `nb.region_escape` must record the pin; asserted);
candidates all refused → `geom_obs_sealed`; any candidate
admitted yet the pin ended `Unattachable` →
`residual_attach_inconsistent` (evidence/classifier divergence —
named, counted, treated as a bug signal).

Strict-`pin_nodes` non-empty pins can never reach this arm
(attach non-empty ⇒ attachable), so every topo-stage
probe-`Unattachable` carries a `region_escape` record — the
rescan discriminates *within* that population by mechanism,
not by the flag.

## 4. Negotiation evidence

For `Connected` pins, the subclass names *where* the foreign
marks sat in the real run:

- real `Unattachable` → `neg_attach_*`. `pin ∈
  route_db.stub_outside_cover` → `neg_attach_stub`
  (extraction-stage refusal — `try_pin_paint` saw foreign
  `Net`/`Pin` on the stub/access paint). `pin ∉` it →
  `neg_attach_port` (topo-stage refusal: `project_attach` /
  `access_vias` ran against premark occupancy where `Net` marks
  cannot yet exist, so the refusing marks were foreign io-port
  `Pin` bands — the only foreign marks present then that the
  probe drops).
- real `NoPath` → `neg_wave*`. `u.blockers` supplies the
  wave-path evidence: it is pushed ≤1 per failed attachable pin
  in `pin_nodes` order, so it aligns 1:1 with the `NoPath`
  subsequence of `u.failed` exactly when `blockers.len()` equals
  the `NoPath` count; otherwise per-pin evidence degrades to the
  net-level set. Subclasses: attributable `(layer, Net|Pin)` →
  `neg_wave`; blockers all `Obs`/`ObsPin` →
  `neg_wave_obs_frontier` (a fixed-frontier wave death that
  nonetheless routes without foreign marks — the freed capacity
  was elsewhere in the pocket); empty → `neg_wave_unattributed`.

`region_escape` provenance is carried as a flag on every pin
row (join `nb.region_escape` on `(net, pin)`), matching the
`fail` line's suffix semantics — orthogonal to the class.

## 5. Unguided row

`probe.unguided`: `join.absent_expected` filtered to nets whose
single member pin is an io port (`db.net_pins(n).len() == 1 &&
db.pin_cells[pin].is_none()`) — the golden verifier's own rule
(`check_drt_ours.sh` step-E: `( PIN <port> )`-only conns,
guide-absent, unrouted-disjoint). Reproduces 67 on picorv32, 0
on gcd; the test joins names against `ours_router.unguided`.

## 6. Parallelism, cost, determinism

- `pool = pool::n(threads)` (`src/drt/pool.rs`, `pub(crate)`);
  `unrouted.par_iter()` with
  `map_init(Vec::new(), ...)` for `hull_scratch`, `.collect()`
  preserving `unrouted` order — the same pattern as the soft
  search arm. Probes share `&Occupancy` read-only (already
  `Sync` — the parallel search relies on it); results fold in
  order → deterministic `ProbeReport`.
- Cost model: one `Occupancy::new` + `premark` (≈ the
  `premark_obs` phase of a real run) plus per unrouted net
  one graph+topo+adj+search — proportional to the unrouted
  set, never a re-route of committed nets. `probe_elapsed` is
  measured on both designs and recorded in `docs/results.md`
  (expectation, not a bound: gcd ~seconds on ~80 nets;
  picorv32 a fraction of one hard iteration since probes
  parallelize where the hard arm is serial).
- `ProbeReport` ordering = `route_db.unrouted` order; per-pin
  rows in `u.failed` order; class assignment is a pure
  function → two probe runs on the same `RouteDb` are equal.
