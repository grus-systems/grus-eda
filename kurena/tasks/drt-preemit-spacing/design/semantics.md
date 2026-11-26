# Semantics — what `preemit_check` measures

## Signature and inputs

```rust
// crates/grus-core/src/drt/emit.rs
pub fn preemit_check(db: &DesignDb, tech: &TechDb) -> Result<SubsetReport>
```

- Called on the `DesignDb` after `apply_routes` has committed
  `route_db` into `db.net_geoms`, before `write_routed_def`.
- `assemble` errors propagate (`Err`) — via-def/cut-name resolution is
  a real boundary; the CLI arm bails with `?`. `run_all` is infallible.
- `&DesignDb`, not `&mut` — the check never mutates emit geometry.

## Shape-set coverage

`assemble(&db)` emits the identical five sources `--self-drc`'s
`assemble(&db2)` does, because emit changes only `net_geoms` and those
round-trip byte-exact (proof: existing `test_emit_roundtrip_gcd`).

| `ShapeSrc` | provenance at pre-emit point | checkable endpoint? |
|---|---|---|
| `SignalWire` | `net_geoms[*].wire_rects` — `seg_rect` wires + fills + foreign `RECT` stubs | yes (routing) |
| `SpecialWire` | `pdn_nets` from the placed template | yes (routing) |
| `Via` | `net_geoms[*].vias` — `ViaPlace`s resolved to pair defaults | yes (cuts + metals) |
| `MacroPin` / `MacroObs` | placed cells' transformed macro `PIN`/`OBS` rects | **islands only — no findings** (documented under-report, `docs/e7-drc.md` §12) |
| `IoPort` | top-level `PINS` `+ LAYER` rects — the foreign-pin class occupancy soft-priced | yes (routing) |

"Foreign pin shapes" in the acceptance = the `IoPort` endpoint class —
the one `Occupant::Pin` marked soft until `drt-pin-collision`. Instance
(`MacroPin`) shapes are present and merge into islands but cannot be a
finding endpoint under current checker semantics; that limitation is
shared verbatim with `--self-drc` and `grus-eda drc`, and changing it
is a separate golden-re-calibration ticket.

## Finding classes this check does and does not see

Sees (identically to `--self-drc`):

- positive sub-bound gaps between different-island checkable pairs —
  wire↔wire, wire↔via metal, wire↔io-port, via↔via (cut spacing);
- width/enclosure/area/hole findings on the same shape set.

Does not see (shared subset boundary — name it, don't hide it):

- any pair whose counterpart is `MacroPin`/`MacroObs` (e.g. the
  historical in-hull li1 adjacency — Magic-only findings);
- distance-0 overlaps (`axis_gap` clamps to `(0,0)` → `touching_ok`):
  committed wire×pin overlaps are `RouteDb.pin_collisions`' class, not
  this checker's;
- same-net disjoint tiles inside `s` — same-net shapes union through
  islands; Magic flags them net-agnostically (recorded divergence,
  `docs/results.md` 2026-11-24 entry);
- emit grammar/serialization faults — there is no text yet. That is
  `--self-drc`'s unique value and why it stays.

## Equivalence contract

`preemit_check(db)` findings ≡ `run_all(assemble(parse(write(db))))`
findings, given a clean write/parse roundtrip (itself gated by
`test_emit_roundtrip_gcd` and `test_emit_determinism`). The clean-case
test asserts the two `SubsetReport`s are `==` — candidates, bounds,
layers and findings all included. A divergence there means the emit
text does not describe the in-memory geometry: a writer bug, which is
exactly the class `--self-drc` exists to catch — the assert fails
loudly rather than papering over it.

## Output

`SubsetReport` — the same normalized 50-rule report `--self-drc` and
`grus-eda drc` print. Findings carry `rule`/`rule_name`, `layer`,
`bound`, `candidates`, `bboxes`, `measured`, `nets` — so the CLI's
existing `drc_finding_line`/`drc_family_name` formatters produce
identical lines (acceptance: same rule names, same bbox output).
