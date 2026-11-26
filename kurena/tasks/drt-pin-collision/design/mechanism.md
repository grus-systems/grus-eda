# Mechanism — wire-side record + hard-mode block for `Occupant::Pin`

All edits are in `crates/grus-core/src/drt/maze.rs`. No mark kind is
added — `Occupant::Pin` already exists and `graph.rs` needs no
change (the spec's conditional does not trigger).

## 1. `hit_verdict` (~L815–855): `Pin` joins `blocked`

Current:

```rust
let m = match h.iv.who {
    Occupant::Net(m) => { blocked |= m != own; m }
    Occupant::Pin(m) => m,
    _ => continue,
};
```

Change: merge the arms — `Occupant::Net(m) | Occupant::Pin(m) => {
blocked |= m != own; m }`. Soft-mode price unchanged (`Pin` already
pools into `foreign`); on `hard` the `blocked` flag now covers
foreign pins → `None` → edge forbidden. Own `Pin(own)` stays free
(`m != own` guard, as today).

Doc updates in place: the `Pin` paragraph at ~L810 ("never a hard
block") and the `Occupant::Pin` enum doc at ~L268–274 both state the
old rule — rewrite to the new contract: soft-priced and recorded in
soft iterations, a hard block on the last, own free.

## 2. `edge_collisions` (~L999+): directed pin records

The `record` closure filters `Occupant::Net(m) if m != own.0` into a
canonical `(a < b)` `Collision`. Extend it to also route
`Occupant::Pin(m) if m != own.0` hits into a **second** sink of
`Collision`, but **non-canonical — directed**:

```rust
Collision { a: NetId(own.0), b: NetId(m), layer, line, lo, hi }
```

`a` is always the committing (wire) net, `b` the pin's owning net.
The direction is the semantic payload: the wire is the only movable
party. `Occupant::ObsPin` stays unrecorded (it is already a hard
block — a hit means the edge was never committed, or a bug).

Signature: `edge_collisions(..., out: &mut BTreeSet<Collision>,
out_pins: &mut BTreeSet<Collision>, ...)`. The stub/`scan_rect` arms
(via footprints, relaxed-via stubs) inherit the same `record` — all
committed-element kinds get the record.

`NetOutcome::Routed(NetRoute, Vec<Collision>, ClosureResiduals)`
gains the second vec:
`Routed(NetRoute, Vec<Collision>, Vec<Collision>, ClosureResiduals)`
with a doc comment — `prov` canonical net×net, `prov_pins` directed
wire→pin.

## 3. `route_nets` (~L3766–3847): wire-only re-queue

```rust
let mut prov_pins: BTreeSet<Collision> = BTreeSet::new();
...
NetOutcome::Routed(route, hits, pin_hits, res) => {
    commit_route(...);
    if !hard && cfg.occ == OccMode::Enabled {
        prov.extend(hits);
        prov_pins.extend(pin_hits);
    }
    ...
}
```

Re-queue loop: `prov` keeps the existing both-parties insert;
`prov_pins` inserts **`c.a` only** — the pin's net (`c.b`) is never
re-queued on a pin hit: its port cannot move, and re-queuing it
would waste a route attempt and escalate `hist[b]` unfairly. This is
the spec's "net×pin or pseudo-net pairing so rip-up targets the
wire", expressed without a new type: the directed `Collision` in a
dedicated set carries the pairing.

`IterStat.prov_collisions = (prov.len() + prov_pins.len()) as u32`
— the field's doc already says "the re-queue trigger"; both classes
trigger re-queue, so both count. Doc the `prov_pins` half inside the
`IterStat`/`edge_collisions` comments. (If this moves a golden
`iter_prov` line on gcd, see `steps.md` §6.)

## 4. Hard-mode postmortem (~L2289–2303)

The `or_else` blocker search currently finds
`hard && Net(m ≠ own)`. Extend to
`hard && (Net(m) | Pin(m)) if m != g.net.0` so a last-iteration pin
block attributes `kind = 3` (`Pin`) — the `match` at ~L2322 already
maps it.

## 5. Attach-side stub scans

### 5a. `project_attach` (~L730)

The projection fallback admits on cover alone; its implied stub —
node position → pin-rect clamp — is committed metal. Add the same
occupancy admission the access-via generator (`confined`, ~L1467)
already applies. Signature gains `occ` and the owning net:

```rust
fn project_attach(
    g: &NetGraph, shapes: &[(LayerId, RectDbu)], tracks: &[LayerTracks],
    occ: &Occupancy, mask: &HullMask, cover: &PinCover, own: u32,
) -> Vec<u32>
```

Per candidate node `v` on `lid`: after the `mask.seg_allowed` check,
scan the implied stub —
`scan_stub(g, tracks, occ, cover, v, lid.0, cx, cy, &mut hits)` —
and reject `v` when `hits.iter().any(|h| fill_blocked(h, own))`. At
topo time `occ` holds only premarks (`Obs`/`ObsPin`/`Pin`), so this
fails no candidate a `Net` mark would newly block — that class is
rechecked at extraction (5b). The stub is axis-aligned by
`project_attach`'s own construction (doc ~L2694–2699), so
`scan_stub`'s single-axis model fits.

A pin whose every candidate is rejected gets `projected[pi] =
false` → `Unattachable` (named), not a hang.

### 5b. `try_pin_paint` `AttachElem::Proj` branch (~L2700–2743)

Extraction re-admits the stub against *live* occupancy (committed
`Net` marks did not exist at topo time). Before pushing paint for
candidate `(si, v)`:

```rust
let mut hits = Vec::new();
scan_stub(g, tracks, occ, &topo.cover, v, lid.0, cx, cy, &mut hits);
if hits.iter().any(|h| fill_blocked(h, g.net.0)) {
    tried_proj.insert((si, v));
    continue;
}
```

Strict, matching the Access branch's "emit never gets a soft pass"
rule — no pricing, no record; admission is binary. `tried_proj`
lands the pair in the tried-set so the pin advances to its next
candidate and, on exhaustion, `stub_blocked` → named
`stub_outside_cover`.

## 6. `pin_overlap_census` + `RouteDb.pin_collisions`

New pub function next to `overlap_census` (~L3920):

```rust
/// Committed-wire × foreign-pin overlaps — the named residual for
/// the `Occupant::Pin`/`ObsPin` classes. Rasterizes io-port `Pin`
/// and instance-pin `ObsPin` marks (the `premark_obs` pin loop) plus
/// every route's `Net` marks into a fresh occupancy, then runs the
/// same per-line `R + s` sweep as `overlap_census`, reporting
/// directed `Collision`s: `a` = wire's net, `b` = pin's owning net.
pub fn pin_overlap_census(
    db: &DesignDb, tech: &TechDb, tracks: &[LayerTracks],
    vias: &ViaTable, routes: &[NetRoute],
) -> Vec<Collision>
```

Implementation reuses `premark_obs`'s pin rasterization predicate
(pins with `pin_nets[p].is_some()`: `pin_cells[p].is_some() →
ObsPin`, else `Pin`) and `commit_route` into a throwaway `Vec<Mark>`
sink — same body as `overlap_census`'s rebuild. Pair arm:
`(Net(m1), Pin|ObsPin(m2))` or swapped → `Collision{a:
NetId(m1), b: NetId(m2)}` into a `BTreeSet`.

`RouteDb` gains:

```rust
/// Committed routes' wire×foreign-pin overlaps — directed
/// (`a` = wire's net, `b` = pin's owning net), `Enabled` only.
/// Expected empty after the verdict change; an entry is a named
/// residual, never hidden.
pub pin_collisions: Vec<Collision>,
```

`route_nets` populates it symmetrically with `collisions`
(`overlap_census` is left untouched — its contract and its callers
stay frozen; the double rasterization is one tail pass, trivial at
gcd scale and once per picorv32 run).

## 7. `route_digest` (~L3967)

Serialize `pin_collisions` after `collisions` (same `put` pattern).
New fail-proof test mirroring `test_route_digest_covers_iter_stats`.

## 8. `main.rs` census print (conditional)

After the `collisions=` line block (~L2069): print per-entry
`pin_collision <a_net> <b_net> <layer> line=<l> iv=[lo,hi]` **only
when `route_db.pin_collisions` is non-empty**. Zero → byte-identical
emit census → zero golden churn on a clean design; nonzero → the
residual is named in the committed log.
