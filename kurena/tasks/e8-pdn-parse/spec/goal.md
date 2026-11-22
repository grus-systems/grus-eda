# e8-pdn-parse — Per-net PDN extraction from SPECIALNETS

E8 ticket 2 (reference ticket `e8-irdrop-ref` landed at ff24949). The
IR-drop solver needs a resistor-ready geometry model of the power grid.
`DesignDb::special_wire_rects` today is a flat `(LayerId, RectDbu)` list
with no net attribution, keeps only `width > 0` wires, drops the
zero-width via-connection records entirely, and exists solely for gcell
blockage. This ticket replaces it with a per-net PDN structure.

## Scope

1. Parse DEF `SPECIALNETS` into `PdnNet` records: net name, `+ USE`
   (POWER/GROUND), per-layer wire rects, and via stacks — the
   `NEW <layer> 0 + SHAPE STRIPE ( x y ) <via-name>` records that link
   consecutive layers at a point.
2. Parse the LEF `VIA` statements currently skipped
   (`lef: skipped 60 statements (VIA x25 ...)`) enough to resolve a via
   name → its CUT layer (the three `LAYER`/`RECT` groups per `VIA`, only
   the cut-layer identity is needed for resistance lookup via
   `TechDb::via_res_ohm_above`; geometry may be ignored or stored).
3. `DesignDb` gains `pdn_nets` (or equivalent) keyed by net id;
   `special_wire_rects` becomes a derived view or is replaced by
   accessor calls at its one consumer (`congestion/gcell.rs`).
4. Attach standard-cell power/ground pins: for each instance, the LEF
   `PIN VPWR`/`VGND` `PORT` rect (transformed by placement) intersects
   PDN rects on the same layer — record pin→node attachment.
5. Self-connectivity check mirroring `check_power_grid`: every attached
   power pin must reach the special net's top-level `PIN` (voltage
   source) through wire-overlap unions + via stacks. Expose a census
   (rects/nodes/vias/attached pins per net) and an `unreachable_pins`
   list.

## Golden cross-check

`tests/designs/gcd/gcd.irdrop.reference.json` carries the per-net
`check_power_grid` result (all connected) and `csv_rows` (384 = the
per-instance voltage map size = number of attached instance terminals,
both nets combined over the two CSV files — verify the split per net
against `gcd.irdrop.voltage.{vpwr,vgnd}.embedded.csv` row counts). Our
connectivity check must agree: zero unreachable pins on both nets on
`gcd.placed.def`, and the attached-terminal count per net must equal
the reference CSV row counts.

Also run picorv32 (its `picorv32.irdrop.reference.json` + CSVs are
committed); record census and unattached count — picorv32 reference
reports all-connected per its golden.

## Fail-proof

Drop via-stack edges (or one via layer) and the connectivity check must
report unreachable pins (test goes red; then restore).

## Files

- `crates/grus-core/src/readers/def.rs` (SPECIALNETS per-net parse)
- `crates/grus-core/src/readers/lef.rs` (VIA statement → cut layer map)
- `crates/grus-core/src/design_db.rs` (`pdn_nets` + PDN types)
- `crates/grus-core/src/congestion/gcell.rs` (consumer repoint)
- `crates/grus-core/src/pdn.rs` or `src/pdn/` (node/edge extraction,
  connectivity, census) — new module
- `crates/grus-core/src/lib.rs`
- `docs/results.md` entry (census, connectivity vs `check_power_grid`)
