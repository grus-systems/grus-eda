# drt-unrouted-census — Census semantics (the ever-failed vs final question)

The goal asks to resolve which semantics the printed census carries
before adding per-pin detail. Verified against the code, not the
goal's phrasing — see §1's correction.

## 1. `u.failed` is already the *final* classification

`route_nets` writes `route_db.unrouted` exactly once: on the `hard`
(last) iteration, `unrouted = failed.map(|(_, u)| u)`
(`maze.rs` ~3774-3776) and the loop breaks. Non-hard iterations
never touch `route_db.unrouted` — a soft-iteration failure only
re-queues the net into `bad` (every failed net is retried until it
routes or dies on the hard pass). `u.failed` itself is rebuilt per
attempt by `ordered_failed(&run.connected, &attachable)` over the
fresh `NetRun` — no cross-iteration accumulation, each `pin_nodes`
member appears at most once.

**Correction to the goal**: the accumulating field is
`stub_outside_cover` (`route_nets` pushes `stub_blocked` pins every
iteration a net fails — a pin may repeat across iterations and nets
that later routed keep their records; `docs/results.md`
2026-11-25: 3,288 pins / 1,988 nets, 168 re-attached). The
ever-failed *pin* census therefore prints `stub_lines ≥
Unattachable`, not `⊆ unrouted`. `u.failed` needs no fix — the
honest answer for the per-pin print is the **final** `FailKind`,
which is what `u.failed` already holds and what the emitted DEF's
connectivity truth reflects.

So the printed surfaces decompose cleanly:

| field | semantics | printed as |
|---|---|---|
| `unrouted_kinds` | Σ per-pin **final** kinds over `u.failed` | `NoPath=n Unattachable=m` (landed) |
| `    fail` lines | per-pin **final** kind, `u.failed` order | this ticket |
| `stub_outside_cover` | pins that **ever** had no legal paint (all iterations) | landed |
| `region_escape` flag | graph-build fact: pin shapes hold no strict in-region node | suffix on `fail` lines |

## 2. The provenance taxonomy `region_escape` creates

`RegionEscape` is recorded at graph build when a pin's shapes hold
no in-region strict node on any layer (`graph.rs` ~922-924).
`NetTopo::new` then: strict empty → try `project_attach`
(`projected`/`attachable`), then `access_vias` arcs may re-set
`attachable` (`maze.rs` ~1702-1745). `FailKind::Unattachable` is
produced two ways:

- `topo.attachable[pi] == false` — no strict node, no projected
  node, no access arc. Strict-empty implies `region_escape` was
  recorded ⇒ **every topo-`Unattachable` carries the flag**.
- `stub_blocked` override (`maze.rs` ~2465-2475): the pin *was*
  attachable and graph-connected, but `try_pin_paint` found no
  legal paint (projected stub crossing an uncovered hull, access
  arcs blocked by live occupancy) ⇒ **`Unattachable` without the
  flag means "had candidates, no legal paint"** — and such a pin
  necessarily has a `stub_outside_cover` record.

Resulting reading of a `fail` line:

| line | meaning |
|---|---|
| `fail p Unattachable region_escape` | no attach candidate ever existed — guide/track coverage gap |
| `fail p Unattachable` | candidates existed; every paint was illegal — occupancy/hull (pin appears in `stub_outside_cover`) |
| `fail p NoPath region_escape` | recovered candidates via projection/access arcs, still unreachable — stranded pocket |
| `fail p NoPath` | attachable, wave exhausted — the `unrouted_blockers` class |

That is exactly the operator distinction the review asked for:
"no candidate" vs "no legal paint" vs "no path".

## 3. Zero-failure nets: unreachable — documented, not printed

Every `fail(…)` site in `route_one_net` enumerates `!connected`
pins; a fully-connected net returns `NetOutcome::Routed` and never
reaches the unrouted list. Guided nets carry ≥2 pins, so each
`UnroutedNet.failed` is non-empty (`test_maze_census` already
asserts it, `tests/drt.rs` ~1657). The acceptance's "nets with
zero failures recorded if reachable" arm is therefore a no-op —
recorded here as verified-unreachable rather than printed as a
phantom `0` class.

## 4. Digest contract

`route_digest` (`maze.rs` ~3928+) serializes `nets`, `unrouted`
(per `u`: net id, `connected`, `failed` pins **with kind**),
`collisions`, residuals, and `stub_outside_cover` — already covering
everything this ticket prints. The ticket adds no `RouteDb` field
and touches no production path: the digest is unchanged *by
construction*, and `steps.md` verifies it by re-running the gcd
maze census and comparing the printed digest.

## 5. Doc-comment delta (maze.rs, optional but recommended)

`UnroutedNet.failed`'s comment should name the semantics resolved
above — e.g. extend "…named per pin" with "final hard-iteration
classification in `pin_nodes` order; `stub_outside_cover` is the
ever-failed census". One comment hunk; no code change.

## Files

- `crates/grus-core/src/drt/maze.rs`
- `docs/e6-drt.md`
