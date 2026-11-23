# e6-drt-emit — Self-DRC gate design

The gate: `run_all` on **our own re-parsed output** — the same
50-rule subset that shows 0 findings on both committed TritonRoute
DEFs (`tests/drc.rs` asserts this as its mutation-test control).
The gate exercises the whole serialization path: emit → write →
parse → assemble → check. A violation found here is a real routing
bug, not a writer quirk — the in-memory `RouteDb` is never checked
directly.

## Sequence

```
route_nets → apply_routes → write_routed_def(out)
           → parse_def_bytes(out)  → db2
           → assemble(&db2, &tech) → DrcDesign
           → run_all(&drc, &tech, &db2.strings) → SubsetReport
           → assert: every RuleOutcome.findings is empty
```

The re-parse (`db2`, not `db`) is deliberate — it forces the emitted
text through `parse_net_wire`/`resolve_def_via` so broken grammar,
unresolvable via names, or dropped conn tokens surface as parse
errors rather than as wrong geometry.

`assemble` sees the complete picture: our signal wires + vias from
`net_geoms`, the template's SPECIALNETS PDN stripes, placed-cell
macro pins/OBS, and top-level IO pin rects — no special-casing.

## Expected finding classes (maze looseness inventory)

Maze occupancy is deliberately conservative-but-loose in named
places. If `run_all` fires, expect findings from these classes —
each must be fixed in the router or named with rule + count:

| class | mechanism | likely rules |
|---|---|---|
| foreign-pin landing | `Pin` marks are soft (`hit_verdict`: never hard-block) — a route may end within spacing of another net's pin metal | `*.2` spacing |
| OBS crossing | host-cell OBS carved unconditionally; approach edges clear the OBS box but the wire itself may clip OBS shapes | `*.2` spacing vs OBS net |
| projected attach | `projected` pins land at the nearest legal point — the stub may sit inside OBS or near foreign metal | spacing/enclosure |
| io-port landing | top-level `PIN` metal is not in occupancy | spacing vs `net 0` shapes |
| cut-layer spacing | cuts unmodeled in occupancy — two adjacent vias' cuts may violate cut spacing | spacing on cut layers |
| short stub | relaxed-via stubs paint beyond the pin — may touch foreign metal | spacing |

The checker unions same-net shapes through via stacks (`group`
tags), so same-net T-junctions and via-to-wire abutment inside one
net do **not** self-flag.

Acceptance wording: "0 violations across all 50 subset rules —
every violation is either fixed or explicitly named with its rule
and count." The test prints all `RuleOutcome`s (rule, layer,
bound, candidates, findings) and asserts the empty set; on a red
run, `results.md` names each firing rule, its finding count, and
the root-cause class from the table — matching the existing
enclosure/area test idiom (`{name}: {v:?}` dumps per finding).

## Fail-proof (must go red)

Two independent arms — the test uses both:

1. **Injected geometry** (deterministic, emit-level): after
   `apply_routes`, pick two different-net wires on the same layer
   that sit one spacing-bound apart in the census, extend one
   `wire_rect` across the gap (`xh += bound`), then write → parse →
   `run_all` → assert `findings ≥ 1`. Proves the emit→parse→DRC
   path is not vacuous — the gate must detect what we emit.
2. **Occupancy-disabled route** (already proven by
   `test_maze_occupancy`: `OccMode::Disabled` ⇒ 6,429 track
   collisions on gcd): route with `occ: Disabled`, emit, gate →
   overlapping different-net wires must produce spacing findings.
   This is the acceptance's "skip occupancy" arm and doubles as the
   CLI's `--occ disabled` evidence.

Non-vacuity asserts inside the gate test (mirroring the existing
`test_*_zero_violations` census idiom): emitted `wire_rects > 0`,
`vias > 0`, per-rule `candidates > 0` on every spacing/enclosure/
width rule that TritonRoute's DEF exercises — a gate that checks
nothing proves nothing.

## Determinism

`run_all` twice on the same `DrcDesign` must return identical
`SubsetReport`s (assert `ra == rb`, same as the spacing/enclosure/
area tests). Emitted-DEF determinism itself is the roundtrip test —
two `write_routed_def` calls on the same db produce identical bytes.

## What the gate does NOT claim

- No Magic `drc(full)` on our DEF — that is `e6-drt-e2e`'s
  container gate (`check_ours_drt.sh`). Our subset is 50 rules of
  the deck, not the deck.
- No antenna, no connectivity check (netlist-level opens beyond the
  named `unrouted` set — `drc` checks geometry, not netlist
  continuity). Both are e2e scope.

## Files

- `crates/grus-core/tests/drt.rs`
- `crates/grus-core/src/drt/emit.rs` (the injected-geometry arm
  needs `net_geoms` writable post-`apply_routes` — plain field
  mutation, no new API)
