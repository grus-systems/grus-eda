# drt-unrouted-analysis — Root-cause taxonomy

Every `(pin, kind)` entry of every `UnroutedNet` in
`route_db.unrouted` gets exactly one `RootCause`. Classes are
mutually exclusive by construction: the decision tree below is a
total function of `(real FailKind, probe state, evidence)`.

## 1. Decision tree

```
probe_state(pin)                               real kind
───────────────                                ─────────
Connected                                    Unattachable → neg_attach_stub | neg_attach_port
                                             NoPath       → neg_wave | neg_wave_obs_frontier
                                                          | neg_wave_unattributed
ProbeFailed(Unattachable)                    *            → geom_off_track | geom_no_in_region
                                                          | geom_obs_sealed | residual_attach_inconsistent
ProbeFailed(NoPath)                          *            → geom_met3_bridge | geom_pocket
                                                          | geom_disjoint
pin ∉ g.pin_nodes                            *            → residual_missing_pin
```

`*` = real kind does not discriminate (the class answers "would
it route without foreign marks?" — a probe failure is geometric
whatever the real kind was; a real `Unattachable` that the probe
promotes to `NoPath` is recorded on the row as
`probe_attached` evidence — the pin's foreign-mark-sensitive
attach was real, but it would not have routed anyway).

## 2. Geometric classes (probe still fails)

Foreign `Net`/`Pin` absent — `Obs`/`ObsPin`, off-track,
guide-coverage, and region topology only.

| class | assigned when | fix direction |
|---|---|---|
| `geom_off_track` | probe `Unattachable`; no pin shape holds a covered track on either axis | access-arc / attach-ladder widening (the `u2/C`-fixture class) |
| `geom_no_in_region` | probe `Unattachable`; covered tracks exist but zero in-region nodes in the ±1 projection window (pin carries a `region_escape` record) | guide coverage / halo-`escape` bounds |
| `geom_obs_sealed` | probe `Unattachable` via `stub_blocked`, or all rescan candidates refused by `Obs`/`ObsPin` | pin-access geometry: relaxed-via/access candidates around fixed walls |
| `geom_met3_bridge` | probe `NoPath`; `!guide_union_connected(boxes, 2) && guide_union_connected(boxes, MAX)` — the ≤met2 union only met3+ rejoins | met2–met3 transition domain (`M2M3_PR` edge domain / relaxed-via reach — the named "dead capacity" gap) |
| `geom_pocket` | probe `NoPath`; `guide_union_connected(boxes, 2)` | pocket walls: OBS-sealed pockets, masked-region splits, single-nearest stranded-pocket arms (probe `u2.blockers` recorded for the fixed-wall evidence) |
| `geom_disjoint` | probe `NoPath`; fully-disjoint guide union at all layers | residual-named — "not possible on real GRT output" per the pinned classifier; ~0 expected, counted loudly |

## 3. Negotiation classes (probe connects)

The pin routes once foreign `Net`/`Pin` marks are gone — the
real hard-iteration failure was ordering/occupancy, not
geometry.

| class | assigned when | evidence |
|---|---|---|
| `neg_attach_stub` | real `Unattachable`, probe `Connected`, `pin ∈ stub_outside_cover` | foreign `Net`/`Pin` refused the extraction paint (`try_pin_paint` exhaustion) |
| `neg_attach_port` | real `Unattachable`, probe `Connected`, `pin ∉ stub_outside_cover` | topo-stage refusal: foreign io-port `Pin` band on `project_attach`/`access_vias` candidates (premark-time occupancy holds no `Net` marks, so `Pin` is the only foreign kind the probe drops that could refuse) |
| `neg_wave` | real `NoPath`, probe `Connected`; attributable `(layer, Net|Pin)` blocker evidence | `u.blockers` paired per pin (exact when `blockers.len() == #NoPath-failed`, else net-level set) |
| `neg_wave_obs_frontier` | real `NoPath`, probe `Connected`; all attributable blockers are `Obs`/`ObsPin` | wave died on a fixed wall yet routes without foreign marks — freed capacity elsewhere in the pocket; named, counted |
| `neg_wave_unattributed` | real `NoPath`, probe `Connected`; `u.blockers` empty | wave died with no attributed mark (PQ exhaustion inside a foreign-sealed component); named, counted |

## 4. Residual named classes

Anything the tree misses — each counted separately in the
table, each treated as a defect signal, never folded into a
neighbor class.

| class | meaning |
|---|---|
| `residual_missing_pin` | a `u.failed` pin absent from the rebuilt graph's `pin_nodes` — join/graph invariant break |
| `residual_attach_inconsistent` | probe `Unattachable` whose evidence rescan admits a candidate — classifier/evidence divergence |

Per-pin `wave` evidence pairing (probe.md §4): `u.blockers`
aligns 1:1 with `u.failed`'s `NoPath` subsequence in order iff
`blockers.len() == count(failed kind == NoPath)`; otherwise the
pin row carries the net-level blocker set and the results
entry says so. `neg_wave_obs_frontier`/`neg_wave_unattributed`
are assigned per pin from the *pin-attributable* evidence when
paired, else the net set.

## 5. Reconciliation rules (the acceptance's sum)

- `Σ pins per class == Σ u.failed.len()` — on gcd the pinned
  totals are `NoPath 46 + Unattachable 52 = 98` pin-failures;
  on picorv32 `2,225 + 1,480 = 3,705`. The test asserts the
  report's pin rows are exactly the `u.failed` multiset —
  each pin once.
- `geometric + negotiation + residual == total`, and the
  residual count prints separately so it can never hide inside
  a real class.

## 6. Anchor reconciliation

- **met3-bridge residual**: let `B` = nets the
  `guide_union_connected` classifier marks bridged
  (`!conn(≤2) && conn(all)`) among `route_db.unrouted`. Pins on
  `B` split into `geom_met3_bridge` and negotiation classes
  (probe-rescued bridges are negotiation — honest per the
  definition). Assert `geom_met3_bridge nets ⊆ B` and
  `geom_met3_bridge nets ∪ rescued-B nets == B`; the results
  entry names the split vs the pinned census numbers
  (maze-census 43; emit-leg recount 138 overall / 48 no-stub —
  the current-leg number is measured, and a delta from those
  is named, per the spec's "or names the delta" arm).
- **`region_escape` population**: the flag is orthogonal to
  class (a probe-rescued escaped pin is negotiation). The
  table reports `u.failed ∩ region_escape` split across
  classes; the pinned populations are gcd 2 fail-line records
  (`_134_`, `_138_` — measured outcome recorded, not assumed)
  and the per-net `escapes` census.
- **unguided io-port nets**: `probe.unguided` vs the golden's
  `ours_router.unguided` — exact set equality asserted
  (gcd ∅, picorv32 67 `irq[*]`/`pcpi_*`).

## 7. Decomposition table shape (results.md)

Per design, one table: rows = the 13 classes above (zero rows
kept — a named class at 0 is itself evidence), columns =
pins, nets (distinct nets carrying ≥1 pin of the class).
Followed by: the anchor reconciliation block (met3_bridge
split, region_escape split, unguided), `probe_ms` vs
`route_ms`, and the prioritized target list — classes ordered
by pin count with their fix directions from §2/§3.
